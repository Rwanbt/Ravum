//! Patch-level semantic facts used by the COLLAB-2/COLLAB-3 gate.
//!
//! This module is deliberately a **heuristic**, not a Rust parser and not an
//! AST: the coordinator must not depend on `rustc_private`. Every fact produced
//! here is a candidate signal; the authoritative check remains the injected
//! `MergeVerifier` (`rustc` compile, and optionally the test harness). The
//! signals are used to:
//!
//! * require a semantic verification instead of accepting a textually clean
//!   merge (behavioural change in a snapshot that ships tests);
//! * refuse contract/ABI modifications without human arbitration (INV-02);
//! * detect an *undeclared* dependency: a changed line references a symbol whose
//!   definition another agent already modified, and the proposal did not declare
//!   that dependency.
//!
//! Known limits (documented in `docs/mvp/MVP-0b-collab-2-3.md`): multi-line
//! signatures, macros generating items, raw strings, and `impl`/trait method
//! disambiguation are only approximated. A heuristic refusal is fail-closed
//! (`MANUAL_REQUIRED`), never a silent acceptance.

use crate::patch::{touched_ranges, FilePatch};
use ravum_protocol::sha256_hex;
use ravum_snapshot::SourceSnapshot;
use std::collections::{BTreeMap, BTreeSet};

/// A definition span attributed by the brace-matching heuristic (1-based,
/// inclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Removed,
    Added,
}

/// One line the patch actually adds or removes (context is never a change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedLine {
    pub path: String,
    /// Old-side line number. For an addition, the line *before which* the line is
    /// inserted (the base coordinate the agent authored against).
    pub old_line: usize,
    pub kind: ChangeKind,
    pub text: String,
    /// Symbol the changed line belongs to, in base coordinates.
    pub symbol: Option<String>,
    /// True when the changed line is a definition header (signature line).
    pub is_signature: bool,
}

impl ChangedLine {
    pub fn is_comment(&self) -> bool {
        let trimmed = self.text.trim_start();
        trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*")
    }
}

/// A contract-level modification that must not be auto-integrated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractRisk {
    /// A public item header was removed or rewritten (public API change).
    PublicSurface(ChangedLine),
    /// An ABI marker (`extern "C"`, `#[no_mangle]`, `#[repr(C)]`, ...) changed.
    AbiSurface(ChangedLine),
    /// A contract assertion (`assert!`, `debug_assert!`, ...) was removed.
    RemovedAssertion(ChangedLine),
}

impl ContractRisk {
    pub fn line(&self) -> &ChangedLine {
        match self {
            ContractRisk::PublicSurface(line)
            | ContractRisk::AbiSurface(line)
            | ContractRisk::RemovedAssertion(line) => line,
        }
    }

    pub fn reason(&self) -> String {
        match self {
            ContractRisk::PublicSurface(line) => format!(
                "public API surface changed at {}:{} ({}): external contracts require human arbitration",
                line.path,
                line.old_line,
                one_line(&line.text)
            ),
            ContractRisk::AbiSurface(line) => format!(
                "ABI surface changed at {}:{} ({}): ABI compatibility cannot be proven by a local compile, human arbitration required",
                line.path,
                line.old_line,
                one_line(&line.text)
            ),
            ContractRisk::RemovedAssertion(line) => format!(
                "a contract assertion was removed at {}:{} ({}): weakening a contract requires human arbitration",
                line.path,
                line.old_line,
                one_line(&line.text)
            ),
        }
    }
}

/// Everything the gate learned from one proposal's patch, against the snapshot
/// the proposal was authored on (the coordinator's *current* snapshot).
#[derive(Debug, Clone, Default)]
pub struct PatchFacts {
    pub changed_lines: Vec<ChangedLine>,
    pub touched: Vec<(String, usize, usize)>,
    /// Symbols whose definition span contains a changed line.
    pub modified_symbols: BTreeSet<String>,
    /// Identifiers referenced by changed lines that are defined somewhere in the
    /// snapshot (used for undeclared-dependency detection).
    pub referenced_symbols: BTreeSet<String>,
    pub contract_risks: Vec<ContractRisk>,
}

