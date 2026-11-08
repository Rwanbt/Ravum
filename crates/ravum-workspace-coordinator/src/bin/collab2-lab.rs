//! `collab2-lab` — measure four merge strategies on the `fixtures/collab2/corpus`
//! scenarios.
//!
//! Every merge that a strategy accepts is **actually compiled** with the public
//! `rustc` CLI (through `ravum-rust-adapter`, `--emit=metadata`), and the merged
//! snapshot id is recorded next to the verdict. A "score" never replaces the
//! compile check: an accepted merge without a compile verdict is a hard error.
//!
//! The four strategies are compared for measurement only; the coordinator keeps
//! the symbol/real-line strategy (see docs/mvp/MVP-0b-collab-2-3.md for the
//! measured table and the rationale).
//!
//! Usage:
//!   collab2-lab --corpus <dir> --work <dir> [--json <path>] [--rustc <path>]
//!
//! Exit codes: 0 = corpus fully measured and invariant holds; 1 = invariant
//! violated (e.g. an accepted merge with no compile verdict); 2 = unusable input.

mod common;

use common::RustcMergeVerifier;
use ravum_protocol::{sha256_hex, ChangeProposal, DecisionKind, SnapshotId, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::patch::{self, DiffLine, FilePatch};
use ravum_workspace_coordinator::{
    semantics, touched_ranges, Coordinator, MergeCheckKind, MergeVerifier, VerificationOutcome,
    VerificationRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

const STRATEGY_IDS: [&str; 4] = [
    "naive_textual",
    "strict_hunks",
    "syntactic_anchors",
    "symbols_ranges_deps",
];
const VERIFIED_ID: &str = "symbols_ranges_deps_verified";

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    description: String,
    path: String,
    /// `accept` | `textual_conflict` | `same_function_conflict` |
    /// `compile_failure` | `semantic_risk`.
    expect: String,
    #[serde(default)]
    symbols_a: Vec<String>,
    #[serde(default)]
    symbols_b: Vec<String>,
    #[serde(default)]
    merged_contains: Vec<String>,
}

struct Attempt {
    accepted: bool,
    reason: String,
    merged: Option<String>,
}

impl Attempt {
    fn accept(merged: String) -> Self {
        Self {
            accepted: true,
            reason: String::new(),
            merged: Some(merged),
        }
    }

    fn refuse(reason: impl Into<String>) -> Self {
        Self {
            accepted: false,
            reason: reason.into(),
            merged: None,
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct CompileVerdict {
    ok: bool,
    diagnostic_codes: Vec<String>,
    diagnostics: Vec<String>,
    snapshot_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct Outcome {
    accepted: bool,
    refused: bool,
    reason: String,
    merged_compiles: Option<bool>,
    diagnostic_codes: Vec<String>,
    merged_snapshot: Option<String>,
    contains_expected: Option<bool>,
    classification: String,
}

#[derive(Debug, Serialize)]
struct ScenarioResult {
    id: String,
    description: String,
    expect: String,
    base_compiles: bool,
    strategies: BTreeMap<String, Outcome>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    accepted: usize,
    refused: usize,
    correct_accepts: usize,
    correct_refusals: usize,
    false_conflicts: usize,
    incorrect_merges: usize,
    compile_failures_detected: usize,
    unsafe_accepts: usize,
    refused_before_compile: usize,
    accepted_without_compile: usize,
    compiled_all_accepted: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    corpus_dir: String,
    toolchain: Toolchain,
    scenario_count: usize,
    strategies: Vec<String>,
    results: Vec<ScenarioResult>,
    summary: BTreeMap<String, Summary>,
    elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
struct Toolchain {
    rustc: String,
    release: String,
    host: String,
    edition: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(corpus) = flag(&args, "--corpus").map(PathBuf::from) else {
        eprintln!("missing --corpus <dir>");
        std::process::exit(2);
    };
    let work = flag(&args, "--work")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/ravum/collab2-lab"));
    let json_path = flag(&args, "--json").map(PathBuf::from);
    let rustc = flag(&args, "--rustc")
        .or_else(|| std::env::var("RUSTC").ok())
        .unwrap_or_else(|| "rustc".to_owned());

    let code = run(&corpus, &work, &rustc, json_path.as_deref());
    std::process::exit(code);
}

fn run(corpus: &Path, work: &Path, rustc: &str, json_path: Option<&Path>) -> i32 {
    let started = Instant::now();
    let verifier = RustcMergeVerifier::new(rustc, work.join("verifier"), false);
    let (release, host) = verifier.toolchain();
    let toolchain = Toolchain {
        rustc: rustc.to_owned(),
        release,
        host,
        edition: "2021".to_owned(),
    };

    let scenarios = match load_scenarios(corpus) {
        Ok(scenarios) => scenarios,
        Err(error) => {
            eprintln!("cannot load corpus {}: {error}", corpus.display());
            return 2;
        }
    };
    if scenarios.len() < 8 {
        eprintln!(
            "corpus has {} scenarios, the mandate requires at least 8",
            scenarios.len()
        );
        return 2;
    }

    let mut results = Vec::new();
    let mut summaries: BTreeMap<String, Summary> = BTreeMap::new();
    for strategy in STRATEGY_IDS.iter().copied().chain(std::iter::once(VERIFIED_ID)) {
        summaries.insert(strategy.to_owned(), Summary::default());
    }

    for scenario in &scenarios {
        let base = match std::fs::read_to_string(corpus.join(&scenario.id).join("base.rs")) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("cannot read base.rs for {}: {error}", scenario.id);
                return 2;
            }
        };
        let a_patch = match std::fs::read_to_string(corpus.join(&scenario.id).join("a.patch")) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("cannot read a.patch for {}: {error}", scenario.id);
                return 2;
            }
        };
        let b_patch = match std::fs::read_to_string(corpus.join(&scenario.id).join("b.patch")) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("cannot read b.patch for {}: {error}", scenario.id);
                return 2;
            }
        };
        let base_verdict = compile_probe(&verifier, &scenario.path, &base);
        if !base_verdict.ok {
            eprintln!(
                "scenario {} base does not compile: {:?}",
                scenario.id, base_verdict.diagnostics
            );
            return 2;
        }

        let attempts: Vec<(&str, Attempt)> = vec![
            (
                "naive_textual",
                naive_textual(&base, &a_patch, &b_patch, &scenario.path),
            ),
            (
                "strict_hunks",
                strict_hunks(&base, &a_patch, &b_patch, &scenario.path),
            ),
            (
                "syntactic_anchors",
                syntactic_anchors(&base, &a_patch, &b_patch, &scenario.path),
            ),
            (
                "symbols_ranges_deps",
                symbols_ranges_deps(
                    &base,
                    &a_patch,
                    &b_patch,
                    &scenario.path,
                    &scenario.symbols_a,
                    &scenario.symbols_b,
                ),
            ),
        ];

        let mut verified_attempt = None;
        let mut outcomes = BTreeMap::new();
        for (id, attempt) in attempts {
            if id == "symbols_ranges_deps" {
                // The production pipeline adds the compile gate on top of the
                // retained textual strategy.
                verified_attempt = Some(retained_pipeline(&verifier, &scenario.path, &attempt));
            }
            let outcome = measure(
                &verifier,
                &scenario.path,
                &scenario.expect,
                &scenario.merged_contains,
                &attempt,
            );
            summaries.get_mut(id).expect("summary").record(&outcome);
            outcomes.insert(id.to_owned(), outcome);
        }
        if let Some(attempt) = verified_attempt {
            let outcome = measure(
                &verifier,
                &scenario.path,
                &scenario.expect,
                &scenario.merged_contains,
                &attempt,
            );
            summaries.get_mut(VERIFIED_ID).expect("summary").record(&outcome);
            outcomes.insert(VERIFIED_ID.to_owned(), outcome);
        }

        results.push(ScenarioResult {
            id: scenario.id.clone(),
            description: scenario.description.clone(),
            expect: scenario.expect.clone(),
            base_compiles: true,
            strategies: outcomes,
        });
    }

    let mut strategies: Vec<String> = STRATEGY_IDS.iter().map(|id| (*id).to_owned()).collect();
    strategies.push(VERIFIED_ID.to_owned());
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let report = Report {
        schema_version: 1,
        corpus_dir: corpus.display().to_string(),
        toolchain,
        scenario_count: results.len(),
        strategies,
        results,
        summary: summaries,
        elapsed_ms,
    };

    print_report(&report);

    if let Some(path) = json_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&report) {
            Ok(text) => {
                if let Err(error) = std::fs::write(path, text) {
                    eprintln!("cannot write report {}: {error}", path.display());
                    return 2;
                }
                println!("\nreport written to {}", path.display());
            }
            Err(error) => {
                eprintln!("cannot serialize report: {error}");
                return 2;
            }
        }
    }

    // Invariants: a merge accepted without a compile verdict is not evidence.
    let mut violations = 0;
    for result in &report.results {
        for (strategy, outcome) in &result.strategies {
            if outcome.accepted && outcome.merged_compiles.is_none() {
                eprintln!(
                    "invariant violated: {}/{} accepted without a compile verdict",
                    result.id, strategy
                );
                violations += 1;
            }
            if outcome.accepted && outcome.merged_snapshot.is_none() {
                eprintln!(
                    "invariant violated: {}/{} accepted without a merged snapshot id",
                    result.id, strategy
                );
                violations += 1;
            }
        }
    }
    if violations > 0 {
        return 1;
    }
    0
}

