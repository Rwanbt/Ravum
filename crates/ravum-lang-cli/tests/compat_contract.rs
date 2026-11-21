//! Contract tests for `spec/language/compatibility.md` rules that need two
//! runs or a file comparison: a dependency path change is visible in the
//! digest and the lockfile (COMPAT-4.3), and an invalid lockfile is never
//! rewritten or partially used (COMPAT-1.4, COMPAT-7.1).

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ravum-compat-contract-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create workspace");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        fs::write(path, contents).expect("write file");
    }

    fn package(&self, dir: &str, name: &str, deps: &str) {
        let deps = if deps.is_empty() {
            String::new()
        } else {
            format!("\n[dependencies]\n{deps}")
        };
        self.write(
            &format!("{dir}/ravum.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n{deps}"
            ),
        );
    }

    fn check(&self, dir: &str) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .args(["check", "--json", "."])
            .current_dir(self.root.join(dir))
            .output()
            .expect("run ravum-lang");
        serde_json::from_slice(&output.stdout).expect("one JSON object")
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn codes(report: &Value) -> Vec<String> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_string())
        .collect()
}

fn app_with_util(workspace: &Workspace, util_dir: &str) {
    workspace.package(
        "app",
        "app",
        &format!("util = {{ path = \"../{util_dir}\" }}\n"),
    );
    workspace.write(
        "app/src/main.rv",
        "use std::io;\nfn main() {\n    io::println(&util::v().to_string());\n}\n",
    );
}

fn util_at(workspace: &Workspace, dir: &str, body: &str) {
    workspace.package(dir, "util", "");
    workspace.write(&format!("{dir}/src/lib.rv"), body);
}

#[test]
fn a_dependency_path_change_changes_the_digest_and_stales_the_lockfile() {
    let workspace = Workspace::new("digest");
    util_at(&workspace, "util_a", "pub fn v() -> i32 { 1 }\n");
    util_at(&workspace, "util_b", "pub fn v() -> i32 { 2 }\n");
    app_with_util(&workspace, "util_a");

    let first = workspace.check("app");
    assert_eq!(first["exit_code"], 0, "{first}");
    let second = workspace.check("app");
    assert_eq!(second["lockfile_stale"], false);
    assert_eq!(first["dependencies_digest"], second["dependencies_digest"]);

    // Same name and version, another path: legal, but visible.
    app_with_util(&workspace, "util_b");
    let moved = workspace.check("app");
    assert_ne!(
        moved["dependencies_digest"], first["dependencies_digest"],
        "the digest must follow the source actually present"
    );
    assert_eq!(moved["lockfile_stale"], true);
}

#[test]
fn an_invalid_lockfile_is_refused_and_left_untouched() {
    let workspace = Workspace::new("lock");
    util_at(&workspace, "util", "pub fn v() -> i32 { 1 }\n");
    app_with_util(&workspace, "util");
    let invalid = "lockfile_version = 99\n";
    workspace.write("app/ravum.lock", invalid);

    let report = workspace.check("app");
    assert_eq!(report["exit_code"], 1);
    assert_eq!(codes(&report), ["E-MOD-LOCK"]);
    assert_eq!(
        fs::read_to_string(workspace.path("app/ravum.lock")).unwrap(),
        invalid,
        "fail-closed: the lockfile is never rewritten from a partial reading"
    );
    assert!(!Path::new(&workspace.path("app/.ravum/bin")).exists());
}
