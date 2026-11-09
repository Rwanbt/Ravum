//! L4.6 — deterministic tests for the transient-verification retry policy.
//!
//! A fake `rustc` (a tiny Rust program compiled at test setup) is injected
//! through `RAVUM_RUSTC`, because the real failure is environmental and cannot
//! be reproduced deterministically on the shared Windows machine. The fake:
//!
//! * delegates `-vV` (toolchain identity) to the real `rustc`;
//! * in `transient-once` mode, fails the **first** compilation with a no-code
//!   JSON diagnostic (the incident pattern) and delegates the retry;
//! * in `ecode` mode, always fails with a real `E0061` diagnostic code;
//! * in `permanent-transient` mode, always fails with no-code diagnostics.
//!
//! Three decisions are asserted end-to-end through the real `ravum-collab`
//! binary:
//!
//! 1. transient no-code failure → **one bounded retry** → success is a normal
//!    semantic pass (`SEMANTICALLY_CHECKED`), never `REJECT`;
//! 2. real `E0061` failure → `REJECT` unchanged (fail-closed preserved);
//! 3. persistent no-code failure → `MANUAL_REQUIRED` (`Unavailable`), never
//!    `REJECT`, with exit code + bounded stderr excerpt + retry mention in the
//!    durable decision journal.
//!
//! No existing test is modified. Tests spawn real processes and are meant to be
//! run with `cargo test -p ravum-workspace-coordinator -- --test-threads=1`.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const FAKE_RUSTC_SOURCE: &str = r##"
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{exit, Command};

fn real_rustc() -> String {
    env::var("FAKE_RUSTC_REAL").unwrap_or_else(|_| "rustc".to_owned())
}

fn bump_counter(path: &str) -> u64 {
    let mut value = fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    value += 1;
    let _ = fs::write(path, value.to_string());
    value
}

