//! `packages/sqlite` end to end (decision D-19-7): the repository script
//! builds the native library from the pinned SQLite amalgamation, and
//! `packages/sqlite-notes` runs against it. It checks, on a real SQLite:
//! a parameter that looks like SQL is stored as text (DB-1.2), a UNIQUE
//! violation and a syntax error are `Err` (DB-1.3), a rolled-back insert is
//! gone, and the version is the pinned one (DB-1.7).
//!
//! Unix only, like `tools/build_sqlite_native.py` (it needs `cc`, `ar` and
//! `python3`).
#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repository root")
        .to_path_buf()
}

#[test]
fn the_sqlite_package_runs_against_the_pinned_sqlite() {
    let repo = repo();
    let built = Command::new("python3")
        .arg("tools/build_sqlite_native.py")
        .current_dir(&repo)
        .output()
        .expect("python3 is required by this test");
    assert!(
        built.status.success(),
        "build script failed:\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["run", "."])
        .current_dir(repo.join("packages/sqlite-notes"))
        .output()
        .expect("run ravum-lang");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout,
        "3.54.1\n1\nduplicate is Err\nsyntax error is Err\n1 '; DROP TABLE t; --\n2 ada\n"
    );
}
