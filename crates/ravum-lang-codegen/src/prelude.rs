//! Ravum runtime prelude, embedded verbatim in every generated program.
//!
//! OWNER: worker `codegen`. The prelude must compile with `rustc --edition 2021`
//! without external crates. It defines: panic hook and exit convention, print
//! helpers, argument/environment access, file helpers, `Error`, assertions and
//! the test runner support. It never exposes `unsafe` to generated user code —
//! and contains none at all.
//!
//! Semantics fixed by `docs/architecture/RAVUM-CORE-DESIGN.md` §3.8:
//! * run mode panic: `panic: <message>` on stderr, exit code `1`;
//! * `main -> Result<(), Error>`: `Err` prints `error: <message>`, exit code `1`;
//! * assertions report the Ravum `file:line` decided by the generator;
//! * every I/O error keeps the operating-system message (`std::io::Error`
//!   `Display`), never a bare code.
//!
//! The runtime performs no effect checking and no overflow instrumentation: the
//! checker decides effects and the CLI compiles with `-C overflow-checks=on`.

/// Runtime prelude source, inserted verbatim after the crate-level attributes.
pub const PRELUDE: &str = r##"// ---------------------------------------------------------------------------
// Ravum Core 1.0 runtime prelude (generated; do not edit by hand).
//
// Safe Rust only: no `unsafe`, no raw pointers, no `transmute`. Generated
// programs are compiled with `-C overflow-checks=on`; this runtime never
// weakens a check.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: &str) -> Error {
        Error {
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Human-readable text of a panic payload (safe: no `unsafe`, no raw pointers).
fn rv_panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic".to_string()
    }
}

/// Run-mode panic hook: `panic: <message>` on stderr, then exit code 1.
pub fn rv_install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = rv_panic_text(info.payload());
        eprintln!("panic: {}", text);
        std::process::exit(1);
    }));
}

thread_local! {
    static RV_TEST_PANIC: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
}

/// Test-mode panic hook: capture the message and let unwinding continue so the
/// generated runner can report the failure. Never exits the process.
pub fn rv_install_test_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = rv_panic_text(info.payload());
        RV_TEST_PANIC.with(|slot| {
            *slot.borrow_mut() = Some(text);
        });
    }));
}

fn rv_take_test_panic() -> Option<String> {
    RV_TEST_PANIC.with(|slot| slot.borrow_mut().take())
}

/// Run one generated test function; print `test <name> ... ok|FAILED`.
pub fn rv_test(name: &str, test: fn()) -> bool {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
    match outcome {
        Ok(()) => {
            println!("test {} ... ok", name);
            true
        }
        Err(payload) => {
            let text = rv_take_test_panic().unwrap_or_else(|| rv_panic_text(&*payload));
            println!("test {} ... FAILED", name);
            println!("    panicked: {}", text);
            false
        }
    }
}

/// Run one generated `test fn` that returns `Result<(), Error>`:
/// `Ok(())` succeeds, `Err(error)` fails with the error message, a panic fails.
pub fn rv_test_result(name: &str, test: fn() -> Result<(), Error>) -> bool {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
    match outcome {
        Ok(Ok(())) => {
            println!("test {} ... ok", name);
            true
        }
        Ok(Err(error)) => {
            println!("test {} ... FAILED", name);
            println!("    error: {}", error.message);
            false
        }
        Err(payload) => {
            let text = rv_take_test_panic().unwrap_or_else(|| rv_panic_text(&*payload));
            println!("test {} ... FAILED", name);
            println!("    panicked: {}", text);
            false
        }
    }
}

pub fn rv_println(text: &str) {
    println!("{}", text);
}

pub fn rv_print(text: &str) {
    print!("{}", text);
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

pub fn rv_eprintln(text: &str) {
    eprintln!("{}", text);
}

pub fn rv_eprint(text: &str) {
    eprint!("{}", text);
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

pub fn rv_read_line() -> Result<String, Error> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(line)
        }
        Err(error) => Err(Error::new(&format!("cannot read standard input: {}", error))),
    }
}