impl Summary {
    fn record(&mut self, outcome: &Outcome) {
        if outcome.accepted {
            self.accepted += 1;
            if outcome.merged_compiles.is_none() {
                self.accepted_without_compile += 1;
            }
        } else {
            self.refused += 1;
        }
        match outcome.classification.as_str() {
            "correct_accept" => self.correct_accepts += 1,
            "correct_refusal" => self.correct_refusals += 1,
            "false_conflict" => self.false_conflicts += 1,
            "incorrect_merge_compile_error" | "incorrect_merge_missing_content" => {
                self.incorrect_merges += 1
            }
            "compile_failure_detected" => self.compile_failures_detected += 1,
            "unsafe_accept_semantic" | "unsafe_accept_compiles" => self.unsafe_accepts += 1,
            "refused_before_compile" => self.refused_before_compile += 1,
            _ => {}
        }
        self.compiled_all_accepted = self.accepted_without_compile == 0;
    }
}

fn measure(
    verifier: &RustcMergeVerifier,
    path: &str,
    expect: &str,
    merged_contains: &[String],
    attempt: &Attempt,
) -> Outcome {
    let mut verdict = CompileVerdict::default();
    let mut contains_expected = None;
    let mut merged_snapshot = None;
    if let Some(merged) = &attempt.merged {
        verdict = compile_probe(verifier, path, merged);
        merged_snapshot = Some(sha256_hex(merged.as_bytes()));
        if expect == "accept" {
            contains_expected = Some(merged_contains.iter().all(|marker| merged.contains(marker)));
        }
    }
    let classification = classify(
        expect,
        attempt.accepted,
        verdict.ok,
        contains_expected,
        attempt.accepted && attempt.merged.is_some(),
        attempt.reason.contains("semantic verification failed"),
    );
    Outcome {
        accepted: attempt.accepted,
        refused: !attempt.accepted,
        reason: attempt.reason.clone(),
        merged_compiles: if attempt.accepted { Some(verdict.ok) } else { None },
        diagnostic_codes: verdict.diagnostic_codes,
        merged_snapshot,
        contains_expected,
        classification: classification.to_owned(),
    }
}

