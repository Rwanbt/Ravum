//! Merge-strategy comparison (COLLAB-2) — the measurement is produced by the
//! `collab2-lab` binary on the static corpus under `fixtures/collab2/corpus`, and
//! this test asserts the properties the measurement must satisfy.
//!
//! A score never replaces a test: the lab compiles **every** merge it accepts with
//! the production `rustc` verifier, and records the merged snapshot digest next to
//! the verdict. Accepting without a compile verdict is a lab invariant failure.

mod support;

use serde_json::Value;
use std::process::Command;
use std::sync::OnceLock;
use support::*;

static REPORT: OnceLock<Value> = OnceLock::new();

fn report() -> &'static Value {
    REPORT.get_or_init(|| {
        let corpus = fixtures().join("corpus");
        assert!(corpus.is_dir(), "missing corpus at {}", corpus.display());
        let work = tests_root().join("collab2-lab-work");
        let report_path = tests_root().join("collab2-lab-report.json");
        let output = Command::new(LAB_BIN)
            .arg("--corpus")
            .arg(&corpus)
            .arg("--work")
            .arg(&work)
            .arg("--json")
            .arg(&report_path)
            .output()
            .expect("run collab2-lab");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            output.status.success(),
            "collab2-lab failed (exit {:?}):\nstderr: {stderr}\nstdout: {stdout}",
            output.status.code()
        );
        let text = std::fs::read_to_string(&report_path)
            .unwrap_or_else(|e| panic!("cannot read lab report: {e}\nstdout: {stdout}"));
        let parsed: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("lab report is not JSON: {e}"));
        // The human table must name every scenario; it is the artifact quoted in
        // docs/mvp/MVP-0b-collab-2-3.md.
        for result in parsed["results"].as_array().expect("results") {
            let id = result["id"].as_str().expect("scenario id");
            assert!(stdout.contains(id), "lab table is missing {id}:\n{stdout}");
        }
        parsed
    })
}

fn results() -> &'static Vec<Value> {
    report()["results"].as_array().expect("results")
}

fn scenario(id: &str) -> &'static Value {
    results()
        .iter()
        .find(|result| result["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("scenario {id} is missing from the corpus"))
}

fn outcome(id: &str, strategy: &str) -> &'static Value {
    &scenario(id)["strategies"][strategy]
}

fn classification(id: &str, strategy: &str) -> &'static str {
    outcome(id, strategy)["classification"]
        .as_str()
        .unwrap_or("<missing>")
}

fn summary(strategy: &str) -> &'static Value {
    &report()["summary"][strategy]
}

#[test]
fn the_corpus_covers_the_mandate_and_every_accepted_merge_is_compiled() {
    let report = report();
    let scenario_count = report["scenario_count"].as_u64().expect("scenario_count");
    assert!(
        scenario_count >= 8,
        "the mandate requires at least 8 scenarios, got {scenario_count}"
    );
    assert_eq!(results().len() as u64, scenario_count);

    // The required axes of the mandate are all represented.
    for required in [
        "s01_two_functions",
        "s02_adjacent_functions",
        "s03_same_function",
        "s04_line_shift_insert_head",
        "s05_append_tail",
        "s06_symbol_moved",
        "s07_name_conflict",
        "s08_import_change",
        "s09_same_function_disjoint_lines",
        "s10_undeclared_dependency",
    ] {
        scenario(required);
    }

    let strategies = report["strategies"].as_array().expect("strategies");
    assert_eq!(strategies.len(), 5, "4 compared strategies + the verified pipeline");

    let mut accepted_total = 0usize;
    for result in results() {
        assert_eq!(result["base_compiles"], true, "base must compile: {result}");
        for (strategy, outcome) in result["strategies"].as_object().expect("strategies map") {
            if outcome["accepted"] == true {
                accepted_total += 1;
                assert!(
                    outcome["merged_compiles"].is_boolean(),
                    "accepted merge without a compile verdict: {}/{}",
                    result["id"],
                    strategy
                );
                assert!(
                    outcome["merged_snapshot"]
                        .as_str()
                        .map(|snapshot| snapshot.len() == 64)
                        .unwrap_or(false),
                    "accepted merge without a merged snapshot digest: {}/{}",
                    result["id"],
                    strategy
                );
            } else {
                assert!(
                    outcome["merged_compiles"].is_null(),
                    "a refusal must not claim a compile verdict: {}/{}",
                    result["id"],
                    strategy
                );
            }
        }
    }
    assert!(accepted_total > 0);
    for strategy in strategies.iter().filter_map(Value::as_str) {
        assert_eq!(
            summary(strategy)["compiled_all_accepted"], true,
            "strategy {strategy} accepted a merge it never compiled"
        );
        assert_eq!(summary(strategy)["accepted_without_compile"], 0);
    }
}

