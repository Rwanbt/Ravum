//! T13 step 3: incremental check of a **package** (several files) with
//! complete facts.
//!
//! The loader reuses the parse of a file whose text (and `FileId`) did not
//! change, so every `NodeId` of that file is the same as in the previous
//! check. A function body of such a file is not checked again when:
//! * the previous check of it is cached;
//! * every item its facts refer to (call targets, resolved items, user types)
//!   lies in an unchanged file — ids in an edited file may have shifted;
//! * none of its identifiers names an item of an edited file whose
//!   interface changed, appeared or disappeared (name-based, conservative).
//!
//! Its facts are copied from the previous check ([`Facts::merge_reused`]):
//! the result is a complete `CheckedProgram`, usable by every consumer
//! (codegen, JIT, LSP). `tests/package_cold_warm.rs` checks, over generated
//! multi-file packages and many edits, that the diagnostics **and the
//! facts** equal those of a full check.
//!
//! Fail-closed: a full check when there is no previous complete check, when
//! a fingerprint cannot be planned or a structural item (`use`, `mod`)
//! changed, when an error lies outside every function, or when a
//! `&'static str` status changed.

use std::collections::{BTreeMap, BTreeSet};

use ravum_lang_sema::check::check_reusing;
use ravum_lang_sema::facts::{CallTarget, EffectSet, Facts, MethodTarget, Resolution, ReusedBody};
use ravum_lang_sema::syntax::ast::{Item, ItemKind, NodeId, Program};
use ravum_lang_sema::syntax::diag::{Diagnostic, Severity};
use ravum_lang_sema::syntax::source::SourceMap;
use ravum_lang_sema::syntax::Span;
use ravum_lang_sema::{CheckOptions, CheckResult, CheckedProgram, Type};

use crate::{fingerprint_source, FileFingerprints, ItemClass};

fn file_of(id: NodeId) -> u32 {
    (id >> 32) as u32
}

#[derive(Debug, Clone)]
struct FnCache {
    diags: Vec<Diagnostic>,
    direct: EffectSet,
    callees: Vec<NodeId>,
    used_imports: Vec<String>,
    static_str: bool,
    /// Per file the function's facts refer to, the largest node id referred
    /// to there (T13: reuse inside an edited file needs it in the prefix).
    refs: BTreeMap<u32, NodeId>,
}