fn classify(
    expect: &str,
    accepted: bool,
    compiles: bool,
    contains_expected: Option<bool>,
    had_merged: bool,
    semantic_refusal: bool,
) -> &'static str {
    match expect {
        "accept" => {
            if !accepted || !had_merged {
                "false_conflict"
            } else if !compiles {
                "incorrect_merge_compile_error"
            } else if contains_expected == Some(true) {
                "correct_accept"
            } else {
                "incorrect_merge_missing_content"
            }
        }
        "textual_conflict" | "same_function_conflict" => {
            if !accepted {
                "correct_refusal"
            } else if !compiles {
                "incorrect_merge_compile_error"
            } else {
                "unsafe_accept_semantic"
            }
        }
        "compile_failure" => {
            if !accepted {
                if semantic_refusal {
                    "correct_refusal"
                } else {
                    "refused_before_compile"
                }
            } else if !compiles {
                "compile_failure_detected"
            } else {
                "unsafe_accept_compiles"
            }
        }
        "semantic_risk" => {
            if !accepted {
                "correct_refusal"
            } else if !compiles {
                "incorrect_merge_compile_error"
            } else {
                "unsafe_accept_semantic"
            }
        }
        _ => "unknown_expectation",
    }
}

/// The retained approach plus the semantic compile gate used by the binary.
fn retained_pipeline(verifier: &RustcMergeVerifier, path: &str, attempt: &Attempt) -> Attempt {
    if !attempt.accepted {
        return Attempt::refuse(attempt.reason.clone());
    }
    let Some(merged) = &attempt.merged else {
        return Attempt::refuse("no merged content");
    };
    let verdict = compile_probe(verifier, path, merged);
    if verdict.ok {
        Attempt::accept(merged.clone())
    } else {
        Attempt::refuse(format!(
            "semantic verification failed: merged snapshot does not compile ({:?})",
            verdict.diagnostic_codes
        ))
    }
}

