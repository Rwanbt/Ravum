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

const MAIN: &str = "use std::io;\nextern \"C\" from \"demo\" {\n    fn demo_scale(x: i32) -> i32;\n}\n\n\
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

const TEXT_C: &str = r#"#include <stdlib.h>
#include <stdio.h>
typedef struct { int v; } box;
void *box_new(int v) { box *b = malloc(sizeof *b); b->v = v; return b; }
int box_get(void *b) { return ((box *)b)->v; }
void box_free(void *b) { free(b); }
void *box_none(void) { return NULL; }
static char buf[256];
const char *greet(const char *p, size_t n) { snprintf(buf, sizeof buf, "hello %.*s (%zu bytes)", (int)n, p, n); return buf; }
const char *nothing(void) { return NULL; }
const char *bad_utf8(void) { return "\xff\xfe"; }
"#;

const TEXT_MAIN: &str = r#"use std::io;
use std::ffi;
extern "C" from "t" {
    fn box_new(v: i32) -> CHandle;
    fn box_get(b: CHandle) -> i32;
    fn box_free(b: CHandle);
    fn box_none() -> CHandle;
    fn greet(name: &str) -> Option<String>;
    fn nothing() -> Option<String>;
    fn bad_utf8() -> Option<String>;
}

fn show(o: Option<String>) -> () effects(io, alloc) {
    match o {
        Some(s) => io::println(s.as_str()),
        None => io::println("<none>"),
    }
}

fn main() -> () effects(io, alloc, unsafe, ffi) {
    let b: CHandle = unsafe { box_new(41) };
    let c: CHandle = b;
    io::println((unsafe { box_get(b) } + 1).to_string().as_str());
    unsafe { box_free(c) };
    io::println(if ffi::is_null(unsafe { box_none() }) { "null" } else { "not null" });
    let name: String = String::from("Ravum\u{0}x");
    show(unsafe { greet(name.as_str()) });
    show(unsafe { nothing() });
    show(unsafe { bad_utf8() });
}
"#;

/// ABI-3.6 to ABI-3.8 against a real C library: a handle survives a copy, a
/// null handle is detected, `&str` arrives as pointer + length with its
/// interior NUL (7 bytes, no truncation), and a NULL or non-UTF-8 C string
/// becomes `None`.
#[test]
fn handles_and_text_cross_the_c_boundary() {
    let root = std::env::temp_dir().join(format!("ravum-native-text-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("native")).unwrap();
    fs::write(root.join("native/t.c"), TEXT_C).unwrap();
    let ok = Command::new("cc")
        .args(["-c", "-fPIC", "native/t.c", "-o", "native/t.o"])
        .current_dir(&root)
        .status()
        .expect("cc")
        .success();
    assert!(ok, "cc failed");
    let ok = Command::new("ar")
        .args(["rcs", "native/libt.a", "native/t.o"])
        .current_dir(&root)
        .status()
        .expect("ar")
        .success();
    assert!(ok, "ar failed");
    let digest =
        ravum_lang_sema::loader::sha256_hex(&fs::read(root.join("native/libt.a")).unwrap());
    fs::write(
        root.join("ravum.toml"),
        format!(
            "[package]\nname = \"ffitext\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n\
             [native]\nt = {{ kind = \"static\", path = \"native\", lib = \"t\", sha256 = \"{digest}\" }}\n"
        ),
    )
    .unwrap();
    fs::write(root.join("src/main.rv"), TEXT_MAIN).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["run", "."])
        .current_dir(&root)
        .output()
        .expect("run ravum-lang");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(stdout, "42\nnull\nhello Ravum (7 bytes)\n<none>\n<none>\n");
    let _ = fs::remove_dir_all(&root);
}
