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
