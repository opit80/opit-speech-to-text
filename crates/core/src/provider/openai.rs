//! Client for any server that implements OpenAI's `/audio/transcriptions`.

use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::audio::encode::encode;

use super::{Profile, ProviderError, RawTranscript, ResponseFormat, TranscribeRequest, Transcriber};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const BASE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const NO_SPEECH_PROB_LIMIT: f64 = 0.6;
const AVG_LOGPROB_LIMIT: f64 = -1.0;

pub fn request_timeout(base: Duration, audio_ms: u64) -> Duration {
    base + Duration::from_millis(audio_ms / 4)
}

pub struct OpenAiCompatible {
    client: reqwest::Client,
    profile: Profile,
    api_key: Option<String>,
    base_timeout: Duration,
}

impl OpenAiCompatible {
    pub fn new(profile: Profile, api_key: Option<String>) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let api_key = api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        Ok(Self { client, profile, api_key, base_timeout: BASE_REQUEST_TIMEOUT })
    }

    /// Overrides the 30 s base timeout (tests, slow self-hosted servers).
    pub fn with_base_timeout(mut self, base: Duration) -> Self {
        self.base_timeout = base;
        self
    }

    pub fn endpoint(&self) -> String {
        format!("{}/audio/transcriptions", self.profile.base_url.trim().trim_end_matches('/'))
    }

    /// Checks the base URL and key with `GET {base_url}/models`. Servers without
    /// that endpoint get a 1 s silent transcription instead.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let url = format!("{}/models", self.profile.base_url.trim().trim_end_matches('/'));
        let mut request = self.client.get(url).timeout(Duration::from_secs(10));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        if let Ok(response) = request.send().await {
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(());
            }
            if matches!(status, 401 | 403) {
                return Err(ProviderError::Unauthorized(status));
            }
        }
        let silence =
            encode(&[0.0; 16_000], self.profile.audio_format).map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        let request = TranscribeRequest { audio: &silence, audio_ms: 1_000, prompt: None, keywords: &[] };
        self.transcribe(&request).await.map(|_| ())
    }

    fn form(&self, request: &TranscribeRequest<'_>) -> Form {
        let audio = request.audio;
        let file = Part::bytes(audio.bytes.clone())
            .file_name(audio.file_name())
            .mime_str(audio.mime())
            .expect("static MIME types are valid");
        let mut form = Form::new()
            .part("file", file)
            .text("model", self.profile.model.clone())
            .text("temperature", "0")
            .text("response_format", self.profile.response_format.as_str());
        let language = self.profile.language.trim();
        if !language.is_empty() && language != "auto" {
            form = form.text("language", language.to_string());
        }
        if self.profile.send_prompt
            && let Some(prompt) = request.prompt.filter(|p| !p.trim().is_empty())
        {
            form = form.text("prompt", prompt.to_string());
        }
        if self.profile.send_keywords {
            for keyword in request.keywords {
                form = form.text("keywords[]", keyword.clone());
            }
        }
        form
    }
}

impl Transcriber for OpenAiCompatible {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        let mut builder = self
            .client
            .post(self.endpoint())
            .timeout(request_timeout(self.base_timeout, request.audio_ms))
            .multipart(self.form(request));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        let verbose = self.profile.response_format == ResponseFormat::VerboseJson;
        async move {
            let response = builder.send().await.map_err(send_error)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(send_error)?;
            if !(200..300).contains(&status) {
                return Err(status_error(status, &body));
            }
            parse_body(&body, verbose)
        }
    }
}

fn send_error(err: reqwest::Error) -> ProviderError {
    if err.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(err.to_string()) }
}

fn status_error(status: u16, body: &str) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Unauthorized(status),
        413 => ProviderError::PayloadTooLarge,
        429 => ProviderError::RateLimited,
        500..=599 => ProviderError::Server(status),
        _ => ProviderError::Http { status, message: error_message(body) },
    }
}

/// OpenAI-style `{"error":{"message":…}}`, else the first 200 chars of the body.
fn error_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct Envelope {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        message: String,
    }
    serde_json::from_str::<Envelope>(body).map(|e| e.error.message).unwrap_or_else(|_| body.chars().take(200).collect())
}

#[derive(Deserialize)]
struct ApiResponse {
    text: String,
    #[serde(default)]
    segments: Option<Vec<Segment>>,
}

