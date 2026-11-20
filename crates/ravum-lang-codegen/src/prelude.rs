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

// The socket is shared (`Arc`) so that an `async` operation can own a handle to
// it without borrowing the Ravum value: the future is `'static`, the Ravum
// value stays usable, and buffered bytes are never lost between the blocking
// and the asynchronous functions (they read the same `BufReader`).
pub struct TcpListener {
    inner: std::sync::Arc<std::net::TcpListener>,
}

pub struct TcpStream {
    shared: std::sync::Arc<std::sync::Mutex<std::io::BufReader<std::net::TcpStream>>>,
}

fn rv_stream(socket: std::net::TcpStream) -> TcpStream {
    TcpStream {
        shared: std::sync::Arc::new(std::sync::Mutex::new(std::io::BufReader::new(socket))),
    }
}

fn rv_reader(
    stream: &TcpStream,
) -> std::sync::MutexGuard<'_, std::io::BufReader<std::net::TcpStream>> {
    stream.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn rv_net_error(what: &str, error: std::io::Error) -> Error {
    Error::new(&format!("{}: {}", what, error))
}

pub fn rv_net_listen(addr: &str) -> Result<TcpListener, Error> {
    match std::net::TcpListener::bind(addr) {
        Ok(inner) => Ok(TcpListener {
            inner: std::sync::Arc::new(inner),
        }),
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
        Ok((stream, _)) => Ok(rv_stream(stream)),
        Err(error) => Err(rv_net_error("cannot accept a connection", error)),
    }
}

pub fn rv_net_connect(addr: &str) -> Result<TcpStream, Error> {
    match std::net::TcpStream::connect(addr) {
        Ok(stream) => Ok(rv_stream(stream)),
        Err(error) => Err(rv_net_error(&format!("cannot connect to `{}`", addr), error)),
    }
}

