//! T13 step 2: an incremental checker for one file.
//!
//! Each check keeps, per function, its diagnostics (relative to the
//! function's start), its direct effects and its callees (by item key). The
//! next check asks the planner which functions changed; the bodies of the
//! others are **not** checked again: the checker assumes their effect facts
//! and the driver puts back their cached diagnostics, relocated to where the
//! function now is.
//!
//! Soundness is checked, not assumed: `tests/cold_warm.rs` compares, over
//! thousands of edits, the diagnostics of an incremental check with those of
//! a full one (they must be identical). When the planner cannot plan, or the
//! new version has an error outside every function (a package-wide stop),
//! the driver checks everything again (fail-closed).

use std::collections::BTreeMap;

use ravum_lang_sema::check::{check_reusing, DriverFacts};
use ravum_lang_sema::facts::{EffectSet, ReusedBody};
use ravum_lang_sema::syntax::ast::{Item, ItemKind, NodeId};
use ravum_lang_sema::syntax::diag::{Diagnostic, Severity};
use ravum_lang_sema::syntax::source::SourceMap;
use ravum_lang_sema::syntax::{parser, Span};
use ravum_lang_sema::{CheckOptions, Profile};

use crate::{fingerprint_parsed, plan, FileFingerprints, ItemKey, Plan};

/// Path under which the file is checked (a library: no `main` required).
pub const VIRTUAL_PATH: &str = "src/lib.rv";

#[derive(Debug, Clone)]
struct FnCache {
    /// Diagnostics inside the function, spans relative to its start.
    diags: Vec<Diagnostic>,
    direct: EffectSet,
    callees: Vec<ItemKey>,
    used_imports: Vec<String>,
    /// In `static_str_fns`: a body-derived fact other functions depend on.
    static_str: bool,
}

/// What the next incremental check reuses.
#[derive(Debug, Clone)]
pub struct CheckCache {
    fingerprints: FileFingerprints,
    fns: BTreeMap<ItemKey, FnCache>,
}

/// What one check did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStats {
    /// Function bodies checked in this run.
    pub checked_bodies: usize,
    /// Function bodies whose previous results were reused.
    pub reused_bodies: usize,
    /// `Some(reason)` when everything was checked again.
    pub full: Option<String>,
}

fn options() -> CheckOptions {
    CheckOptions {
        profile: Profile::Core,
        edition: ravum_lang_sema::syntax::LANGUAGE_EDITION.to_string(),
    }
}

/// Every function item (free, in `impl`, in inline modules) with its span.
fn functions(items: &[Item], out: &mut Vec<(NodeId, Span)>) {
    for item in items {
        match &item.kind {
            ItemKind::Fn(_) => out.push((item.id, item.span)),
            ItemKind::Impl(imp) => functions(&imp.items, out),
            ItemKind::Mod(m) => functions(&m.items, out),
            _ => {}
        }
    }
}

fn inside(outer: Span, s: Span) -> bool {
    outer.file == s.file && outer.start <= s.start && s.end <= outer.end
}

fn shift(d: &Diagnostic, from: Span, to_start: u32, file: u32) -> Diagnostic {
    let mv = |s: Span| {
        if inside(from, s) {
            Span {
                file,
                start: s.start - from.start + to_start,
                end: s.end - from.start + to_start,
            }
        } else {
            s
        }
    };
    let mut d = d.clone();
    d.span = mv(d.span);
    for n in &mut d.notes {
        n.span = n.span.map(mv);
    }
    d
}

/// A full check (no reuse): the reference the incremental one must equal.
pub fn check_cold(src: &str) -> Vec<Diagnostic> {
    check_incremental(None, src).0
}

