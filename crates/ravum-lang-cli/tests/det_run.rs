//! RFC-DETERMINISTIC-PROFILE-1, step 2: `run --profile det --seed N`.
//! DET-T1 same seed => same output; DET-T2 several seeds explore several
//! interleavings; DET-T4 a deadlock stops with the seed instead of hanging;
//! DET-1 still refuses inputs; without `det` the program runs as before.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Three threads send two values each; `main` prints them in arrival order,
/// which depends on the interleaving.
const RACE: &str = r#"use std::io;
use std::sync;
use std::thread;

fn worker(job: (Sender<i64>, i64)) -> () effects(thread, alloc) {
    match job {
        (tx, id) => {
            let a: bool = sync::send(&tx, id * 10 + 1);
            let b: bool = sync::send(&tx, id * 10 + 2);
            drop(a);
            drop(b);
        }
    }
}

fn main() -> () effects(io, thread, alloc, panic) {
    let ch: Channel<i64> = sync::channel();
    let t1: JoinHandle<()> = thread::spawn(worker, (sync::sender(&ch), 1));
    let t2: JoinHandle<()> = thread::spawn(worker, (sync::sender(&ch), 2));
    let t3: JoinHandle<()> = thread::spawn(worker, (sync::sender(&ch), 3));
    let rx: Receiver<i64> = sync::receiver(ch);
    let mut line: String = String::new();
    let mut done: bool = false;
    while !done {
        match sync::recv(&rx) {
            Some(v) => {
                line.push_str(v.to_string().as_str());
                line.push_str(" ");
            }
            None => {
                done = true;
            }
        }
    }
    thread::join(t1);
    thread::join(t2);
    thread::join(t3);
    io::println(line.as_str());
}
"#;

/// `main` waits on a channel whose only sender it holds itself.
const DEADLOCK: &str = r#"use std::io;
use std::sync;

fn main() -> () effects(io, thread, alloc, panic) {
    let ch: Channel<i64> = sync::channel();
    let tx: Sender<i64> = sync::sender(&ch);
    let rx: Receiver<i64> = sync::receiver(ch);
    match sync::recv(&rx) {
        Some(v) => io::println(&v.to_string()),
        None => io::println("closed"),
    }
    drop(tx);
}
"#;

/// A test whose assertion depends on the interleaving (DET-T3).
const ORDER_TEST: &str = r#"use std::sync;
use std::thread;

fn worker(job: (Sender<i64>, i64)) -> () effects(thread, alloc) {
    match job {
        (tx, id) => {
            let sent: bool = sync::send(&tx, id);
            drop(sent);
        }
    }
}

fn main() {
}


test fn first_message_comes_from_worker_one() {
    let ch: Channel<i64> = sync::channel();
    let a: JoinHandle<()> = thread::spawn(worker, (sync::sender(&ch), 1));
    let b: JoinHandle<()> = thread::spawn(worker, (sync::sender(&ch), 2));
    let rx: Receiver<i64> = sync::receiver(ch);
    let first: Option<i64> = sync::recv(&rx);
    thread::join(a);
    thread::join(b);
    assert_eq(first.unwrap_or(0), 1);
}
"#;

/// Reads a file and an environment variable, writes a file, prints (DET-9..13).
const IO_PROGRAM: &str = r#"use std::env;
use std::fs;
use std::io;

fn main() -> Result<(), Error> effects(io, fs, alloc) {
    let text: String = fs::read_to_string("input.txt")?;
    let who: String = env::var("RR_WHO")?;
    let mut out: String = String::from("hello ");
    out.push_str(who.as_str());
    out.push_str(": ");
    out.push_str(text.as_str());
    fs::write_string("out.txt", out.as_str())?;
    io::println(out.as_str());
    Ok(())
}
"#;

fn package(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ravum-det-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    fs::write(dir.join("src/main.rv"), source).unwrap();
    dir
}

