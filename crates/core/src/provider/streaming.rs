//! Streaming protocols and native WebSocket transport. All transport errors are sanitized.

use super::{Profile, ProviderError, ProviderKind, RawTranscript};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream,
    tungstenite::{self, Message, client::IntoClientRequest},
};

pub type LiveFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

/// A session is owned by one worker. Dropping it cancels without committing audio.
pub trait LiveSession: Send {
    fn send_audio<'a>(&'a mut self, samples: &'a [i16]) -> LiveFuture<'a, ()>;
    fn next_update(&mut self) -> LiveFuture<'_, Option<LiveUpdate>>;
    fn keep_alive(&mut self) -> LiveFuture<'_, ()>;
    fn finish(self: Box<Self>) -> LiveFuture<'static, RawTranscript>;
}

/// Deliberately not Debug: this contains private dictated text.
pub struct LiveUpdate {
    pub text: String,
}

#[derive(Default)]
pub struct Assembly {
    committed: String,
    partial: String,
    language: Option<String>,
}

pub struct ParsedUpdate {
    pub live: Option<LiveUpdate>,
    pub final_ack: bool,
}

fn append(target: &mut String, text: &str) {
    if !text.trim().is_empty() {
        if !target.is_empty() {
            target.push(' ');
        }
        target.push_str(text.trim());
    }
}

impl Assembly {
    pub fn parse(&mut self, provider: ProviderKind, body: &str) -> Result<ParsedUpdate, ProviderError> {
        let value: Value = serde_json::from_str(body).map_err(|_| bad_response())?;
        let (text, committed, final_ack) = match provider {
            ProviderKind::Elevenlabs => {
                let kind = value["message_type"].as_str().ok_or_else(bad_response)?;
                if value.get("error").is_some()
                    || matches!(
                        kind,
                        "error"
                            | "auth_error"
                            | "quota_exceeded"
                            | "commit_throttled"
                            | "unaccepted_terms"
                            | "rate_limited"
                            | "queue_overflow"
                            | "resource_exhausted"
                            | "session_time_limit_exceeded"
                            | "input_error"
                            | "invalid_request"
                            | "chunk_size_exceeded"
                            | "insufficient_audio_activity"
                            | "transcriber_error"
                    )
                {
                    return Err(match kind {
                        "auth_error" => ProviderError::Unauthorized(401),
                        "quota_exceeded" | "rate_limited" | "commit_throttled" => ProviderError::RateLimited,
                        _ => bad_response(),
                    });
                }
                if !matches!(
                    kind,
                    "partial_transcript" | "committed_transcript" | "committed_transcript_with_timestamps"
                ) {
                    return Ok(ParsedUpdate { live: None, final_ack: false });
                }
                if let Some(language) = value["language_code"].as_str() {
                    self.language = super::language::normalize(language);
                }
                (
                    value["text"].as_str().ok_or_else(bad_response)?,
                    kind != "partial_transcript",
                    kind != "partial_transcript",
                )
            }
            ProviderKind::Deepgram => {
                match value["type"].as_str().ok_or_else(bad_response)? {
                    // A server Error is non-fatal to the socket but invalidates this dictation;
                    // the recorded audio remains available to the batch retry ladder.
                    "Error" => return Err(bad_response()),
                    "Results" => {}
                    _ => return Ok(ParsedUpdate { live: None, final_ack: false }),
                }
                if let Some(language) = value["channel"]["detected_language"].as_str() {
                    self.language = super::language::normalize(language);
                }
                (
                    value["channel"]["alternatives"][0]["transcript"].as_str().ok_or_else(bad_response)?,
                    value["is_final"].as_bool().unwrap_or(false),
                    value["from_finalize"].as_bool().unwrap_or(false),
                )
            }
            _ => return Err(bad_response()),
        };
        if committed {
            append(&mut self.committed, text);
            self.partial.clear();
        } else {
            self.partial = text.trim().into();
        }
        let mut text = self.committed.clone();
        append(&mut text, &self.partial);
        Ok(ParsedUpdate { live: Some(LiveUpdate { text }), final_ack })
    }

    fn transcript(&self) -> RawTranscript {
        RawTranscript {
            text: self.committed.clone(),
            language: self.language.clone(),
            dropped_segments: 0,
            segments: Vec::new(),
        }
    }
}

fn bad_response() -> ProviderError {
    ProviderError::BadResponse("invalid streaming response".into())
}

fn socket_error(error: tungstenite::Error) -> ProviderError {
    if let tungstenite::Error::Http(response) = error {
        return super::deepgram::status(response.status().as_u16(), response.headers())
            .err()
            .unwrap_or_else(bad_response);
    }
    ProviderError::Network("streaming connection closed or failed".into())
}

pub fn tls_config() -> Result<rustls::ClientConfig, ProviderError> {
    use rustls_platform_verifier::BuilderVerifierExt;
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| bad_response())?
        .with_platform_verifier()
        .map_err(|_| ProviderError::Network("TLS verifier unavailable".into()))
        .map(|builder| builder.with_no_client_auth())
}