pub fn rv_net_read_line(stream: &mut TcpStream) -> Result<Option<String>, Error> {
    use std::io::BufRead;
    let mut bytes = Vec::new();
    match rv_reader(stream).read_until(b'\n', &mut bytes) {
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
    match rv_reader(stream).read_exact(&mut bytes) {
        Ok(()) => match String::from_utf8(bytes) {
            Ok(text) => Ok(text),
            Err(_) => Err(Error::new("received bytes are not valid UTF-8")),
        },
        Err(error) => Err(rv_net_error("cannot read from the connection", error)),
    }
}

pub fn rv_net_write_str(stream: &mut TcpStream, text: &str) -> Result<(), Error> {
    use std::io::Write;
    let mut reader = rv_reader(stream);
    let socket = reader.get_mut();
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
    let mut reader = rv_reader(stream);
    let line = match rv_http_line(&mut *reader)? {
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
    let headers = rv_http_headers(&mut *reader)?;
    let body = rv_http_read_body(&mut *reader, &headers, false)?;
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
    let mut reader = rv_reader(&stream);
    let line = match rv_http_line(&mut *reader)? {
        Some(line) => line,
        None => return Err(Error::new("http: connection closed before a response")),
    };
    let status: u16 = match line.split(' ').nth(1).map(|code| code.parse()) {
        Some(Ok(status)) if line.starts_with("HTTP/1.") => status,
        _ => return Err(Error::new("http: malformed status line")),
    };
    let headers = rv_http_headers(&mut *reader)?;
    let body = rv_http_read_body(&mut *reader, &headers, true)?;
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
    /// Thread id under the deterministic scheduler (`rv_det`), if active.
    det_id: Option<u64>,
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.inner.take() {
            rv_det::wait_finished(self.det_id);
            let _ = handle.join();
        }
    }
}

// --- Deterministic scheduler (RFC-DETERMINISTIC-PROFILE-1 DET-3..DET-6) ---
//
// Active only when the process is started with `RAVUM_DET_SEED=<u64>`
// (`ravum-lang run --profile det --seed N`). Then exactly one Ravum thread
// runs at a time; at each synchronisation point (`spawn`, `send`, `recv`,
// `join`, thread end) the next thread is drawn among the runnable ones by a
// SplitMix64 generator seeded with N. Same seed, same binary, same input:
// same interleaving, hence the same output. When no thread can run while
// some wait, the program stops: `panic: deadlock (det, seed=N)`. Without the
// variable nothing changes (the functions below return at once).
mod rv_det {
    use std::cell::Cell;
    use std::collections::BTreeSet;
    use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};

    struct State {
        current: u64,
        runnable: BTreeSet<u64>,
        blocked: BTreeSet<u64>,
        finished: BTreeSet<u64>,
        next: u64,
        rng: u64,
        seed: u64,
    }

    pub struct DetSched {
        state: Mutex<State>,
        turn: Condvar,
    }

    static SCHED: OnceLock<Option<DetSched>> = OnceLock::new();

    thread_local! {
        static ME: Cell<u64> = const { Cell::new(0) };
    }

    pub fn sched() -> Option<&'static DetSched> {
        SCHED
            .get_or_init(|| {
                let text = std::env::var("RAVUM_DET_SEED").ok()?;
                let seed: u64 = match text.parse() {
                    Ok(seed) => seed,
                    Err(_) => panic!("RAVUM_DET_SEED is not a u64: {text:?}"),
                };
                Some(DetSched {
                    state: Mutex::new(State {
                        current: 0,
                        runnable: BTreeSet::from([0]),
                        blocked: BTreeSet::new(),
                        finished: BTreeSet::new(),
                        next: 1,
                        rng: seed,
                        seed,
                    }),
                    turn: Condvar::new(),
                })
            })
            .as_ref()
    }

    /// SplitMix64. Its arithmetic is modulo 2^64 by definition: it is done in
    /// `u128`, where it cannot overflow, then truncated explicitly — the
    /// overflow checks of the program are untouched.
    fn mod64_add(a: u64, b: u64) -> u64 {
        ((a as u128 + b as u128) & 0xFFFF_FFFF_FFFF_FFFF) as u64
    }

    fn mod64_mul(a: u64, b: u64) -> u64 {
        ((a as u128 * b as u128) & 0xFFFF_FFFF_FFFF_FFFF) as u64
    }

    fn splitmix64(x: &mut u64) -> u64 {
        *x = mod64_add(*x, 0x9E37_79B9_7F4A_7C15);
        let mut z = *x;
        z = mod64_mul(z ^ (z >> 30), 0xBF58_476D_1CE4_E5B9);
        z = mod64_mul(z ^ (z >> 27), 0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn me() -> u64 {
        ME.with(|m| m.get())
    }

    impl DetSched {
        fn lock(&self) -> MutexGuard<'_, State> {
            match self.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            }
        }

        /// Choose the next thread; DET-6 when none can run but some wait.
        fn pick<'a>(&'a self, mut st: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
            if st.runnable.is_empty() {
                if st.blocked.is_empty() {
                    return st;
                }
                let seed = st.seed;
                drop(st);
                panic!("deadlock (det, seed={seed})");
            }
            let n = st.runnable.len() as u64;
            let k = (splitmix64(&mut st.rng) % n) as usize;
            st.current = st.runnable.iter().copied().nth(k).unwrap_or(0);
            self.turn.notify_all();
            st
        }

        fn wait_turn<'a>(&'a self, mut st: MutexGuard<'a, State>, id: u64) {
            while st.current != id {
                st = match self.turn.wait(st) {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }
        }

        /// A synchronisation point: let the scheduler choose who runs next.
        pub fn switch(&self) {
            let id = me();
            let st = self.pick(self.lock());
            self.wait_turn(st, id);
        }

        /// Wait for an event (a `send`, a sender dropped, a thread ended).
        pub fn block(&self) {
            let id = me();
            let mut st = self.lock();
            st.runnable.remove(&id);
            st.blocked.insert(id);
            let st = self.pick(st);
            self.wait_turn(st, id);
        }

        /// Something a waiting thread may need happened: all of them retry.
        pub fn event(&self) {
            let mut st = self.lock();
            let woken: Vec<u64> = std::mem::take(&mut st.blocked).into_iter().collect();
            st.runnable.extend(woken);
        }

        pub fn spawn_id(&self) -> u64 {
            let mut st = self.lock();
            let id = st.next;
            st.next += 1;
            st.runnable.insert(id);
            id
        }

        /// First instruction of a spawned thread: wait to be chosen.
        pub fn enter(&self, id: u64) {
            ME.with(|m| m.set(id));
            let st = self.lock();
            self.wait_turn(st, id);
        }

        /// Last instruction of a spawned thread: leave and hand over.
        pub fn exit(&self) {
            let id = me();
            let mut st = self.lock();
            st.runnable.remove(&id);
            st.finished.insert(id);
            let woken: Vec<u64> = std::mem::take(&mut st.blocked).into_iter().collect();
            st.runnable.extend(woken);
            drop(self.pick(st));
        }

        fn is_finished(&self, id: u64) -> bool {
            self.lock().finished.contains(&id)
        }
    }

    /// `join` and `JoinHandle` drop: wait, under the scheduler, until the
    /// thread has left it (its OS thread then only returns its value).
    pub fn wait_finished(id: Option<u64>) {
        if let (Some(s), Some(id)) = (sched(), id) {
            s.switch();
            while !s.is_finished(id) {
                s.block();
            }
        }
    }

    pub fn switch() {
        if let Some(s) = sched() {
            s.switch();
        }
    }

    pub fn event() {
        if let Some(s) = sched() {
            s.event();
        }
    }

    pub fn block() {
        if let Some(s) = sched() {
            s.block();
        }
    }
}

