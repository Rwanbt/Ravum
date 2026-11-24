//! The trial applications of `apps/` (V2-G3, docs/reports/V2-TRIAL-APPS.md):
//! their own `test fn`s pass, and their commands behave end to end, on a
//! copy of each package in a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ravum-lang")
}

fn copy_app(name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps")
        .join(name);
    let dst = std::env::temp_dir().join(format!("ravum-trial-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("src")).unwrap();
    for f in ["ravum.toml", "ravum.lock"] {
        if src.join(f).exists() {
            fs::copy(src.join(f), dst.join(f)).unwrap();
        }
    }
    fs::copy(src.join("src/main.rv"), dst.join("src/main.rv")).unwrap();
    dst
}

fn run(dir: &Path, args: &[&str]) -> Output {
    let mut all = vec!["run", dir.to_str().unwrap(), "--"];
    all.extend_from_slice(args);
    Command::new(bin()).args(&all).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn unit_tests_pass(dir: &Path) {
    let o = Command::new(bin())
        .args(["test", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{}{}",
        out(&o),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(out(&o).contains("0 failed"), "{}", out(&o));
}

#[test]
fn todo_app_works_end_to_end() {
    let dir = copy_app("todo");
    unit_tests_pass(&dir);
    let file = dir.join("tasks.txt");
    let f = file.to_str().unwrap();
    assert!(out(&run(&dir, &[f, "add", "acheter du pain"])).contains("ok"));
    assert!(out(&run(&dir, &[f, "add", "  écrire  "])).contains("ok"));
    assert!(out(&run(&dir, &[f, "done", "1"])).contains("ok"));
    let list = out(&run(&dir, &[f, "list"]));
    assert!(
        list.contains("1. [x] acheter du pain\n2. [ ] écrire\n1 pending\n"),
        "{list}"
    );
    run(&dir, &[f, "clear-done"]);
    assert!(out(&run(&dir, &[f, "list"])).contains("1. [ ] écrire\n1 pending\n"));
    // Negative: a missing task is an error (exit 1), a bad command is usage (2).
    let missing = run(&dir, &[f, "done", "9"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no such task"));
    assert_eq!(run(&dir, &[f, "bogus"]).status.code(), Some(2));
    // The file is untouched by the refused commands.
    assert_eq!(fs::read_to_string(&file).unwrap(), "todo\nécrire\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn log_report_app_works_end_to_end() {
    let dir = copy_app("log-report");
    unit_tests_pass(&dir);
    let log = dir.join("app.log");
    fs::write(
        &log,
        "INFO start\nERROR disk full\nWARN slow\n\nDEBUG x\nnoise\n",
    )
    .unwrap();
    let o = run(&dir, &[log.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "errors present: exit 1");
    assert!(
        out(&o).contains("error: 1\nwarn: 1\ninfo: 1\ndebug: 1\nother: 1\nERROR disk full\n"),
        "{}",
        out(&o)
    );
    fs::write(&log, "INFO only\n").unwrap();
    assert_eq!(run(&dir, &[log.to_str().unwrap()]).status.code(), Some(0));
    // Negative: a missing file is an error, not a report.
    let gone = run(&dir, &[dir.join("missing.log").to_str().unwrap()]);
    assert_eq!(gone.status.code(), Some(1));
    assert!(!out(&gone).contains("error: "));
    let _ = fs::remove_dir_all(&dir);
}
