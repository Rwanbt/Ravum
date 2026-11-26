//! RFC-UI-LANG-2 P1 (STD-21): every `arena-*` conformance case, end to end
//! on path B. A `compile-fail` case must be refused with exactly its codes;
//! a `run-pass` case must be accepted, generated, compiled by rustc and print
//! its expected output. Small inline programs cover the remaining branches
//! (thread transfer through generic arguments, recursive types, handles that
//! do cross threads).

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use std::path::{Path, PathBuf};

fn cases_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases")
}

/// Value of a JSON string field (the case files are flat and hand-written).
fn field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let rest = &json[start + key.len() + 2..];
    let open = rest.find('"')? + 1;
    let mut out = String::new();
    let mut chars = rest[open..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

fn codes(json: &str) -> Vec<String> {
    let Some(start) = json.find("\"expect_codes\"") else {
        return Vec::new();
    };
    let list = &json[start..];
    let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
    list.split(',')
        .map(|c| c.trim().trim_matches('"').to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

/// Error codes of `check` on a package directory, and the generated Rust if
/// it was accepted.
fn check_dir(dir: &Path) -> (Vec<String>, Option<String>) {
    let loaded = load_package_with_options(
        dir,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
            ..Default::default()
        },
    );
    let program = loaded.program.expect("the case loads");
    let result = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    let mut errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| d.severity == ravum_lang_sema::syntax::diag::Severity::Error)
        .map(|d| d.code.clone())
        .collect();
    errors.sort();
    errors.dedup();
    let rust = if errors.is_empty() {
        let checked = result.checked.expect("an accepted program is checked");
        Some(generate(&checked, &GenOptions::default()).expect("codegen"))
    } else {
        None
    };
    (errors, rust)
}

fn rustc_run(rust: &str, label: &str) -> String {
    let dir = std::env::temp_dir().join(format!("ravum-arena-e2e-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.rs");
    std::fs::write(&file, rust).unwrap();
    let bin = dir.join(if cfg!(windows) { "p.exe" } else { "p" });
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let out = std::process::Command::new(rustc)
        .args([
            "--edition",
            "2021",
            "-C",
            "overflow-checks=on",
            "-A",
            "warnings",
            "-o",
        ])
        .arg(&bin)
        .arg(&file)
        .output()
        .expect("rustc");
    assert!(
        out.status.success(),
        "{label}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = std::process::Command::new(&bin).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn every_arena_conformance_case_holds_on_path_b() {
    let mut seen = 0;
    for group in std::fs::read_dir(cases_root()).unwrap() {
        for case in std::fs::read_dir(group.unwrap().path()).unwrap() {
            let dir = case.unwrap().path();
            let name = dir.file_name().unwrap().to_string_lossy().to_string();
            if !name.starts_with("arena-") {
                continue;
            }
            let json = std::fs::read_to_string(dir.join("case.json")).unwrap();
            let mode = field(&json, "mode").unwrap();
            let (errors, rust) = check_dir(&dir);
            match mode.as_str() {
                "compile-fail" => assert_eq!(errors, codes(&json), "{name}"),
                "run-pass" => {
                    assert!(errors.is_empty(), "{name}: {errors:?}");
                    let want = field(&json, "stdout_contains").unwrap();
                    let got = rustc_run(&rust.unwrap(), &name);
                    assert!(got.contains(&want), "{name}: {got:?}");
                }
                // `run-fail` (a 2^20-arena loop) is left to the conformance
                // runner; here it must at least be accepted.
                _ => assert!(errors.is_empty(), "{name}: {errors:?}"),
            }
            seen += 1;
        }
    }
    assert!(seen >= 18, "only {seen} arena cases");
}

fn check_src(src: &str, label: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("ravum-arena-src-{}-{label}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        format!("[package]\nname = \"{label}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let (errors, _) = check_dir(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    errors
}

#[test]
fn arenas_never_cross_threads_but_handles_do() {
    let refused = [
        (
            "direct",
            "use std::thread;\nfn work(a: Arena<i64>) -> () { drop(a); }\nfn main() -> () effects(thread, alloc) {\n    let a: Arena<i64> = Arena::new();\n    let t: JoinHandle<()> = thread::spawn(work, a);\n    thread::join(t);\n}\n",
        ),
        (
            "result",
            "use std::thread;\nfn make(n: i64) -> Vec<Arena<i64>> effects(alloc) { let mut v: Vec<Arena<i64>> = Vec::new(); let mut a: Arena<i64> = Arena::new(); let _h = a.insert(n); v.push(a); v }\nfn main() -> () effects(thread, alloc) {\n    let t: JoinHandle<Vec<Arena<i64>>> = thread::spawn(make, 1i64);\n    let a = thread::join(t);\n    drop(a);\n}\n",
        ),
        (
            "tuple-result",
            "use std::sync;\nfn main() -> () effects(thread, alloc) {\n    let c: Channel<(i64, Result<i64, Arena<i64>>)> = sync::channel();\n    drop(c);\n}\n",
        ),
        (
            "variant-field",
            "use std::thread;\nenum Msg { Scene { items: Arena<i64> }, Quit }\nfn work(m: Msg) -> () { drop(m); }\nfn main() -> () effects(thread, alloc) {\n    let t: JoinHandle<()> = thread::spawn(work, Msg::Quit);\n    thread::join(t);\n}\n",
        ),
    ];
    for (label, src) in refused {
        assert_eq!(
            check_src(src, label),
            vec!["E-CONC-NOT-TRANSFERABLE".to_string()],
            "{label}"
        );
    }
    // A recursive type of handles crosses threads (the `seen` guard stops
    // the walk); so does an enum of handles.
    let accepted = "use std::thread;\nstruct Link { next: Option<Handle<Link>>, again: Vec<Link> }\nenum Pick { One(Handle<i64>), None2 }\nfn work(l: Link) -> () { drop(l); }\nfn pick(p: Pick) -> () { drop(p); }\nfn main() -> () effects(thread, alloc) {\n    let t: JoinHandle<()> = thread::spawn(work, Link { next: None, again: Vec::new() });\n    thread::join(t);\n    let u: JoinHandle<()> = thread::spawn(pick, Pick::None2);\n    thread::join(u);\n}\n";
    assert_eq!(check_src(accepted, "accepted"), Vec::<String>::new());
}