fn request(
    profile: &Profile,
    key: Option<&str>,
    terms: &[String],
) -> Result<tungstenite::handshake::client::Request, ProviderError> {
    let mut url = url::Url::parse(profile.base_url.trim()).map_err(|_| bad_response())?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return Err(bad_response()),
    };
    url.set_scheme(scheme).map_err(|_| bad_response())?;
    let path = format!(
        "{}/{}",
        url.path().trim_end_matches('/'),
        if profile.provider == ProviderKind::Elevenlabs { "speech-to-text/realtime" } else { "listen" }
    );
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad_response());
    }
    {
        let mut query = url.query_pairs_mut();
        let language = profile.language.trim();
        match profile.provider {
            ProviderKind::Elevenlabs => {
                query
                    .append_pair("model_id", "scribe_v2_realtime")
                    .append_pair("audio_format", "pcm_16000")
                    .append_pair("commit_strategy", "manual");
                if language.is_empty() || language == "auto" {
                    query.append_pair("include_language_detection", "true");
                } else {
                    query.append_pair("language_code", language);
                }
            }
            ProviderKind::Deepgram => {
                query
                    .append_pair("model", &profile.model)
                    .append_pair("language", if language.is_empty() || language == "auto" { "multi" } else { language })
                    .append_pair("encoding", "linear16")
                    .append_pair("sample_rate", "16000")
                    .append_pair("channels", "1")
                    .append_pair("interim_results", "true")
                    .append_pair("punctuate", "true")
                    .append_pair("smart_format", "true");
            }
            _ => return Err(bad_response()),
        }
        if profile.send_keywords {
            // A small conservative prompt budget, shared by both adapters.
            let mut budget = 0;
            for term in terms.iter().filter(|t| !t.trim().is_empty() && t.chars().count() <= 48).take(32) {
                budget += term.chars().count();
                if budget > 384 {
                    break;
                }
                query
                    .append_pair(if profile.provider == ProviderKind::Deepgram { "keyterm" } else { "keyterms" }, term);
            }
        }
    }
    let mut request = url.as_str().into_client_request().map_err(socket_error)?;
    if let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) {
        let (name, value) = if profile.provider == ProviderKind::Elevenlabs {
            ("xi-api-key", key.to_owned())
        } else {
            ("Authorization", format!("Token {key}"))
        };
        let mut value =
            value.parse::<tungstenite::http::HeaderValue>().map_err(|_| ProviderError::Unauthorized(401))?;
        value.set_sensitive(true);
        request.headers_mut().insert(name, value);
    }
    Ok(request)
}

pub struct WebSocketSession {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    provider: ProviderKind,
    assembly: Assembly,
}

impl WebSocketSession {
    pub async fn connect(
        profile: Profile,
        key: Option<String>,
        terms: &[String],
    ) -> Result<Box<dyn LiveSession>, ProviderError> {
        let request = request(&profile, key.as_deref(), terms)?;
        let tls = tls_config()?;
        let config = tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(1024 * 1024));
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_tls_with_config(
                request,
                Some(config),
                false,
                Some(Connector::Rustls(Arc::new(tls))),
            ),
        )
        .await
        .map_err(|_| ProviderError::Timeout)?
        .map_err(socket_error)?;
        Ok(Box::new(Self { socket, provider: profile.provider, assembly: Assembly::default() }))
    }

    async fn read(&mut self) -> Result<ParsedUpdate, ProviderError> {
        match self.socket.next().await {
            Some(Ok(Message::Text(body))) => self.assembly.parse(self.provider, &body),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(ParsedUpdate { live: None, final_ack: false }),
            Some(Ok(_)) | None => Err(socket_error(tungstenite::Error::ConnectionClosed)),
            Some(Err(error)) => Err(socket_error(error)),
        }
    }

    async fn chunk(&mut self, samples: &[i16], commit: bool) -> Result<(), ProviderError> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let message = if self.provider == ProviderKind::Deepgram {
            Message::Binary(bytes.into())
        } else {
            Message::Text(json!({"message_type":"input_audio_chunk", "audio_base_64":STANDARD.encode(bytes), "commit":commit, "sample_rate":16000}).to_string().into())
        };
        self.socket.send(message).await.map_err(socket_error)
    }
}

impl LiveSession for WebSocketSession {
    fn send_audio<'a>(&'a mut self, samples: &'a [i16]) -> LiveFuture<'a, ()> {
        Box::pin(async move {
            for chunk in samples.chunks(3200) {
                self.chunk(chunk, false).await?;
            }
            Ok(())
        })
    }
    fn next_update(&mut self) -> LiveFuture<'_, Option<LiveUpdate>> {
        Box::pin(async { Ok(self.read().await?.live) })
    }
    fn keep_alive(&mut self) -> LiveFuture<'_, ()> {
        Box::pin(async {
            if self.provider == ProviderKind::Deepgram {
                self.socket
                    .send(Message::Text(json!({"type":"KeepAlive"}).to_string().into()))
                    .await
                    .map_err(socket_error)?;
            }
            Ok(())
        })
    }
    fn finish(mut self: Box<Self>) -> LiveFuture<'static, RawTranscript> {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(4), async {
                if self.provider == ProviderKind::Elevenlabs {
                    self.chunk(&[], true).await?;
                } else {
                    self.socket
                        .send(Message::Text(json!({"type":"Finalize"}).to_string().into()))
                        .await
                        .map_err(socket_error)?;
                }
                loop {
                    if self.read().await?.final_ack {
                        break;
                    }
                }
                if self.provider == ProviderKind::Deepgram {
                    self.socket
                        .send(Message::Text(json!({"type":"CloseStream"}).to_string().into()))
                        .await
                        .map_err(socket_error)?;
                }
                let _ = self.socket.close(None).await;
                Ok(self.assembly.transcript())
            })
            .await
            .map_err(|_| ProviderError::Timeout)?
        })
    }
}

#[cfg(test)]
mod tests;