// ---------------------------------------------------------------------------
// Strategy 1: naive textual merge (line numbers adjusted by prior hunk deltas).
// ---------------------------------------------------------------------------

fn naive_textual(base: &str, a_patch: &str, b_patch: &str, path: &str) -> Attempt {
    let a_files = match patch::parse(a_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("A patch invalid: {error}")),
    };
    let b_files = match patch::parse(b_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("B patch invalid: {error}")),
    };
    let Some(a_file) = a_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("A patch does not target the scenario path");
    };
    let Some(b_file) = b_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("B patch does not target the scenario path");
    };
    let ranges_a = touched_ranges(&a_files);
    let ranges_b = touched_ranges(&b_files);
    if let Some((overlap_path, line)) = range_overlap(&ranges_a, &ranges_b) {
        return Attempt::refuse(format!("modified line ranges overlap at {overlap_path}:{line}"));
    }
    let after_a = match patch::apply(a_file, base) {
        Ok(content) => content,
        Err(error) => return Attempt::refuse(format!("A does not apply: {error}")),
    };
    match apply_shifted(b_file, &after_a, a_file) {
        Ok(merged) => Attempt::accept(merged),
        Err(reason) => Attempt::refuse(reason),
    }
}

/// Apply `file` to `content`, shifting hunk positions by the deltas introduced by
/// `applied_before` (both patches were authored against the same base).
fn apply_shifted(
    file: &FilePatch,
    content: &str,
    applied_before: &FilePatch,
) -> Result<String, String> {
    let normalized = content.replace("\r\n", "\n");
    let trailing_newline = normalized.ends_with('\n');
    let mut lines: Vec<String> = if normalized.is_empty() {
        Vec::new()
    } else {
        normalized
            .trim_end_matches('\n')
            .split('\n')
            .map(ToOwned::to_owned)
            .collect()
    };
    for hunk in &file.hunks {
        let delta = delta_before(applied_before, hunk.old_start);
        let start = hunk.old_start as isize - 1 + delta;
        if start < 0 {
            return Err(format!("shifted hunk start is negative for line {}", hunk.old_start));
        }
        let start = start as usize;
        let mut index = start;
        let mut insertion: Vec<String> = Vec::new();
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) | DiffLine::Remove(text) => {
                    if lines.get(index) != Some(text) {
                        return Err(format!(
                            "context does not match after line shift at line {}",
                            index + 1
                        ));
                    }
                    if matches!(line, DiffLine::Context(_)) {
                        insertion.push(text.clone());
                    }
                    index += 1;
                }
                DiffLine::Add(text) => insertion.push(text.clone()),
            }
        }
        lines.splice(start..index, insertion);
    }
    let mut result = lines.join("\n");
    if trailing_newline {
        result.push('\n');
    }
    Ok(result)
}

