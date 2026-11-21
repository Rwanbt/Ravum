use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
use std::env;
use std::http;
use std::io;
use std::net;
use std::task;

fn answer(body: String, first: String, last: String) -> String effects(alloc) {
    let mut text = body;
    text.push_str(first.as_str());
    text.push_str(last.as_str());
    return text;
}

fn serve_blocking(mut stream: TcpStream) -> Result<(), Error> effects(net, alloc) {
    let req = http::read_request(&mut stream)?;
    let first = net::read_exact(&mut stream, 2usize)?;
    let last = net::read_exact(&mut stream, 2usize)?;
    let text = answer(http::body(&req), first, last);
    return http::respond(&mut stream, 200u16, "text/plain", text.as_str());
}

async fn progress() -> () effects(io, async) {
    task::yield_now().await;
    io::println("other task advanced");
}

async fn serve_async(mut stream: TcpStream) -> Result<(), Error> effects(io, net, async, alloc) {
    task::spawn(progress());
    let req = http::read_request_async(&mut stream).await?;
    let first = net::read_exact_async(&mut stream, 2usize).await?;
    let last = net::read_exact(&mut stream, 2usize)?;
    let text = answer(http::body(&req), first, last);
    return http::respond_async(&mut stream, 200u16, "text/plain", text.as_str()).await;
}

async fn timeout_then_read(mut stream: TcpStream) -> Result<(), Error> effects(io, net, async, nondet, alloc) {
    io::println("timeout started");
    let timed: Option<Result<String, Error>> = task::timeout(80u64, net::read_exact_async(&mut stream, 4usize)).await;
    if timed.is_some() { return Err(Error::new("stalled read finished before timeout")); }
    io::println("socket timeout");
    let text = net::read_exact_async(&mut stream, 4usize).await?;
    return http::respond_async(&mut stream, 200u16, "text/plain", text.as_str()).await;
}

fn main() -> Result<(), Error> effects(io, net, async, nondet, alloc, panic) {
    let mut args = env::args();
    let mode = args.pop().unwrap();
    let addr = args.pop().unwrap();
    let stream = net::connect(addr.as_str())?;
    if mode.as_str() == "async" {
        return task::block_on(serve_async(stream));
    }
    if mode.as_str() == "timeout" {
        return task::block_on(timeout_then_read(stream));
    }
    return serve_blocking(stream);
}
"#;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) && self.0.kill().is_ok() {
            self.0.wait().expect("reap test server");
        }
    }
}

fn build() -> PathBuf {
    let project = std::env::temp_dir().join(format!("ravum-async-http-{}", std::process::id()));
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("ravum.toml"),
        "[package]\nname = \"async-http-test\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.rv"), SOURCE).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["build", "."])
        .current_dir(project)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("built: "))
        .unwrap()
        .into()
}

fn transact(
    bin: &PathBuf,
    mode: &str,
    request: &[u8],
    split: Option<usize>,
) -> (bool, Vec<u8>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut server = Server(
        Command::new(bin)
            .arg(listener.local_addr().unwrap().to_string())
            .arg(mode)
            .stdout(if split.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let timeout = Duration::from_secs(5);
    let deadline = Instant::now() + timeout;
    let (mut peer, accepted) = loop {
        match listener.accept() {
            Ok((peer, _)) => break (peer, Instant::now()),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("server connection: {error}"),
        }
    };
    // On macOS (BSD) an accepted socket inherits the listener's
    // non-blocking flag: make the peer blocking again, bounded by the
    // timeouts below, or `read_to_end` returns `WouldBlock` early.
    peer.set_nonblocking(false).unwrap();
    peer.set_read_timeout(Some(timeout)).unwrap();
    peer.set_write_timeout(Some(timeout)).unwrap();
    if let Some(at) = split {
        peer.write_all(&request[..at]).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let stdout = server.0.stdout.take().unwrap();
        let lines = if mode == "timeout" { 2 } else { 1 };
        let worker = std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            for _ in 0..lines {
                let mut line = String::new();
                stdout.read_line(&mut line).unwrap();
                send.send((Instant::now(), line))
                    .expect("deliver the task progress marker");
            }
        });
        let (start, marker) = receive.recv_timeout(timeout).unwrap();
        if mode == "timeout" {
            assert_eq!(marker, "timeout started\n");
            let (end, marker) = receive.recv_timeout(timeout).unwrap();
            assert_eq!(marker, "socket timeout\n");
            let elapsed = end.duration_since(start);
            // Lower bound: measured from the accept, which precedes the
            // timer's creation (the program connects, prints, then starts
            // the 80 ms timeout). The marker's timestamp is taken by a
            // reader thread and can be late, which would make a correct
            // timer look early (74 ms seen on macOS CI).
            let since_accept = end.duration_since(accepted);
            assert!(
                since_accept >= Duration::from_millis(75),
                "80 ms timer fired early: {since_accept:?} after the accept"
            );
            assert!(
                elapsed <= Duration::from_millis(90),
                "80 ms timeout took {elapsed:?}"
            );
            eprintln!("80 ms timeout observed after {elapsed:?}");
        } else {
            assert_eq!(marker, "other task advanced\n");
        }
        worker.join().unwrap();
        peer.write_all(&request[at..]).unwrap();
    } else {
        peer.write_all(request).unwrap();
    }
    peer.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    peer.read_to_end(&mut response).unwrap();
    let status = loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "server did not exit");
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut error = String::new();
    server
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    (status.success(), response, error)
}

#[test]
fn blocking_and_async_http_match_and_preserve_buffered_bytes() {
    let bin = build();
    let request = b"POST /notes HTTP/1.1\r\nContent-Length: 4\r\n\r\nnoteABCD";
    let blocking = transact(&bin, "blocking", request, None);
    let asynchronous = transact(&bin, "async", request, Some(request.len() - 6));
    assert!(blocking.0, "{blocking:?}");
    assert_eq!(asynchronous, blocking);
    assert!(blocking.1.ends_with(b"\r\n\r\nnoteABCD"));

    let timed = transact(&bin, "timeout", b"late", Some(0));
    assert!(timed.0, "{timed:?}");
    assert!(timed.1.ends_with(b"\r\n\r\nlate"));

    for (request, expected) in [
        (
            format!("GET /{} HTTP/1.1\r\n\r\n", "x".repeat(8193)).into_bytes(),
            "line too long or truncated",
        ),
        (
            format!("GET / HTTP/1.1\r\n{}\r\n", "X: y\r\n".repeat(101)).into_bytes(),
            "too many headers",
        ),
        (
            b"POST / HTTP/1.1\r\nContent-Length: 16777217\r\n\r\n".to_vec(),
            "body too large",
        ),
        (
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\n\r\n\xff".to_vec(),
            "body is not valid UTF-8",
        ),
    ] {
        let blocking = transact(&bin, "blocking", &request, None);
        let asynchronous = transact(&bin, "async", &request, None);
        assert!(!blocking.0 && blocking.2.contains(expected), "{blocking:?}");
        assert_eq!(asynchronous, blocking);
    }
}
