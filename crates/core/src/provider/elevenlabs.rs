//! Native ElevenLabs batch speech-to-text adapter. No OpenAI fields or post-editing.

use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use super::openai::{BASE_REQUEST_TIMEOUT, CONNECT_TIMEOUT, request_timeout};
use super::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};

pub struct ElevenLabs {
    client: reqwest::Client,
    profile: Profile,
    api_key: Option<String>,
}

impl ElevenLabs {
    pub fn new(profile: Profile, api_key: Option<String>) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // Custom API-key headers and uploaded audio must never follow a redirect.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(send_error)?;
        let api_key = api_key.map(|key| key.trim().to_string()).filter(|key| !key.is_empty());
        Ok(Self { client, profile, api_key })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.profile.base_url.trim().trim_end_matches('/'))
    }

    fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => request.header("xi-api-key", key),
            None => request,
        }
    }

    /// An authenticated read only. Never uploads audio or consumes transcription credits.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let response = self
            .authenticate(self.client.get(self.url("user")))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(send_error)?;
        let status = response.status().as_u16();
        // Discard account information; only the HTTP status is needed.
        let _ = response.bytes().await;
        check_status(status)
    }

    /// Best-effort pool warm-up, without transcribing or inspecting account data.
    pub async fn warm_up(&self) {
        let _ = self.test_connection().await;
    }

    fn form(&self, request: &TranscribeRequest<'_>) -> Form {
        let audio = request.audio;
        let file = Part::bytes(audio.bytes.clone())
            .file_name(audio.file_name())
            .mime_str(audio.mime())
            .expect("static MIME types are valid");
        let mut form = Form::new()
            .part("file", file)
            .text("model_id", self.profile.model.clone())
            .text("temperature", "0")
            .text("diarize", "false")
            .text("tag_audio_events", "false")
            .text("no_verbatim", "false");
        let language = self.profile.language.trim();
        if !language.is_empty() && language != "auto" {
            form = form.text("language_code", language.to_string());
        }
        // Keyterms are a paid add-on. The preset keeps this explicit opt-in off.
        if self.profile.send_keywords {
            for term in request.keywords.iter().filter(|term| valid_keyterm(term)).take(1000) {
                form = form.text("keyterms", term.clone());
            }
        }
        form
    }
}

fn valid_keyterm(term: &str) -> bool {
    !term.trim().is_empty()
        && term.chars().count() < 50
        && term.split_whitespace().count() <= 5
        && !term.contains(['<', '>', '{', '}', '[', ']', '\\'])
}

impl Transcriber for ElevenLabs {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    async fn transcribe(&self, request: &TranscribeRequest<'_>) -> Result<RawTranscript, ProviderError> {
        let response = self
            .authenticate(self.client.post(self.url("speech-to-text")))
            .timeout(request_timeout(BASE_REQUEST_TIMEOUT, request.audio_ms))
            .multipart(self.form(request))
            .send()
            .await
            .map_err(send_error)?;
        if response.status().as_u16() == 429
            && let Some(delay) = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| super::retry::retry_after(v, std::time::SystemTime::now()))
        {
            return Err(ProviderError::RateLimitedAfter(delay));
        }
        check_status(response.status().as_u16())?;
        let body = response.text().await.map_err(send_error)?;
        #[derive(Deserialize)]
        struct Transcript {
            text: String,
        }
        let transcript: Transcript = serde_json::from_str(&body)
            .map_err(|_| ProviderError::BadResponse("invalid ElevenLabs transcription response".into()))?;
        Ok(RawTranscript { text: transcript.text.trim().to_string(), dropped_segments: 0 })
    }
}

fn send_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(error.without_url().to_string()) }
}

