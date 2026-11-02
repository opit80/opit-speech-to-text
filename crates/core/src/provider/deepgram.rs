//! Native prerecorded Deepgram transcription. Provider response bodies never enter errors.

use super::openai::{BASE_REQUEST_TIMEOUT, CONNECT_TIMEOUT, request_timeout};
use super::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};

pub struct Deepgram {
    client: reqwest::Client,
    profile: Profile,
    key: Option<String>,
}

impl Deepgram {
    pub fn new(mut profile: Profile, key: Option<String>) -> Result<Self, ProviderError> {
        profile.audio_format = crate::audio::encode::AudioFormat::Wav;
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ProviderError::Network("HTTP client unavailable".into()))?;
        Ok(Self { client, profile, key: key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) })
    }

    fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(key) => request.header("Authorization", format!("Token {key}")),
            None => request,
        }
    }

    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let response = self
            .authenticate(self.client.get(format!("{}/projects", self.profile.base_url.trim_end_matches('/'))))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(send_error)?;
        status(response.status().as_u16(), response.headers())
    }
}

pub(super) fn status(code: u16, headers: &reqwest::header::HeaderMap) -> Result<(), ProviderError> {
    match code {
        200..=299 => Ok(()),
        401 | 403 => Err(ProviderError::Unauthorized(code)),
        413 => Err(ProviderError::PayloadTooLarge),
        429 => Err(headers
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| super::retry::retry_after(h, std::time::SystemTime::now()))
            .map_or(ProviderError::RateLimited, ProviderError::RateLimitedAfter)),
        500..=599 => Err(ProviderError::Server(code)),
        _ => Err(ProviderError::Http { status: code, message: "the provider refused the request".into() }),
    }
}

fn send_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() { ProviderError::Timeout } else { ProviderError::Network("HTTP request failed".into()) }
}

impl Transcriber for Deepgram {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    async fn transcribe(&self, request: &TranscribeRequest<'_>) -> Result<RawTranscript, ProviderError> {
        if request.audio.format != crate::audio::encode::AudioFormat::Wav {
            return Err(ProviderError::BadResponse("Deepgram requires WAV audio".into()));
        }
        let language = match self.profile.language.trim() {
            "" | "auto" => "multi",
            lang => lang,
        };
        let mut url = url::Url::parse(&format!("{}/listen", self.profile.base_url.trim_end_matches('/')))
            .map_err(|_| ProviderError::Network("invalid provider URL".into()))?;
        url.query_pairs_mut().extend_pairs([
            ("model", self.profile.model.as_str()),
            ("language", language),
            ("smart_format", "true"),
            ("punctuate", "true"),
        ]);
        let response = self
            .authenticate(self.client.post(url))
            .header("Content-Type", "audio/wav")
            .body(request.audio.bytes.clone())
            .timeout(request_timeout(BASE_REQUEST_TIMEOUT, request.audio_ms))
            .send()
            .await
            .map_err(send_error)?;
        status(response.status().as_u16(), response.headers())?;
        let value: serde_json::Value =
            response.json().await.map_err(|_| ProviderError::BadResponse("invalid transcription JSON".into()))?;
        let channel = &value["results"]["channels"][0];
        let text = channel["alternatives"][0]["transcript"]
            .as_str()
            .ok_or_else(|| ProviderError::BadResponse("missing transcript".into()))?;
        Ok(RawTranscript {
            text: text.trim().into(),
            language: channel["detected_language"].as_str().and_then(super::language::normalize),
            dropped_segments: 0,
            segments: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::encode::{AudioFormat, encode},
        provider::presets,
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    #[tokio::test]
    async fn native_wav_request_auth_language_and_detection() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/listen")).and(header("Authorization", "Token secret"))
            .and(header("Content-Type", "audio/wav")).and(query_param("model", "nova-3"))
            .and(query_param("language", "multi")).and(query_param("smart_format", "true")).and(query_param("punctuate", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"results":{"channels":[{"detected_language":"tr","alternatives":[{"transcript":"hello"}]}]}})))
            .expect(1).mount(&server).await;
        let mut profile = presets::deepgram();
        profile.base_url = format!("{}/v1", server.uri());
        profile.language = "auto".into();
        let client = Deepgram::new(profile, Some(" secret ".into())).unwrap();
        let audio = encode(&[0.1; 16000], AudioFormat::Wav).unwrap();
        let raw = client
            .transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &[] })
            .await
            .unwrap();
        assert_eq!((raw.text.as_str(), raw.language.as_deref()), ("hello", Some("tr")));
        assert_eq!(server.received_requests().await.unwrap()[0].body, audio.bytes);
    }

    #[tokio::test]
    async fn connection_check_is_read_only_and_http_errors_are_safe() {
        for (code, expected) in [
            (200, "ok"),
            (401, "invalid_key"),
            (403, "invalid_key"),
            (429, "rate_limited"),
            (503, "server"),
            (400, "rejected"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/projects"))
                .respond_with(
                    ResponseTemplate::new(code)
                        .insert_header("Retry-After", "7")
                        .set_body_string("private body secret"),
                )
                .mount(&server)
                .await;
            let mut profile = presets::deepgram();
            profile.base_url = format!("{}/v1", server.uri());
            let result = Deepgram::new(profile, None).unwrap().test_connection().await;
            assert_eq!(result.as_ref().err().map_or("ok", ProviderError::code), expected);
            if let Err(error) = result {
                assert!(!error.to_string().contains("secret"));
                if code == 429 {
                    assert_eq!(error.retry_after().unwrap().as_secs(), 7);
                }
            }
        }
    }
}
