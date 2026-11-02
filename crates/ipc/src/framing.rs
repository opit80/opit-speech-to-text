use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::Error;

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
