//! `ravum-lang watch --backend jit`: continuous compilation on the native
//! backend, per-function recompilation, fail-closed activation.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn package(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rv-watch-jit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    dir
}

fn spawn(args: &[&str]) -> (Child, Receiver<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    (child, rx)
}

/// Lines up to and including the first one containing `needle`.
fn until(rx: &Receiver<String>, needle: &str) -> Vec<String> {
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(l) => {
                let hit = l.contains(needle);
                seen.push(l);
                if hit {
                    return seen;
                }
            }
            Err(_) => panic!(
                "timed out waiting for `{needle}`; got:\n{}",
                seen.join("\n")
            ),
        }
    }
}

fn edit(file: &Path, from: &str, to: &str) {
    // Make sure the modification time moves even on coarse file systems.
    std::thread::sleep(Duration::from_millis(30));
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains(from), "`{from}` not in source");
    std::fs::write(file, text.replacen(from, to, 1)).unwrap();
}

const PROGRAM: &str = "fn square(x: i64) -> i64 effects() {\n    return x * x;\n}\n\nfn offset() -> i64 effects() {\n    return 1;\n}\n\nfn check() -> i64 effects() {\n    return square(6) + offset();\n}\n";

#[test]
fn watch_jit_recompiles_only_the_edited_function_and_keeps_the_last_good_version() {
    let dir = package("basic", PROGRAM);
    let file = dir.join("src/main.rv");
    let (mut child, rx) = spawn(&[
        "watch",
        "--backend",
        "jit",
        "--entry",
        "check",
        "--max-rebuilds",
        "4",
        dir.to_str().unwrap(),
    ]);
    until(&rx, "check() -> value i64 37");

    edit(&file, "return 1;", "return 2;");
    let lines = until(&rx, "check() -> ");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("1 recompiled (offset), 2 reused")),
        "{lines:?}"
    );
    assert!(lines.last().unwrap().ends_with("value i64 38"), "{lines:?}");

    // Negative: an ill-typed edit is reported, nothing is activated.
    edit(&file, "return 2;", "return true;");
    let lines = until(&rx, "previous version kept");
    let diags = until(&rx, "E-TYP");
    assert!(
        diags
            .iter()
            .any(|l| l.contains("main.rv:6:12: E-TYP-MISMATCH")),
        "{diags:?}"
    );
    assert!(
        !lines.iter().chain(&diags).any(|l| l.contains("E-TYP-MAIN")),
        "spurious binary diagnostics"
    );

    // Negative: a construct the IR does not cover is refused, nothing activated.
    edit(
        &file,
        "return true;",
        "let v: f64 = -3.5;\n    return v.round() as i64;",
    );
    until(
        &rx,
        "not supported by the jit backend: method call `.round()`",
    );

    // Recovery: the next good edit compiles from the last good version.
    edit(
        &file,
        "let v: f64 = -3.5;\n    return v.round() as i64;",
        "return 3;",
    );
    let lines = until(&rx, "check() -> ");
    assert!(lines.last().unwrap().ends_with("value i64 39"), "{lines:?}");
    assert!(child.wait().unwrap().success());
}

#[test]
fn watch_jit_drives_a_dsp_block_and_refuses_a_state_schema_change() {
    let src = "struct State {\n    z1: f32,\n    z2: f32,\n}\n\nfn init() -> State effects() {\n    return State { z1: 0.0, z2: 0.0 };\n}\n\nfn gain() -> f32 effects() {\n    return 0.5;\n}\n\nfn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let y = input * gain() + state.z1 * 0.5;\n    state.z1 = y;\n    return y;\n}\n";
    let dir = package("dsp", src);
    let file = dir.join("src/main.rv");
    let (mut child, rx) = spawn(&[
        "watch",
        "--backend",
        "jit",
        "--max-rebuilds",
        "2",
        dir.to_str().unwrap(),
    ]);
    until(&rx, "process_sample: 4096 samples");
    edit(&file, "return 0.5;", "return 0.25;");
    let lines = until(&rx, "process_sample: 4096 samples");
    assert!(
        lines.iter().any(|l| l.contains("1 recompiled (gain)")),
        "{lines:?}"
    );
    // Negative: same program, wider `State` (struct and `init` changed in one
    // save): the checker accepts it, the schema check refuses it (HR-1.3).
    std::thread::sleep(Duration::from_millis(30));
    let wider = std::fs::read_to_string(&file)
        .unwrap()
        .replacen("    z2: f32,\n}", "    z2: f32,\n    z3: f32,\n}", 1)
        .replacen(
            "State { z1: 0.0, z2: 0.0 }",
            "State { z1: 0.0, z2: 0.0, z3: 0.0 }",
            1,
        );
    std::fs::write(&file, wider).unwrap();
    let lines = until(&rx, "previous version kept");
    assert!(
        lines
            .last()
            .unwrap()
            .contains("refused: state schema changed"),
        "{lines:?}"
    );
    assert!(child.wait().unwrap().success());
}

