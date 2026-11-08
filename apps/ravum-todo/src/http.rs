//! Minimal HTTP/1.1 request parsing and response writing (M4 demo server).
//!
//! Deliberately small: one request per connection, `Content-Length` bodies only, no
//! chunked encoding, no keep-alive, no TLS. That is enough to exercise the vertical
//! slice and it is stated in the report instead of pretending otherwise.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Path without query string.
    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec()),
        }
    }

    pub fn text(status: u16, content_type: &'static str, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type,
            body: body.into().into_bytes(),
        }
    }

    pub fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }
}

/// Error code returned when a request cannot even be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooLarge,
    Incomplete,
    Malformed(String),
}

/// Diagnostic aid, disabled by default. `RAVUM_TODO_HTTP_DEBUG=1` prints the raw
/// request bytes whenever parsing fails; the browser E2E harness uses it to explain
/// intermittent malformed requests instead of guessing.
fn debug_raw(tag: &str, bytes: &[u8]) {
    if std::env::var_os("RAVUM_TODO_HTTP_DEBUG").is_some() {
        eprintln!(
            "ravum-todo http debug [{tag}]: {:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// Read one request. Returns `Ok(None)` on a clean EOF before any byte.
pub fn read_request(stream: &TcpStream) -> Result<Option<Request>, ParseError> {
    let mut reader = BufReader::new(stream);
    let mut header_block = Vec::new();
    let mut total = 0usize;

    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|e| {
            if std::env::var_os("RAVUM_TODO_HTTP_DEBUG").is_some() {
                eprintln!(
                    "ravum-todo http debug [read_line-error] peer={:?} error={e} raw={:?}",
                    stream.peer_addr(),
                    String::from_utf8_lossy(&header_block)
                );
            }
            ParseError::Malformed(e.to_string())
        })?;
        if read == 0 {
            if header_block.is_empty() {
                return Ok(None);
            }
            debug_raw("incomplete-headers", &header_block);
            return Err(ParseError::Incomplete);
        }
        total += read;
        if total > MAX_HEADER_BYTES {
            debug_raw("headers-too-large", &header_block);
            return Err(ParseError::TooLarge);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        header_block.extend_from_slice(line.as_bytes());
    }

    let text = String::from_utf8(header_block.clone()).map_err(|_| {
        debug_raw("non-utf8-headers", &header_block);
        ParseError::Malformed("headers are not UTF-8".to_owned())
    })?;
    let mut lines = text.lines();
    let request_line = lines.next().ok_or_else(|| {
        debug_raw("missing-request-line", text.as_bytes());
        ParseError::Malformed("missing request line".to_owned())
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| {
            debug_raw("missing-method", text.as_bytes());
            ParseError::Malformed("missing method".to_owned())
        })?
        .to_uppercase();
    let path = parts
        .next()
        .ok_or_else(|| {
            debug_raw("missing-path", text.as_bytes());
            ParseError::Malformed("missing path".to_owned())
        })?
        .to_owned();

    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(|| {
            debug_raw("bad-header-line", text.as_bytes());
            ParseError::Malformed(format!("bad header line: {line:?}"))
        })?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }

    let mut request = Request {
        method,
        path,
        headers,
        body: Vec::new(),
    };

    let length = request
        .header("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        debug_raw("body-too-large", text.as_bytes());
        return Err(ParseError::TooLarge);
    }
    if length > 0 {
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).map_err(|e| {
            debug_raw("short-body", text.as_bytes());
            ParseError::Malformed(e.to_string())
        })?;
        request.body = body;
    }
    Ok(Some(request))
}

pub fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    // RFC 7230 §3.3.2: a 204 response MUST NOT carry Content-Length (nor a body).
    // Sending `Content-Length: 0` made Chromium report the completed DELETE as
    // `net::ERR_ABORTED` (canceled=true) in the CDP browser harness; omitting the
    // entity headers makes the 204 spec-compliant and the request clean.
    let mut head = if response.status == 204 {
        format!(
            "HTTP/1.1 {} {}\r\nConnection: close\r\n\r\n",
            response.status, reason
        )
        .into_bytes()
    } else {
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            reason,
            response.content_type,
            response.body.len()
        )
        .into_bytes()
    };
    head.extend_from_slice(&response.body);
    stream.write_all(&head)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn parse(raw: &str) -> Request {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(raw.as_bytes()).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        read_request(&server_side).unwrap().unwrap()
    }

    #[test]
    fn parses_a_request_with_a_body() {
        let request = parse(
            "POST /api/tasks HTTP/1.1\r\nHost: x\r\nContent-Length: 7\r\n\r\n{\"a\":1}",
        );
        assert_eq!(request.method, "POST");
        assert_eq!(request.route(), "/api/tasks");
        assert_eq!(request.body, b"{\"a\":1}");
        assert_eq!(request.header("host"), Some("x"));
    }

    #[test]
    fn routes_strip_the_query_string() {
        let request = parse("GET /api/tasks?limit=5 HTTP/1.1\r\n\r\n");
        assert_eq!(request.route(), "/api/tasks");
    }

    #[test]
    fn method_is_upper_cased() {
        let request = parse("get /health HTTP/1.1\r\n\r\n");
        assert_eq!(request.method, "GET");
    }

    #[test]
    fn a_missing_header_line_is_malformed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nbroken-header\r\n\r\n").unwrap();
        let (server_side, _) = listener.accept().unwrap();
        assert!(matches!(
            read_request(&server_side),
            Err(ParseError::Malformed(_))
        ));
    }
}
