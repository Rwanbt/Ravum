//! Per-item fingerprints and **fail-closed** invalidation planning for
//! incremental checking of Ravum Core 1.0 (gate T13, ADR-012 / ADR-020).
//!
//! # What this crate does (step 1 of T13)
//!
//! Given two versions of a source file, it decides **which items must be
//! re-checked** (types, ownership, effects) and which ones may reuse their
//! previous results. It does *not* run the checker itself: `ravum-lang-sema`
//! still checks a whole package. The plan is validated by an oracle
//! (`tests/oracle.rs`) that cold-checks both versions and requires every item
//! the planner declares reusable to have **identical diagnostics** (code and
//! position relative to the item) in both versions.
//!
//! # Soundness rules (over-approximation, never under-approximation)
//!
//! * Fingerprints hash **tokens** (kind and text), not bytes: whitespace and
//!   comments never invalidate anything; any other edit does.
//! * Dependencies are **name-based**: an item depends on every item whose bare
//!   name appears as an identifier in its tokens. Shadowing or homonyms only
//!   add edges, so they can only make the plan larger.
//! * The *interface* of an item is what other items can observe:
//!   - a `fn` with an explicit `effects(...)` clause: its signature;
//!   - a `fn` with **inferred** effects (EFF-1.5): signature **and** body,
//!     because a body edit may change the inferred effect set;
//!   - `struct`, `enum`, `const`, `extern` fns: the whole item.
//!
//!   An interface also counts as changed when it mentions a name whose
//!   interface changed (fixpoint), e.g. a `fn` whose signature mentions a
//!   modified `struct`.
//! * An item is re-checked when it was added, its tokens changed, or any of
//!   its tokens mentions a name whose interface changed.
//! * **Full re-check** (no reuse at all) when: either version has a parse
//!   diagnostic, a `use`/`mod` item changed, or two items share a key.
//!
//! Package-level diagnostics (outside any item, e.g. a missing `main`) are not
//! attributed to an item: callers must always recompute them.

use std::collections::{BTreeMap, BTreeSet};

pub mod driver;

use ravum_lang_syntax::ast::{Item, ItemKind, Module};
use ravum_lang_syntax::lexer;
use ravum_lang_syntax::token::{Token, TokenKind};
use ravum_lang_syntax::{parser, FileId, Span};
use sha2::{Digest, Sha256};

/// Stable identity of an item inside a file: module path, optional impl self
/// type text, and name (e.g. `geo::impl Point::norm`).
pub type ItemKey = String;

/// A 256-bit content fingerprint.
pub type Fingerprint = [u8; 32];

/// What kind of item a fingerprint describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemClass {
    /// `fn` with an explicit `effects(...)` clause.
    FnDeclaredEffects,
    /// `fn` without an `effects(...)` clause (inferred, EFF-1.5).
    FnInferredEffects,
    /// `struct`, `enum`, `const`, `extern` fn.
    Definition,
    /// `use`, `mod`, `mod x;` — any change forces a full re-check.
    Structural,
}

/// Fingerprints and references of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInfo {
    pub key: ItemKey,
    /// Bare name used for name-based dependency edges.
    pub name: String,
    pub class: ItemClass,
    /// Byte range of the item in its file.
    pub span: Span,
    /// Hash of all tokens of the item.
    pub full: Fingerprint,
    /// Hash of what other items observe (see module docs).
    pub interface: Fingerprint,
    /// Identifiers appearing in the interface part (signature or definition).
    pub interface_refs: BTreeSet<String>,
    /// Identifiers appearing anywhere in the item.
    pub refs: BTreeSet<String>,
}

/// Fingerprints of one version of a file.
#[derive(Debug, Clone)]
pub struct FileFingerprints {
    pub items: BTreeMap<ItemKey, ItemInfo>,
    /// `Some(reason)` when this version cannot be planned incrementally.
    pub unplannable: Option<String>,
}

