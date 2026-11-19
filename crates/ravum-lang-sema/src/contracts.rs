//! T19 (first slice): the contracts of a checked program as facts a tool
//! can link — stable id, clause, location, and the tests that reach it.
//!
//! Honest status (ADR-040): a contract is checked **at run time**
//! (RFC-CONTRACTS-1). `exercised_by` lists the `test fn`s whose call graph
//! reaches the function: running them evaluates the clause; it does not
//! prove it. No field ever says `PROVED`.

use std::collections::{BTreeMap, BTreeSet};

use ravum_lang_syntax::ast::{ContractKind, Item, ItemKind};
use ravum_lang_syntax::NodeId;

use crate::facts::CheckedProgram;

/// One contract clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractFact {
    /// `path::fn#requires.0` / `#ensures.1`: stable across edits that do not
    /// rename the function or reorder its clauses (no line number, ADR-040).
    pub id: String,
    /// The function's path without `crate::` (`div`, `Acc::add`).
    pub function: String,
    pub kind: &'static str,
    pub text: String,
    pub file: u32,
    pub start: u32,
    pub end: u32,
    /// Names of the `test fn`s that reach the function (sorted).
    pub exercised_by: Vec<String>,
    /// `runtime-checked`: evaluated on every call, never statically proved.
    pub verification: &'static str,
}

/// Every contract of `checked`, in source order.
pub fn contract_facts(checked: &CheckedProgram) -> Vec<ContractFact> {
    let facts = &checked.facts;
    let mut fns: Vec<&Item> = Vec::new();
    fn collect<'a>(items: &'a [Item], out: &mut Vec<&'a Item>) {
        for item in items {
            match &item.kind {
                ItemKind::Fn(_) => out.push(item),
                ItemKind::Impl(imp) => collect(&imp.items, out),
                ItemKind::Mod(m) => collect(&m.items, out),
                _ => {}
            }
        }
    }
    collect(&checked.program.root.items, &mut fns);
    // Reverse reachability from every test.
    let mut reached_by: BTreeMap<NodeId, BTreeSet<String>> = BTreeMap::new();
    for item in &fns {
        let ItemKind::Fn(f) = &item.kind else {
            continue;
        };
        if !f.is_test {
            continue;
        }
        let mut seen = BTreeSet::new();
        let mut stack = vec![item.id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            reached_by
                .entry(id)
                .or_default()
                .insert(f.name.name.clone());
            if let Some(next) = facts.call_graph.get(&id) {
                stack.extend(next.iter().copied());
            }
        }
    }
    let mut out = Vec::new();
    for item in fns {
        let ItemKind::Fn(f) = &item.kind else {
            continue;
        };
        let path = facts
            .fn_sigs
            .get(&item.id)
            .map(|s| s.rust_path.trim_start_matches("crate::").to_string())
            .unwrap_or_else(|| f.name.name.clone());
        let (mut nr, mut ne) = (0, 0);
        for c in &f.contracts {
            let (kind, n) = match c.kind {
                ContractKind::Requires => {
                    nr += 1;
                    ("requires", nr - 1)
                }
                ContractKind::Ensures => {
                    ne += 1;
                    ("ensures", ne - 1)
                }
            };
            out.push(ContractFact {
                id: format!("{path}#{kind}.{n}"),
                function: path.clone(),
                kind,
                text: c.text.clone(),
                file: c.span.file,
                start: c.span.start,
                end: c.span.end,
                exercised_by: reached_by
                    .get(&item.id)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default(),
                verification: "runtime-checked",
            });
        }
    }
    out
}