fn run(dir: &Path, extra: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(dir)
        .args(extra)
        .output()
        .expect("ravum-lang runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_same_seed_gives_the_same_output_and_seeds_explore_orders() {
    let dir = package("race", RACE);
    let mut orders = std::collections::BTreeSet::new();
    for seed in 0..12u64 {
        let seed_text = seed.to_string();
        let (code, first, err) = run(&dir, &["--profile", "det", "--seed", &seed_text]);
        assert_eq!(code, 0, "seed {seed}: {err}");
        assert!(err.contains(&format!("profile det: seed={seed}")), "{err}");
        for _ in 0..4 {
            let (code, again, _) = run(&dir, &["--profile", "det", "--seed", &seed_text]);
            assert_eq!(code, 0);
            assert_eq!(again, first, "seed {seed} is not reproducible");
        }
        let mut values: Vec<&str> = first.split_whitespace().collect();
        values.sort_unstable();
        assert_eq!(
            values,
            ["11", "12", "21", "22", "31", "32"],
            "every value once"
        );
        orders.insert(first);
    }
    assert!(
        orders.len() >= 2,
        "12 seeds gave a single order: {orders:?}"
    );
    // Without `det` the program still runs (OS scheduling, order unspecified).
    let (code, plain, err) = run(&dir, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(plain.split_whitespace().count(), 6);
    assert!(!err.contains("profile det"));
}

#[test]
fn a_deadlock_stops_with_its_seed_instead_of_hanging() {
    let dir = package("deadlock", DEADLOCK);
    let started = Instant::now();
    let (code, _, err) = run(&dir, &["--profile", "det", "--seed", "7"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("panic: deadlock (det, seed=7)"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(120));
}

#[test]
fn det_still_refuses_inputs_and_bad_arguments() {
    let dir = package(
        "det-stdin",
        "use std::io;\n\nfn main() {\n    let l = io::read_line();\n}\n",
    );
    let (code, _, err) = run(&dir, &["--profile", "det"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("E-EFF-PROFILE"), "{err}");
    let race = package("det-args", RACE);
    for (args, why) in [
        (&["--seed", "3"][..], "--seed without --profile det"),
        (&["--profile", "pure"][..], "run takes only the det profile"),
        (&["--profile", "det", "--seed", "x"][..], "non-numeric seed"),
    ] {
        let (code, _, err) = run(&race, args);
        assert_eq!(code, 2, "{why}: {err}");
    }
}

#[test]
fn test_seeds_finds_an_order_dependent_failure_and_names_its_seed() {
    let dir = package("det-seeds", ORDER_TEST);
    let test = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .arg("test")
            .arg(&dir)
            .args(args)
            .output()
            .expect("ravum-lang runs");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (code, err) = test(&["--profile", "det", "--seeds", "20"]);
    assert_eq!(code, 1, "{err}");
    let seed: u64 = err
        .split("profile det: seed=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no failing seed reported: {err}"));
    // The reported command reproduces the failure, every time.
    let again = format!("{}", seed + 1);
    for _ in 0..3 {
        let (code, err2) = test(&["--profile", "det", "--seeds", &again]);
        assert_eq!(code, 1, "{err2}");
        assert!(
            err2.contains(&format!("profile det: seed={seed} failed")),
            "{err2}"
        );
    }
    // The seeds before it pass.
    if seed > 0 {
        let (code, err3) = test(&["--profile", "det", "--seeds", &seed.to_string()]);
        assert_eq!(code, 0, "{err3}");
        assert!(err3.contains("seed(s) passed"), "{err3}");
    }
    let (code, _) = test(&["--seeds", "3"]);
    assert_eq!(code, 2, "--seeds without --profile det is a usage error");
}

fn run_env(dir: &Path, extra: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ravum-lang"));
    command.arg("run").arg(dir).args(extra).current_dir(dir);
    for (k, v) in env {
        command.env(k, v);
    }
    let out = command.output().expect("ravum-lang runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn record_then_replay_serves_the_recorded_inputs_and_performs_no_io() {
    let dir = package("det-record", IO_PROGRAM);
    fs::write(dir.join("input.txt"), "first").unwrap();
    let journal = dir.join("s.rvlog");
    let journal_arg = journal.to_string_lossy().into_owned();
    // Without a journal, det refuses fs (DET-1).
    let (code, _, err) = run_env(&dir, &["--profile", "det"], &[("RR_WHO", "ana")]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("E-EFF-PROFILE"), "{err}");

    let (code, recorded, err) = run_env(
        &dir,
        &["--profile", "det", "--record", &journal_arg],
        &[("RR_WHO", "ana")],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(recorded, "hello ana: first\n");
    assert!(
        err.contains("it contains the data the program reads"),
        "{err}"
    );
    let text = fs::read_to_string(&journal).unwrap();
    assert!(
        text.starts_with("ravum-rvlog-1\tseed=0\tbuild=sha256:"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 4, "header + 3 events: {text}");

    // Replay: inputs changed, output file removed, variable unset.
    fs::write(dir.join("input.txt"), "CHANGED").unwrap();
    fs::remove_file(dir.join("out.txt")).unwrap();
    let (code, replayed, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(replayed, recorded, "DET-3/DET-10: same output");
    assert!(
        !dir.join("out.txt").exists(),
        "DET-12: a replayed write is not performed"
    );

    // DET-11: a tampered argument digest stops the replay at its rank.
    let tampered: String = text
        .lines()
        .map(|l| {
            if l.starts_with("1\t") {
                let f: Vec<&str> = l.splitn(4, '\t').collect();
                format!("{}\t{}\t0000000000000000\t{}", f[0], f[1], f[3])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&journal, tampered + "\n").unwrap();
    let (code, _, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("panic: replay diverged at #1"), "{err}");

    // DET-11: a truncated journal (header + first event) ends the replay.
    let truncated: String = text.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(&journal, truncated).unwrap();
    let (code, _, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("the journal has ended"), "{err}");

    // DET-13: a journal of another program is refused before running.
    let other = text.replacen("build=sha256:", "build=sha256:00", 1);
    fs::write(&journal, other).unwrap();
    let (code, _, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("refusing to replay (DET-13)"), "{err}");

    // Usage: --replay with --seed, --record without det.
    let (code, _, _) = run_env(
        &dir,
        &["--profile", "det", "--seed", "1", "--replay", &journal_arg],
        &[],
    );
    assert_eq!(code, 2);
    let (code, _, _) = run_env(&dir, &["--record", &journal_arg], &[]);
    assert_eq!(code, 2);
}

/// DET-T8 / DET-9..DET-12 for `net`: an HTTP server records the request it
/// receives from a real client; the replay serves the same request from the
/// journal with no client at all (a real `accept` would wait forever).
const NET_SERVER: &str = r#"use std::io;
use std::net;
use std::http;

fn main() -> Result<(), Error> effects(io, net, alloc) {
    let listener: TcpListener = net::listen("127.0.0.1:0")?;
    let mut line: String = String::from("listening ");
    line.push_str(&net::local_port(&listener).to_string());
    io::println(&line);
    let mut stream: TcpStream = net::accept(&listener)?;
    let request: HttpRequest = http::read_request(&mut stream)?;
    let mut seen: String = http::method(&request);
    seen.push_str(" ");
    seen.push_str(&http::path(&request));
    seen.push_str(" ");
    seen.push_str(&http::body(&request));
    io::println(&seen);
    http::respond(&mut stream, 200, "text/plain", "pong")?;
    Ok(())
}
"#;

#[test]
fn a_recorded_http_server_replays_without_any_socket() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let dir = package("det-net", NET_SERVER);
    let journal = dir.join("net.rvlog");
    let journal_arg = journal.to_string_lossy().into_owned();
    // Without a journal, det still refuses net (DET-1).
    let (code, _, err) = run_env(&dir, &["--profile", "det"], &[]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("E-EFF-PROFILE"), "{err}");

    let mut child = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .args(["--profile", "det", "--record", &journal_arg])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ravum-lang runs");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    let port: u16 = first
        .trim()
        .strip_prefix("listening ")
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port: {first:?}"));
    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .write_all(b"POST /ping HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello")
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("\r\n\r\npong"), "{response}");
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    let status = child.wait().unwrap();
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(status.success(), "{err}");
    let recorded = format!("{first}{rest}");
    assert_eq!(rest, "POST /ping hello\n");
    let text = fs::read_to_string(&journal).unwrap();
    for name in [
        "net::listen",
        "net::local_port",
        "net::accept",
        "http::read_request",
        "http::respond",
    ] {
        assert!(
            text.contains(&format!("\t{name}\t")),
            "{name} not journaled: {text}"
        );
    }

    // Replay: no client exists; a real accept would block forever.
    let started = Instant::now();
    let (code, replayed, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(replayed, recorded, "DET-10: same output, port included");
    assert!(started.elapsed() < Duration::from_secs(120));

    // DET-11: a different response body diverges at http::respond.
    let tampered = NET_SERVER.replace("\"pong\"", "\"PONG\"");
    let other = package("det-net-tampered", &tampered);
    let header_free = fs::read_to_string(&journal).unwrap();
    let journal2 = other.join("net.rvlog");
    fs::write(&journal2, &header_free).unwrap();
    let (code, _, err) = run_env(
        &other,
        &["--profile", "det", "--replay", &journal2.to_string_lossy()],
        &[],
    );
    assert_ne!(
        code, 0,
        "a journal of another program must not replay: {err}"
    );
}

/// DET-9..DET-11 for the `*_async` forms: an asynchronous HTTP server is
/// recorded with a real client, then replayed with none; a journal whose
/// response was edited diverges instead of replaying.
const ASYNC_SERVER: &str = "use std::io;\nuse std::net;\nuse std::http;\nuse std::task;\n\nasync fn serve(listener: TcpListener) -> Result<(), Error> effects(io, net, async, alloc) {\n    let mut stream: TcpStream = net::accept_async(&listener).await?;\n    let request: HttpRequest = http::read_request_async(&mut stream).await?;\n    io::println(&http::path(&request));\n    http::respond_async(&mut stream, 200, \"text/plain\", \"pong\").await\n}\n\nfn main() -> Result<(), Error> effects(io, net, async, alloc) {\n    let listener: TcpListener = net::listen(\"127.0.0.1:0\")?;\n    let mut line: String = String::from(\"listening \");\n    line.push_str(&net::local_port(&listener).to_string());\n    io::println(&line);\n    task::block_on(serve(listener))\n}\n";

#[test]
fn a_recorded_async_server_replays_without_any_socket() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let dir = package("det-net-async", ASYNC_SERVER);
    let journal = dir.join("a.rvlog");
    let journal_arg = journal.to_string_lossy().into_owned();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .args(["--profile", "det", "--record", &journal_arg])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ravum-lang runs");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    let port: u16 = first
        .trim()
        .strip_prefix("listening ")
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port: {first:?}"));
    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .write_all(b"GET /async HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("\r\n\r\npong"), "{response}");
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    let mut err = String::new();
    let status = child.wait().unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(status.success(), "{err}");
    assert_eq!(rest, "/async\n");
    let text = fs::read_to_string(&journal).unwrap();
    for name in [
        "net::accept_async",
        "http::read_request_async",
        "http::respond_async",
    ] {
        assert!(
            text.contains(&format!("\t{name}\t")),
            "{name} not journaled: {text}"
        );
    }

    let (code, replayed, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(replayed, format!("{first}{rest}"), "same output, no client");

    // An edited respond argument digest: the future never gets its turn.
    let edited: String = text
        .lines()
        .map(|l| {
            if l.contains("\thttp::respond_async\t") {
                let f: Vec<&str> = l.splitn(4, '\t').collect();
                format!("{}\t{}\t0000000000000000\t{}", f[0], f[1], f[3])
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&journal, edited).unwrap();
    let (code, _, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("replay diverged"), "{err}");
}

/// `connect_async` under `--record` journals one event (not also an inner
/// `net::connect`), so its replay does not diverge.
#[test]
fn connect_async_records_one_event_and_replays() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = closed.local_addr().unwrap().to_string();
    drop(closed);
    let source = format!(
        "use std::io;\nuse std::net;\nuse std::task;\n\nasync fn go() -> () effects(io, net, async, alloc) {{\n    match net::connect_async(\"{addr}\").await {{\n        Ok(s) => io::println(\"connected\"),\n        Err(e) => io::println(\"refused\"),\n    }}\n}}\n\nfn main() -> () effects(io, net, async, alloc) {{\n    task::block_on(go());\n}}\n"
    );
    let dir = package("det-connect-async", &source);
    let journal = dir.join("c.rvlog");
    let journal_arg = journal.to_string_lossy().into_owned();
    let (code, recorded, err) = run_env(&dir, &["--profile", "det", "--record", &journal_arg], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(recorded, "refused\n");
    let text = fs::read_to_string(&journal).unwrap();
    assert_eq!(text.lines().count(), 2, "header + one event: {text}");
    assert!(text.contains("\tnet::connect_async\t"), "{text}");
    let (code, replayed, err) = run_env(&dir, &["--profile", "det", "--replay", &journal_arg], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(replayed, recorded);
}