#[derive(Deserialize)]
struct Segment {
    #[serde(default)]
    text: String,
    #[serde(default)]
    no_speech_prob: f64,
    #[serde(default)]
    avg_logprob: f64,
}

fn parse_body(body: &str, verbose: bool) -> Result<RawTranscript, ProviderError> {
    let response: ApiResponse = serde_json::from_str(body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;
    let segments = if verbose { response.segments.unwrap_or_default() } else { Vec::new() };
    let is_silence = |s: &Segment| s.no_speech_prob > NO_SPEECH_PROB_LIMIT && s.avg_logprob < AVG_LOGPROB_LIMIT;
    let dropped = segments.iter().filter(|s| is_silence(s)).count();
    if dropped == 0 {
        return Ok(RawTranscript { text: response.text.trim().to_string(), dropped_segments: 0 });
    }
    let text = segments
        .iter()
        .filter(|s| !is_silence(s))
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(RawTranscript { text, dropped_segments: dropped })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::audio::encode::{AudioFormat, EncodedAudio, encode};
    use crate::provider::presets;

    const PATH: &str = "/v1/audio/transcriptions";

    async fn server(status: u16, body: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        server
    }

    fn profile_for(server: &MockServer) -> Profile {
        let mut profile = presets::groq();
        profile.base_url = format!("{}/v1/", server.uri());
        profile
    }

    fn wav() -> EncodedAudio {
        encode(&[0.0; 16_000], AudioFormat::Wav).unwrap()
    }

    async fn send(
        profile: Profile,
        key: Option<&str>,
        prompt: Option<&str>,
        keywords: &[String],
    ) -> Result<RawTranscript, ProviderError> {
        let audio = wav();
        let client = OpenAiCompatible::new(profile, key.map(str::to_string)).unwrap();
        client.transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1_000, prompt, keywords }).await
    }

    /// Values of every multipart field called `name`.
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

    async fn only_request(server: &MockServer) -> wiremock::Request {
        let mut requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        requests.remove(0)
    }

    #[tokio::test]
    async fn sends_the_expected_multipart_form() {
        let server = server(200, r#"{"text":" Merhaba dünya "}"#).await;
        let keywords = vec!["Claude Code".to_string(), "ox_lib".to_string()];
        let mut profile = profile_for(&server);
        profile.send_keywords = true;
        let raw = send(profile, Some("sk-test"), Some("Bağlam."), &keywords).await.unwrap();
        assert_eq!(raw, RawTranscript { text: "Merhaba dünya".into(), dropped_segments: 0 });

        let request = only_request(&server).await;
        assert_eq!(request.headers.get("authorization").unwrap(), "Bearer sk-test");
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert_eq!(fields(&body, "model"), ["whisper-large-v3"]);
        assert_eq!(fields(&body, "temperature"), ["0"]);
        assert_eq!(fields(&body, "response_format"), ["verbose_json"]);
        assert_eq!(fields(&body, "language"), ["tr"]);
        assert_eq!(fields(&body, "prompt"), ["Bağlam."]);
        assert_eq!(fields(&body, "keywords[]"), ["Claude Code", "ox_lib"]);
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.to_ascii_lowercase().contains("content-type: audio/wav"));
    }

    #[tokio::test]
    async fn optional_fields_are_left_out() {
        let server = server(200, r#"{"text":"ok"}"#).await;
        let mut profile = profile_for(&server);
        profile.language = "auto".into();
        profile.send_prompt = false;
        let keywords = vec!["GitHub".to_string()];
        send(profile, None, Some("Bağlam."), &keywords).await.unwrap();

        let request = only_request(&server).await;
        assert!(request.headers.get("authorization").is_none());
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert!(fields(&body, "language").is_empty());
        assert!(fields(&body, "prompt").is_empty());
        assert!(fields(&body, "keywords[]").is_empty(), "groq preset has send_keywords off");
    }

    #[tokio::test]
    async fn verbose_json_drops_silent_segments() {
        let body = r#"{"text":"Merhaba altyazı m.k","segments":[
            {"text":" Merhaba","no_speech_prob":0.1,"avg_logprob":-0.2},
            {"text":" altyazı m.k","no_speech_prob":0.9,"avg_logprob":-1.5}]}"#;
        let server = server(200, body).await;
        let raw = send(profile_for(&server), None, None, &[]).await.unwrap();
        assert_eq!(raw, RawTranscript { text: "Merhaba".into(), dropped_segments: 1 });
    }

    #[tokio::test]
    async fn uncertain_but_speechy_segments_are_kept() {
        let body = r#"{"text":"a b","segments":[
            {"text":"a","no_speech_prob":0.9,"avg_logprob":-0.5},
            {"text":"b","no_speech_prob":0.2,"avg_logprob":-1.5}]}"#;
        let server = server(200, body).await;
        let raw = send(profile_for(&server), None, None, &[]).await.unwrap();
        assert_eq!(raw.text, "a b");
    }

    #[tokio::test]
    async fn http_statuses_map_to_typed_errors() {
        let cases = [
            (401, "{}", ProviderError::Unauthorized(401)),
            (403, "{}", ProviderError::Unauthorized(403)),
            (413, "{}", ProviderError::PayloadTooLarge),
            (429, "{}", ProviderError::RateLimited),
            (503, "<html>Service Unavailable</html>", ProviderError::Server(503)),
            (
                400,
                r#"{"error":{"message":"model not found"}}"#,
                ProviderError::Http { status: 400, message: "model not found".into() },
            ),
        ];
        for (status, body, expected) in cases {
            let server = server(status, body).await;
            assert_eq!(send(profile_for(&server), None, None, &[]).await.unwrap_err(), expected, "HTTP {status}");
        }
    }

    #[tokio::test]
    async fn non_json_success_is_a_bad_response() {
        let server = server(200, "<html>captive portal</html>").await;
        let err = send(profile_for(&server), None, None, &[]).await.unwrap_err();
        assert!(matches!(err, ProviderError::BadResponse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn slow_server_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}").set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;
        let audio = wav();
        let client =
            OpenAiCompatible::new(profile_for(&server), None).unwrap().with_base_timeout(Duration::from_millis(200));
        let request = TranscribeRequest { audio: &audio, audio_ms: 0, prompt: None, keywords: &[] };
        assert_eq!(client.transcribe(&request).await.unwrap_err(), ProviderError::Timeout);
    }

    #[tokio::test]
    async fn closed_port_is_a_network_error() {
        // Bind and immediately release a port so nothing listens on it.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut profile = presets::groq();
        profile.base_url = format!("http://127.0.0.1:{port}/v1");
        let err = send(profile, None, None, &[]).await.unwrap_err();
        assert!(matches!(err, ProviderError::Network(_)), "{err:?}");
    }

    #[test]
    fn timeout_grows_with_audio_length() {
        assert_eq!(request_timeout(BASE_REQUEST_TIMEOUT, 8_000), Duration::from_secs(32));
    }

    #[test]
    fn endpoint_joins_without_double_slash() {
        let mut profile = presets::groq();
        profile.base_url = "https://example.com/v1/".into();
        assert_eq!(
            OpenAiCompatible::new(profile, None).unwrap().endpoint(),
            "https://example.com/v1/audio/transcriptions"
        );
    }

    async fn connection_server(models_status: u16, transcription_status: u16) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(models_status).set_body_string(r#"{"data":[]}"#))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(transcription_status).set_body_string(r#"{"text":""}"#))
            .mount(&server)
            .await;
        server
    }

    async fn test_connection_with(server: &MockServer) -> Result<(), ProviderError> {
        OpenAiCompatible::new(profile_for(server), Some("k".into())).unwrap().test_connection().await
    }

    #[tokio::test]
    async fn connection_ok_via_models_without_transcribing() {
        let server = connection_server(200, 500).await;
        assert_eq!(test_connection_with(&server).await, Ok(()));
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
    }

    #[tokio::test]
    async fn connection_reports_a_rejected_key_from_models() {
        let server = connection_server(401, 200).await;
        assert_eq!(test_connection_with(&server).await, Err(ProviderError::Unauthorized(401)));
    }

    #[tokio::test]
    async fn connection_falls_back_to_a_silent_transcription() {
        let server = connection_server(404, 200).await;
        assert_eq!(test_connection_with(&server).await, Ok(()));
        let failing = connection_server(404, 500).await;
        assert_eq!(test_connection_with(&failing).await, Err(ProviderError::Server(500)));
    }

    #[tokio::test]
    async fn pasted_key_with_newline_is_trimmed() {
        let server = server(200, r#"{"text":"ok"}"#).await;
        send(profile_for(&server), Some("sk-test\r\n"), None, &[]).await.unwrap();
        let request = only_request(&server).await;
        assert_eq!(request.headers.get("authorization").unwrap(), "Bearer sk-test");
    }
}