// --- std::task: single-threaded FIFO executor (RFC-E11-CONCURRENCY phase 2) ---

/// Value of a call to an `async fn`: a boxed future, polled by `task::block_on`.
pub type Future<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T>>>;

struct RvNoopWake;

impl std::task::Wake for RvNoopWake {
    fn wake(self: std::sync::Arc<Self>) {}
}

thread_local! {
    /// One FIFO queue of spawned tasks per running `block_on` (innermost last).
    static RV_TASKS: std::cell::RefCell<Vec<std::collections::VecDeque<Future<()>>>> =
        std::cell::RefCell::new(Vec::new());
}

/// Run `main` to completion, then every task it spawned (CONC-6.2): each round
/// polls `main` first, then each ready task once, in spawn order. The order
/// depends only on the program, never on timing.
pub fn rv_task_block_on<T>(main: Future<T>) -> T {
    let waker = std::task::Waker::from(std::sync::Arc::new(RvNoopWake));
    let mut context = std::task::Context::from_waker(&waker);
    RV_TASKS.with(|tasks| tasks.borrow_mut().push(std::collections::VecDeque::new()));
    let mut main = main;
    let mut result: Option<T> = None;
    loop {
        let mut progressed = false;
        RV_IO_WAITING.with(|flag| flag.set(false));
        if result.is_none() {
            if let std::task::Poll::Ready(value) = main.as_mut().poll(&mut context) {
                result = Some(value);
                progressed = true;
            }
        }
        let pending = RV_TASKS.with(|tasks| tasks.borrow().last().map_or(0, |q| q.len()));
        for _ in 0..pending {
            let next = RV_TASKS.with(|tasks| tasks.borrow_mut().last_mut().and_then(|q| q.pop_front()));
            if let Some(mut task) = next {
                let pending = task.as_mut().poll(&mut context).is_pending();
                progressed |= !pending;
                if pending {
                    RV_TASKS.with(|tasks| {
                        if let Some(queue) = tasks.borrow_mut().last_mut() {
                            queue.push_back(task);
                        }
                    });
                }
            }
        }
        // Every task waits on a socket: sleep instead of spinning. A round with
        // a completion, or with a task that only yielded, does not sleep.
        if !progressed && RV_IO_WAITING.with(|flag| flag.get()) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let idle = RV_TASKS.with(|tasks| tasks.borrow().last().map_or(true, |q| q.is_empty()));
        if idle {
            if let Some(value) = result.take() {
                RV_TASKS.with(|tasks| {
                    tasks.borrow_mut().pop();
                });
                return value;
            }
        }
    }
}

