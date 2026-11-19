//! T9 vertical (D-19-8): `packages/http-notes`, an HTTP/1.1 service on its
//! own thread that stores notes in SQLite, driven by a client in the same
//! program over 127.0.0.1. The output is fixed: two notes created (one of
//! them SQL-looking text, stored as text), listed, two 404s, listed again.
//!
//! It builds `packages/sqlite`'s native library first (C toolchain required).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn the_http_sqlite_service_answers_its_scenario() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repository root")
        .to_path_buf();
    let built = Command::new(python())
        .arg("tools/build_sqlite_native.py")
        .current_dir(&repo)
        .output()
        .expect("python3 is required by this test");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["run", "."])
        .current_dir(repo.join("packages/http-notes"))
        .output()
        .expect("run ravum-lang");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list = "1 buy milk\n2 '); DROP TABLE notes; --\n";
    let expected = format!(
        "POST /notes -> 201\ncreated 1\nPOST /notes -> 201\ncreated 2\n\
         GET /notes -> 200\n{list}GET /missing -> 404\nnot found\n\
         POST /missing -> 404\nnot found\nGET /notes -> 200\n{list}"
    );
    assert_eq!(stdout, expected);
}

/// `python3` on Unix; on Windows the installer names it `python`.
fn python() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}