#[test]
fn the_measures_separate_the_four_strategies_and_justify_the_retained_one() {
    // (1) Pure textual merges cannot see a semantic conflict that compiles:
    //     same function, disjoint lines. The symbol-aware strategies refuse.
    assert_eq!(classification("s09_same_function_disjoint_lines", "naive_textual"), "unsafe_accept_semantic");
    assert_eq!(classification("s09_same_function_disjoint_lines", "strict_hunks"), "unsafe_accept_semantic");
    assert_eq!(
        classification("s09_same_function_disjoint_lines", "symbols_ranges_deps"),
        "correct_refusal"
    );
    assert_eq!(
        classification("s09_same_function_disjoint_lines", "symbols_ranges_deps_verified"),
        "correct_refusal"
    );

    // (2) A line shift after a head insertion falsely conflicts when the merge
    //     layer refuses offsets (the retained strategy), and is handled by the
    //     naive offset compensation and by syntactic anchoring.
    assert_eq!(classification("s04_line_shift_insert_head", "naive_textual"), "correct_accept");
    assert_eq!(classification("s04_line_shift_insert_head", "strict_hunks"), "false_conflict");
    assert_eq!(classification("s04_line_shift_insert_head", "syntactic_anchors"), "correct_accept");
    assert_eq!(
        classification("s04_line_shift_insert_head", "symbols_ranges_deps"),
        "false_conflict"
    );

    // (3) A moved symbol defeats both line offsets and strict context: only the
    //     symbol anchor relocates the edit correctly.
    assert_eq!(classification("s06_symbol_moved", "naive_textual"), "false_conflict");
    assert_eq!(classification("s06_symbol_moved", "strict_hunks"), "false_conflict");
    assert_eq!(classification("s06_symbol_moved", "syntactic_anchors"), "correct_accept");

    // (4) Two independent-looking renames collide at compile time: only the
    //     compile gate turns this into a refusal. The comparison records the
    //     compile evidence instead of silently accepting.
    for strategy in ["naive_textual", "strict_hunks", "syntactic_anchors", "symbols_ranges_deps"] {
        assert_eq!(
            classification("s07_name_conflict", strategy),
            "compile_failure_detected",
            "strategy {strategy} should accept textually and be caught by compilation"
        );
        assert_eq!(
            classification("s08_import_change", strategy),
            "compile_failure_detected",
            "strategy {strategy} should accept textually and be caught by compilation"
        );
    }
    assert_eq!(
        classification("s07_name_conflict", "symbols_ranges_deps_verified"),
        "correct_refusal"
    );
    assert_eq!(
        classification("s08_import_change", "symbols_ranges_deps_verified"),
        "correct_refusal"
    );
    let codes = outcome("s07_name_conflict", "symbols_ranges_deps")["diagnostic_codes"]
        .as_array()
        .expect("diagnostic_codes");
    assert!(
        codes.iter().any(|code| code == "E0428"),
        "expected E0428, got {codes:?}"
    );
    let codes = outcome("s08_import_change", "symbols_ranges_deps")["diagnostic_codes"]
        .as_array()
        .expect("diagnostic_codes");
    assert!(
        codes
            .iter()
            .any(|code| code == "E0404" || code == "E0405"),
        "expected a missing/ambivalent trait error (E0404/E0405), got {codes:?}"
    );

    // (5) An undeclared semantic dependency compiles and is textually clean; only
    //     the dependency graph refuses it.
    for strategy in ["naive_textual", "strict_hunks", "syntactic_anchors"] {
        assert_eq!(
            classification("s10_undeclared_dependency", strategy),
            "unsafe_accept_semantic",
            "strategy {strategy} cannot see a semantic dependency"
        );
    }
    assert_eq!(
        classification("s10_undeclared_dependency", "symbols_ranges_deps"),
        "correct_refusal"
    );

    // (6) The retained approach + the compile gate has no incorrect merge and no
    //     unsafe semantic acceptance in the corpus. Its measured cost is the two
    //     false conflicts on line shifts/moves, documented in the report.
    let verified = summary("symbols_ranges_deps_verified");
    assert_eq!(verified["incorrect_merges"], 0);
    assert_eq!(verified["unsafe_accepts"], 0);
    assert_eq!(verified["compile_failures_detected"], 0);
    assert_eq!(verified["false_conflicts"], 2);
    assert_eq!(verified["correct_accepts"], 3);
    assert_eq!(verified["correct_refusals"], 5);

    // The naive strategy is the most permissive one: it accepts more merges, and
    // pays with incorrect semantic/compile accepts. A score never replaces the
    // compile check, which is why the retained strategy does not adopt it.
    assert!(
        summary("naive_textual")["unsafe_accepts"].as_u64().unwrap()
            >= 1,
        "naive textual merging should show at least one unsafe accept"
    );
    assert!(
        summary("naive_textual")["compile_failures_detected"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert!(
        summary("strict_hunks")["false_conflicts"].as_u64().unwrap() >= 2
    );
    assert!(
        summary("syntactic_anchors")["false_conflicts"].as_u64().unwrap()
            <= summary("strict_hunks")["false_conflicts"].as_u64().unwrap()
    );
}