/// Queue a task on the innermost running `task::block_on`.
pub fn rv_task_spawn(task: Future<()>) {
    let queued = RV_TASKS.with(|tasks| match tasks.borrow_mut().last_mut() {
        Some(queue) => {
            queue.push_back(task);
            true
        }
        None => false,
    });
    if !queued {
        panic!("task::spawn outside task::block_on");
    }
}

struct RvYield(bool);

impl std::future::Future for RvYield {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            std::task::Poll::Ready(())
        } else {
            self.0 = true;
            context.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }
}

thread_local! {
    /// Set by an I/O future that returned `Pending` because the socket was not
    /// ready; read by the executor to decide whether a round may sleep.
    static RV_IO_WAITING: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

fn rv_io_wait<T>(context: &mut std::task::Context<'_>) -> std::task::Poll<T> {
    RV_IO_WAITING.with(|flag| flag.set(true));
    context.waker().wake_by_ref();
    std::task::Poll::Pending
}

fn rv_would_block(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
}

fn rv_line_text(mut bytes: Vec<u8>) -> Result<Option<String>, Error> {
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

struct RvAccept {
    listener: std::sync::Arc<std::net::TcpListener>,
}

impl std::future::Future for RvAccept {
    type Output = Result<TcpStream, Error>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if let Err(error) = self.listener.set_nonblocking(true) {
            return std::task::Poll::Ready(Err(rv_net_error("cannot accept a connection", error)));
        }
        let accepted = self.listener.accept();
        let _ = self.listener.set_nonblocking(false);
        match accepted {
            Ok((socket, _)) => {
                // Some systems hand out the accepted socket non-blocking.
                let _ = socket.set_nonblocking(false);
                std::task::Poll::Ready(Ok(rv_stream(socket)))
            }
            Err(error) if rv_would_block(&error) => rv_io_wait(context),
            Err(error) => {
                std::task::Poll::Ready(Err(rv_net_error("cannot accept a connection", error)))
            }
        }
    }
}

/// `net::accept_async`: a connection, without blocking the other tasks.
pub fn rv_net_accept_async(listener: &TcpListener) -> Future<Result<TcpStream, Error>> {
    Box::pin(RvAccept {
        listener: listener.inner.clone(),
    })
}

/// `net::connect_async`: the connection itself is established on the first
/// poll, blocking (no non-blocking `connect` in `std`); reads and writes on the
/// stream never block the executor.
pub fn rv_net_connect_async(addr: &str) -> Future<Result<TcpStream, Error>> {
    let addr = addr.to_string();
    Box::pin(async move { rv_net_connect(&addr) })
}

struct RvReadLine {
    shared: std::sync::Arc<std::sync::Mutex<std::io::BufReader<std::net::TcpStream>>>,
    partial: Vec<u8>,
}

impl std::future::Future for RvReadLine {
    type Output = Result<Option<String>, Error>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::io::BufRead;
        let this = self.get_mut();
        let shared = this.shared.clone();
        let mut reader = shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = reader.get_ref().set_nonblocking(true) {
            return std::task::Poll::Ready(Err(rv_net_error("cannot read from the connection", error)));
        }
        let outcome = loop {
            let (found, used) = match reader.fill_buf() {
                Ok(buffer) if buffer.is_empty() => {
                    let bytes = std::mem::take(&mut this.partial);
                    break if bytes.is_empty() {
                        std::task::Poll::Ready(Ok(None))
                    } else {
                        std::task::Poll::Ready(rv_line_text(bytes))
                    };
                }
                Ok(buffer) => match buffer.iter().position(|b| *b == b'\n') {
                    Some(at) => {
                        this.partial.extend_from_slice(&buffer[..=at]);
                        (true, at + 1)
                    }
                    None => {
                        this.partial.extend_from_slice(buffer);
                        (false, buffer.len())
                    }
                },
                Err(error) if rv_would_block(&error) => break rv_io_wait(context),
                Err(error) => {
                    break std::task::Poll::Ready(Err(rv_net_error(
                        "cannot read from the connection",
                        error,
                    )))
                }
            };
            reader.consume(used);
            if found {
                break std::task::Poll::Ready(rv_line_text(std::mem::take(&mut this.partial)));
            }
        };
        let _ = reader.get_ref().set_nonblocking(false);
        outcome
    }
}