pub fn rv_args() -> Vec<String> {
    std::env::args().collect()
}

pub fn rv_env_var(name: &str) -> Result<String, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(error) => Err(Error::new(&format!(
            "cannot read environment variable `{}`: {}",
            name, error
        ))),
    }
}

pub fn rv_fs_read_to_string(path: &str) -> Result<String, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) => Err(Error::new(&format!(
            "cannot read file `{}`: {}",
            path, error
        ))),
    }
}

pub fn rv_fs_write_string(path: &str, contents: &str) -> Result<(), Error> {
    match std::fs::write(path, contents) {
        Ok(()) => Ok(()),
        Err(error) => Err(Error::new(&format!(
            "cannot write file `{}`: {}",
            path, error
        ))),
    }
}

pub fn rv_fs_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

pub struct TcpListener {
    inner: std::net::TcpListener,
}

pub struct TcpStream {
    reader: std::io::BufReader<std::net::TcpStream>,
}

fn rv_net_error(what: &str, error: std::io::Error) -> Error {
    Error::new(&format!("{}: {}", what, error))
}

pub fn rv_net_listen(addr: &str) -> Result<TcpListener, Error> {
    match std::net::TcpListener::bind(addr) {
        Ok(inner) => Ok(TcpListener { inner }),
        Err(error) => Err(rv_net_error(&format!("cannot listen on `{}`", addr), error)),
    }
}

pub fn rv_net_local_port(listener: &TcpListener) -> u16 {
    match listener.inner.local_addr() {
        Ok(addr) => addr.port(),
        Err(_) => 0,
    }
}

pub fn rv_net_accept(listener: &TcpListener) -> Result<TcpStream, Error> {
    match listener.inner.accept() {
        Ok((stream, _)) => Ok(TcpStream {
            reader: std::io::BufReader::new(stream),
        }),
        Err(error) => Err(rv_net_error("cannot accept a connection", error)),
    }
}

pub fn rv_net_connect(addr: &str) -> Result<TcpStream, Error> {
    match std::net::TcpStream::connect(addr) {
        Ok(stream) => Ok(TcpStream {
            reader: std::io::BufReader::new(stream),
        }),
        Err(error) => Err(rv_net_error(&format!("cannot connect to `{}`", addr), error)),
    }
}

pub fn rv_net_read_line(stream: &mut TcpStream) -> Result<Option<String>, Error> {
    use std::io::BufRead;
    let mut bytes = Vec::new();
    match stream.reader.read_until(b'\n', &mut bytes) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
            }
            match String::from_utf8(bytes) {
                Ok(line) => Ok(Some(line)),
                Err(_) => Err(Error::new("received bytes are not valid UTF-8")),
            }
        }
        Err(error) => Err(rv_net_error("cannot read from the connection", error)),
    }
}

pub fn rv_net_read_exact(stream: &mut TcpStream, count: usize) -> Result<String, Error> {
    use std::io::Read;
    let mut bytes = vec![0u8; count];
    match stream.reader.read_exact(&mut bytes) {
        Ok(()) => match String::from_utf8(bytes) {
            Ok(text) => Ok(text),
            Err(_) => Err(Error::new("received bytes are not valid UTF-8")),
        },
        Err(error) => Err(rv_net_error("cannot read from the connection", error)),
    }
}

pub fn rv_net_write_str(stream: &mut TcpStream, text: &str) -> Result<(), Error> {
    use std::io::Write;
    let socket = stream.reader.get_mut();
    match socket.write_all(text.as_bytes()).and_then(|_| socket.flush()) {
        Ok(()) => Ok(()),
        Err(error) => Err(rv_net_error("cannot write to the connection", error)),
    }
}

/// An HTTP/1.1 request read from a connection (`std::http`, RFC-E11-NET-HTTP §4).
pub struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

/// An HTTP/1.1 response received by `http::get` / `http::post`.
pub struct HttpResponse {
    status: u16,
    body: String,
}