/// Result of comparing two versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Re-check everything; nothing may be reused.
    Full { reason: String },
    /// Re-check `recheck`; items in `reuse` keep their previous results.
    Partial {
        recheck: BTreeSet<ItemKey>,
        reuse: BTreeSet<ItemKey>,
        removed: BTreeSet<ItemKey>,
    },
}

impl Plan {
    /// Items whose previous results may be reused (empty for `Full`).
    pub fn reused(&self) -> BTreeSet<ItemKey> {
        match self {
            Plan::Full { .. } => BTreeSet::new(),
            Plan::Partial { reuse, .. } => reuse.clone(),
        }
    }

    /// Apply the **phase-gate rule** (finding F1 of the T13 oracle).
    ///
    /// `ravum-lang-sema` stops the effects and ownership phases for the
    /// **whole package** as soon as resolution or typing reports an error, so
    /// an error in one item hides later-phase diagnostics of every other item.
    /// A partial plan is therefore valid only when **both** versions passed
    /// resolution and typing (`CheckResult::checked.is_some()`); otherwise the
    /// plan becomes `Full`. A driver re-checks the dirty items first, learns
    /// the new gate, and calls this before reusing anything.
    pub fn confirm_gate(self, old_typed: bool, new_typed: bool) -> Plan {
        match self {
            Plan::Partial { .. } if old_typed && new_typed => self,
            Plan::Partial { .. } => Plan::Full {
                reason: "phase gate: resolve/typeck errors stop later phases package-wide"
                    .to_string(),
            },
            full => full,
        }
    }

    /// Whether `key` must be re-checked.
    pub fn must_recheck(&self, key: &str) -> bool {
        match self {
            Plan::Full { .. } => true,
            Plan::Partial { recheck, .. } => recheck.contains(key),
        }
    }
}

/// Parse `src` and compute the fingerprints of every item.
pub fn fingerprint_source(src: &str) -> FileFingerprints {
    let parsed = parser::parse_file(0, src);
    fingerprint_parsed(src, &parsed)
}

/// [`fingerprint_source`] for a file already parsed (as file 0).
pub fn fingerprint_parsed(src: &str, parsed: &parser::ParseResult) -> FileFingerprints {
    let file: FileId = 0;
    let (tokens, _) = lexer::lex(file, src, false);
    let mut out = FileFingerprints {
        items: BTreeMap::new(),
        unplannable: None,
    };
    if !parsed.diags.is_empty() {
        out.unplannable = Some("parse diagnostics present".to_string());
    }
    let mut walker = Walker {
        src,
        tokens: &tokens,
        out: &mut out,
    };
    walker.module(&parsed.program.root, "");
    out
}