impl PatchFacts {
    /// A body-level change: at least one changed line belongs to a definition but
    /// is not its header. Such a change can preserve compilation while changing
    /// behaviour, so tests are required when the snapshot ships tests.
    pub fn behavioural_change(&self) -> bool {
        self.changed_lines
            .iter()
            .any(|line| line.symbol.is_some() && !line.is_signature && !line.is_comment())
    }

    pub fn changed_paths(&self) -> Vec<String> {
        let mut paths: BTreeSet<&str> = BTreeSet::new();
        for line in &self.changed_lines {
            paths.insert(line.path.as_str());
        }
        for (path, _, _) in &self.touched {
            paths.insert(path.as_str());
        }
        paths.into_iter().map(ToOwned::to_owned).collect()
    }

    pub fn contract_reasons(&self) -> Vec<String> {
        self.contract_risks
            .iter()
            .map(ContractRisk::reason)
            .collect()
    }

    /// Deterministic digest of the facts, used in policy evidence ids.
    pub fn evidence_id(&self, candidate_snapshot: &ravum_protocol::SnapshotId) -> String {
        let mut buf = format!("ravum-patch-facts-v1\0{}\0", candidate_snapshot.as_str());
        for line in &self.changed_lines {
            buf.push_str(&line.path);
            buf.push('\0');
            buf.push_str(&line.old_line.to_string());
            buf.push('\0');
            buf.push(match line.kind {
                ChangeKind::Removed => '-',
                ChangeKind::Added => '+',
            });
            buf.push('\0');
            buf.push_str(&line.text);
            buf.push('\0');
            buf.push_str(line.symbol.as_deref().unwrap_or(""));
            buf.push('\0');
        }
        for (path, start, end) in &self.touched {
            buf.push_str(path);
            buf.push('\0');
            buf.push_str(&format!("{start}:{end}"));
            buf.push('\0');
        }
        sha256_hex(buf.as_bytes())
    }
}

/// Compute the semantic facts of `files` against `snapshot`.
pub fn analyse(snapshot: &SourceSnapshot, files: &[FilePatch]) -> PatchFacts {
    let mut facts = PatchFacts {
        touched: touched_ranges(files),
        ..PatchFacts::default()
    };

    // Symbols defined anywhere in the current snapshot: the candidate universe
    // for a referenced-symbol dependency.
    let mut workspace_symbols: BTreeSet<String> = BTreeSet::new();
    for path in snapshot.paths() {
        if let Some(content) = snapshot.content_of(path) {
            for span in symbol_spans(content) {
                workspace_symbols.insert(span.name);
            }
        }
    }

    for file in files {
        let Some(content) = snapshot.content_of(&file.path) else {
            continue;
        };
        let spans = symbol_spans(content);
        for hunk in &file.hunks {
            let mut old_line = hunk.old_start;
            for line in &hunk.lines {
                use crate::patch::DiffLine;
                match line {
                    DiffLine::Context(_) => old_line += 1,
                    DiffLine::Remove(text) => {
                        let changed = changed_line(&file.path, old_line, ChangeKind::Removed, text, &spans);
                        record(&mut facts, changed);
                        old_line += 1;
                    }
                    DiffLine::Add(text) => {
                        let changed = changed_line(&file.path, old_line, ChangeKind::Added, text, &spans);
                        record(&mut facts, changed);
                    }
                }
            }
        }
    }

    // Only *added* lines create a dependency: removing the last call to a symbol
    // removes a dependency, it does not add one. (The measurement corpus checks
    // both directions: `s10` keeps the call, the positive control removes it.)
    for line in &facts.changed_lines {
        if line.kind != ChangeKind::Added || line.is_comment() {
            continue;
        }
        for identifier in identifiers(&line.text) {
            if workspace_symbols.contains(&identifier) {
                facts.referenced_symbols.insert(identifier);
            }
        }
    }
    facts
}

