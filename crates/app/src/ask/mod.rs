//! Local Ask orchestration. Request and answer buffers never implement Debug or enter logs.
mod cli;
pub(crate) mod process;

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use opit_core::ask::{Agent, AskConfig, Conversation, MAX_REQUEST, SYSTEM_PROMPT};
use opit_core::cleanup::{ChatMessage, CleanupClient, CleanupError};
use tokio::sync::{Mutex, watch};

use crate::controller::ErrorKind;
use crate::platform::SecretStore;

pub use cli::{AgentStatus, statuses};

pub struct AskService {
    secrets: Arc<dyn SecretStore>,
    conversation: Mutex<Conversation>,
    generation: AtomicU64,
    consumed_generation: AtomicU64,
}

impl AskService {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            secrets,
            conversation: Mutex::default(),
            generation: AtomicU64::new(0),
            consumed_generation: AtomicU64::new(0),
        }
    }

    pub fn isolated(&self) -> Self {
        Self::new(self.secrets.clone())
    }

    /// Invalidates a running turn as well: its eventual result cannot restore old context.
    pub fn reset(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut conversation) = self.conversation.try_lock() {
            *conversation = Conversation::default();
        }
    }

    pub async fn answer(
        &self,
        config: AskConfig,
        request: String,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<String, ErrorKind> {
        if request.trim().is_empty() || request.len() > MAX_REQUEST || !config.valid() {
            return Err(ErrorKind::Rejected);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let mut conversation = self.conversation.lock().await;
        // Generation is consumed under the same lock as conversation state.
        if self.consumed_generation.swap(generation, Ordering::SeqCst) != generation {
            *conversation = Conversation::default();
        }
        let last_generation = self.generation.load(Ordering::SeqCst);
        if generation != last_generation {
            return Err(ErrorKind::Cancelled);
        }
        let now = Instant::now();
        conversation.prepare(&config, now);
        let process_cancel = cancel.clone();
        let work = async {
            let working_dir = config.working_dir.clone();
            let directory = tokio::task::spawn_blocking(move || resolve_directory(&working_dir))
                .await
                .map_err(|_| ErrorKind::Rejected)??;
            match config.agent {
                Agent::Chat => {
                    let secrets = self.secrets.clone();
                    let chat_config = config.chat_config();
                    let client = tokio::task::spawn_blocking(move || {
                        let key = match &chat_config.api_key_ref {
                            Some(key_ref) => secrets.get(key_ref).map_err(|_| ErrorKind::Credentials)?,
                            None => None,
                        };
                        CleanupClient::new(chat_config, key).map_err(chat_error)
                    })
                    .await
                    .map_err(|_| ErrorKind::Network)??;
                    let mut messages = vec![ChatMessage { role: "system", content: SYSTEM_PROMPT.into() }];
                    messages.extend(conversation.turns.clone());
                    messages.push(ChatMessage { role: "user", content: request.clone() });
                    Ok((client.complete(&messages).await.map_err(chat_error)?, None))
                }
                _ => {
                    let config = config.clone();
                    let session = conversation.session_id.clone();
                    let input = format!("{SYSTEM_PROMPT}\n\n{request}");
                    let cancel = process_cancel.clone();
                    tokio::task::spawn_blocking(move || {
                        let executable = cli::resolve(config.agent).ok_or(ErrorKind::Rejected)?;
                        let args = cli::arguments(&config, session.as_deref())?;
                        let stdout = process::run(
                            executable,
                            args,
                            directory,
                            input,
                            Duration::from_secs(u64::from(config.timeout_seconds)),
                            cancel,
                        )?;
                        cli::parse(config.agent, &stdout)
                    })
                    .await
                    .map_err(|_| ErrorKind::Network)?
                }
            }
        };
        let result = if config.agent != Agent::Chat {
            work.await
        } else {
            tokio::select! {
                biased;
                _ = async { loop { if *cancel.borrow_and_update() || cancel.changed().await.is_err() { break; } } } => Err(ErrorKind::Cancelled),
                result = tokio::time::timeout(Duration::from_secs(u64::from(config.timeout_seconds)), work) => result.unwrap_or(Err(ErrorKind::Timeout)),
            }
        };
        let (answer, session) = match result {
            Ok(answer) => answer,
            Err(kind) => {
                *conversation = Conversation::default();
                return Err(kind);
            }
        };
        if self.generation.load(Ordering::SeqCst) == generation {
            conversation.finish(request, answer.clone(), session, Instant::now());
        }
        Ok(answer)
    }
}

fn chat_error(error: CleanupError) -> ErrorKind {
    error.into()
}

pub fn resolve_directory(value: &str) -> Result<std::path::PathBuf, ErrorKind> {
    let directory = if value.is_empty() { dirs::home_dir().ok_or(ErrorKind::Rejected)? } else { value.into() };
    if !directory.is_absolute() || !directory.is_dir() {
        return Err(ErrorKind::Rejected);
    }
    let directory = directory.canonicalize().map_err(|_| ErrorKind::Rejected)?;
    #[cfg(windows)]
    let directory = {
        let value = directory.to_str().ok_or(ErrorKind::Rejected)?;
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            std::path::PathBuf::from(format!(r"\\{unc}"))
        } else {
            std::path::PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(value))
        }
    };
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::FakeSecrets;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn chat_http_failures_keep_their_kind_without_saving_private_turns() {
        for (status, body, kind) in [
            (402, "private provider response", ErrorKind::PaymentRequired),
            (400, r#"{"error":{"details":[{"reason":"API_KEY_INVALID"}]}}"#, ErrorKind::InvalidKey),
            (400, "private provider response", ErrorKind::Rejected),
            (429, "private provider response", ErrorKind::RateLimited),
            (503, "private provider response", ErrorKind::Server),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .expect(1)
                .mount(&server)
                .await;
            let config = AskConfig { base_url: server.uri(), api_key_ref: None, ..Default::default() };
            let service = AskService::new(Arc::new(FakeSecrets::default()));
            let (_cancel, rx) = watch::channel(false);
            assert_eq!(service.answer(config, "private request".into(), rx).await, Err(kind));
            assert!(service.conversation.lock().await.turns.is_empty());
        }
    }

    #[tokio::test]
    async fn chat_request_shape_continuation_and_reset_use_only_the_ask_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"choices":[{"message":{"content":"A longer answer, with ```code``` preserved."},"finish_reason":"stop"}]})))
            .mount(&server).await;
        let config = AskConfig { base_url: server.uri(), chat_model: "ask-test-model".into(), ..Default::default() };
        let service = AskService::new(Arc::new(FakeSecrets::with("ask", "ask-only-secret")));
        let (_cancel, rx) = watch::channel(false);
        for request in ["first", "second"] {
            let answer = service.answer(config.clone(), request.into(), rx.clone()).await.unwrap();
            assert!(answer.contains("```code```"));
        }
        service.reset();
        service.answer(config, "fresh".into(), rx).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        for (i, count) in [2, 4, 2].into_iter().enumerate() {
            let body: serde_json::Value = requests[i].body_json().unwrap();
            assert_eq!(body["model"], "ask-test-model");
            assert_eq!(body["messages"].as_array().unwrap().len(), count);
            assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
            assert_eq!(requests[i].headers["authorization"], "Bearer ask-only-secret");
        }
        assert_eq!(requests[1].body_json::<serde_json::Value>().unwrap()["messages"][1]["content"], "first");
    }

    #[tokio::test]
    async fn chat_cancel_and_timeout_do_not_commit_partial_turns() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let config = AskConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 1, ..Default::default() };
        let service = Arc::new(AskService::new(Arc::new(FakeSecrets::default())));
        let (cancel, rx) = watch::channel(false);
        let pending = service.answer(config.clone(), "private request".into(), rx.clone());
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("unexpected completion: {:?}", result.err()),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
        cancel.send(true).unwrap();
        assert_eq!(pending.await, Err(ErrorKind::Cancelled));
        assert!(service.conversation.lock().await.turns.is_empty());
        cancel.send(false).unwrap();
        assert_eq!(service.answer(config, "request".into(), rx).await, Err(ErrorKind::Timeout));
        assert!(service.conversation.lock().await.turns.is_empty());
    }

    #[test]
    fn working_directory_must_exist_and_be_absolute() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_directory(dir.path().to_str().unwrap()).is_ok());
        assert!(resolve_directory("relative/path").is_err());
        assert!(resolve_directory(dir.path().join("missing").to_str().unwrap()).is_err());
        assert!(resolve_directory("").is_ok());
    }
}