const RV_HTTP_MAX_LINE: u64 = 8 * 1024;
const RV_HTTP_MAX_HEADERS: usize = 100;
const RV_HTTP_MAX_BODY: usize = 16 * 1024 * 1024;

/// One CRLF- or LF-terminated line of at most `RV_HTTP_MAX_LINE` bytes.
fn rv_http_line(reader: &mut std::io::BufReader<std::net::TcpStream>) -> Result<Option<String>, Error> {
    use std::io::{BufRead, Read};
    let mut bytes = Vec::new();
    match reader.by_ref().take(RV_HTTP_MAX_LINE + 1).read_until(b'\n', &mut bytes) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(rv_net_error("cannot read from the connection", error)),
    }
    if bytes.last() != Some(&b'\n') {
        return Err(Error::new("http: line too long or truncated"));
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    match String::from_utf8(bytes) {
        Ok(line) => Ok(Some(line)),
        Err(_) => Err(Error::new("http: header bytes are not valid UTF-8")),
    }
}

/// Header lines up to the blank line; names are kept as received.
fn rv_http_headers(
    reader: &mut std::io::BufReader<std::net::TcpStream>,
) -> Result<Vec<(String, String)>, Error> {
    let mut headers = Vec::new();
    loop {
        let line = match rv_http_line(reader)? {
            Some(line) => line,
            None => return Err(Error::new("http: connection closed inside the headers")),
        };
        if line.is_empty() {
            return Ok(headers);
        }
        if headers.len() == RV_HTTP_MAX_HEADERS {
            return Err(Error::new("http: too many headers"));
        }
        match line.split_once(':') {
            Some((name, value)) => headers.push((name.trim().to_string(), value.trim().to_string())),
            None => return Err(Error::new("http: malformed header line")),
        }
    }
}

fn rv_http_find(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

/// The body announced by `Content-Length` (0 when absent), or `until_eof`.
fn rv_http_read_body(
    reader: &mut std::io::BufReader<std::net::TcpStream>,
    headers: &[(String, String)],
    until_eof: bool,
) -> Result<String, Error> {
    use std::io::Read;
    if rv_http_find(headers, "transfer-encoding").is_some() {
        return Err(Error::new("http: Transfer-Encoding is not supported"));
    }
    let mut bytes = Vec::new();
    match rv_http_find(headers, "content-length") {
        Some(text) => {
            let length: usize = match text.parse() {
                Ok(length) => length,
                Err(_) => return Err(Error::new("http: invalid Content-Length")),
            };
            if length > RV_HTTP_MAX_BODY {
                return Err(Error::new("http: body too large"));
            }
            bytes.resize(length, 0);
            if let Err(error) = reader.read_exact(&mut bytes) {
                return Err(rv_net_error("cannot read the body", error));
            }
        }
        None if until_eof => {
            if let Err(error) = reader
                .by_ref()
                .take(RV_HTTP_MAX_BODY as u64 + 1)
                .read_to_end(&mut bytes)
            {
                return Err(rv_net_error("cannot read the body", error));
            }
            if bytes.len() > RV_HTTP_MAX_BODY {
                return Err(Error::new("http: body too large"));
            }
        }
        None => {}
    }
    match String::from_utf8(bytes) {
        Ok(body) => Ok(body),
        Err(_) => Err(Error::new("http: body is not valid UTF-8")),
    }
}

pub fn rv_http_read_request(stream: &mut TcpStream) -> Result<HttpRequest, Error> {
    let line = match rv_http_line(&mut stream.reader)? {
        Some(line) => line,
        None => return Err(Error::new("http: connection closed before a request")),
    };
    let mut parts = line.split(' ');
    let (method, path, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(p), Some(v), None) if !m.is_empty() && p.starts_with('/') => (m, p, v),
        _ => return Err(Error::new("http: malformed request line")),
    };
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(Error::new("http: unsupported HTTP version"));
    }
    let (method, path) = (method.to_string(), path.to_string());
    let headers = rv_http_headers(&mut stream.reader)?;
    let body = rv_http_read_body(&mut stream.reader, &headers, false)?;
    Ok(HttpRequest { method, path, headers, body })
}