fn append_log(path: &str, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn emit(line: &str) {
    eprintln!("{line}");
}

fn transient_diagnostics() {
    emit(r#"{"$message_type":"diagnostic","message":"only metadata stub found for `rlib` dependency `std` please provide path to the corresponding .rmeta file with full metadata","code":null,"level":"error","spans":[]}"#);
    emit(r#"{"$message_type":"diagnostic","message":"cannot resolve a prelude import","code":null,"level":"error","spans":[]}"#);
}

fn compiler_error_diagnostic() {
    emit(r#"{"$message_type":"diagnostic","message":"this function takes 2 arguments but 1 argument was supplied","code":{"code":"E0061"},"level":"error","spans":[]}"#);
}

fn delegate(args: &[String]) -> ! {
    match Command::new(real_rustc()).args(args).status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("fake rustc: cannot spawn real rustc: {error}");
            exit(97);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args
        .iter()
        .any(|arg| arg == "-vV" || arg == "--version" || arg == "-V")
    {
        delegate(&args);
    }
    let mode = env::var("FAKE_RUSTC_MODE").unwrap_or_default();
    let counter_path = env::var("FAKE_RUSTC_COUNTER").expect("FAKE_RUSTC_COUNTER");
    let call = bump_counter(&counter_path);
    if let Ok(log_path) = env::var("FAKE_RUSTC_LOG") {
        append_log(&log_path, &args.join(" "));
    }
    match mode.as_str() {
        "ecode" => {
            compiler_error_diagnostic();
            exit(101);
        }
        "transient-once" if call == 1 => {
            transient_diagnostics();
            exit(101);
        }
        "permanent-transient" => {
            transient_diagnostics();
            exit(101);
        }
        _ => delegate(&args),
    }
}
"##;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collab_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ravum-collab"))
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn test_work(tag: &str) -> PathBuf {
    let dir = repo_root()
        .join("target")
        .join("ravum-tests")
        .join("transient-verification")
        .join(tag);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create work dir");
    dir
}

/// Compiles the fake `rustc` once per test with the real toolchain.
fn build_fake_rustc(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).expect("fake dir");
    let source = dir.join("fake_rustc.rs");
    fs::write(&source, FAKE_RUSTC_SOURCE).expect("write fake source");
    let executable = dir.join(exe_name("fake-rustc"));
    let output = Command::new("rustc")
        .args(["--edition", "2021", "-o"])
        .arg(&executable)
        .arg(&source)
        .output()
        .expect("spawn rustc to build the fake rustc");
    assert!(
        output.status.success(),
        "cannot compile the fake rustc: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// Small, realistic candidate unit with two independent functions.
const WORKSPACE_SOURCE: &str = "//! Transient verification fixture.\n\npub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n\npub fn beta(x: u32) -> u32 {\n    x * 2\n}\n";

fn replacement_patch(path: &str, content: &str, old_line: &str, new_line: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let indexes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == old_line)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        indexes.len(),
        1,
        "{old_line:?} must be a unique line of {path}"
    );
    let index = indexes[0];
    let first = index.saturating_sub(1);
    let last = if index + 1 < lines.len() {
        index + 1
    } else {
        index
    };
    let mut hunk = Vec::new();
    let mut old_len = 0usize;
    let mut new_len = 0usize;
    for line in &lines[first..index] {
        hunk.push(format!(" {line}"));
        old_len += 1;
        new_len += 1;
    }
    hunk.push(format!("-{}", lines[index]));
    old_len += 1;
    hunk.push(format!("+{new_line}"));
    new_len += 1;
    for line in &lines[index + 1..=last] {
        hunk.push(format!(" {line}"));
        old_len += 1;
        new_len += 1;
    }
    format!(
        "+++ b/{path}\n@@ -{},{old_len} +{},{new_len} @@\n{}\n",
        first + 1,
        first + 1,
        hunk.join("\n")
    )
}

struct Prepared {
    work: PathBuf,
    workspace: PathBuf,
    proposals: PathBuf,
    journal: PathBuf,
    verify_work: PathBuf,
    fake_rustc: PathBuf,
    counter: PathBuf,
    log: PathBuf,
    mode: &'static str,
}

fn prepare(tag: &str, mode: &'static str) -> Prepared {
    let work = test_work(tag);
    let workspace = work.join("workspace");
    fs::create_dir_all(workspace.join("src")).expect("workspace");
    fs::write(workspace.join("src/lib.rs"), WORKSPACE_SOURCE).expect("write lib.rs");

    // Real base identity through the same content snapshot engine as the binary.
    let snapshot = ravum_snapshot::SourceSnapshot::from_directory(&workspace, |_| true)
        .expect("snapshot workspace");
    let base_id = snapshot.id().as_str().to_owned();
    let base_hash = snapshot
        .file_hash("src/lib.rs")
        .expect("hash src/lib.rs")
        .to_owned();

    let patch_alpha = replacement_patch("src/lib.rs", WORKSPACE_SOURCE, "    x + 1", "    x + 2");
    let patch_beta = replacement_patch("src/lib.rs", WORKSPACE_SOURCE, "    x * 2", "    x * 3");
    let proposals = work.join("proposals");
    fs::create_dir_all(&proposals).expect("proposals dir");
    for (index, (agent, task, symbols, patch)) in [
        ("agent-a", "alpha", vec!["alpha"], patch_alpha),
        ("agent-b", "beta", vec!["beta"], patch_beta),
    ]
    .into_iter()
    .enumerate()
    {
        let proposal = json!({
            "schema_version": 1,
            "agent_id": agent,
            "task_id": task,
            "base_snapshot_id": base_id.clone(),
            "target_paths": ["src/lib.rs"],
            "target_symbols": symbols,
            "expected_hashes": { "src/lib.rs": base_hash.clone() },
            "declared_dependencies": [],
            "impacted_contracts": [],
            "unified_patch": patch,
            "note": "L4.6 deterministic transient-verification test",
        });
        fs::write(
            proposals.join(format!("{:02}-{agent}-{task}.json", index + 1)),
            serde_json::to_string_pretty(&proposal).expect("serialize proposal"),
        )
        .expect("write proposal");
    }

    let fake_rustc = build_fake_rustc(&work.join("fake"));
    Prepared {
        counter: work.join("fake-calls.txt"),
        log: work.join("fake-log.txt"),
        journal: work.join("journal.jsonl"),
        verify_work: work.join("verify-work"),
        workspace,
        proposals,
        work,
        fake_rustc,
        mode,
    }
}

struct RunResult {
    payload: Value,
    journal: Vec<Value>,
    stderr: String,
    exit_code: Option<i32>,
    duration_ms: u64,
    fake_calls: u64,
    fake_log_lines: Vec<String>,
}

fn execute(prepared: &Prepared) -> RunResult {
    let started = Instant::now();
    let output = Command::new(collab_binary())
        .args([
            "integrate",
            "--workspace",
            prepared.workspace.to_str().expect("utf-8 workspace"),
            "--proposals",
            prepared.proposals.to_str().expect("utf-8 proposals"),
            "--journal",
            prepared.journal.to_str().expect("utf-8 journal"),
            "--json",
            "--verify",
            "--work",
            prepared.verify_work.to_str().expect("utf-8 verify work"),
        ])
        .env_remove("RUSTC")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env("RAVUM_RUSTC", &prepared.fake_rustc)
        .env("FAKE_RUSTC_MODE", prepared.mode)
        .env("FAKE_RUSTC_REAL", "rustc")
        .env("FAKE_RUSTC_COUNTER", &prepared.counter)
        .env("FAKE_RUSTC_LOG", &prepared.log)
        .output()
        .expect("spawn ravum-collab");
    let duration_ms = started.elapsed().as_millis() as u64;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "ravum-collab integrate must exit 0 (decisions, not the process, carry verdicts); stderr: {stderr}"
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "ravum-collab stdout is not JSON ({error}); stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    let journal = fs::read_to_string(&prepared.journal)
        .expect("journal readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("journal line is JSON"))
        .collect::<Vec<_>>();
    let fake_calls = fs::read_to_string(&prepared.counter)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let fake_log_lines = fs::read_to_string(&prepared.log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    RunResult {
        payload,
        journal,
        stderr,
        exit_code: output.status.code(),
        duration_ms,
        fake_calls,
        fake_log_lines,
    }
}

fn decisions(payload: &Value) -> Vec<&str> {
    payload["decisions"]
        .as_array()
        .expect("decisions array")
        .iter()
        .map(|decision| decision["decision"].as_str().expect("decision string"))
        .collect()
}

fn levels(payload: &Value) -> Vec<&str> {
    payload["evaluations"]
        .as_array()
        .expect("evaluations array")
        .iter()
        .map(|evaluation| evaluation["level"].as_str().expect("level string"))
        .collect()
}

fn journal_reasons(journal: &[Value]) -> Vec<String> {
    journal
        .iter()
        .flat_map(|entry| {
            entry["decision"]["reasons"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|reason| reason.as_str().map(str::to_owned))
        .collect()
}

fn write_report(prepared: &Prepared, run: &RunResult, scenario: &str) {
    let report = json!({
        "scenario": scenario,
        "fake_rustc_mode": prepared.mode,
        "fake_rustc_path": prepared.fake_rustc.display().to_string(),
        "exit_code": run.exit_code,
        "duration_ms": run.duration_ms,
        "decisions": decisions(&run.payload),
        "levels": levels(&run.payload),
        "accepted_changes": run.payload["accepted_changes"].as_array().map(|changes| changes.len()).unwrap_or(0),
        "accepted_verification_statuses": run.payload["accepted_changes"]
            .as_array()
            .map(|changes| changes.iter().map(|change| change["verification"]["status"].as_str().unwrap_or("?").to_owned()).collect::<Vec<_>>())
            .unwrap_or_default(),
        "fake_compile_calls": run.fake_calls,
        "fake_log_lines": run.fake_log_lines,
        "journal_lines": run.journal.len(),
        "journal_reasons": journal_reasons(&run.journal),
        "stderr": run.stderr,
    });
    fs::write(
        prepared.work.join("report.json"),
        serde_json::to_string_pretty(&report).expect("serialize report"),
    )
    .expect("write report");
}

// ---------------------------------------------------------------------------
// 1. Transient no-code failure → one bounded retry → SEMANTICALLY_CHECKED
// ---------------------------------------------------------------------------

#[test]
fn a_transient_no_code_failure_is_retried_then_verified() {
    let prepared = prepare("transient-once", "transient-once");
    let run = execute(&prepared);
    write_report(&prepared, &run, "1-transient-once-retry-success");

    assert_eq!(
        decisions(&run.payload),
        vec!["AUTO_SAFE", "AUTO_SAFE"],
        "the bounded retry must turn the transient incident into a normal pass; reasons: {:?}",
        journal_reasons(&run.journal)
    );
    assert_eq!(
        levels(&run.payload),
        vec!["SEMANTICALLY_CHECKED", "SEMANTICALLY_CHECKED"],
        "a retry success is SEMANTICALLY_CHECKED as before, not PROVED/TESTED"
    );
    let accepted = run.payload["accepted_changes"]
        .as_array()
        .expect("accepted changes");
    assert_eq!(accepted.len(), 2, "both proposals must be integrated");
    for change in accepted {
        assert_eq!(
            change["verification"]["status"].as_str(),
            Some("passed"),
            "accepted changes carry the real passed verification"
        );
    }
    let reasons = journal_reasons(&run.journal).join(" | ");
    assert!(
        reasons.contains("retry"),
        "the journal must record that the verification was retried: {reasons}"
    );
    assert!(
        reasons.contains("only metadata stub found"),
        "the journal must preserve the first (transient) stderr excerpt: {reasons}"
    );
    assert!(
        !decisions(&run.payload).contains(&"REJECT"),
        "a transient incident must never become a definitive REJECT"
    );
    // 1 failed attempt + 1 retry + 1 normal proposal = 3 real compile calls.
    assert_eq!(
        run.fake_calls, 3,
        "exactly one bounded retry is allowed: {:#?}",
        run.fake_log_lines
    );
    assert_eq!(run.fake_log_lines.len(), 3);
}

// ---------------------------------------------------------------------------
// 2. A real compiler error code stays a definitive REJECT (no retry)
// ---------------------------------------------------------------------------

#[test]
fn a_compiler_error_code_is_still_a_definitive_reject() {
    let prepared = prepare("ecode", "ecode");
    let run = execute(&prepared);
    write_report(&prepared, &run, "2-ecode-reject-preserved");

    assert_eq!(
        decisions(&run.payload),
        vec!["REJECT", "REJECT"],
        "a real E-code failure must remain a definitive REJECT"
    );
    assert_eq!(levels(&run.payload), vec!["REJECTED", "REJECTED"]);
    assert_eq!(
        run.payload["accepted_changes"]
            .as_array()
            .expect("accepted changes")
            .len(),
        0,
        "nothing may be integrated on a failed verification (fail-closed)"
    );
    let reasons = journal_reasons(&run.journal).join(" | ");
    assert!(
        reasons.contains("E0061"),
        "the compiler diagnostic code must be journaled: {reasons}"
    );
    assert!(
        reasons.contains("exit code 101"),
        "the rustc exit code must be journaled: {reasons}"
    );
    assert!(
        reasons.contains("stderr excerpt"),
        "a bounded stderr excerpt must be journaled: {reasons}"
    );
    // One attempt per proposal: a real compilation failure is not retried.
    assert_eq!(
        run.fake_calls, 2,
        "a real E-code failure must not consume a retry: {:#?}",
        run.fake_log_lines
    );
}

// ---------------------------------------------------------------------------
// 3. A persistent no-code failure is Unavailable/MANUAL_REQUIRED, never REJECT
// ---------------------------------------------------------------------------

#[test]
fn a_persistent_no_code_failure_is_unavailable_not_reject() {
    let prepared = prepare("permanent-transient", "permanent-transient");
    let run = execute(&prepared);
    write_report(&prepared, &run, "3-permanent-transient-manual-required");

    assert_eq!(
        decisions(&run.payload),
        vec!["MANUAL_REQUIRED", "MANUAL_REQUIRED"],
        "a persistent no-code failure must fail closed as MANUAL_REQUIRED, never REJECT"
    );
    // The Unavailable path is not a semantic pass: the evaluation level stays
    // TEXTUALLY_APPLICABLE while the terminal decision is MANUAL_REQUIRED.
    assert_eq!(
        levels(&run.payload),
        vec!["TEXTUALLY_APPLICABLE", "TEXTUALLY_APPLICABLE"]
    );
    assert!(
        !decisions(&run.payload).contains(&"AUTO_SAFE"),
        "nothing may be integrated without a successful verification"
    );
    assert_eq!(
        run.payload["accepted_changes"]
            .as_array()
            .expect("accepted changes")
            .len(),
        0
    );
    let reasons = journal_reasons(&run.journal).join(" | ");
    assert!(
        reasons.contains("no compiler error code"),
        "the journal must classify the failure as non-compiler: {reasons}"
    );
    assert!(
        reasons.contains("2 attempt(s)"),
        "the journal must mention the bounded retry: {reasons}"
    );
    assert!(
        reasons.contains("attempt 1: exit code 101")
            && reasons.contains("attempt 2: exit code 101"),
        "both attempts' exit codes must be journaled: {reasons}"
    );
    assert!(
        reasons.contains("only metadata stub found")
            && reasons.contains("cannot resolve a prelude import"),
        "the bounded stderr excerpt of both attempts must be journaled: {reasons}"
    );
    assert!(
        journal_reasons(&run.journal).len() < 16,
        "journal reasons stay bounded (no unbounded stderr dump): {} reasons",
        journal_reasons(&run.journal).len()
    );
    // 2 proposals x (attempt 1 + bounded retry) = 4 real compile calls.
    assert_eq!(
        run.fake_calls, 4,
        "the retry must be exactly one per proposal: {:#?}",
        run.fake_log_lines
    );
}
