//! Integration smoke tests for the parser against the real `examples/` corpus.
//!
//! These tests are owned by the Lead (not the syntax worker): they state the
//! minimum contract every example must satisfy. A parser regression shows up
//! here even if the worker's own unit tests pass.

use ravum_lang_syntax::ast::{ItemKind, Program};
use ravum_lang_syntax::parser::parse_file;
use std::path::{Path, PathBuf};

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn all_rv_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read examples dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            all_rv_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rv") {
            out.push(path);
        }
    }
}

fn parse_ok(path: &Path) -> Program {
    let src = std::fs::read_to_string(path).expect("read source");
    let parsed = parse_file(0, &src);
    let errors: Vec<String> = parsed
        .diags
        .iter()
        .map(|d| format!("{}: {} [{}..{}]", d.code, d.message, d.span.start, d.span.end))
        .collect();
    assert!(
        !parsed.diags.has_errors(),
        "{} should parse without errors, got:\n{}",
        path.display(),
        errors.join("\n")
    );
    parsed.program
}

fn count_fns(program: &Program) -> usize {
    fn walk(module: &ravum_lang_syntax::ast::Module, acc: &mut usize) {
        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(_) => *acc += 1,
                ItemKind::Mod(m) => walk(m, acc),
                ItemKind::Impl(i) => {
                    for sub in &i.items {
                        if matches!(sub.kind, ItemKind::Fn(_)) {
                            *acc += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut n = 0;
    walk(&program.root, &mut n);
    n
}

#[test]
fn every_example_parses_without_diagnostics() {
    let root = examples_root();
    let mut files = Vec::new();
    all_rv_files(&root, &mut files);
    assert!(!files.is_empty(), "no .rv example found under {}", root.display());
    let mut failures = Vec::new();
    for file in files {
        let src = std::fs::read_to_string(&file).expect("read source");
        let parsed = parse_file(0, &src);
        let errors: Vec<String> = parsed
            .diags
            .iter()
            .filter(|d| d.severity == ravum_lang_syntax::Severity::Error)
            .map(|d| format!("  {}[{}]: {}", d.code, d.span.start, d.message))
            .collect();
        if !errors.is_empty() {
            failures.push(format!("{}\n{}", file.display(), errors.join("\n")));
            continue;
        }
        assert!(
            parsed.program.root.id != 0,
            "{}: root module has no node id",
            file.display()
        );
        assert!(
            count_fns(&parsed.program) >= 1,
            "{}: no function parsed",
            file.display()
        );
    }
    assert!(failures.is_empty(), "parse failures:\n{}", failures.join("\n\n"));
}

#[test]
fn hello_has_one_main() {
    let program = parse_ok(&examples_root().join("hello").join("src").join("main.rv"));
    let fns: Vec<&ravum_lang_syntax::ast::FnItem> = program
        .root
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(fns.len(), 1, "hello must have exactly one fn");
    assert_eq!(fns[0].name.name, "main");
    assert!(fns[0].params.is_empty());
    assert!(matches!(fns[0].ret.kind, ravum_lang_syntax::ast::TypeKind::Unit));
}

#[test]
fn truncated_input_never_panics() {
    let full = std::fs::read_to_string(
        examples_root().join("fib").join("src").join("main.rv"),
    )
    .expect("read fib");
    for cut in (0..full.len()).step_by(7) {
        let mut end = cut;
        while end < full.len() && !full.is_char_boundary(end) {
            end += 1;
        }
        let parsed = parse_file(0, &full[..end]);
        // No panic and ids must remain unique-ish (non-zero) even on recovery.
        assert!(parsed.program.root.id != 0);
    }
}

#[test]
fn empty_and_comment_only_files_parse() {
    for src in ["", "// only a comment\n", "/* block */\n"] {
        let parsed = parse_file(0, src);
        assert!(!parsed.diags.has_errors(), "unexpected errors for {src:?}");
        assert!(parsed.program.root.items.is_empty());
    }
}