pub fn rv_http_method(request: &HttpRequest) -> String {
    request.method.clone()
}

pub fn rv_http_path(request: &HttpRequest) -> String {
    request.path.clone()
}

pub fn rv_http_header(request: &HttpRequest, name: &str) -> Option<String> {
    rv_http_find(&request.headers, name)
}

pub fn rv_http_body(request: &HttpRequest) -> String {
    request.body.clone()
}

fn rv_http_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// One complete response, then the connection is meant to close (HTTP-1.4).
pub fn rv_http_respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), Error> {
    if !(100..=999).contains(&status) || content_type.contains(['\r', '\n']) {
        return Err(Error::new("http: invalid status or content type"));
    }
    let text = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        rv_http_reason(status),
        content_type,
        body.len(),
        body
    );
    rv_net_write_str(stream, &text)
}

fn rv_http_send(url: &str, method: &str, content_type: &str, body: &str) -> Result<HttpResponse, Error> {
    if url.starts_with("https://") {
        return Err(Error::new("http: https is not supported (no TLS in Core 1.0)"));
    }
    let rest = match url.strip_prefix("http://") {
        Some(rest) => rest,
        None => return Err(Error::new("http: the URL must start with http://")),
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() || content_type.contains(['\r', '\n']) {
        return Err(Error::new("http: invalid URL or content type"));
    }
    let address = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{}:80", authority)
    };
    let mut stream = rv_net_connect(&address)?;
    let request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        method,
        path,
        authority,
        content_type,
        body.len(),
        body
    );
    rv_net_write_str(&mut stream, &request)?;
    let line = match rv_http_line(&mut stream.reader)? {
        Some(line) => line,
        None => return Err(Error::new("http: connection closed before a response")),
    };
    let status: u16 = match line.split(' ').nth(1).map(|code| code.parse()) {
        Some(Ok(status)) if line.starts_with("HTTP/1.") => status,
        _ => return Err(Error::new("http: malformed status line")),
    };
    let headers = rv_http_headers(&mut stream.reader)?;
    let body = rv_http_read_body(&mut stream.reader, &headers, true)?;
    Ok(HttpResponse { status, body })
}

pub fn rv_http_get(url: &str) -> Result<HttpResponse, Error> {
    rv_http_send(url, "GET", "text/plain", "")
}

pub fn rv_http_post(url: &str, content_type: &str, body: &str) -> Result<HttpResponse, Error> {
    rv_http_send(url, "POST", content_type, body)
}

pub fn rv_http_status(response: &HttpResponse) -> u16 {
    response.status
}

pub fn rv_http_response_body(response: &HttpResponse) -> String {
    response.body.clone()
}

/// A running thread (`std::thread`, RFC-E11-CONCURRENCY). Dropping a handle
/// that was not joined joins it: no thread outlives its handle (CONC-3.5).
pub struct JoinHandle<T> {
    inner: Option<std::thread::JoinHandle<T>>,
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.inner.take() {
            let _ = handle.join();
        }
    }
}

pub fn rv_thread_spawn<A: Send + 'static, R: Send + 'static>(
    worker: fn(A) -> R,
    arg: A,
) -> JoinHandle<R> {
    JoinHandle {
        inner: Some(std::thread::spawn(move || worker(arg))),
    }
}

