//! STD-1.4: the signatures of `spec/language/stdlib.md` are normative (name,
//! parameter types in order, return type, effects). Every `pub fn` line of a
//! `std::<module>` section must match the checker's `BUILTIN_FNS` entry.

use ravum_lang_sema::builtins::{builtin_fn, SPECIAL_FORM_FNS};
use ravum_lang_sema::facts::Effect;
use std::path::Path;

fn norm(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("< ", "<")
        .replace(" >", ">")
}

/// `pub fn name<G>(a: A, b: B) -> R effects(x, y)` → (name, params, ret, effects).
fn parse(line: &str) -> Option<(String, Vec<String>, String, Vec<String>)> {
    let rest = line.strip_prefix("pub fn ")?;
    let rest = rest.split("//").next()?.trim();
    let open = rest.find('(')?;
    let name = rest[..open].split('<').next()?.trim().to_string();
    let mut depth = 0i32;
    let mut close = None;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => {
                depth -= 1;
                if depth == 0 && c == ')' {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let inner = &rest[open + 1..close];
    let mut params = Vec::new();
    let (mut d, mut cur) = (0i32, String::new());
    for c in inner.chars() {
        match c {
            '(' | '<' | '[' => d += 1,
            ')' | '>' | ']' => d -= 1,
            _ => {}
        }
        if c == ',' && d == 0 {
            params.push(cur.clone());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        params.push(cur);
    }
    let params = params
        .iter()
        .map(|p| norm(p.split_once(':').map_or(p.as_str(), |(_, t)| t)))
        .collect();
    let tail = rest[close + 1..].trim();
    let (ret, effects) = match tail.find("effects(") {
        Some(i) => (tail[..i].trim(), &tail[i + 8..tail.rfind(')')?]),
        None => (tail, ""),
    };
    let ret = ret.strip_prefix("->").map_or("()".to_string(), norm);
    let effects = effects
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    Some((name, params, ret, effects))
}

/// Divergences found by this test and left to the owner (stdlib.md gives
/// `io` to these, the checker does not; COMPAT-3.5 makes adding an effect a
/// break). Each must still diverge exactly as written: fixing one without
/// removing it here fails the test, and any other mismatch fails too.
const KNOWN_DIVERGENCES: &[&str] = &[
    "std::fs::read_to_string: effects [\"alloc\", \"fs\", \"io\"] vs checker [\"alloc\", \"fs\"]",
    "std::fs::write_string: effects [\"fs\", \"io\"] vs checker [\"fs\"]",
];

#[test]
fn stdlib_md_signatures_match_the_checker() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/language/stdlib.md");
    let text = std::fs::read_to_string(path).unwrap();
    // A section may cover several modules (`std::thread` et `std::sync`).
    let mut modules: Vec<String> = Vec::new();
    let mut checked = 0;
    let mut failures = Vec::new();
    for line in text.lines() {
        if line.starts_with("## ") {
            modules = line
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|m| m.starts_with("std::"))
                .map(str::to_string)
                .collect();
            continue;
        }
        let Some((name, params, ret, effects)) = parse(line.trim()) else {
            continue;
        };
        if modules.is_empty() {
            continue;
        }
        let found = modules
            .iter()
            .map(|m| format!("{m}::{name}"))
            .find_map(|full| builtin_fn(&full).map(|sig| (full, sig)));
        let Some((full, sig)) = found else {
            failures.push(format!(
                "{}::{name}: in stdlib.md, unknown to the checker",
                modules[0]
            ));
            continue;
        };
        // A special form (`thread::spawn`) is typed by its own rule.
        if SPECIAL_FORM_FNS.contains(&full.as_str()) {
            continue;
        }
        checked += 1;
        let want: Vec<String> = sig.params.iter().map(|p| norm(p)).collect();
        if want != params {
            failures.push(format!("{full}: params {params:?} vs checker {want:?}"));
        }
        if norm(sig.ret) != ret {
            failures.push(format!("{full}: returns `{ret}` vs checker `{}`", sig.ret));
        }
        let mut have: Vec<String> = sig
            .effects
            .iter()
            .map(|e: &Effect| e.name().to_string())
            .collect();
        let mut doc = effects.clone();
        have.sort();
        doc.sort();
        if have != doc {
            failures.push(format!("{full}: effects {doc:?} vs checker {have:?}"));
        }
    }
    assert!(checked >= 40, "only {checked} signatures checked");
    let unexpected: Vec<&String> = failures
        .iter()
        .filter(|f| !KNOWN_DIVERGENCES.contains(&f.as_str()))
        .collect();
    let resolved: Vec<&&str> = KNOWN_DIVERGENCES
        .iter()
        .filter(|k| !failures.iter().any(|f| f == *k))
        .collect();
    assert!(unexpected.is_empty(), "new mismatch(es):\n{unexpected:#?}");
    assert!(
        resolved.is_empty(),
        "fixed, remove from KNOWN_DIVERGENCES: {resolved:?}"
    );
}