/// `net::read_line_async`: the next line, `None` at end of stream.
pub fn rv_net_read_line_async(stream: &mut TcpStream) -> Future<Result<Option<String>, Error>> {
    Box::pin(RvReadLine {
        shared: stream.shared.clone(),
        partial: Vec::new(),
    })
}

struct RvWrite {
    shared: std::sync::Arc<std::sync::Mutex<std::io::BufReader<std::net::TcpStream>>>,
    data: Vec<u8>,
    written: usize,
}

impl std::future::Future for RvWrite {
    type Output = Result<(), Error>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::io::Write;
        let this = self.get_mut();
        let shared = this.shared.clone();
        let mut reader = shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let socket = reader.get_mut();
        if let Err(error) = socket.set_nonblocking(true) {
            return std::task::Poll::Ready(Err(rv_net_error("cannot write to the connection", error)));
        }
        let outcome = loop {
            if this.written == this.data.len() {
                break match socket.flush() {
                    Ok(()) => std::task::Poll::Ready(Ok(())),
                    Err(error) if rv_would_block(&error) => rv_io_wait(context),
                    Err(error) => std::task::Poll::Ready(Err(rv_net_error(
                        "cannot write to the connection",
                        error,
                    ))),
                };
            }
            match socket.write(&this.data[this.written..]) {
                Ok(0) => {
                    break std::task::Poll::Ready(Err(Error::new(
                        "cannot write to the connection: closed",
                    )))
                }
                Ok(count) => this.written += count,
                Err(error) if rv_would_block(&error) => break rv_io_wait(context),
                Err(error) => {
                    break std::task::Poll::Ready(Err(rv_net_error(
                        "cannot write to the connection",
                        error,
                    )))
                }
            }
        };
        let _ = socket.set_nonblocking(false);
        outcome
    }
}

/// `net::write_str_async`: the whole text, without blocking the other tasks.
pub fn rv_net_write_str_async(stream: &mut TcpStream, text: &str) -> Future<Result<(), Error>> {
    Box::pin(RvWrite {
        shared: stream.shared.clone(),
        data: text.as_bytes().to_vec(),
        written: 0,
    })
}

/// A future that is pending once: the other ready tasks run before it resumes.
pub fn rv_task_yield_now() -> Future<()> {
    Box::pin(RvYield(false))
}

pub fn rv_thread_spawn<A: Send + 'static, R: Send + 'static>(
    worker: fn(A) -> R,
    arg: A,
) -> JoinHandle<R> {
    match rv_det::sched() {
        None => JoinHandle {
            inner: Some(std::thread::spawn(move || worker(arg))),
            det_id: None,
        },
        Some(s) => {
            let id = s.spawn_id();
            let inner = std::thread::spawn(move || {
                s.enter(id);
                let value = worker(arg);
                s.exit();
                value
            });
            s.switch();
            JoinHandle {
                inner: Some(inner),
                det_id: Some(id),
            }
        }
    }
}

