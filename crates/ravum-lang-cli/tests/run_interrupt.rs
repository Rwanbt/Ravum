//! Decision V2-K1: an interrupted `ravum-lang run` stops the program it
//! launched. The program rewrites a file forever; after SIGTERM to `run`,
//! `run` exits 130 and the file stops changing (the child is gone).
#![cfg(unix)]

use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FOREVER: &str = "use std::fs;\nuse std::io;\n\nfn main() -> () effects(io, fs, alloc, panic) {\n    io::println(\"started\");\n    let mut n: i64 = 0;\n    let running: bool = true;\n    while running {\n        n = n + 1;\n        let w = fs::write_string(\"beat.txt\", n.to_string().as_str());\n        drop(w);\n    }\n}\n";

#[test]
fn sigterm_to_run_stops_the_launched_program() {
    let dir = std::env::temp_dir().join(format!("ravum-run-interrupt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"forever\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.rv"), FOREVER).unwrap();
    let beat = dir.join("beat.txt");

    let mut run = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !beat.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(300),
            "program never started"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let killed = Command::new("kill")
        .args(["-TERM", &run.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = run.wait().unwrap();
    assert_eq!(status.code(), Some(130), "run exit code");

    std::thread::sleep(Duration::from_millis(300));
    let a = fs::read_to_string(&beat).unwrap_or_default();
    std::thread::sleep(Duration::from_millis(500));
    let b = fs::read_to_string(&beat).unwrap_or_default();
    assert_eq!(
        a, b,
        "the launched program is still running after run was interrupted"
    );
    let _ = fs::remove_dir_all(&dir);
}
