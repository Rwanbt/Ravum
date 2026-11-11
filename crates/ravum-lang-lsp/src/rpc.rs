//! LSP base-protocol framing (JSON-RPC 2.0 over a byte stream).
//!
//! The server implements the *base protocol* itself rather than depending on an
//! external LSP crate: the framing is small, fully specified, and keeping it in
//! tree means the wire format is auditable and the crate has no network-build
//! dependency. A malformed header is an I/O error, never a silently dropped
//! message.

use serde_json::Value;
use std::io::{self, BufRead, Write};

/// One decoded JSON-RPC envelope.
#[derive(Debug, Clone)]
pub enum Message {
    /// A call that expects a response (`id` present).
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A fire-and-forget call (`id` absent).
    Notification { method: String, params: Value },
    /// A response to a server-initiated request (unsupported here, tolerated).
    Response { id: Value },
}

fn protocol_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Read one framed message. Returns `Ok(None)` on a clean EOF.
pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            // EOF before any header byte: normal end of stream.
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            match content_length {
                Some(_) => break,
                None => return Err(protocol_error("empty header block without Content-Length")),
            }
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            let parsed = value
                .trim()
                .parse::<usize>()
                .map_err(|_| protocol_error(format!("invalid Content-Length: {trimmed}")))?;
            // Hard cap: a hostile or broken client cannot ask for a huge buffer.
            if parsed > 64 * 1024 * 1024 {
                return Err(protocol_error(format!(
                    "Content-Length {parsed} exceeds the 64 MiB limit"
                )));
            }
            content_length = Some(parsed);
        } else if trimmed.starts_with("Content-Type:") {
            // Optional and ignored, as the specification allows.
        } else {
            return Err(protocol_error(format!("unsupported header: {trimmed}")));
        }
    }
    let length = content_length.ok_or_else(|| protocol_error("missing Content-Length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|error| protocol_error(format!("invalid JSON body: {error}")))?;
    Ok(Some(value))
}

/// Classify a decoded envelope.
pub fn classify(value: &Value) -> Option<Message> {
    let object = value.as_object()?;
    let method = object.get("method")?.as_str()?.to_string();
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match object.get("id") {
        Some(id) => Some(Message::Request {
            id: id.clone(),
            method,
            params,
        }),
        None => Some(Message::Notification { method, params }),
    }
}

pub fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}
