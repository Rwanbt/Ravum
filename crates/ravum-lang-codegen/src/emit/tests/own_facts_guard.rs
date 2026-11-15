//! Non-vacuity of the step-B guard for the RFC-OWN-FACTS-2 tables
//! (`owned_position`, `consumed`, `binding_identity`): every test corrupts one
//! table of a really checked program and requires the guard to refuse it.

use super::super::*;
use ravum_lang_sema::facts::{BindingId, CheckOptions, CheckedProgram, Projection};
use ravum_lang_sema::syntax::{parser, source::SourceMap};

/// `take(a[0])` moves a `String` out of an array; `b[0] + 1` copies an `i32`.
const SOURCE: &str = "fn take(s: String) -> i32 { 1 }\n\
fn main() {\n\
    let a = [String::from(\"p\"), String::from(\"q\")];\n\
    let b = [1, 2];\n\
    let n = take(a[0]);\n\
    let m = b[0] + n;\n\
}\n";

fn checked_fixture() -> CheckedProgram {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", SOURCE);
    let parsed = parser::parse_file(file, SOURCE);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    result.checked.expect("the fixture program must check")
}

/// The node of the index expression rooted at the local `root` (`a[0]`, `b[0]`).
fn index_node(checked: &CheckedProgram, root: &str) -> NodeId {
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    exprs
        .iter()
        .find(|(_, expr)| match &expr.kind {
            ExprKind::Index { base, .. } => matches!(
                &base.kind,
                ExprKind::Path(path) if path.segments[0].name.name == root
            ),
            _ => false,
        })
        .map(|(id, _)| *id)
        .expect("the fixture must contain the index expression")
}

/// The node of the path expression `name` used as the base of an index.
fn root_node(checked: &CheckedProgram, name: &str) -> NodeId {
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    exprs
        .values()
        .find_map(|expr| match &expr.kind {
            ExprKind::Index { base, .. } => match &base.kind {
                ExprKind::Path(path) if path.segments[0].name.name == name => Some(base.id),
                _ => None,
            },
            _ => None,
        })
        .expect("the fixture must contain the indexed path")
}

fn rejection(checked: &CheckedProgram, why: &str) -> String {
    crate::generate(checked, &GenOptions::default())
        .expect_err(why)
        .message
}

#[test]
fn the_fixture_is_accepted_untouched() {
    let checked = checked_fixture();
    let moved = index_node(&checked, "a");
    let copied = index_node(&checked, "b");
    assert!(checked.facts.own.consumed.contains(&moved));
    assert!(checked.facts.own.owned_position.contains(&moved));
    assert!(!checked.facts.own.consumed.contains(&copied));
    assert!(checked.facts.own.owned_position.contains(&copied));
    crate::generate(&checked, &GenOptions::default()).expect("the untouched fixture generates");
}

/// C3: a move the checker took but did not publish as `consumed`.
#[test]
#[cfg(debug_assertions)]
fn a_move_missing_from_consumed_is_rejected() {
    let mut checked = checked_fixture();
    let moved = index_node(&checked, "a");
    assert!(checked.facts.own.consumed.remove(&moved));
    let message = rejection(&checked, "a move absent from `consumed` must be rejected");
    assert!(message.contains("consumed=false"), "unexpected: {message}");
}

/// C3, other direction: a `consumed` entry on an expression that is only copied.
#[test]
#[cfg(debug_assertions)]
fn a_copy_published_as_consumed_is_rejected() {
    let mut checked = checked_fixture();
    let copied = index_node(&checked, "b");
    checked.facts.own.consumed.insert(copied);
    let message = rejection(&checked, "a copy listed in `consumed` must be rejected");
    assert!(message.contains("consumed=true"), "unexpected: {message}");
}

/// C2: a consumed expression must be in an owned position.
#[test]
#[cfg(debug_assertions)]
fn a_move_outside_an_owned_position_is_rejected() {
    let mut checked = checked_fixture();
    let moved = index_node(&checked, "a");
    assert!(checked.facts.own.owned_position.remove(&moved));
    let message = rejection(&checked, "a move outside `owned_position` must be rejected");
    assert!(message.contains("owned position"), "unexpected: {message}");
}

/// C2: an owned position is a move or a copy, never a borrow.
#[test]
#[cfg(debug_assertions)]
fn a_borrow_published_as_owned_position_is_rejected() {
    let mut checked = checked_fixture();
    let copied = index_node(&checked, "b");
    checked.facts.own.use_kind.insert(copied, UseKind::Borrow);
    let message = rejection(&checked, "a borrowed owned position must be rejected");
    assert!(message.contains("owned position"), "unexpected: {message}");
}