/// A panic in any thread ends the process through the run-mode hook
/// (`panic: <message>`, exit code 1), so `join` only sees finished threads.
pub fn rv_thread_join<T>(mut handle: JoinHandle<T>) -> T {
    match handle.inner.take() {
        Some(inner) => match {
            rv_det::wait_finished(handle.det_id);
            inner.join()
        } {
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

impl<T> Drop for Sender<T> {
    /// A receiver waiting under the deterministic scheduler may now see the
    /// end of the channel.
    fn drop(&mut self) {
        rv_det::event();
    }
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
    let sent = sender.inner.send(value).is_ok();
    rv_det::event();
    rv_det::switch();
    sent
}

pub fn rv_sync_recv<T>(receiver: &Receiver<T>) -> Option<T> {
    if rv_det::sched().is_none() {
        return receiver.inner.recv().ok();
    }
    rv_det::switch();
    loop {
        match receiver.inner.try_recv() {
            Ok(value) => return Some(value),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
            Err(std::sync::mpsc::TryRecvError::Empty) => rv_det::block(),
        }
    }
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

/// Reload runtime (RFC-E11-HOT-RELOAD), appended after [`PRELUDE`] only when a
/// program is built with `GenOptions::reload`. Safe Rust only, like the
/// prelude. It defines the canonical state text (HR-1.1, §3) and the line
/// protocol that `ravum-lang watch --reload` speaks with the module.
pub const RELOAD_PRELUDE: &str = r##"// ---------------------------------------------------------------------------
// Ravum reload runtime (generated; do not edit by hand). Safe Rust only.
// ---------------------------------------------------------------------------

/// Canonical text of a state value: one line, fields in schema order.
pub trait RvCanon: Sized {
    fn rv_put(&self, out: &mut String);
    fn rv_get(input: &mut RvReader) -> Result<Self, String>;
}

pub struct RvReader {
    chars: Vec<char>,
    pos: usize,
}

impl RvReader {
    pub fn new(text: &str) -> RvReader {
        RvReader { chars: text.chars().collect(), pos: 0 }
    }
    pub fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    pub fn next_char(&mut self) -> Result<char, String> {
        let c = self.peek().ok_or_else(|| "unexpected end of state text".to_string())?;
        self.pos += 1;
        Ok(c)
    }
    pub fn expect(&mut self, wanted: char) -> Result<(), String> {
        let c = self.next_char()?;
        if c == wanted {
            Ok(())
        } else {
            Err(format!("expected `{}` at {}, found `{}`", wanted, self.pos - 1, c))
        }
    }
    pub fn token(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' {
                text.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        text
    }
    pub fn at_end(&self) -> bool {
        self.pos == self.chars.len()
    }
}

macro_rules! rv_canon_int {
    ($($t:ty),*) => {$(
        impl RvCanon for $t {
            fn rv_put(&self, out: &mut String) {
                out.push_str(&self.to_string());
            }
            fn rv_get(input: &mut RvReader) -> Result<Self, String> {
                let token = input.token();
                token.parse::<$t>().map_err(|_| format!("invalid integer `{}`", token))
            }
        }
    )*};
}
rv_canon_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl RvCanon for f32 {
    fn rv_put(&self, out: &mut String) {
        out.push_str(&format!("x{:08x}", self.to_bits()));
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        input.expect('x')?;
        let token = input.token();
        u32::from_str_radix(&token, 16).map(f32::from_bits).map_err(|_| format!("invalid f32 `{}`", token))
    }
}

impl RvCanon for f64 {
    fn rv_put(&self, out: &mut String) {
        out.push_str(&format!("x{:016x}", self.to_bits()));
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        input.expect('x')?;
        let token = input.token();
        u64::from_str_radix(&token, 16).map(f64::from_bits).map_err(|_| format!("invalid f64 `{}`", token))
    }
}

impl RvCanon for bool {
    fn rv_put(&self, out: &mut String) {
        out.push(if *self { 't' } else { 'f' });
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        match input.next_char()? {
            't' => Ok(true),
            'f' => Ok(false),
            other => Err(format!("invalid bool `{}`", other)),
        }
    }
}

impl RvCanon for () {
    fn rv_put(&self, out: &mut String) {
        out.push('u');
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        input.expect('u')
    }
}

fn rv_put_text(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
}

fn rv_get_text(input: &mut RvReader) -> Result<String, String> {
    input.expect('"')?;
    let mut text = String::new();
    loop {
        match input.next_char()? {
            '"' => return Ok(text),
            '\\' => match input.next_char()? {
                '\\' => text.push('\\'),
                '"' => text.push('"'),
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                other => return Err(format!("invalid escape `\\{}`", other)),
            },
            c => text.push(c),
        }
    }
}

impl RvCanon for String {
    fn rv_put(&self, out: &mut String) {
        rv_put_text(self, out);
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        rv_get_text(input)
    }
}

impl RvCanon for char {
    fn rv_put(&self, out: &mut String) {
        rv_put_text(&self.to_string(), out);
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        let text = rv_get_text(input)?;
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(c),
            _ => Err("invalid char".to_string()),
        }
    }
}

fn rv_get_items<T: RvCanon>(input: &mut RvReader) -> Result<Vec<T>, String> {
    input.expect('[')?;
    let mut items = Vec::new();
    while input.peek() != Some(']') {
        items.push(T::rv_get(input)?);
        input.expect(';')?;
    }
    input.expect(']')?;
    Ok(items)
}

impl<T: RvCanon> RvCanon for Vec<T> {
    fn rv_put(&self, out: &mut String) {
        out.push('[');
        for item in self {
            item.rv_put(out);
            out.push(';');
        }
        out.push(']');
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        rv_get_items(input)
    }
}

impl<T: RvCanon, const N: usize> RvCanon for [T; N] {
    fn rv_put(&self, out: &mut String) {
        out.push('[');
        for item in self.iter() {
            item.rv_put(out);
            out.push(';');
        }
        out.push(']');
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        let items: Vec<T> = rv_get_items(input)?;
        let found = items.len();
        items.try_into().map_err(|_| format!("expected {} array items, found {}", N, found))
    }
}

impl<T: RvCanon> RvCanon for Option<T> {
    fn rv_put(&self, out: &mut String) {
        match self {
            None => out.push('N'),
            Some(value) => {
                out.push('S');
                value.rv_put(out);
            }
        }
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        match input.next_char()? {
            'N' => Ok(None),
            'S' => Ok(Some(T::rv_get(input)?)),
            other => Err(format!("invalid option tag `{}`", other)),
        }
    }
}

impl<T: RvCanon, E: RvCanon> RvCanon for Result<T, E> {
    fn rv_put(&self, out: &mut String) {
        match self {
            Ok(value) => {
                out.push('O');
                value.rv_put(out);
            }
            Err(value) => {
                out.push('E');
                value.rv_put(out);
            }
        }
    }
    fn rv_get(input: &mut RvReader) -> Result<Self, String> {
        match input.next_char()? {
            'O' => Ok(Ok(T::rv_get(input)?)),
            'E' => Ok(Err(E::rv_get(input)?)),
            other => Err(format!("invalid result tag `{}`", other)),
        }
    }
}

macro_rules! rv_canon_tuple {
    ($($name:ident),+) => {
        impl<$($name: RvCanon),+> RvCanon for ($($name,)+) {
            fn rv_put(&self, out: &mut String) {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                out.push('(');
                $( $name.rv_put(out); out.push(';'); )+
                out.push(')');
            }
            fn rv_get(input: &mut RvReader) -> Result<Self, String> {
                input.expect('(')?;
                let value = ($( { let item = $name::rv_get(input)?; input.expect(';')?; item }, )+);
                input.expect(')')?;
                Ok(value)
            }
        }
    };
}
rv_canon_tuple!(A);
rv_canon_tuple!(A, B);
rv_canon_tuple!(A, B, C);
rv_canon_tuple!(A, B, C, D);
rv_canon_tuple!(A, B, C, D, E);
rv_canon_tuple!(A, B, C, D, E, F);

/// Enum tag: variant index, then `(`.
pub fn rv_get_tag(input: &mut RvReader) -> Result<usize, String> {
    let token = input.token();
    let tag = token.parse::<usize>().map_err(|_| format!("invalid variant tag `{}`", token))?;
    input.expect('(')?;
    Ok(tag)
}

pub fn rv_canon_text<T: RvCanon>(value: &T) -> String {
    let mut out = String::new();
    value.rv_put(&mut out);
    out
}

pub fn rv_canon_parse<T: RvCanon>(text: &str) -> Result<T, String> {
    let mut input = RvReader::new(text);
    let value = T::rv_get(&mut input)?;
    if input.at_end() {
        Ok(value)
    } else {
        Err("trailing text after the state".to_string())
    }
}

/// Marker of a protocol reply on stdout; any other line is program output.
const RV_REPLY: &str = "\u{1}rvr ";

fn rv_reply(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}{}", RV_REPLY, text);
    let _ = out.flush();
}

fn rv_one_line(text: &str) -> String {
    text.replace('\n', " ").replace('\r', " ")
}

/// Run `body` and report a panic as `Err(message)` (the test hook captures it).
fn rv_guard<R>(body: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => Ok(value),
        Err(payload) => Err(rv_take_test_panic().unwrap_or_else(|| rv_panic_text(&*payload))),
    }
}

/// Module side of the reload protocol. One command per stdin line; one reply
/// per command. The state lives here between two `step` commands (HR-1.2); a
/// panic in `step` or `migrate` leaves the previous state in place (HR-1.5).
pub fn rv_reload_main<S: RvCanon + Clone, O: RvCanon>(
    schema: &str,
    old_schema: Option<&str>,
    init: fn() -> S,
    step: fn(S, String) -> S,
    migrate: Option<fn(O) -> S>,
) -> std::process::ExitCode {
    crate::rv_install_test_panic_hook();
    let stdin = std::io::stdin();
    let mut state: Option<S> = None;
    let mut line = String::new();
    loop {
        line.clear();
        match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
            Ok(0) | Err(_) => return std::process::ExitCode::SUCCESS,
            Ok(_) => {}
        }
        let command = line.strip_suffix('\n').unwrap_or(&line);
        let command = command.strip_suffix('\r').unwrap_or(command);
        let (verb, rest) = match command.find(' ') {
            Some(at) => (&command[..at], &command[at + 1..]),
            None => (command, ""),
        };
        match verb {
            "schema" => rv_reply(&format!("schema {} {}", schema, old_schema.unwrap_or("-"))),
            "init" => match rv_guard(init) {
                Ok(value) => {
                    state = Some(value);
                    rv_reply("ok");
                }
                Err(text) => rv_reply(&format!("panic {}", rv_one_line(&text))),
            },
            "load" => match rv_canon_parse::<S>(rest) {
                Ok(value) => {
                    state = Some(value);
                    rv_reply("ok");
                }
                Err(text) => rv_reply(&format!("err {}", rv_one_line(&text))),
            },
            "migrate" => match (migrate, rv_canon_parse::<O>(rest)) {
                (None, _) => rv_reply("err this module has no `migrate`"),
                (Some(_), Err(text)) => rv_reply(&format!("err {}", rv_one_line(&text))),
                (Some(convert), Ok(old)) => match rv_guard(move || convert(old)) {
                    Ok(value) => {
                        state = Some(value);
                        rv_reply("ok");
                    }
                    Err(text) => rv_reply(&format!("panic {}", rv_one_line(&text))),
                },
            },
            "step" => match &state {
                None => rv_reply("err no state"),
                Some(current) => {
                    let before = current.clone();
                    let event = rest.to_string();
                    match rv_guard(move || step(before, event)) {
                        Ok(value) => {
                            state = Some(value);
                            rv_reply("ok");
                        }
                        Err(text) => rv_reply(&format!("panic {}", rv_one_line(&text))),
                    }
                }
            },
            "dump" => match &state {
                None => rv_reply("err no state"),
                Some(current) => rv_reply(&format!("state {}", rv_canon_text(current))),
            },
            "quit" => return std::process::ExitCode::SUCCESS,
            other => rv_reply(&format!("err unknown command `{}`", rv_one_line(other))),
        }
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
        // The reload runtime is safe Rust too, and holds the protocol entry.
        assert!(!super::RELOAD_PRELUDE.contains("unsafe"));
        assert!(!super::RELOAD_PRELUDE.contains("*mut "));
        assert!(super::RELOAD_PRELUDE.contains("pub fn rv_reload_main<"));
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