/// A panic in any thread ends the process through the run-mode hook
/// (`panic: <message>`, exit code 1), so `join` only sees finished threads.
pub fn rv_thread_join<T>(mut handle: JoinHandle<T>) -> T {
    match handle.inner.take() {
        Some(inner) => match inner.join() {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        None => panic!("thread handle already joined"),
    }
}

pub struct Channel<T> {
    sender: std::sync::mpsc::Sender<T>,
    receiver: std::sync::mpsc::Receiver<T>,
}

pub struct Sender<T> {
    inner: std::sync::mpsc::Sender<T>,
}

pub struct Receiver<T> {
    inner: std::sync::mpsc::Receiver<T>,
}

pub fn rv_sync_channel<T>() -> Channel<T> {
    let (sender, receiver) = std::sync::mpsc::channel();
    Channel { sender, receiver }
}

pub fn rv_sync_sender<T>(channel: &Channel<T>) -> Sender<T> {
    Sender {
        inner: channel.sender.clone(),
    }
}

pub fn rv_sync_clone_sender<T>(sender: &Sender<T>) -> Sender<T> {
    Sender {
        inner: sender.inner.clone(),
    }
}

/// Consumes the channel: its own sender is dropped, so `recv` returns `None`
/// once every `Sender` taken from it is gone (CONC-4.2).
pub fn rv_sync_receiver<T>(channel: Channel<T>) -> Receiver<T> {
    Receiver {
        inner: channel.receiver,
    }
}

pub fn rv_sync_send<T>(sender: &Sender<T>, value: T) -> bool {
    sender.inner.send(value).is_ok()
}

pub fn rv_sync_recv<T>(receiver: &Receiver<T>) -> Option<T> {
    receiver.inner.recv().ok()
}

/// An opaque C pointer (`std::ffi`, RFC-E11-SQLITE NAT-3.3). Ravum code can
/// only receive it from an `extern` function, pass it to another, or compare
/// it with null; it is stored as an address so this runtime holds no raw
/// pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CHandle(pub usize);

pub fn rv_ffi_null() -> CHandle {
    CHandle(0)
}

pub fn rv_ffi_is_null(handle: CHandle) -> bool {
    handle.0 == 0
}

pub fn rv_exit(code: i32) -> ! {
    std::process::exit(code)
}

pub fn rv_panic(message: &str) -> ! {
    panic!("{}", message)
}

pub fn rv_assert(condition: bool, file: &str, line: u32) {
    if !condition {
        panic!("assertion failed at {}:{}", file, line);
    }
}

pub fn rv_assert_msg(condition: bool, message: &str, file: &str, line: u32) {
    if !condition {
        panic!("assertion failed at {}:{}: {}", file, line, message);
    }
}

pub fn rv_assert_eq<T: std::fmt::Debug + PartialEq>(left: &T, right: &T, file: &str, line: u32) {
    if left != right {
        panic!(
            "assertion failed: left == right at {}:{}\n  left: {:?}\n right: {:?}",
            file, line, left, right
        );
    }
}

/// `assert_eq` for Ravum text comparisons between two different Rust types
/// (`String` versus `&str`): Rust's `PartialEq` cross-implementations apply.
pub fn rv_assert_eq_cross<A, B>(left: &A, right: &B, file: &str, line: u32)
where
    A: std::fmt::Debug + PartialEq<B> + ?Sized,
    B: std::fmt::Debug + ?Sized,
{
    if left != right {
        panic!(
            "assertion failed: left == right at {}:{}\n  left: {:?}\n right: {:?}",
            file, line, left, right
        );
    }
}
"##;

#[cfg(test)]
mod tests {
    use super::PRELUDE;

    #[test]
    fn prelude_is_embedded_and_safe() {
        assert!(PRELUDE.contains("pub struct Error"));
        assert!(PRELUDE.contains("pub fn rv_assert("));
        assert!(PRELUDE.contains("pub fn rv_assert_eq<"));
        assert!(PRELUDE.contains("pub fn rv_test("));
        assert!(PRELUDE.contains("pub fn rv_test_result("));
        for forbidden in ["unsafe fn", "unsafe {", "transmute(", "*const ", "*mut "] {
            assert!(
                !PRELUDE.contains(forbidden),
                "prelude must not use `{forbidden}`"
            );
        }
        assert!(PRELUDE.ends_with('\n'));
    }

    #[test]
    fn prelude_does_not_weaken_overflow_checks() {
        for forbidden in [
            "wrapping_add",
            "wrapping_sub",
            "wrapping_mul",
            "overflow-checks=off",
            "overflow_checks=off",
        ] {
            assert!(
                !PRELUDE.contains(forbidden),
                "prelude must not weaken overflow checks: {forbidden}"
            );
        }
    }
}