/// What the next check of the same package reuses.
pub struct PackageState {
    texts: BTreeMap<u32, String>,
    fingerprints: BTreeMap<u32, FileFingerprints>,
    /// The previous complete facts (only after a check without error).
    facts: Option<Facts>,
    fns: BTreeMap<NodeId, FnCache>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStats {
    pub checked_bodies: usize,
    pub reused_bodies: usize,
    /// Of `reused_bodies`, those of an edited file (before its first change).
    pub reused_in_edited_files: usize,
    pub full: Option<String>,
}

/// Function items (any nesting) with their file and span, and every item id
/// per file (for the node ranges).
fn collect(
    items: &[Item],
    fns: &mut Vec<(NodeId, Span)>,
    ids: &mut BTreeMap<u32, Vec<NodeId>>,
    all: &mut Vec<(NodeId, Span)>,
) {
    for item in items {
        ids.entry(file_of(item.id)).or_default().push(item.id);
        all.push((item.id, item.span));
        match &item.kind {
            ItemKind::Fn(_) => fns.push((item.id, item.span)),
            ItemKind::Impl(imp) => collect(&imp.items, fns, ids, all),
            ItemKind::Mod(m) => collect(&m.items, fns, ids, all),
            _ => {}
        }
    }
}

/// Node range `(lo, hi]` of item `id`: the parser numbers nodes in reading
/// order and gives an item its id after its children.
fn range_of(id: NodeId, ids: &BTreeMap<u32, Vec<NodeId>>) -> (NodeId, NodeId) {
    let file = file_of(id);
    let lo = ids
        .get(&file)
        .and_then(|v| v.iter().copied().filter(|x| *x < id).max())
        .unwrap_or(((file as u64) << 32).wrapping_sub(1));
    (lo, id)
}

/// Per referred file, the largest node id that the facts of the nodes in
/// `(lo, hi]` refer to (a local counts as the function itself).
fn refs_of(f: &Facts, (lo, hi): (NodeId, NodeId)) -> BTreeMap<u32, NodeId> {
    let mut ids: BTreeSet<NodeId> = BTreeSet::new();
    let mut files = BTreeSet::new();
    let r = (lo + 1)..=hi;
    for (_, t) in f
        .expr_types
        .range(r.clone())
        .chain(f.pat_types.range(r.clone()))
        .chain(f.type_nodes.range(r.clone()))
    {
        type_ids(t, &mut ids);
    }
    for (_, res) in f.resolutions.range(r.clone()) {
        match res {
            Resolution::Item { item } => {
                ids.insert(*item);
            }
            Resolution::Variant { enum_item, .. } => {
                ids.insert(*enum_item);
            }
            Resolution::Local { decl, .. } => {
                files.insert(decl.file);
            }
            _ => {}
        }
    }
    for (_, c) in f.calls.range(r.clone()) {
        match c {
            CallTarget::UserFn { item } => {
                ids.insert(*item);
            }
            CallTarget::VariantCtor { enum_item, .. } => {
                ids.insert(*enum_item);
            }
            _ => {}
        }
    }
    for (_, m) in f.methods.range(r) {
        if let MethodTarget::User { item } = m {
            ids.insert(*item);
        }
    }
    let mut out: BTreeMap<u32, NodeId> = BTreeMap::new();
    for file in files {
        // A local is declared in the function itself, or (defensively) at
        // the largest id of its file.
        let m = if file == file_of(hi) { hi } else { NodeId::MAX };
        out.insert(file, m);
    }
    for id in ids {
        let e = out.entry(file_of(id)).or_insert(id);
        *e = (*e).max(id);
    }
    out
}

fn type_ids(t: &Type, out: &mut BTreeSet<NodeId>) {
    match t {
        Type::Nominal { def, args, .. } => {
            out.insert(*def);
            for a in args {
                type_ids(a, out);
            }
        }
        Type::Ref { inner, .. }
        | Type::Slice(inner)
        | Type::Vec(inner)
        | Type::Option(inner)
        | Type::Array { elem: inner, .. } => type_ids(inner, out),
        Type::Tuple(items) => items.iter().for_each(|i| type_ids(i, out)),
        Type::Result(a, b) => {
            type_ids(a, out);
            type_ids(b, out);
        }
        _ => {}
    }
}

/// Names of items whose interface differs between two fingerprints of a
/// file (added, removed or changed); `None` when not plannable.
fn changed_names(
    old: Option<&FileFingerprints>,
    new: &FileFingerprints,
) -> Option<BTreeSet<String>> {
    if new.unplannable.is_some() || old.is_some_and(|o| o.unplannable.is_some()) {
        return None;
    }
    let mut out = BTreeSet::new();
    let empty = BTreeMap::new();
    let old_items = old.map(|o| &o.items).unwrap_or(&empty);
    for (k, n) in &new.items {
        match old_items.get(k) {
            Some(o) if o.interface == n.interface => {}
            Some(o) if o.class == ItemClass::Structural || n.class == ItemClass::Structural => {
                return None
            }
            _ => {
                if n.class == ItemClass::Structural {
                    return None;
                }
                out.insert(n.name.clone());
            }
        }
    }
    for (k, o) in old_items {
        if !new.items.contains_key(k) {
            if o.class == ItemClass::Structural {
                return None;
            }
            out.insert(o.name.clone());
        }
    }
    Some(out)
}

fn inside(outer: Span, s: Span) -> bool {
    outer.file == s.file && outer.start <= s.start && s.end <= outer.end
}

/// Check the loaded package `program`, reusing `prev` where sound.
pub fn check_package(
    prev: Option<&PackageState>,
    program: &Program,
    sources: &SourceMap,
    options: &CheckOptions,
) -> (CheckResult, PackageState, PackageStats) {
    let mut fns = Vec::new();
    let mut ids = BTreeMap::new();
    let mut all_items = Vec::new();
    collect(&program.root.items, &mut fns, &mut ids, &mut all_items);
    let files: BTreeSet<u32> = fns
        .iter()
        .map(|(id, _)| file_of(*id))
        .chain(ids.keys().copied())
        .collect();
    let texts: BTreeMap<u32, String> = files
        .iter()
        .filter_map(|f| sources.file(*f).map(|sf| (*f, sf.text.to_string())))
        .collect();
    let fingerprints: BTreeMap<u32, FileFingerprints> = texts
        .iter()
        .map(|(f, t)| (*f, fingerprint_source(t)))
        .collect();

    let mut full: Option<String> = None;
    let mut reuse: BTreeMap<NodeId, ReusedBody> = BTreeMap::new();
    match prev {
        None => full = Some("first check".into()),
        Some(p) if p.facts.is_none() => full = Some("previous check had errors".into()),
        Some(p) => {
            let unchanged: BTreeSet<u32> = texts
                .iter()
                .filter(|(f, t)| p.texts.get(f) == Some(*t))
                .map(|(f, _)| *f)
                .collect();
            let mut names = BTreeSet::new();
            let mut plannable = true;
            for (f, fp) in &fingerprints {
                if unchanged.contains(f) {
                    continue;
                }
                match changed_names(p.fingerprints.get(f), fp) {
                    Some(n) => names.extend(n),
                    None => plannable = false,
                }
            }
            for (f, fp) in &p.fingerprints {
                if !fingerprints.contains_key(f) {
                    names.extend(fp.items.values().map(|i| i.name.clone()));
                }
            }
            // T13: in an edited file, the nodes of the items that end before
            // the first changed byte keep their ids and spans (the parser
            // numbers in reading order): `bound[f]` is the largest such
            // item id. A file with nothing before the change has no bound.
            let mut bound: BTreeMap<u32, NodeId> = BTreeMap::new();
            for (f, t) in &texts {
                if unchanged.contains(f) {
                    continue;
                }
                let Some(old) = p.texts.get(f) else { continue };
                let prefix = old
                    .bytes()
                    .zip(t.bytes())
                    .take_while(|(a, b)| a == b)
                    .count() as u32;
                // Margin of one byte: a token touching the change could lex
                // differently.
                if let Some(b) = all_items
                    .iter()
                    .filter(|(id, s)| file_of(*id) == *f && s.end < prefix)
                    .map(|(id, _)| *id)
                    .max()
                {
                    bound.insert(*f, b);
                }
            }
            let stable = |id: NodeId| {
                let f = file_of(id);
                unchanged.contains(&f) || bound.get(&f).is_some_and(|b| id <= *b)
            };
            if !plannable {
                full = Some("an edited file cannot be planned".into());
            } else {
                for (id, span) in &fns {
                    let file = file_of(*id);
                    if !stable(*id) {
                        continue;
                    }
                    let Some(c) = p.fns.get(id) else { continue };
                    if !c
                        .refs
                        .iter()
                        .all(|(f, m)| unchanged.contains(f) || bound.get(f).is_some_and(|b| m <= b))
                        || !c.callees.iter().all(|x| stable(*x))
                    {
                        continue;
                    }
                    let idents = fingerprints[&file]
                        .items
                        .values()
                        .find(|i| i.span.start == span.start && i.span.end == span.end)
                        .map(|i| &i.refs);
                    let Some(idents) = idents else { continue };
                    if idents.iter().any(|n| names.contains(n)) {
                        continue;
                    }
                    reuse.insert(
                        *id,
                        ReusedBody {
                            direct: c.direct.clone(),
                            edges: c.callees.clone(),
                            used_imports: c.used_imports.clone(),
                            static_str: c.static_str,
                        },
                    );
                }
            }
        }
    }

    let spans: BTreeMap<NodeId, Span> = fns.iter().copied().collect();
    let run = |reuse: BTreeMap<NodeId, ReusedBody>| check_reusing(program, sources, options, reuse);
    let (mut result, mut df) = run(reuse.clone());
    let outside = |r: &CheckResult| {
        r.diags
            .iter()
            .any(|d| d.severity == Severity::Error && !fns.iter().any(|(_, s)| inside(*s, d.span)))
    };
    let static_changed = |df: &ravum_lang_sema::check::DriverFacts| {
        prev.is_some_and(|p| {
            fns.iter().any(|(id, _)| {
                !reuse.contains_key(id)
                    && p.fns
                        .get(id)
                        .is_some_and(|c| c.static_str != df.static_str_fns.contains(id))
            })
        })
    };
    if !reuse.is_empty() && (outside(&result) || static_changed(&df)) {
        full = Some("package-wide stop or changed `&'static str` status".into());
        reuse.clear();
        (result, df) = run(BTreeMap::new());
    }

    // Diagnostics of the reused bodies (their file is unchanged: same spans).
    let prev_fns = prev.map(|p| &p.fns);
    let mut diags = result.diags.into_vec();
    for id in reuse.keys() {
        diags.extend(
            prev_fns.expect("reuse implies a cache")[id]
                .diags
                .iter()
                .cloned(),
        );
    }
    diags.sort_by(|a, b| {
        (a.span.file, a.span.start, &a.code, &a.message).cmp(&(
            b.span.file,
            b.span.start,
            &b.code,
            &b.message,
        ))
    });
    let mut bag = ravum_lang_sema::syntax::diag::DiagnosticBag::new();
    for d in diags.iter().cloned() {
        bag.push(d);
    }

    // Complete the facts with the reused bodies'.
    let mut checked: Option<CheckedProgram> = result.checked;
    if let (Some(c), Some(p)) = (checked.as_mut(), prev) {
        if !reuse.is_empty() {
            let old = p.facts.as_ref().expect("reuse implies previous facts");
            let ranges: Vec<(NodeId, NodeId)> =
                reuse.keys().map(|id| range_of(*id, &ids)).collect();
            c.facts
                .merge_reused(old, |n| ranges.iter().any(|(lo, hi)| n > *lo && n <= *hi));
        }
        c.facts.reuse_bodies.clear();
    }

    let reused_in_edited_files = reuse
        .keys()
        .filter(|id| prev.is_some_and(|p| p.texts.get(&file_of(**id)) != texts.get(&file_of(**id))))
        .count();

    // The next state.
    let mut state = PackageState {
        texts,
        fingerprints,
        facts: checked.as_ref().map(|c| c.facts.clone()),
        fns: BTreeMap::new(),
    };
    if let Some(c) = &checked {
        for (id, span) in &fns {
            if let Some(old) = reuse.get(id).and(prev_fns.and_then(|m| m.get(id))) {
                state.fns.insert(*id, old.clone());
                continue;
            }
            let Some(direct) = df.direct_effects.get(id) else {
                continue;
            };
            let mine: Vec<Diagnostic> = diags
                .iter()
                .filter(|d| inside(*span, d.span))
                .cloned()
                .collect();
            state.fns.insert(
                *id,
                FnCache {
                    diags: mine,
                    direct: direct.clone(),
                    callees: df.call_graph.get(id).cloned().unwrap_or_default(),
                    used_imports: df.fn_used_imports.get(id).cloned().unwrap_or_default(),
                    static_str: df.static_str_fns.contains(id),
                    refs: refs_of(&c.facts, range_of(*id, &ids)),
                },
            );
        }
    }
    let _ = spans;
    let stats = PackageStats {
        checked_bodies: fns.len() - reuse.len(),
        reused_bodies: reuse.len(),
        reused_in_edited_files,
        full,
    };
    (
        CheckResult {
            checked,
            diags: bag,
        },
        state,
        stats,
    )
}
