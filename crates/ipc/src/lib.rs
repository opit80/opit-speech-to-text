//! Versioned, bounded local control protocol for Opit Speech to Text.
//! Payloads deliberately have no Debug implementation: responses can contain transcripts.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(windows)]
pub mod windows;

pub const VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 16 * 1024;
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub command: Command,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status,
    Toggle,
    Start,
    Stop,
    Cancel,
    Pause,
    Resume,
    Retry,
    Ask { text: Option<String> },
    Record { seconds: u32 },
    HistoryList { limit: usize, search: Option<String> },
    HistoryRetry { id: i64 },
    Transcribe { path: String, format: Format, timestamps: bool, cleanup: bool },
    ProfilesList,
    ProfileUse { id: String },
    Version,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Txt,
    Srt,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Ok { result: Value },
    Err { error: Error },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &str, message: &str) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl Response {
    pub fn from_result(result: Result<Value, Error>) -> Self {
        Self {
            version: VERSION,
            outcome: match result {
                Ok(result) => Outcome::Ok { result },
                Err(error) => Outcome::Err { error },
            },
        }
    }
}

impl Request {
    pub fn new(command: Command) -> Self {
        Self { version: VERSION, command }
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != VERSION {
            return Err(Error::new("protocol_version", "Unsupported local control protocol version."));
        }
        let valid = match &self.command {
            Command::Ask { text } => {
                text.as_ref().is_none_or(|s| !s.trim().is_empty() && s.len() <= 12 * 1024 && !s.contains('\0'))
            }
            Command::Record { seconds } => (1..=300).contains(seconds),
            Command::HistoryList { limit, search } => {
                (1..=1000).contains(limit) && search.as_ref().is_none_or(|s| s.len() <= 1024 && !s.contains('\0'))
            }
            Command::HistoryRetry { id } => *id > 0,
            Command::ProfileUse { id } => !id.is_empty() && id.len() <= 256 && !id.contains('\0'),
            Command::Transcribe { path, .. } => !path.is_empty() && path.len() <= 8192 && !path.contains('\0'),
            _ => true,
        };
        if valid { Ok(()) } else { Err(Error::new("invalid_input", "Invalid command arguments.")) }
    }
}

pub fn decode_request(bytes: &[u8]) -> Result<Request, Error> {
    if bytes.len() > MAX_REQUEST {
        return Err(Error::new("request_limit", "Local control request is too large."));
    }
    // Never include serde's input-dependent diagnostic in an error or log.
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::new("invalid_request", "Malformed request or unknown verb."))?;
    // Serde's internally tagged unit variants ignore additional fields.
    if let Some(command) = value.get("command").and_then(Value::as_object)
        && let Some(verb) = command.get("verb").and_then(Value::as_str)
        && matches!(
            verb,
            "status"
                | "toggle"
                | "start"
                | "stop"
                | "cancel"
                | "pause"
                | "resume"
                | "retry"
                | "profiles_list"
                | "version"
        )
        && command.len() != 1
    {
        return Err(Error::new("invalid_request", "Unexpected command fields."));
    }
    let request: Request = serde_json::from_value(value)
        .map_err(|_| Error::new("invalid_request", "Malformed request or unknown verb."))?;
    request.validate()?;
    Ok(request)
}

/// Stops allocation at the limit, including when a peer never sends a newline.
pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R, limit: usize) -> Result<Vec<u8>, Error> {
    let mut frame = Vec::new();
    loop {
        let buffer =
            reader.fill_buf().await.map_err(|_| Error::new("transport", "Could not read local control data."))?;
        if buffer.is_empty() {
            return Err(Error::new("invalid_request", "A newline-terminated frame is required."));
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |i| i + 1);
        if frame.len() + count > limit {
            return Err(Error::new("request_limit", "Local control frame is too large."));
        }
        frame.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(frame);
        }
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
    limit: usize,
) -> Result<(), Error> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Error::new("protocol", "Could not encode response."))?;
    if bytes.len() + 1 > limit {
        return Err(Error::new("response_limit", "Local control response is too large; request fewer entries."));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|_| Error::new("transport", "Could not write local control data."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_versions() {
        let request = Request::new(Command::Record { seconds: 3 });
        let mut value = serde_json::to_value(&request).unwrap();
        assert!(decode_request(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["version"] = 2.into();
        assert_eq!(decode_request(&serde_json::to_vec(&value).unwrap()).err().unwrap().code, "protocol_version");
        for result in [Ok(serde_json::json!({"text":"sample"})), Err(Error::new("busy", "Busy."))] {
            let response = Response::from_result(result);
            let wire = serde_json::to_vec(&response).unwrap();
            let decoded: Response = serde_json::from_slice(&wire).unwrap();
            assert_eq!(decoded.version, VERSION);
        }
    }

    #[test]
    fn unknown_fields_verbs_and_invalid_arguments_are_safe() {
        for data in [
            r#"{"version":1,"command":{"verb":"secret text"}}"#,
            r#"{"version":1,"command":{"verb":"status","extra":true}}"#,
            r#"{"version":1,"command":{"verb":"record","seconds":0}}"#,
            r#"{"version":1,"command":{"verb":"history_list","limit":1001,"search":null}}"#,
            r#"{"version":1,"command":{"verb":"history_retry","id":-1}}"#,
        ] {
            let error = decode_request(data.as_bytes()).err().unwrap();
            assert!(!error.message.contains("secret text"));
        }
        assert_eq!(decode_request(&vec![b' '; MAX_REQUEST + 1]).err().unwrap().code, "request_limit");
    }

    #[tokio::test]
    async fn framing_limits_and_missing_newline() {
        assert_eq!(read_frame(&mut &b"12345\n"[..], 5).await.err().unwrap().code, "request_limit");
        assert!(read_frame(&mut &b"{}"[..], 10).await.is_err());
        let mut input = &b"{}\nnext\n"[..];
        assert_eq!(read_frame(&mut input, 3).await.unwrap(), b"{}\n");
        assert_eq!(read_frame(&mut input, 5).await.unwrap(), b"next\n");
        assert!(write_frame(&mut Vec::new(), &"large", 3).await.is_err());
    }
}