fn changed_line(
    path: &str,
    old_line: usize,
    kind: ChangeKind,
    text: &str,
    spans: &[SymbolSpan],
) -> ChangedLine {
    let span = enclosing_symbol(spans, old_line);
    ChangedLine {
        path: path.to_owned(),
        old_line,
        kind,
        text: text.to_owned(),
        symbol: span.map(|span| span.name.clone()),
        is_signature: span
            .map(|span| span.start_line == old_line)
            .unwrap_or(false),
    }
}

fn record(facts: &mut PatchFacts, line: ChangedLine) {
    if let Some(symbol) = &line.symbol {
        facts.modified_symbols.insert(symbol.clone());
    }
    if !line.is_comment() {
        if line.kind == ChangeKind::Removed && line_has_public_token(&line.text) {
            facts
                .contract_risks
                .push(ContractRisk::PublicSurface(line.clone()));
        }
        if line_has_abi_marker(&line.text) {
            facts
                .contract_risks
                .push(ContractRisk::AbiSurface(line.clone()));
        }
        if line.kind == ChangeKind::Removed && line_has_assertion(&line.text) {
            facts
                .contract_risks
                .push(ContractRisk::RemovedAssertion(line.clone()));
        }
    }
    facts.changed_lines.push(line);
}

/// True when the snapshot ships test code (`#[test]` or a `#[cfg(test)]` module).
pub fn has_test_code(snapshot: &SourceSnapshot) -> bool {
    snapshot.paths().any(|path| {
        snapshot
            .content_of(path)
            .map(|content| content.contains("#[test]") || content.contains("#[cfg(test)]"))
            .unwrap_or(false)
    })
}

/// Symbols whose definition spans contain `line` (1-based). Innermost wins.
pub fn enclosing_symbol(spans: &[SymbolSpan], line: usize) -> Option<&SymbolSpan> {
    spans
        .iter()
        .filter(|span| span.start_line <= line && line <= span.end_line)
        .max_by_key(|span| (span.start_line, std::cmp::Reverse(span.end_line)))
}

/// Heuristic definition spans of one source file. Brace matching ignores braces
/// inside strings, char literals and line comments; block comments that span
/// lines are not modelled (documented limitation).
pub fn symbol_spans(content: &str) -> Vec<SymbolSpan> {
    let lines: Vec<&str> = content.lines().collect();
    let mut spans = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((name, body)) = definition_head(line) else {
            continue;
        };
        let start_line = index + 1;
        let end_line = if body {
            find_open_brace(&lines, index)
                .and_then(|open| block_end(&lines, open))
                .map(|end| end + 1)
                .unwrap_or(start_line)
        } else {
            start_line
        };
        spans.push(SymbolSpan {
            name,
            start_line,
            end_line,
        });
    }
    spans.sort_by_key(|span| (span.start_line, span.end_line));
    spans
}

/// Identifiers referenced by one line (keywords and literals excluded).
pub fn identifiers(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch == '_' || ch.is_alphanumeric() {
            current.push(ch);
        } else {
            flush_identifier(&mut out, &mut current);
        }
    }
    flush_identifier(&mut out, &mut current);
    out
}

fn flush_identifier(out: &mut BTreeSet<String>, current: &mut String) {
    if current.is_empty() {
        return;
    }
    let word = std::mem::take(current);
    if word.len() < 2 {
        return;
    }
    if word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
        return;
    }
    if KEYWORDS.contains(&word.as_str()) {
        return;
    }
    out.insert(word);
}

/// A dependency that the proposal did not declare: one of its changed lines
/// references a symbol that an already integrated proposal modified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredDependency {
    pub symbol: String,
    pub path: String,
    pub other_proposal: String,
    pub other_agent: String,
}

/// Symbols modified by earlier integrated proposals, keyed by symbol name, with
/// the path and proposal that modified them.
pub type ModifiedSymbols = BTreeMap<String, (String, String, String)>;

pub fn undeclared_dependency(
    facts: &PatchFacts,
    modified_by_others: &ModifiedSymbols,
) -> Option<UndeclaredDependency> {
    facts
        .referenced_symbols
        .iter()
        .filter(|symbol| !facts.modified_symbols.contains(*symbol))
        .find_map(|symbol| {
            modified_by_others
                .get(symbol)
                .map(|(path, proposal, agent)| UndeclaredDependency {
                    symbol: symbol.clone(),
                    path: path.clone(),
                    other_proposal: proposal.clone(),
                    other_agent: agent.clone(),
                })
        })
}