fn delta_before(applied: &FilePatch, base_line: usize) -> isize {
    let mut delta = 0isize;
    for hunk in &applied.hunks {
        if hunk.old_start <= base_line {
            let mut old_len = 0isize;
            let mut new_len = 0isize;
            for line in &hunk.lines {
                match line {
                    DiffLine::Context(_) => {
                        old_len += 1;
                        new_len += 1;
                    }
                    DiffLine::Remove(_) => old_len += 1,
                    DiffLine::Add(_) => new_len += 1,
                }
            }
            delta += new_len - old_len;
        }
    }
    delta
}

// ---------------------------------------------------------------------------
// Strategy 2: hunk matching with strict context (no offset, no relocation).
// ---------------------------------------------------------------------------

fn strict_hunks(base: &str, a_patch: &str, b_patch: &str, path: &str) -> Attempt {
    let a_files = match patch::parse(a_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("A patch invalid: {error}")),
    };
    let b_files = match patch::parse(b_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("B patch invalid: {error}")),
    };
    let Some(a_file) = a_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("A patch does not target the scenario path");
    };
    let Some(b_file) = b_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("B patch does not target the scenario path");
    };
    let after_a = match patch::apply(a_file, base) {
        Ok(content) => content,
        Err(error) => return Attempt::refuse(format!("A does not apply: {error}")),
    };
    match patch::apply(b_file, &after_a) {
        Ok(merged) => Attempt::accept(merged),
        Err(error) => Attempt::refuse(format!("B does not apply on the merged content: {error}")),
    }
}

// ---------------------------------------------------------------------------
// Strategy 3: syntactic anchors (enclosing symbol + relocation of the changed
// block inside that symbol).
// ---------------------------------------------------------------------------

fn syntactic_anchors(base: &str, a_patch: &str, b_patch: &str, path: &str) -> Attempt {
    let a_files = match patch::parse(a_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("A patch invalid: {error}")),
    };
    let b_files = match patch::parse(b_patch) {
        Ok(files) => files,
        Err(error) => return Attempt::refuse(format!("B patch invalid: {error}")),
    };
    let Some(a_file) = a_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("A patch does not target the scenario path");
    };
    let Some(b_file) = b_files.iter().find(|file| file.path == path) else {
        return Attempt::refuse("B patch does not target the scenario path");
    };
    let spans = semantics::symbol_spans(base);
    let symbols_a = touched_symbols(&spans, &touched_ranges(&a_files));
    let symbols_b = touched_symbols(&spans, &touched_ranges(&b_files));
    if let Some(symbol) = symbols_a.intersection(&symbols_b).next() {
        return Attempt::refuse(format!("both agents modify symbol {symbol:?}"));
    }
    let after_a = match patch::apply(a_file, base) {
        Ok(content) => content,
        Err(error) => return Attempt::refuse(format!("A does not apply: {error}")),
    };
    match apply_anchored(b_file, base, &after_a) {
        Ok(merged) => Attempt::accept(merged),
        Err(reason) => Attempt::refuse(reason),
    }
}

fn touched_symbols(spans: &[semantics::SymbolSpan], ranges: &[(String, usize, usize)]) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    for (_, start, end) in ranges {
        for line in *start..=*end {
            if let Some(span) = semantics::enclosing_symbol(spans, line) {
                symbols.insert(span.name.clone());
            }
        }
    }
    symbols
}

#[derive(Default)]
struct BlockOp {
    insertion_line: usize,
    removed: Vec<(usize, String)>,
    added: Vec<String>,
}