/// Never include the provider's body: it can echo keys, transcript text or keyterms.
fn check_status(status: u16) -> Result<(), ProviderError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(ProviderError::Unauthorized(status)),
        413 => Err(ProviderError::PayloadTooLarge),
        429 => Err(ProviderError::RateLimited),
        500..=599 => Err(ProviderError::Server(status)),
        _ => Err(ProviderError::Http { status, message: "ElevenLabs refused the request".into() }),
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::audio::encode::{AudioFormat, encode};
    use crate::provider::{ProviderClient, presets};

    fn profile(server: &MockServer) -> Profile {
        let mut profile = presets::elevenlabs();
        profile.base_url = format!("{}/v1/", server.uri());
        profile
    }

    fn fields(body: &str, name: &str) -> Vec<String> {
        let marker = format!("name=\"{name}\"");
        body.match_indices(&marker)
            .map(|(at, _)| {
                let rest = &body[at..];
                let start = rest.find("\r\n\r\n").unwrap() + 4;
                let end = rest[start..].find("\r\n").unwrap();
                rest[start..start + end].to_string()
            })
            .collect()
    }

    #[tokio::test]
    async fn dispatches_scribe_with_native_fields_and_no_paid_addons() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/speech-to-text"))
            .and(header("xi-api-key", "synthetic-key"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"text":" Merhaba dünya ","words":[]}"#))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProviderClient::new(profile(&server), Some(" synthetic-key\r\n".into())).unwrap();
        let audio = encode(&[0.0; 16_000], AudioFormat::Wav).unwrap();
        let terms = ["GitHub".into()];
        let raw = client
            .transcribe(&TranscribeRequest {
                audio: &audio,
                audio_ms: 1000,
                prompt: Some("private context"),
                keywords: &terms,
            })
            .await
            .unwrap();
        assert_eq!(raw.text, "Merhaba dünya");
        assert_eq!(raw.dropped_segments, 0);
        let requests = server.received_requests().await.unwrap();
        assert!(requests[0].headers.get("authorization").is_none());
        let body = String::from_utf8_lossy(&requests[0].body);
        for (name, value) in [
            ("model_id", "scribe_v2"),
            ("language_code", "tr"),
            ("temperature", "0"),
            ("diarize", "false"),
            ("tag_audio_events", "false"),
            ("no_verbatim", "false"),
        ] {
            assert_eq!(fields(&body, name), [value]);
        }
        for name in ["model", "language", "response_format", "prompt", "keywords[]", "keyterms", "edit_instruction"] {
            assert!(fields(&body, name).is_empty(), "unexpected field {name}");
        }
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.contains("Content-Type: audio/wav") || body.contains("content-type: audio/wav"));
    }

    #[tokio::test]
    async fn auto_language_and_explicit_keyterms_follow_native_contract() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"text":"ok"}"#))
            .mount(&server)
            .await;
        let mut profile = profile(&server);
        profile.language = "auto".into();
        profile.send_keywords = true;
        let client = ElevenLabs::new(profile, None).unwrap();
        let audio = encode(&[0.0; 16_000], AudioFormat::Wav).unwrap();
        let terms = ["Opit Speech".into(), "invalid[term]".into(), " ".into(), "a b c d e f".into(), "x".repeat(50)];
        client
            .transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &terms })
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(fields(&body, "language_code").is_empty());
        assert_eq!(fields(&body, "keyterms"), ["Opit Speech"]);
    }

    #[tokio::test]
    async fn connection_and_warmup_are_authenticated_reads_even_on_failure() {
        for status in [200, 401, 403, 404, 429, 503] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/user"))
                .and(header("xi-api-key", "synthetic-key"))
                .respond_with(ResponseTemplate::new(status).set_body_string("private account data"))
                .expect(2)
                .mount(&server)
                .await;
            let client = ProviderClient::new(profile(&server), Some("synthetic-key".into())).unwrap();
            assert_eq!(client.test_connection().await, check_status(status));
            client.warm_up().await;
            assert!(server.received_requests().await.unwrap().iter().all(|r| r.method.as_str() == "GET"));
        }
    }

    #[tokio::test]
    async fn transcription_preserves_the_rate_limit_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "90").set_body_string("private body"))
            .mount(&server)
            .await;
        let client = ElevenLabs::new(profile(&server), None).unwrap();
        let audio = encode(&[0.1; 16_000], AudioFormat::Wav).unwrap();
        let result =
            client.transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &[] }).await;
        assert_eq!(result.unwrap_err(), ProviderError::RateLimitedAfter(Duration::from_secs(90)));
    }

    #[tokio::test]
    async fn redirects_never_forward_credentials_or_audio() {
        let server = MockServer::start().await;
        let destination = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200)).mount(&destination).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"text":"ok"}"#))
            .mount(&destination)
            .await;
        for endpoint in ["/v1/user", "/v1/speech-to-text"] {
            Mock::given(path(endpoint))
                .respond_with(ResponseTemplate::new(307).insert_header("Location", destination.uri()))
                .expect(1)
                .mount(&server)
                .await;
        }
        let client = ProviderClient::new(profile(&server), Some("synthetic-key".into())).unwrap();
        let connection = client.test_connection().await;
        let audio = encode(&[0.0; 16_000], AudioFormat::Wav).unwrap();
        let transcription =
            client.transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &[] }).await;

        let forwarded = destination.received_requests().await.unwrap();
        assert!(
            forwarded.iter().all(|request| !request.headers.contains_key("xi-api-key")),
            "redirect forwarded API credentials"
        );
        assert!(forwarded.is_empty(), "redirect destination received a request");
        assert!(matches!(connection, Err(ProviderError::Http { status: 307, .. })));
        assert!(matches!(transcription, Err(ProviderError::Http { status: 307, .. })));
    }

    #[tokio::test]
    async fn errors_never_expose_response_values() {
        for (status, body) in [
            (400, r#"{"detail":"secret transcript synthetic-key"}"#),
            (200, r#"{"text":{"secret transcript":"synthetic-key"}}"#),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&server)
                .await;
            let client = ElevenLabs::new(profile(&server), None).unwrap();
            let audio = encode(&[0.0; 16_000], AudioFormat::Wav).unwrap();
            let error = client
                .transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &[] })
                .await
                .unwrap_err();
            assert!(!error.to_string().contains("secret transcript"));
            assert!(!error.to_string().contains("synthetic-key"));
            if status == 200 {
                assert!(matches!(error, ProviderError::BadResponse(_)));
            }
        }
    }
}