/// C2–C4: an element path the checker published nothing for. The tables are
/// coherent with each other (the expression is absent from all of them); only
/// the comparison with what the backend lowers can notice it.
#[test]
#[cfg(debug_assertions)]
fn an_element_path_without_facts_is_rejected() {
    let mut checked = checked_fixture();
    let moved = index_node(&checked, "a");
    let own = &mut checked.facts.own;
    own.use_kind.remove(&moved);
    own.place.remove(&moved);
    own.owned_position.remove(&moved);
    own.consumed.remove(&moved);
    let message = rejection(&checked, "an unvisited element path must be rejected");
    assert!(
        message.contains("ownership facts disagree with the backend"),
        "unexpected: {message}"
    );
}

/// C2–C3 on a projection of a temporary: `make()[i]` is no place, so the
/// checker records no place for it — but it does record that the element is
/// moved out, and the backend lowers exactly that.
#[test]
#[cfg(debug_assertions)]
fn a_temporary_element_without_facts_is_rejected() {
    let source = "fn make() -> [String; 2] { [String::from(\"x\"), String::from(\"y\")] }\n\
        fn pick(i: usize) -> String { return make()[i]; }\n\
        fn main() { let s = pick(1); }\n";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture program must check");
    crate::generate(&checked, &GenOptions::default()).expect("the untouched fixture generates");
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    let element = exprs
        .iter()
        .find(|(_, expr)| {
            matches!(&expr.kind, ExprKind::Index { base, .. }
                if matches!(base.kind, ExprKind::Call { .. }))
        })
        .map(|(id, _)| *id)
        .expect("the fixture must contain `make()[i]`");
    let own = &mut checked.facts.own;
    assert!(own.consumed.remove(&element), "`make()[i]` is consumed");
    assert!(own.owned_position.remove(&element));
    assert!(own.use_kind.remove(&element).is_some());
    assert!(
        !own.place.contains_key(&element),
        "a temporary has no place"
    );
    let message = rejection(&checked, "an unvisited temporary element must be rejected");
    assert!(
        message.contains("ownership facts disagree with the backend"),
        "unexpected: {message}"
    );
}

/// C4: the identity recorded for `a` names another binding.
#[test]
#[cfg(debug_assertions)]
fn a_wrong_binding_identity_is_rejected() {
    let mut checked = checked_fixture();
    let a = root_node(&checked, "a");
    let b = root_node(&checked, "b");
    let identity_of_b = checked.facts.own.binding_identity[&b];
    assert_ne!(checked.facts.own.binding_identity[&a], identity_of_b);
    checked.facts.own.binding_identity.insert(a, identity_of_b);
    let message = rejection(&checked, "a wrong binding identity must be rejected");
    assert!(
        message.contains("binding identity"),
        "unexpected: {message}"
    );
}

/// C4: an identity that no declaration backs.
#[test]
#[cfg(debug_assertions)]
fn an_undeclared_binding_identity_is_rejected() {
    let mut checked = checked_fixture();
    let a = root_node(&checked, "a");
    checked
        .facts
        .own
        .binding_identity
        .insert(a, BindingId(u32::MAX));
    let message = rejection(&checked, "an undeclared binding identity must be rejected");
    assert!(
        message.contains("binding identity"),
        "unexpected: {message}"
    );
}

/// C4: the root of an element path has no identity at all.
#[test]
#[cfg(debug_assertions)]
fn a_missing_binding_identity_is_rejected() {
    let mut checked = checked_fixture();
    let a = root_node(&checked, "a");
    assert!(checked.facts.own.binding_identity.remove(&a).is_some());
    let message = rejection(&checked, "a missing binding identity must be rejected");
    assert!(
        message.contains("binding identity"),
        "unexpected: {message}"
    );
}

/// C4 invariant: one identity per parameter, and a shadowing binding is a new
/// identity (the declaration span alone cannot tell two parameters apart).
#[test]
fn parameters_and_shadowing_bindings_have_distinct_identities() {
    let source = "fn f(a: [i32; 2], b: [i32; 2]) -> i32 {\n\
        let x = a[0];\n\
        let a = [x, b[1]];\n\
        a[1]\n\
    }\n\
    fn main() { let n = f([1, 2], [3, 4]); }\n";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let checked = result.checked.expect("the fixture program must check");
    let own = &checked.facts.own;
    let mut identities: Vec<BindingId> = Vec::new();
    for (node, place) in &own.place {
        if matches!(place.steps.first(), Some(Projection::Index(_))) {
            let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
            let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
            collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
            if let ExprKind::Index { base, .. } = &exprs[node].kind {
                identities.push(own.binding_identity[&base.id]);
            }
        }
    }
    // `a[0]` (parameter a), `b[1]` (parameter b), `a[1]` (the shadowing `let a`).
    assert_eq!(identities.len(), 3, "three index places are recorded");
    identities.sort();
    identities.dedup();
    assert_eq!(identities.len(), 3, "three places, three distinct bindings");
    crate::generate(&checked, &GenOptions::default()).expect("the fixture generates");
}
