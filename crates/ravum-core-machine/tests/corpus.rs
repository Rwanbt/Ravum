//! The oracle on its own corpus: every program in `corpus/` states, on its
//! first line, the result the machine must give (`// oracle: …`). The
//! obligations O1 to O10 of `spec/language/core-machine.md` §8 each have at
//! least one program.

use ravum_core_machine::{run, Outcome, DEFAULT_BUDGET, SPEC_VERSION};
use std::fs;
use std::path::Path;

fn corpus() -> Vec<(String, String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("corpus directory") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rv") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let expected = source
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// oracle: "))
            .unwrap_or_else(|| panic!("{} has no `// oracle:` line", path.display()))
            .to_string();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        out.push((name, source, expected));
    }
    out.sort();
    out
}

#[test]
fn every_corpus_program_gives_its_stated_result() {
    let mut failures = Vec::new();
    let all = corpus();
    assert!(all.len() >= 50, "corpus shrank: {}", all.len());
    for (name, source, expected) in &all {
        let got = run(source, "check", SPEC_VERSION, DEFAULT_BUDGET).render();
        if &got != expected {
            failures.push(format!("{name}: expected `{expected}`, got `{got}`"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_obligation_has_a_program() {
    let names: Vec<String> = corpus().into_iter().map(|(n, _, _)| n).collect();
    for o in 1..=10 {
        let prefix = format!("o{o}-");
        assert!(
            names.iter().any(|n| n.starts_with(&prefix)),
            "obligation O{o} has no program"
        );
    }
}

#[test]
fn the_machine_is_deterministic() {
    // MACH-1.2, MACH-9.3: same program, same result, every time.
    for (name, source, _) in corpus().iter().take(20) {
        let a = run(source, "check", SPEC_VERSION, DEFAULT_BUDGET);
        let b = run(source, "check", SPEC_VERSION, DEFAULT_BUDGET);
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn an_unknown_specification_version_is_refused() {
    // MACH-9.2.
    let r = run(
        "fn check() -> i32 effects() { return 1; }",
        "check",
        "core-1.0-draft.0",
        10,
    );
    assert!(matches!(r, Outcome::Fault(m) if m.contains("unknown specification version")));
}

#[test]
fn the_budget_is_a_parameter_and_divergence_is_inconclusive() {
    // MACH-3.3, MACH-7.1, MACH-7.2: a smaller budget changes nothing but the
    // number reported; the outcome is `diverge`, never a value.
    let src = "fn check() -> i32 effects() {\n    loop {\n    }\n}\n";
    assert_eq!(run(src, "check", SPEC_VERSION, 50), Outcome::Diverge(50));
    let fin = "fn check() -> i32 effects() {\n    let mut i: i32 = 0;\n    while i < 100 {\n        i += 1;\n    }\n    return i;\n}\n";
    assert_eq!(run(fin, "check", SPEC_VERSION, 20).render(), "diverge: 20");
    assert_eq!(
        run(fin, "check", SPEC_VERSION, DEFAULT_BUDGET).render(),
        "value i32 100"
    );
}

#[test]
fn an_unbound_name_is_a_fault_never_a_default() {
    // MACH-3.1.
    let r = run(
        "fn check() -> i32 effects() {\n    return y;\n}\n",
        "check",
        SPEC_VERSION,
        100,
    );
    assert_eq!(r, Outcome::Fault("unbound y".into()));
}

#[test]
fn a_program_outside_the_pure_core_is_refused() {
    // MACH-1.1, MACH-2.1: effects, `String`, `Vec` and references other than
    // `&str` are outside the domain.
    for src in [
        "fn check() -> i32 effects(io) {\n    return 1;\n}\n",
        "fn check() -> i32 {\n    return 1;\n}\n",
        "fn f(r: &i32) -> i32 effects() {\n    return 1;\n}\nfn check() -> i32 effects() {\n    return 1;\n}\n",
    ] {
        let r = run(src, "check", SPEC_VERSION, 100);
        assert!(
            matches!(&r, Outcome::Fault(m) if m.starts_with("outside the pure core")),
            "{src}: {r:?}"
        );
    }
}
