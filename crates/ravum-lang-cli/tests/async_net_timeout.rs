use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
use std::io;
use std::task;

async fn immediate() -> i32 effects(async) { return 7; }

async fn wait_forever() -> () effects(async) {
    loop { task::yield_now().await; }
}

async fn first_poll() -> () effects(io, async, nondet, alloc) {
    let ready: Option<i32> = task::timeout(0u64, immediate()).await;
    match ready {
        Some(7) => io::println("ready"),
        _ => io::println("wrong ready result"),
    }
    let pending: Option<()> = task::timeout(0u64, wait_forever()).await;
    if pending.is_none() { io::println("pending expired"); }
    let soon: Option<i32> = task::timeout(1000u64, immediate()).await;
    if soon.is_some() { io::println("ready before deadline"); }
}

async fn virtual_wait() -> () effects(io, async, nondet, alloc) {
    let result: Option<()> = task::timeout(600000u64, wait_forever()).await;
    if result.is_none() { io::println("virtual timer expired"); }
}

fn main() -> () effects(io, async, nondet, alloc) {
    task::block_on(first_poll());
    task::block_on(virtual_wait());
}
"#;

#[test]
fn deterministic_timeout_polls_once_at_zero_and_virtualizes_ten_minutes() {
    let path = std::env::temp_dir().join(format!("ravum-task-timeout-{}", std::process::id()));
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(
        path.join("ravum.toml"),
        "[package]\nname = \"virtual-timeout\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(path.join("src/main.rv"), SOURCE).unwrap();
    let cli = env!("CARGO_BIN_EXE_ravum-lang");
    let check = Command::new(cli)
        .args(["check", "--profile", "det", "."])
        .current_dir(&path)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let build = Command::new(cli)
        .args(["build", "."])
        .current_dir(&path)
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
        .find_map(|line| line.strip_prefix("built: "))
        .unwrap();
    let mut process = Command::new(artifact)
        .env("RAVUM_DET_SEED", "13")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while process.try_wait().unwrap().is_none() {
        if start.elapsed() >= Duration::from_secs(10) {
            process.kill().expect("stop unresponsive timer fixture");
            process.wait().expect("reap unresponsive timer fixture");
            panic!("virtual clock waited in real time");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"ready\npending expired\nready before deadline\nvirtual timer expired\n"
    );
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "virtual clock waited in real time"
    );
}