/// Check `src`, reusing `prev` where the planner allows it. Returns the
/// diagnostics (sorted), the cache for the next check and what was done.
pub fn check_incremental(
    prev: Option<&CheckCache>,
    src: &str,
) -> (Vec<Diagnostic>, CheckCache, CheckStats) {
    let mut sources = SourceMap::new();
    let file = sources.add(VIRTUAL_PATH, src);
    let parsed = parser::parse_file(file, src);
    let fingerprints = fingerprint_parsed(src, &parsed);
    let mut fns = Vec::new();
    functions(&parsed.program.root.items, &mut fns);
    // Item key of each function, through its span (same parse as the
    // fingerprints: file 0).
    let by_span: BTreeMap<(u32, u32), &ItemKey> = fingerprints
        .items
        .values()
        .map(|i| ((i.span.start, i.span.end), &i.key))
        .collect();
    let key_of: BTreeMap<NodeId, ItemKey> = fns
        .iter()
        .filter_map(|(id, span)| {
            by_span
                .get(&(span.start, span.end))
                .map(|k| (*id, (*k).clone()))
        })
        .collect();
    let id_of: BTreeMap<&ItemKey, NodeId> = key_of.iter().map(|(id, k)| (k, *id)).collect();

    // Which bodies to reuse.
    let mut reuse: BTreeMap<NodeId, ReusedBody> = BTreeMap::new();
    let mut full: Option<String> = None;
    if !parsed.diags.is_empty() {
        full = Some("parse diagnostics".into());
    } else if let Some(prev) = prev {
        match plan(&prev.fingerprints, &fingerprints) {
            Plan::Full { reason } => full = Some(reason),
            p @ Plan::Partial { .. } => {
                for (id, key) in &key_of {
                    if p.must_recheck(key) {
                        continue;
                    }
                    let Some(c) = prev.fns.get(key) else { continue };
                    let edges: Option<Vec<NodeId>> =
                        c.callees.iter().map(|k| id_of.get(k).copied()).collect();
                    if let Some(edges) = edges {
                        reuse.insert(
                            *id,
                            ReusedBody {
                                direct: c.direct.clone(),
                                edges,
                                used_imports: c.used_imports.clone(),
                                static_str: c.static_str,
                            },
                        );
                    }
                }
            }
        }
    } else {
        full = Some("first check".into());
    }

    let run = |reuse: BTreeMap<NodeId, ReusedBody>| {
        if !parsed.diags.is_empty() {
            let mut v = parsed.diags.as_slice().to_vec();
            v.sort_by(|a, b| (a.span.start, &a.code).cmp(&(b.span.start, &b.code)));
            return (v, DriverFacts::default());
        }
        let (r, df) = check_reusing(&parsed.program, &sources, &options(), reuse);
        (r.diags.into_vec(), df)
    };
    let (mut diags, mut df) = run(reuse.clone());
    let spans: BTreeMap<NodeId, Span> = fns.iter().copied().collect();
    let outside_error = |ds: &[Diagnostic]| {
        ds.iter()
            .any(|d| d.severity == Severity::Error && !fns.iter().any(|(_, s)| inside(*s, d.span)))
    };
    // A body-derived fact other functions read (`&'static str` returns)
    // changed for a re-checked function: reused callers may depend on it.
    let static_changed = || {
        prev.is_some_and(|p| {
            key_of.iter().any(|(id, key)| {
                !reuse.contains_key(id)
                    && p.fns
                        .get(key)
                        .is_some_and(|c| c.static_str != df.static_str_fns.contains(id))
            })
        })
    };
    if !reuse.is_empty() && (outside_error(&diags) || static_changed()) {
        // A package-wide stop (what the reused bodies would report depends
        // on how far the phases went), or a changed cross-function fact:
        // check everything (fail-closed).
        full = Some(if outside_error(&diags) {
            "error outside every function".into()
        } else {
            "a `&'static str` return status changed".into()
        });
        reuse.clear();
        (diags, df) = run(BTreeMap::new());
    }

    // Put back the reused bodies' diagnostics.
    let prev_fns = prev.map(|p| &p.fns);
    if !reuse.is_empty() {
        diags.retain(|d| !reuse.keys().any(|id| inside(spans[id], d.span)));
        for id in reuse.keys() {
            let c = &prev_fns.expect("reuse implies a cache")[&key_of[id]];
            let to = spans[id];
            for d in &c.diags {
                // Cached spans are relative: `from` is the span at 0.
                let from = Span {
                    file: d.span.file,
                    start: 0,
                    end: u32::MAX,
                };
                diags.push(shift(d, from, to.start, file));
            }
        }
    }
    diags.sort_by(|a, b| {
        (a.span.file, a.span.start, &a.code, &a.message).cmp(&(
            b.span.file,
            b.span.start,
            &b.code,
            &b.message,
        ))
    });

    // The next cache.
    let mut cache = CheckCache {
        fingerprints,
        fns: BTreeMap::new(),
    };
    for (id, span) in &fns {
        let Some(key) = key_of.get(id) else { continue };
        if reuse.contains_key(id) {
            if let Some(c) = prev_fns.and_then(|m| m.get(key)) {
                cache.fns.insert(key.clone(), c.clone());
            }
            continue;
        }
        let Some(direct) = df.direct_effects.get(id) else {
            continue; // effects did not run: not reusable next time
        };
        let callees: Option<Vec<ItemKey>> = df
            .call_graph
            .get(id)
            .map(|v| v.iter().map(|c| key_of.get(c).cloned()).collect())
            .unwrap_or(Some(Vec::new()));
        let Some(callees) = callees else { continue };
        let mine: Vec<&Diagnostic> = diags.iter().filter(|d| inside(*span, d.span)).collect();
        // A note pointing outside the function (another item, which may
        // move) cannot be relocated: such a function is not reusable.
        if mine.iter().any(|d| {
            d.notes
                .iter()
                .any(|n| n.span.is_some_and(|s| !inside(*span, s)))
        }) {
            continue;
        }
        let local: Vec<Diagnostic> = mine
            .iter()
            .map(|d| shift(d, *span, 0, d.span.file))
            .collect();
        cache.fns.insert(
            key.clone(),
            FnCache {
                diags: local,
                direct: direct.clone(),
                callees,
                used_imports: df.fn_used_imports.get(id).cloned().unwrap_or_default(),
                static_str: df.static_str_fns.contains(id),
            },
        );
    }
    let stats = CheckStats {
        checked_bodies: fns.len() - reuse.len(),
        reused_bodies: reuse.len(),
        full,
    };
    (diags, cache, stats)
}