#[test]
fn watch_jit_refuses_an_incompatible_entry_change() {
    let dir = package("entry", PROGRAM);
    let file = dir.join("src/main.rv");
    let (mut child, rx) = spawn(&[
        "watch",
        "--backend",
        "jit",
        "--entry",
        "check",
        "--max-rebuilds",
        "1",
        dir.to_str().unwrap(),
    ]);
    until(&rx, "check() -> value i64 37");
    edit(
        &file,
        "fn check() -> i64 effects() {\n    return square(6) + offset();",
        "fn check() -> bool effects() {\n    return square(6) > offset();",
    );
    let lines = until(&rx, "previous version kept");
    assert!(
        lines
            .last()
            .unwrap()
            .contains("refused: entry `check` changed signature"),
        "{lines:?}"
    );
    assert!(child.wait().unwrap().success());
}

#[test]
fn watch_jit_shows_what_main_prints_and_follows_an_edit() {
    let src = "fn label() -> i64 effects() {\n    return 41;\n}\n\nfn main() -> () effects(io, alloc) {\n    io::print(\"total=\");\n    io::println(&(label() + 1).to_string());\n}\n";
    let dir = package("prints", src);
    let file = dir.join("src/main.rv");
    let (mut child, rx) = spawn(&[
        "watch",
        "--backend",
        "jit",
        "--max-rebuilds",
        "1",
        dir.to_str().unwrap(),
    ]);
    let lines = until(&rx, "main() -> value () ()");
    assert!(lines.iter().any(|l| l == "total=42"), "{lines:?}");
    edit(&file, "return 41;", "return 99;");
    let lines = until(&rx, "main() -> value () ()");
    assert!(lines.iter().any(|l| l == "total=100"), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("1 recompiled (label)")),
        "{lines:?}"
    );
    assert!(child.wait().unwrap().success());
}

#[test]
fn watch_jit_migrates_a_wider_state_with_migrate() {
    let v1 = "struct State {\n    z1: f32,\n}\n\nfn init() -> State effects() {\n    return State { z1: 0.0 };\n}\n\nfn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let y = input * 0.5 + state.z1 * 0.5;\n    state.z1 = y;\n    return y;\n}\n";
    let v2 = "struct Before {\n    z1: f32,\n}\n\nstruct State {\n    z1: f32,\n    gain: f32,\n}\n\nfn init() -> State effects() {\n    return State { z1: 0.0, gain: 1.0 };\n}\n\nfn migrate(old: Before) -> State effects() {\n    return State { z1: old.z1, gain: 2.0 };\n}\n\nfn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let y = input * 0.5 + state.z1 * 0.5;\n    state.z1 = y;\n    return y * state.gain;\n}\n";
    let dir = package("migrate", v1);
    let file = dir.join("src/main.rv");
    let (mut child, rx) = spawn(&[
        "watch",
        "--backend",
        "jit",
        "--max-rebuilds",
        "1",
        dir.to_str().unwrap(),
    ]);
    until(&rx, "process_sample: 4096 samples");
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(&file, v2).unwrap();
    let lines = until(&rx, "process_sample: 4096 samples");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("state migrated with `migrate`")),
        "{lines:?}"
    );
    assert!(child.wait().unwrap().success());
}
