//! MOD-5.14 to MOD-5.19 end to end: a static C library, built here by the
//! system C compiler, is declared in `[native]`, pinned by its digest, linked
//! through `extern "C" from "demo"` and called from Ravum. A rebuilt library
//! with a new digest changes the build identity; a stale pin is refused.
//!
//! Unix only: the test builds the library with `cc` and `ar`. Windows is
//! covered by the platform-neutral conformance cases (`modules/native-*`),
//! not by this test.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

const MAIN: &str = "extern \"C\" from \"demo\" {\n    fn demo_scale(x: i32) -> i32;\n}\n\n\
fn main() -> () effects(io, alloc, unsafe, ffi) {\n    let v: i32 = unsafe { demo_scale(14) };\n    io::println(v.to_string().as_str());\n}\n";

fn build_library(root: &Path, factor: i32) -> String {
    let dir = root.join("native");
    fs::create_dir_all(&dir).unwrap();
    let c = dir.join("demo.c");
    fs::write(
        &c,
        format!("int demo_scale(int x) {{ return {factor} * x; }}\n"),
    )
    .unwrap();
    let object = dir.join("demo.o");
    let status = Command::new("cc")
        .args(["-c", "-fPIC"])
        .arg(&c)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("the system C compiler `cc` is required by this test");
    assert!(status.success(), "cc failed");
    let archive = dir.join("libdemo.a");
    let _ = fs::remove_file(&archive);
    let status = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("`ar` is required by this test");
    assert!(status.success(), "ar failed");
    ravum_lang_sema::loader::sha256_hex(&fs::read(&archive).unwrap())
}

fn write_manifest(root: &Path, digest: &str) {
    fs::write(
        root.join("ravum.toml"),
        format!(
            "[package]\nname = \"natdemo\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n\
             [native]\ndemo = {{ kind = \"static\", path = \"native\", lib = \"demo\", sha256 = \"{digest}\" }}\n"
        ),
    )
    .unwrap();
}

fn ravum(root: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run ravum-lang");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

fn identity(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("identity: "))
        .expect("build prints its identity")
        .to_string()
}

#[test]
fn a_pinned_static_library_is_linked_and_its_digest_drives_the_identity() {
    let root = std::env::temp_dir().join(format!("ravum-native-link-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rv"), MAIN).unwrap();

    let first = build_library(&root, 3);
    write_manifest(&root, &first);
    let (code, built) = ravum(&root, &["build", "."]);
    assert_eq!(code, 0, "{built}");
    assert!(
        built.contains(&format!("native: demo (static demo) sha256:{first}")),
        "{built}"
    );
    let (code, ran) = ravum(&root, &["run", "."]);
    assert_eq!(code, 0, "{ran}");
    assert!(ran.lines().any(|line| line == "42"), "{ran}");

    // A rebuilt library: the old pin is refused and nothing is linked.
    let second = build_library(&root, 5);
    assert_ne!(first, second);
    let (code, refused) = ravum(&root, &["build", "."]);
    assert_eq!(code, 1, "{refused}");
    assert!(refused.contains("E-MOD-NATIVE-DIGEST"), "{refused}");

    // Re-pinned: it links again, with a different identity.
    write_manifest(&root, &second);
    let (code, rebuilt) = ravum(&root, &["build", "."]);
    assert_eq!(code, 0, "{rebuilt}");
    assert_ne!(identity(&built), identity(&rebuilt));
    let (code, ran) = ravum(&root, &["run", "."]);
    assert_eq!(code, 0, "{ran}");
    assert!(ran.lines().any(|line| line == "70"), "{ran}");

    let _ = fs::remove_dir_all(&root);
}
