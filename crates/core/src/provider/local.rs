//! Opit Speech to Text's keyless loopback transcription adapter.

use super::{Profile, ProviderError, RawTranscript, ResponseFormat, TranscribeRequest, Transcriber};
use reqwest::multipart::{Form, Part};
use std::time::Duration;

pub struct LocalWhisper {
    client: reqwest::Client,
    profile: Profile,
}

impl LocalWhisper {
    pub fn new(mut profile: Profile) -> Result<Self, ProviderError> {
        let url = reqwest::Url::parse(&profile.base_url)
            .map_err(|_| ProviderError::Network("invalid local endpoint".into()))?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ProviderError::Network("local endpoint must use IPv4 loopback".into()));
        }
        profile.api_key_ref = None;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| ProviderError::Network("local HTTP client unavailable".into()))?;
        Ok(Self { client, profile })
    }

    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let response = self
            .client
            .get(format!("{}/health", self.profile.base_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(send_error)?;
        if response.status().is_success() { Ok(()) } else { Err(ProviderError::Server(response.status().as_u16())) }
    }
}

impl Transcriber for LocalWhisper {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    async fn transcribe(&self, request: &TranscribeRequest<'_>) -> Result<RawTranscript, ProviderError> {
        let file = Part::bytes(request.audio.bytes.clone())
            .file_name(request.audio.file_name())
            .mime_str(request.audio.mime())
            .expect("static MIME");
        let mut form = Form::new()
            .part("file", file)
            .text(
                "language",
                if self.profile.language.trim().is_empty() { "auto".into() } else { self.profile.language.clone() },
            )
            .text("temperature", "0")
            .text("response_format", self.profile.response_format.as_str());
        if self.profile.send_prompt
            && let Some(prompt) = request.prompt
        {
            form = form.text("prompt", prompt.to_owned());
        }
        let response = self
            .client
            .post(format!("{}/inference", self.profile.base_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(300) + Duration::from_millis(request.audio_ms))
            .multipart(form)
            .send()
            .await
            .map_err(send_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(match status {
                413 => ProviderError::PayloadTooLarge,
                500..=599 => ProviderError::Server(status),
                _ => ProviderError::Http { status, message: "local request refused".into() },
            });
        }
        let body = response.text().await.map_err(send_error)?;
        super::openai::parse_body(&body, self.profile.response_format == ResponseFormat::VerboseJson)
    }
}

fn send_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() { ProviderError::Timeout } else { ProviderError::Network("local connection failed".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    #[tokio::test]
    async fn multipart_is_keyless_and_detected_language_is_normalized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/inference"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"sample", "language":"turkish"})),
            )
            .mount(&server)
            .await;
        let mut profile = super::super::presets::local();
        profile.base_url = server.uri();
        let client = LocalWhisper::new(profile).unwrap();
        let audio = crate::audio::encode::encode(&[0.0; 16000], crate::audio::encode::AudioFormat::Wav).unwrap();
        let result = client
            .transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: Some("context"), keywords: &[] })
            .await
            .unwrap();
        assert_eq!(result.language.as_deref(), Some("tr"));
        let requests = server.received_requests().await.unwrap();
        let request = &requests[0];
        assert!(request.headers.get("authorization").is_none());
        let body = String::from_utf8_lossy(&request.body);
        for (name, value) in
            [("language", "tr"), ("prompt", "context"), ("response_format", "verbose_json"), ("temperature", "0")]
        {
            assert!(body.contains(&format!("name=\"{name}\"\r\n\r\n{value}\r\n")));
        }
        assert!(body.contains("name=\"file\"; filename=\"audio.wav\""));
        assert!(!body.contains("name=\"model\""));
    }
    #[test]
    fn rejects_non_loopback_endpoints() {
        for url in [
            "https://127.0.0.1:80",
            "http://localhost:80",
            "http://192.168.1.1:80",
            "http://127.0.0.1:80/path",
            "http://u@127.0.0.1:80",
        ] {
            let mut profile = super::super::presets::local();
            profile.base_url = url.into();
            assert!(LocalWhisper::new(profile).is_err());
        }
    }
}