fn apply_anchored(file: &FilePatch, base: &str, target: &str) -> Result<String, String> {
    let base_spans = semantics::symbol_spans(base);
    let target_spans = semantics::symbol_spans(target);
    let mut lines: Vec<String> = target.lines().map(ToOwned::to_owned).collect();
    let trailing_newline = target.ends_with('\n');

    let mut operations: Vec<BlockOp> = Vec::new();
    for hunk in &file.hunks {
        let mut old_line = hunk.old_start;
        let mut current: Option<BlockOp> = None;
        for line in &hunk.lines {
            match line {
                DiffLine::Context(_) => {
                    if let Some(op) = current.take() {
                        operations.push(op);
                    }
                    old_line += 1;
                }
                DiffLine::Remove(text) => {
                    let op = current.get_or_insert_with(|| BlockOp {
                        insertion_line: old_line,
                        ..BlockOp::default()
                    });
                    op.removed.push((old_line, text.clone()));
                    old_line += 1;
                }
                DiffLine::Add(text) => {
                    let op = current.get_or_insert_with(|| BlockOp {
                        insertion_line: old_line,
                        ..BlockOp::default()
                    });
                    op.added.push(text.clone());
                }
            }
        }
        if let Some(op) = current.take() {
            operations.push(op);
        }
    }
    // Apply from the end so earlier positions keep their indices.
    operations.sort_by_key(|op| std::cmp::Reverse(op.insertion_line));
    for op in operations {
        if !op.removed.is_empty() {
            let anchor_line = op.removed[0].0;
            let base_span = semantics::enclosing_symbol(&base_spans, anchor_line)
                .ok_or_else(|| format!("no syntactic anchor for line {anchor_line}"))?;
            let target_span = target_spans
                .iter()
                .find(|span| span.name == base_span.name)
                .ok_or_else(|| format!("anchor symbol {:?} disappeared from the merged content", base_span.name))?;
            let delta = target_span.start_line as isize - base_span.start_line as isize;
            let start = anchor_line as isize - 1 + delta;
            if start < 0 {
                return Err(format!("anchored start is negative for line {anchor_line}"));
            }
            let start = start as usize;
            let removed: Vec<&String> = op.removed.iter().map(|(_, text)| text).collect();
            for (offset, expected) in removed.iter().enumerate() {
                if lines.get(start + offset) != Some(*expected) {
                    return Err(format!(
                        "anchored context does not match at merged line {}",
                        start + offset + 1
                    ));
                }
            }
            lines.splice(start..start + op.removed.len(), op.added);
        } else {
            // Pure insertion: place it relative to the enclosing symbol when one
            // exists, otherwise at the head (line 1) or the tail.
            let insertion_line = op.insertion_line;
            let index = if insertion_line <= 1 {
                0
            } else if let Some(span) = semantics::enclosing_symbol(&base_spans, insertion_line) {
                let base_start = span.start_line;
                let target_span = target_spans
                    .iter()
                    .find(|candidate| candidate.name == span.name)
                    .ok_or_else(|| format!("anchor symbol {:?} disappeared", span.name))?;
                let delta = target_span.start_line as isize - base_start as isize;
                (insertion_line as isize - 1 + delta).max(0) as usize
            } else if insertion_line > base.lines().count() {
                lines.len()
            } else {
                return Err(format!(
                    "no syntactic anchor for insertion before line {insertion_line}"
                ));
            };
            let index = index.min(lines.len());
            lines.splice(index..index, op.added);
        }
    }
    let mut result = lines.join("\n");
    if trailing_newline {
        result.push('\n');
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Strategy 4: the coordinator's retained logic (declared symbols + real
// modified lines + exact apply), extended with the dependency graph.
// ---------------------------------------------------------------------------

fn symbols_ranges_deps(
    base: &str,
    a_patch: &str,
    b_patch: &str,
    path: &str,
    symbols_a: &[String],
    symbols_b: &[String],
) -> Attempt {
    let snapshot = SourceSnapshot::new([(path.to_owned(), base.to_owned())]);
    let mut coordinator = Coordinator::new(snapshot);
    let base_id = coordinator.base().id().clone();
    let base_hash = coordinator.base().file_hash(path).unwrap().to_owned();

    let proposal_a = proposal("agent-a", path, symbols_a, a_patch, &base_id, &base_hash);
    let decision_a = coordinator.submit(&proposal_a);
    if decision_a.decision != DecisionKind::AutoSafe {
        return Attempt::refuse(format!(
            "A refused: {} ({})",
            decision_a.decision.as_str(),
            decision_a.reasons.first().cloned().unwrap_or_default()
        ));
    }
    let proposal_b = proposal("agent-b", path, symbols_b, b_patch, &base_id, &base_hash);
    let decision_b = coordinator.submit(&proposal_b);
    if decision_b.decision != DecisionKind::AutoSafe {
        return Attempt::refuse(format!(
            "B refused: {} ({})",
            decision_b.decision.as_str(),
            decision_b.reasons.first().cloned().unwrap_or_default()
        ));
    }
    // Dependency graph: a changed line of B referencing a symbol A modified.
    let a_files = patch::parse(a_patch).unwrap_or_default();
    let b_files = patch::parse(b_patch).unwrap_or_default();
    let facts_a = semantics::analyse(coordinator.base(), &a_files);
    let facts_b = semantics::analyse(coordinator.base(), &b_files);
    let mut modified: semantics::ModifiedSymbols = BTreeMap::new();
    for symbol in &facts_a.modified_symbols {
        modified.insert(
            symbol.clone(),
            (path.to_owned(), "agent-a/t".to_owned(), "agent-a".to_owned()),
        );
    }
    if let Some(dependency) = semantics::undeclared_dependency(&facts_b, &modified) {
        return Attempt::refuse(format!(
            "manual: undeclared semantic dependency on symbol {:?} modified by {}",
            dependency.symbol, dependency.other_proposal
        ));
    }
    match coordinator.current().content_of(path) {
        Some(merged) => Attempt::accept(merged.to_owned()),
        None => Attempt::refuse("coordinator lost the merged content"),
    }
}

fn proposal(
    agent: &str,
    path: &str,
    symbols: &[String],
    unified_patch: &str,
    base_id: &SnapshotId,
    base_hash: &str,
) -> ChangeProposal {
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: agent.to_owned(),
        task_id: "lab".to_owned(),
        base_snapshot_id: base_id.clone(),
        target_paths: vec![path.to_owned()],
        target_symbols: symbols.to_vec(),
        expected_hashes: [(path.to_owned(), base_hash.to_owned())].into_iter().collect(),
        declared_dependencies: Vec::new(),
        impacted_contracts: Vec::new(),
        unified_patch: unified_patch.to_owned(),
        note: None,
    }
}

// ---------------------------------------------------------------------------
// Compile oracle
// ---------------------------------------------------------------------------

fn compile_probe(verifier: &RustcMergeVerifier, path: &str, content: &str) -> CompileVerdict {
    // The measurement oracle is the *production* verifier: what is measured is
    // exactly what gates an AUTO_SAFE in `ravum-collab`.
    let snapshot = SourceSnapshot::new([(path.to_owned(), content.to_owned())]);
    let snapshot_id = snapshot.id().to_string();
    let request = VerificationRequest {
        proposal_id: "lab".to_owned(),
        agent_id: "lab".to_owned(),
        snapshot_before: snapshot.clone(),
        snapshot_after: snapshot.clone(),
        changed_paths: vec![path.to_owned()],
        declared_symbols: Vec::new(),
        required_checks: vec![MergeCheckKind::MetadataCheck],
    };
    match verifier.verify(&request) {
        VerificationOutcome::Passed { .. } => CompileVerdict {
            ok: true,
            diagnostic_codes: Vec::new(),
            diagnostics: Vec::new(),
            snapshot_id: Some(snapshot_id),
        },
        VerificationOutcome::Failed { check, .. } => {
            let mut codes: Vec<String> = check
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.code.clone())
                .collect();
            codes.sort();
            codes.dedup();
            let diagnostics = check
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect();
            CompileVerdict {
                ok: false,
                diagnostic_codes: codes,
                diagnostics,
                snapshot_id: Some(snapshot_id),
            }
        }
        VerificationOutcome::Unavailable { reason } => CompileVerdict {
            ok: false,
            diagnostic_codes: vec!["verifier-unavailable".to_owned()],
            diagnostics: vec![reason],
            snapshot_id: Some(snapshot_id),
        },
    }
}