fn line_has_public_token(text: &str) -> bool {
    token_present(text, "pub")
}

fn line_has_assertion(text: &str) -> bool {
    ["assert!", "assert_eq!", "assert_ne!", "debug_assert!"]
        .iter()
        .any(|macro_name| text.contains(macro_name))
}

fn line_has_abi_marker(text: &str) -> bool {
    [
        "extern \"C\"",
        "extern \"system\"",
        "#[no_mangle]",
        "#[export_name",
        "#[repr(C",
        "#[repr(transparent",
        "#[link_section",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn token_present(text: &str, token: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while let Some(found) = text[index..].find(token) {
        let start = index + found;
        let end = start + token.len();
        let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        index = end;
    }
    false
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn one_line(text: &str) -> String {
    let trimmed = text.trim();
    let mut out: String = trimmed.chars().take(96).collect();
    if trimmed.chars().count() > 96 {
        out.push('…');
    }
    out
}

/// (`name`, `has_braced_body`) for one line, or `None` when the line does not
/// start a definition this heuristic understands.
fn definition_head(line: &str) -> Option<(String, bool)> {
    let mut rest = line.trim_start();
    if rest.is_empty()
        || rest.starts_with("//")
        || rest.starts_with('*')
        || rest.starts_with("/*")
        || rest.starts_with('#')
    {
        return None;
    }
    // Visibility / modifiers. `const` is special: `const fn` is a function.
    loop {
        if let Some(after) = rest.strip_prefix("pub(") {
            let close = after.find(')')?;
            rest = after[close + 1..].trim_start();
        } else if let Some(after) = rest.strip_prefix("pub ") {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix("async ") {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix("unsafe ") {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix("default ") {
            rest = after.trim_start();
        } else if rest.starts_with("const ") && rest.trim_start().starts_with("const fn ") {
            rest = rest.trim_start().trim_start_matches("const ").trim_start();
        } else if let Some(after) = rest.strip_prefix("extern ") {
            let after = after.trim_start();
            if let Some(quoted) = after.strip_prefix('"') {
                let end = quoted.find('"')?;
                rest = quoted[end + 1..].trim_start();
            } else {
                return None;
            }
        } else {
            break;
        }
    }
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_alphabetic()) {
        return None;
    }
    let keyword_end = rest
        .find(|ch: char| !(ch == '_' || ch.is_alphanumeric()))
        .unwrap_or(rest.len());
    let keyword = &rest[..keyword_end];
    let remainder = rest[keyword_end..].trim_start();
    match keyword {
        "fn" | "struct" | "enum" | "trait" | "union" => {
            let name = leading_identifier(remainder)?;
            Some((name, has_brace(rest)))
        }
        "mod" => {
            let name = leading_identifier(remainder)?;
            Some((name, has_brace(rest)))
        }
        "type" | "static" => {
            let name = leading_identifier(remainder)?;
            Some((name, false))
        }
        "const" => {
            // `const NAME: T = ...;` (a `const fn` was consumed above).
            let name = leading_identifier(remainder)?;
            Some((name, false))
        }
        _ => None,
    }
}

fn leading_identifier(text: &str) -> Option<String> {
    let text = text.trim_start();
    let end = text
        .find(|ch: char| !(ch == '_' || ch.is_alphanumeric()))
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    Some(text[..end].to_owned())
}

fn has_brace(line: &str) -> bool {
    line.contains('{')
}

fn find_open_brace(lines: &[&str], start_index: usize) -> Option<usize> {
    for (offset, line) in lines[start_index..].iter().enumerate().take(8) {
        if line.contains('{') {
            return Some(start_index + offset);
        }
        if line.contains(';') {
            return None;
        }
    }
    None
}

/// Line index (0-based) of the closing brace matching the first `{` found at or
/// after `start_index`.
fn block_end(lines: &[&str], start_index: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut opened = false;
    for (offset, line) in lines[start_index..].iter().enumerate() {
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' => skip_string(&mut chars),
                '\'' => skip_char_or_lifetime(&mut chars),
                '/' => match chars.peek() {
                    Some('/') => break,
                    Some('*') => skip_block_comment(&mut chars),
                    _ => {}
                },
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    depth -= 1;
                    if opened && depth == 0 {
                        return Some(start_index + offset);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '"' => return,
            _ => {}
        }
    }
}

fn skip_char_or_lifetime(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut probe = chars.clone();
    match probe.next() {
        Some('\\') => {
            chars.next();
            chars.next();
            chars.next();
        }
        Some(_) => {
            probe.next();
            if probe.peek() == Some(&'\'') {
                chars.next();
                chars.next();
            }
        }
        None => {}
    }
}

fn skip_block_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    chars.next();
    let mut previous = '\0';
    for ch in chars.by_ref() {
        if previous == '*' && ch == '/' {
            return;
        }
        previous = ch;
    }
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while", "String", "Vec", "Option", "Result", "Some",
    "None", "Ok", "Err", "Box", "format", "println", "print", "u8", "u16", "u32", "u64", "u128",
    "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "bool", "char", "str",
    "test", "cfg", "allow", "derive", "Debug", "Display", "Clone", "Copy", "Default", "PartialEq",
    "Eq", "Send", "Sync", "main", "panic", "unreachable", "assert", "assert_eq", "assert_ne",
    "debug_assert", "format", "println", "write", "writeln", "vec", "todo", "unimplemented",
    "matches", "include_str", "include_bytes", "cfg_attr", "env", "concat",
];

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#![allow(dead_code)]\n\nfn alpha() -> u32 {\n    1\n}\n\nfn beta(x: u32) -> u32 {\n    x + 1\n}\n";

    #[test]
    fn spans_cover_braced_functions() {
        let spans = symbol_spans(SAMPLE);
        let names: Vec<&str> = spans.iter().map(|span| span.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert_eq!(spans[0], SymbolSpan { name: "alpha".into(), start_line: 3, end_line: 5 });
        assert_eq!(spans[1], SymbolSpan { name: "beta".into(), start_line: 7, end_line: 9 });
    }

    #[test]
    fn enclosing_symbol_is_the_innermost_span() {
        let spans = symbol_spans("mod m {\n    fn inner() -> u32 {\n        1\n    }\n}\n");
        assert_eq!(enclosing_symbol(&spans, 3).map(|s| s.name.as_str()), Some("inner"));
        assert_eq!(enclosing_symbol(&spans, 1).map(|s| s.name.as_str()), Some("m"));
        assert!(enclosing_symbol(&spans, 6).is_none());
    }

    #[test]
    fn braces_in_strings_and_lifetimes_do_not_confuse_matching() {
        let spans = symbol_spans("fn quoted() -> &'static str {\n    \"}\"\n}\n\nfn after() {}\n");
        assert_eq!(spans[0].name, "quoted");
        assert_eq!(spans[0].end_line, 3);
        assert_eq!(spans[1].name, "after");
    }

    #[test]
    fn identifiers_are_filtered() {
        let ids = identifiers("assert_eq!(helper(x) + 2, y_value)");
        assert!(ids.contains("helper"));
        assert!(ids.contains("y_value"));
        // Single-character local names are deliberately ignored.
        assert!(!ids.contains("x"));
        assert!(!ids.contains("assert_eq"));
    }

    fn file_patch(patch: &str) -> Vec<FilePatch> {
        crate::patch::parse(patch).expect("parse")
    }

    #[test]
    fn body_change_is_behavioural_and_public_header_is_a_contract_risk() {
        let snapshot = SourceSnapshot::new([("lib.rs".to_string(), SAMPLE.to_string())]);
        let body = file_patch("+++ b/lib.rs\n@@ -3,3 +3,3 @@\n fn alpha() -> u32 {\n-    1\n+    10\n }\n");
        let facts = analyse(&snapshot, &body);
        assert!(facts.behavioural_change());
        assert!(facts.contract_risks.is_empty());
        assert_eq!(facts.modified_symbols.iter().cloned().collect::<Vec<_>>(), vec!["alpha"]);

        let public: SourceSnapshot = SourceSnapshot::new([(
            "lib.rs".to_string(),
            "pub fn area(w: u32) -> u32 {\n    w * 2\n}\n".to_string(),
        )]);
        let header = file_patch("+++ b/lib.rs\n@@ -1,3 +1,3 @@\n-pub fn area(w: u32) -> u32 {\n-    w * 2\n+pub fn area(w: u32, h: u32) -> u32 {\n+    w * h\n }\n");
        let facts = analyse(&public, &header);
        assert!(facts
            .contract_risks
            .iter()
            .any(|risk| matches!(risk, ContractRisk::PublicSurface(_))));
    }

    #[test]
    fn abi_and_removed_assertion_are_reported() {
        let abi = SourceSnapshot::new([(
            "lib.rs".to_string(),
            "#[no_mangle]\npub extern \"C\" fn entry(v: u32) -> u32 {\n    v\n}\n".to_string(),
        )]);
        let patch = file_patch("+++ b/lib.rs\n@@ -2,3 +2,3 @@\n-pub extern \"C\" fn entry(v: u32) -> u32 {\n+pub extern \"C\" fn entry(v: u64) -> u32 {\n     v\n }\n");
        let facts = analyse(&abi, &patch);
        assert!(facts
            .contract_risks
            .iter()
            .any(|risk| matches!(risk, ContractRisk::AbiSurface(_))));

        let assert_source = SourceSnapshot::new([(
            "lib.rs".to_string(),
            "fn checked(v: u32) -> u32 {\n    assert!(v != 0);\n    v\n}\n".to_string(),
        )]);
        let patch = file_patch("+++ b/lib.rs\n@@ -1,4 +1,3 @@\n fn checked(v: u32) -> u32 {\n-    assert!(v != 0);\n     v\n }\n");
        let facts = analyse(&assert_source, &patch);
        assert!(facts
            .contract_risks
            .iter()
            .any(|risk| matches!(risk, ContractRisk::RemovedAssertion(_))));
    }

    #[test]
    fn an_undeclared_reference_to_a_modified_symbol_is_detected() {
        let snapshot = SourceSnapshot::new([("lib.rs".to_string(), SAMPLE.to_string())]);
        let helper: SourceSnapshot = SourceSnapshot::new([(
            "lib.rs".to_string(),
            "fn helper(x: u32) -> u32 {\n    x + 1\n}\n\nfn compose(x: u32) -> u32 {\n    helper(x) * 2\n}\n".to_string(),
        )]);
        let a = analyse(&helper, &file_patch("+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn helper(x: u32) -> u32 {\n-    x + 1\n+    x + 5\n }\n"));
        let b = analyse(&helper, &file_patch("+++ b/lib.rs\n@@ -5,3 +5,3 @@\n fn compose(x: u32) -> u32 {\n-    helper(x) * 2\n+    helper(x) * 3\n }\n"));
        let mut modified: ModifiedSymbols = BTreeMap::new();
        for symbol in &a.modified_symbols {
            modified.insert(symbol.clone(), ("lib.rs".to_string(), "a/t".to_string(), "agent-a".to_string()));
        }
        let dependency = undeclared_dependency(&b, &modified).expect("dependency detected");
        assert_eq!(dependency.symbol, "helper");
        assert_eq!(dependency.other_agent, "agent-a");
        // A symbol the current proposal modifies itself is never a dependency.
        assert!(undeclared_dependency(&a, &modified).is_none());
        let _ = snapshot;
    }

    #[test]
    fn test_code_presence_is_observable() {
        let plain = SourceSnapshot::new([("lib.rs".to_string(), SAMPLE.to_string())]);
        assert!(!has_test_code(&plain));
        let with_test = SourceSnapshot::new([(
            "lib.rs".to_string(),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n".to_string(),
        )]);
        assert!(has_test_code(&with_test));
    }
}