/// Compare two versions and return a fail-closed plan.
pub fn plan(old: &FileFingerprints, new: &FileFingerprints) -> Plan {
    if let Some(r) = old.unplannable.as_ref().or(new.unplannable.as_ref()) {
        return Plan::Full { reason: r.clone() };
    }
    let mut interface_changed: BTreeSet<String> = BTreeSet::new();
    let mut recheck: BTreeSet<ItemKey> = BTreeSet::new();
    let mut removed: BTreeSet<ItemKey> = BTreeSet::new();

    for (key, n) in &new.items {
        match old.items.get(key) {
            None => {
                if n.class == ItemClass::Structural {
                    return Plan::Full {
                        reason: format!("structural item added: {key}"),
                    };
                }
                recheck.insert(key.clone());
                interface_changed.insert(n.name.clone());
            }
            Some(o) => {
                if o.full != n.full {
                    if n.class == ItemClass::Structural || o.class == ItemClass::Structural {
                        return Plan::Full {
                            reason: format!("structural item changed: {key}"),
                        };
                    }
                    recheck.insert(key.clone());
                }
                if o.interface != n.interface || o.class != n.class {
                    interface_changed.insert(n.name.clone());
                }
            }
        }
    }
    for (key, o) in &old.items {
        if !new.items.contains_key(key) {
            if o.class == ItemClass::Structural {
                return Plan::Full {
                    reason: format!("structural item removed: {key}"),
                };
            }
            removed.insert(key.clone());
            interface_changed.insert(o.name.clone());
        }
    }

    // Fixpoint: an interface that mentions a changed interface changes too.
    loop {
        let mut grew = false;
        for n in new.items.values() {
            if !interface_changed.contains(&n.name)
                && n.interface_refs
                    .iter()
                    .any(|r| interface_changed.contains(r))
            {
                interface_changed.insert(n.name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    for (key, n) in &new.items {
        if n.refs.iter().any(|r| interface_changed.contains(r)) {
            recheck.insert(key.clone());
        }
    }
    let reuse = new
        .items
        .keys()
        .filter(|k| !recheck.contains(*k))
        .cloned()
        .collect();
    Plan::Partial {
        recheck,
        reuse,
        removed,
    }
}

/// Convenience: fingerprint both sources and plan.
pub fn plan_sources(old: &str, new: &str) -> Plan {
    plan(&fingerprint_source(old), &fingerprint_source(new))
}

struct Walker<'a> {
    src: &'a str,
    tokens: &'a [Token],
    out: &'a mut FileFingerprints,
}

impl Walker<'_> {
    fn module(&mut self, m: &Module, prefix: &str) {
        for item in &m.items {
            self.item(item, prefix, None);
        }
    }

    fn item(&mut self, item: &Item, prefix: &str, impl_header: Option<(Span, String)>) {
        let span = item.span;
        match &item.kind {
            ItemKind::Fn(f) => {
                let class = if f.effects.is_some() {
                    ItemClass::FnDeclaredEffects
                } else {
                    ItemClass::FnInferredEffects
                };
                let sig = Span {
                    file: span.file,
                    start: span.start,
                    end: f.body.span.start,
                };
                let mut sig_tokens = self.tokens_in(sig);
                let mut all = self.tokens_in(span);
                let mut key_prefix = prefix.to_string();
                if let Some((h, text)) = &impl_header {
                    // The impl header (`impl<T> P<T> {`) is part of every method.
                    let header = self.tokens_in(*h);
                    sig_tokens = header.iter().copied().chain(sig_tokens).collect();
                    all = header.into_iter().chain(all).collect();
                    key_prefix.push_str(&format!("impl {text}::"));
                }
                let interface_tokens: Vec<&Token> = if class == ItemClass::FnDeclaredEffects {
                    sig_tokens.clone()
                } else {
                    let mut v = sig_tokens.clone();
                    v.extend(all.iter().copied());
                    v
                };
                self.push(ItemInfo {
                    key: format!("{key_prefix}{}", f.name.name),
                    name: f.name.name.clone(),
                    class,
                    span,
                    full: hash(&all),
                    interface: hash(&interface_tokens),
                    interface_refs: idents(&interface_tokens),
                    refs: idents(&all),
                });
            }
            ItemKind::Struct(s) => self.definition(prefix, &s.name.name, span),
            ItemKind::Enum(e) => self.definition(prefix, &e.name.name, span),
            ItemKind::Const(c) => self.definition(prefix, &c.name.name, span),
            ItemKind::Extern(b) => {
                for f in &b.fns {
                    // The ABI header belongs to every declared fn.
                    let mut t = self.tokens_in(Span {
                        file: span.file,
                        start: span.start,
                        end: b.fns.first().map(|f| f.span.start).unwrap_or(span.end),
                    });
                    t.extend(self.tokens_in(f.span));
                    self.push(ItemInfo {
                        key: format!("{prefix}extern {}", f.name.name),
                        name: f.name.name.clone(),
                        class: ItemClass::Definition,
                        span: f.span,
                        full: hash(&t),
                        interface: hash(&t),
                        interface_refs: idents(&t),
                        refs: idents(&t),
                    });
                }
            }
            ItemKind::Impl(imp) => {
                let header_end = imp.items.first().map(|i| i.span.start).unwrap_or(span.end);
                let header = Span {
                    file: span.file,
                    start: span.start,
                    end: header_end,
                };
                let self_text = self
                    .src
                    .get(imp.self_ty.span.start as usize..imp.self_ty.span.end as usize)
                    .unwrap_or("?")
                    .split_whitespace()
                    .collect::<String>();
                for m in &imp.items {
                    self.item(m, prefix, Some((header, self_text.clone())));
                }
            }
            ItemKind::Mod(m) => {
                let name = m.name.as_ref().map(|n| n.name.clone()).unwrap_or_default();
                self.structural(prefix, &format!("mod {name}"), span, true);
                self.module(m, &format!("{prefix}{name}::"));
            }
            ItemKind::ModDecl(d) => {
                self.structural(prefix, &format!("mod {};", d.name.name), span, false)
            }
            ItemKind::Use(_) => {
                let text = self
                    .src
                    .get(span.start as usize..span.end as usize)
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<String>();
                self.structural(prefix, &format!("use {text}"), span, false);
            }
        }
    }

    fn definition(&mut self, prefix: &str, name: &str, span: Span) {
        let t = self.tokens_in(span);
        self.push(ItemInfo {
            key: format!("{prefix}{name}"),
            name: name.to_string(),
            class: ItemClass::Definition,
            span,
            full: hash(&t),
            interface: hash(&t),
            interface_refs: idents(&t),
            refs: idents(&t),
        });
    }

    /// `header_only`: for inline modules, only the `mod name {` part is
    /// structural; the members are fingerprinted on their own.
    fn structural(&mut self, prefix: &str, label: &str, span: Span, header_only: bool) {
        let t: Vec<&Token> = if header_only {
            Vec::new()
        } else {
            self.tokens_in(span)
        };
        self.push(ItemInfo {
            key: format!("{prefix}{label}"),
            name: label.to_string(),
            class: ItemClass::Structural,
            span,
            full: hash(&t),
            interface: hash(&t),
            interface_refs: BTreeSet::new(),
            refs: BTreeSet::new(),
        });
    }

    fn push(&mut self, info: ItemInfo) {
        if self.out.items.contains_key(&info.key) {
            self.out.unplannable = Some(format!("duplicate item key: {}", info.key));
            return;
        }
        self.out.items.insert(info.key.clone(), info);
    }

    fn tokens_in(&self, span: Span) -> Vec<&'_ Token> {
        // Tokens are in source order: binary-search the first one at or
        // after `span.start` (a linear scan per item was quadratic: 1.75 s
        // for 3 000 functions).
        let first = self.tokens.partition_point(|t| t.span.start < span.start);
        self.tokens[first..]
            .iter()
            .take_while(|t| t.span.start < span.end || t.span.end <= span.end)
            .filter(|t| {
                !matches!(t.kind, TokenKind::Eof | TokenKind::Comment(_))
                    && t.span.start >= span.start
                    && t.span.end <= span.end
            })
            .collect()
    }
}

fn hash(tokens: &[&Token]) -> Fingerprint {
    let mut h = Sha256::new();
    for t in tokens {
        // `Debug` of `TokenKind` carries kind and text but no position.
        h.update(format!("{:?}", t.kind).as_bytes());
        h.update([0u8]);
    }
    h.finalize().into()
}

fn idents(tokens: &[&Token]) -> BTreeSet<String> {
    tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Ident(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "fn helper(x: i64) -> i64 effects() {\n    x + 1\n}\n\nfn other(y: i64) -> i64 effects() {\n    y * 2\n}\n\nfn main() {\n    let a = helper(1);\n    let b = other(a);\n}\n";

    fn partial(p: &Plan) -> (&BTreeSet<ItemKey>, &BTreeSet<ItemKey>) {
        match p {
            Plan::Partial { recheck, reuse, .. } => (recheck, reuse),
            Plan::Full { reason } => panic!("unexpected full plan: {reason}"),
        }
    }

    #[test]
    fn identical_source_rechecks_nothing() {
        let p = plan_sources(BASE, BASE);
        let (recheck, reuse) = partial(&p);
        assert!(recheck.is_empty());
        assert_eq!(reuse.len(), 3);
    }

    #[test]
    fn whitespace_and_comments_recheck_nothing() {
        let edited = BASE
            .replace("x + 1", "x   +   1 // comment")
            .replace("\n\nfn other", "\n// note\n\n\nfn other");
        let p = plan_sources(BASE, &edited);
        assert!(partial(&p).0.is_empty(), "{p:?}");
    }

    #[test]
    fn body_edit_with_declared_effects_rechecks_only_that_fn() {
        let edited = BASE.replace("x + 1", "x + 2");
        let p = plan_sources(BASE, &edited);
        let (recheck, _) = partial(&p);
        assert_eq!(recheck.iter().collect::<Vec<_>>(), vec!["helper"]);
    }

    #[test]
    fn body_edit_with_inferred_effects_rechecks_callers() {
        let src = "fn helper(x: i64) -> i64 {\n    x + 1\n}\nfn main() {\n    let a = helper(1);\n}\nfn lone() {\n}\n";
        let edited = src.replace("x + 1", "x + 2");
        let p = plan_sources(src, &edited);
        let (recheck, reuse) = partial(&p);
        assert!(
            recheck.contains("helper") && recheck.contains("main"),
            "{p:?}"
        );
        assert!(reuse.contains("lone"));
    }

    #[test]
    fn signature_change_rechecks_callers_not_unrelated() {
        let edited = BASE.replace("fn helper(x: i64) -> i64", "fn helper(x: i32) -> i64");
        let p = plan_sources(BASE, &edited);
        let (recheck, reuse) = partial(&p);
        assert!(recheck.contains("helper") && recheck.contains("main"));
        assert!(reuse.contains("other"), "{p:?}");
    }

    #[test]
    fn struct_change_propagates_through_signatures() {
        let src = "struct P {\n    x: i64,\n}\nfn make() -> P effects() {\n    P { x: 1 }\n}\nfn user() -> i64 effects() {\n    make().x\n}\nfn lone() -> i64 effects() {\n    3\n}\n";
        let edited = src.replace("x: i64,", "x: i32,");
        let p = plan_sources(src, &edited);
        let (recheck, reuse) = partial(&p);
        for k in ["P", "make", "user"] {
            assert!(recheck.contains(k), "{k} not rechecked: {p:?}");
        }
        assert!(reuse.contains("lone"));
    }

    #[test]
    fn removed_item_rechecks_its_users() {
        let edited = BASE.replace("fn other(y: i64) -> i64 effects() {\n    y * 2\n}\n", "");
        let p = plan_sources(BASE, &edited);
        match &p {
            Plan::Partial {
                recheck, removed, ..
            } => {
                assert!(removed.contains("other"));
                assert!(recheck.contains("main"));
            }
            _ => panic!("{p:?}"),
        }
    }

    #[test]
    fn parse_error_forces_full_recheck() {
        let edited = BASE.replace("x + 1", "x + ");
        assert!(matches!(plan_sources(BASE, &edited), Plan::Full { .. }));
    }

    #[test]
    fn use_change_forces_full_recheck() {
        let a = "use std::io;\nfn main() {\n}\n";
        let b = "use std::fs;\nfn main() {\n}\n";
        assert!(matches!(plan_sources(a, b), Plan::Full { .. }));
    }

    #[test]
    fn impl_methods_are_keyed_by_self_type() {
        let src = "struct P {\n    x: i64,\n}\nimpl P {\n    fn get(&self) -> i64 effects() {\n        self.x\n    }\n}\n";
        let fp = fingerprint_source(src);
        assert!(
            fp.items.contains_key("impl P::get"),
            "{:?}",
            fp.items.keys()
        );
    }
}