// ---------------------------------------------------------------------------
// Corpus loading and reporting
// ---------------------------------------------------------------------------

fn load_scenarios(corpus: &Path) -> Result<Vec<Scenario>, String> {
    let mut scenarios = Vec::new();
    let entries = std::fs::read_dir(corpus).map_err(|error| error.to_string())?;
    for entry in entries.filter_map(Result::ok) {
        if !entry.path().is_dir() {
            continue;
        }
        let manifest = entry.path().join("scenario.json");
        if !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
        let scenario: Scenario =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", manifest.display()))?;
        scenarios.push(scenario);
    }
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(scenarios)
}

fn range_overlap(
    left: &[(String, usize, usize)],
    right: &[(String, usize, usize)],
) -> Option<(String, usize)> {
    for (lpath, lstart, lend) in left {
        for (rpath, rstart, rend) in right {
            if lpath == rpath && lstart <= rend && rstart <= lend {
                return Some((lpath.clone(), (*lstart).max(*rstart)));
            }
        }
    }
    None
}

fn print_report(report: &Report) {
    println!(
        "collab2-lab — {} scenarios, toolchain rustc {} ({})",
        report.scenario_count, report.toolchain.release, report.toolchain.host
    );
    println!(
        "{:<38} {:<22} {:<14} {:<14} {:<14} {:<14} {:<16}",
        "scenario", "expect", "naive_textual", "strict_hunks", "syntactic", "symbols", "verified"
    );
    for result in &report.results {
        let cell = |id: &str| -> String {
            match result.strategies.get(id) {
                Some(outcome) => match outcome.classification.as_str() {
                    "correct_accept" => "ACCEPT/ok".to_owned(),
                    "correct_refusal" => "REFUSE".to_owned(),
                    "false_conflict" => "FALSE-CONFLICT".to_owned(),
                    "compile_failure_detected" => "ACCEPT+E".to_owned(),
                    "compile_failure_detected_by_oracle" => "REFUSE+E".to_owned(),
                    "unsafe_accept_semantic" => "UNSAFE-ACCEPT".to_owned(),
                    "unsafe_accept_compiles" => "UNSAFE-ACCEPT".to_owned(),
                    "incorrect_merge_compile_error" => "BAD-MERGE".to_owned(),
                    "incorrect_merge_missing_content" => "BAD-MERGE".to_owned(),
                    "refused_before_compile" => "REFUSE".to_owned(),
                    other => other.to_owned(),
                },
                None => "-".to_owned(),
            }
        };
        println!(
            "{:<38} {:<22} {:<14} {:<14} {:<14} {:<14} {:<16}",
            truncate(&result.id, 38),
            result.expect,
            cell("naive_textual"),
            cell("strict_hunks"),
            cell("syntactic_anchors"),
            cell("symbols_ranges_deps"),
            cell(VERIFIED_ID),
        );
    }
    println!(
        "\n{:<26} {:>8} {:>8} {:>15} {:>16} {:>18} {:>18} {:>14} {:>14} {:>12}",
        "strategy", "accepted", "refused", "false_conflict", "bad_merge", "unsafe_semantic", "compile_caught", "correct_accept", "correct_refuse", "all_compiled"
    );
    for (id, summary) in &report.summary {
        println!(
            "{:<26} {:>8} {:>8} {:>15} {:>16} {:>18} {:>18} {:>14} {:>14} {:>12}",
            id,
            summary.accepted,
            summary.refused,
            summary.false_conflicts,
            summary.incorrect_merges,
            summary.unsafe_accepts,
            summary.compile_failures_detected,
            summary.correct_accepts,
            summary.correct_refusals,
            summary.compiled_all_accepted
        );
    }
    println!("\nelapsed: {} ms", report.elapsed_ms);
}

fn truncate(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    if text.chars().count() > width {
        out.push('…');
    }
    out
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}


