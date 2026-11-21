//! RFC-ASYNC-NET-1 ANET-1 (amended, V2) / ANET-T1: `connect_async` does not
//! block the executor. A local listener whose accept queue is full drops new
//! SYNs, so a connection to it hangs; while it hangs, another task must keep
//! running and `task::timeout` must end the wait. Before the amendment the
//! first poll blocked the whole executor for the system TCP timeout.

use std::fs;
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"use std::io;
use std::net;
use std::task;
use std::env;

async fn ticker() -> () effects(io, async) {
    let mut n: i64 = 0;
    while n < 100 {
        task::yield_now().await;
        n = n + 1;
    }
    io::println("ticker finished");
}

async fn go(addr: String) -> () effects(io, net, async, nondet, alloc) {
    task::spawn(ticker());
    match task::timeout(1500u64, net::connect_async(addr.as_str())).await {
        Some(Ok(s)) => io::println("connect: connected"),
        Some(Err(e)) => io::println("connect: refused"),
        None => io::println("connect: timed out"),
    }
}

fn main() -> () effects(io, net, async, nondet, alloc, panic) {
    let mut args = env::args();
    let addr = args.pop().unwrap();
    task::block_on(go(addr));
}
"#;

/// Fill the accept queue of a listener nobody accepts on; returns the held
/// connections (kept open) once one more connection no longer completes.
fn saturate(addr: SocketAddr) -> Option<Vec<TcpStream>> {
    let mut held = Vec::new();
    for _ in 0..4096 {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
            Ok(s) => held.push(s),
            Err(_) => return Some(held),
        }
    }
    None
}

#[test]
fn connect_async_lets_other_tasks_run_and_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let held =
        saturate(addr).expect("the accept queue never filled: no hanging connect on this host");
    let dir = std::env::temp_dir().join(format!("ravum-connect-helper-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"connect-helper\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.rv"), SOURCE).unwrap();
    let cli = env!("CARGO_BIN_EXE_ravum-lang");
    let build = Command::new(cli)
        .args(["build", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let stdout = String::from_utf8(build.stdout).unwrap();
    let artifact = stdout
        .lines()
        .find_map(|l| l.strip_prefix("built: "))
        .unwrap()
        .to_string();

    let start = Instant::now();
    let mut child = Command::new(&artifact)
        .arg(addr.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("connect_async blocked the executor (no result in 30 s)");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let elapsed = start.elapsed();
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert_eq!(
        out, "ticker finished\nconnect: timed out\n",
        "after {elapsed:?}"
    );
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    drop(held);
    drop(listener);
}

const HTTP_SOURCE: &str = r#"use std::io;
use std::http;
use std::task;
use std::env;

async fn ticker() -> () effects(io, async) {
    task::yield_now().await;
    io::println("ticker ran");
}

async fn go(base: String) -> () effects(io, net, async, alloc) {
    task::spawn(ticker());
    let mut url: String = base.clone();
    url.push_str("/get");
    match http::get_async(url.as_str()).await {
        Ok(r) => {
            io::println(&http::status(&r).to_string());
            io::println(&http::response_body(&r));
        }
        Err(e) => io::println("get failed"),
    }
    let mut url2: String = base.clone();
    url2.push_str("/post");
    match http::post_async(url2.as_str(), "text/plain", "abc").await {
        Ok(r) => io::println(&http::response_body(&r)),
        Err(e) => io::println("post failed"),
    }
    match http::get_async("https://example.invalid/").await {
        Ok(r) => io::println("https accepted"),
        Err(e) => io::println("https refused"),
    }
}

fn main() -> () effects(io, net, async, alloc, panic) {
    let mut args = env::args();
    let base = args.pop().unwrap();
    task::block_on(go(base));
}
"#;

/// ANET-2: `get_async`/`post_async` give the answers of `get`/`post` (status,
/// body, `https://` refused) and another task runs while the server delays.
#[test]
fn http_get_and_post_async_answer_without_blocking_other_tasks() {
    use std::io::{BufRead, BufReader, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut length = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; length];
            std::io::Read::read_exact(&mut reader, &mut body).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            let answer = if line.starts_with("GET /get ") {
                "got".to_string()
            } else {
                format!("posted {}", String::from_utf8(body).unwrap())
            };
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!("ravum-http-async-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"http-async\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.rv"), HTTP_SOURCE).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["run", ".", "--"])
        .arg(format!("http://127.0.0.1:{port}"))
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ticker ran\n201\ngot\nposted abc\nhttps refused\n"
    );
    server.join().unwrap();
}
