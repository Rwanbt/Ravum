//! RFC-OWN-FACTS-2, step C — the checker publishes `owned_position`,
//! `consumed` and `binding_identity`, so the backend has nothing left to
//! re-derive.
//!
//! Each test drives the real parser + checker and reads the published tables.
//! A recorded place is rendered as `name` + projections through the binding
//! declarations, so the assertions name source places, not node ids.

use ravum_lang_sema::facts::{
    BindingId, CheckOptions, CheckedProgram, OwnFacts, Projection, ProjectionIndex,
};
use ravum_lang_sema::syntax::ast::NodeId;
use ravum_lang_sema::syntax::{parser, source::SourceMap};
use std::collections::{BTreeMap, BTreeSet};

fn checked(source: &str) -> CheckedProgram {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| diag.code.clone())
        .collect();
    assert!(errors.is_empty(), "checker errors: {errors:?}");
    result.checked.expect("the program must check")
}

/// `a[0]`, `p.b`, ... for the place recorded on `node`. A recorded place names
/// its binding by declaration span, which the parameters of one function
/// share: the fixtures below never read two parameters of the same function.
fn render(own: &OwnFacts, node: NodeId) -> String {
    let place = &own.place[&node];
    let mut text = own
        .bindings
        .iter()
        .find(|((span, _), _)| *span == place.binding)
        .map(|((_, name), _)| name.clone())
        .unwrap_or_else(|| "?".to_string());
    for step in &place.steps {
        match step {
            Projection::Deref => text = format!("(*{text})"),
            Projection::Field(name) => text.push_str(&format!(".{name}")),
            Projection::Tuple(index) => text.push_str(&format!(".{index}")),
            Projection::Index(ProjectionIndex::Lit(index)) => text.push_str(&format!("[{index}]")),
            Projection::Index(ProjectionIndex::Opaque) => text.push_str("[*]"),
        }
    }
    text
}

fn rendered(own: &OwnFacts, nodes: &BTreeSet<NodeId>) -> Vec<String> {
    let mut places: Vec<String> = nodes.iter().map(|node| render(own, *node)).collect();
    places.sort();
    places
}

#[test]
fn a_move_is_consumed_and_a_copy_is_only_an_owned_position() {
    let checked = checked(
        "fn take(s: String) -> i32 { 1 }\n\
         fn main() {\n\
             let a = [String::from(\"p\"), String::from(\"q\")];\n\
             let b = [1, 2];\n\
             let n = take(a[0]);\n\
             let m = b[1] + n;\n\
         }\n",
    );
    let own = &checked.facts.own;
    assert_eq!(rendered(own, &own.consumed), vec!["a[0]"]);
    // `b[1]` and `n` are taken by value and copied; `s` is never read.
    assert_eq!(
        rendered(own, &own.owned_position),
        vec!["a[0]", "b[1]", "n"]
    );
    assert!(own.consumed.is_subset(&own.owned_position));
}

/// P-7: a move in an `if` branch, a `match` arm or a nested block is recorded
/// exactly like a move in the direct flow.
#[test]
fn consumption_is_recorded_through_if_match_and_block() {
    let checked = checked(
        "fn pick(a: [String; 2], c: bool) -> String {\n\
             if c { a[0] } else { { a[1] } }\n\
         }\n\
         fn arm(b: [String; 2], o: Option<i32>) -> String {\n\
             match o {\n\
                 Some(n) => b[0],\n\
                 None => { b[1] }\n\
             }\n\
         }\n\
         fn main() {}\n",
    );
    let own = &checked.facts.own;
    assert_eq!(
        rendered(own, &own.consumed),
        vec!["a[0]", "a[1]", "b[0]", "b[1]"]
    );
    assert!(own.consumed.is_subset(&own.owned_position));
}

/// A borrow or a write is neither an owned position nor a consumption.
#[test]
fn borrows_and_writes_are_not_owned_positions() {
    let checked = checked(
        "fn main() {\n\
             let mut a = [String::from(\"p\"), String::from(\"q\")];\n\
             let n = a[0].len();\n\
             a[1] = String::from(\"r\");\n\
         }\n",
    );
    let own = &checked.facts.own;
    assert!(
        own.consumed.is_empty(),
        "{:?}",
        rendered(own, &own.consumed)
    );
    assert!(
        own.owned_position.is_empty(),
        "{:?}",
        rendered(own, &own.owned_position)
    );
}

/// The backend emits unreachable code too, so its decisions are recorded —
/// silently, and without touching the reachable ownership state.
#[test]
fn unreachable_code_is_recorded_without_diagnostics_or_state_change() {
    let checked = checked(
        "fn after_return(a: [String; 2]) -> String {\n\
             return a[0];\n\
             let again = a[0];\n\
             again\n\
         }\n\
         fn after_break(s: String) -> String {\n\
             loop {\n\
                 break;\n\
                 let gone = s;\n\
             }\n\
             s\n\
         }\n\
         fn main() {}\n",
    );
    let own = &checked.facts.own;
    // Both `a[0]` (the reachable one and the dead one), the dead `again`, the
    // dead `s`, and the live `s` after the loop — which is still initialised.
    assert_eq!(
        rendered(own, &own.consumed),
        vec!["a[0]", "a[0]", "again", "s", "s"]
    );
}

/// A projection of a temporary is no place — it has no ownership state — but
/// taking its value is the same decision, and it is published the same way.
#[test]
fn a_projection_of_a_temporary_is_recorded_without_a_place() {
    let checked = checked(
        "fn make() -> [String; 2] { [String::from(\"x\"), String::from(\"y\")] }\n\
         fn pair() -> (i32, String) { (1, String::from(\"z\")) }\n\
         fn main() {\n\
             let s = make()[1];\n\
             let n = pair().0;\n\
         }\n",
    );
    let own = &checked.facts.own;
    let temporaries: Vec<NodeId> = own
        .owned_position
        .iter()
        .copied()
        .filter(|node| !own.place.contains_key(node))
        .collect();
    assert_eq!(temporaries.len(), 2, "`make()[1]` and `pair().0`");
    let moved: Vec<&NodeId> = temporaries
        .iter()
        .filter(|node| own.consumed.contains(node))
        .collect();
    assert_eq!(moved.len(), 1, "only the `String` element is moved out");
}

/// P-7: one identity per parameter, and a shadowing binding is a new identity.
#[test]
fn every_parameter_and_every_shadowing_binding_has_its_own_identity() {
    let checked = checked(
        "fn f(a: [i32; 2], b: [i32; 2]) -> i32 {\n\
             let x = a[0];\n\
             let a = [x, b[1]];\n\
             a[1]\n\
         }\n\
         fn g(a: [i32; 2]) -> i32 { a[0] }\n\
         fn main() {}\n",
    );
    let own = &checked.facts.own;
    let mut by_name: BTreeMap<String, BTreeSet<BindingId>> = BTreeMap::new();
    for ((_, name), id) in &own.bindings {
        by_name.entry(name.clone()).or_default().insert(*id);
    }
    // `a`: parameter of `f`, shadowing `let a` in `f`, parameter of `g`.
    assert_eq!(by_name["a"].len(), 3);
    assert_eq!(by_name["b"].len(), 1);
    let all: BTreeSet<BindingId> = own.bindings.values().copied().collect();
    assert_eq!(all.len(), own.bindings.len(), "identities are never reused");
    // Every identity a path refers to is a declared one.
    for id in own.binding_identity.values() {
        assert!(all.contains(id));
    }
    // The two parameters of `f` share a declaration span and still differ.
    let spans: BTreeSet<_> = own
        .bindings
        .iter()
        .filter(|((_, name), _)| name == "a" || name == "b")
        .map(|((span, _), _)| *span)
        .collect();
    assert!(spans.len() < 4, "`f`'s parameters share the item span");
}

/// The fixpoint passes of a loop and its final pass agree on identities.
#[test]
fn a_loop_body_binding_keeps_one_identity_across_fixpoint_passes() {
    let checked = checked(
        "fn main() {\n\
             let mut total = 0;\n\
             let mut i = 0;\n\
             while i < 3 {\n\
                 let step = i + 1;\n\
                 total = total + step;\n\
                 i = i + 1;\n\
             }\n\
         }\n",
    );
    let own = &checked.facts.own;
    let steps = own
        .bindings
        .keys()
        .filter(|(_, name)| name == "step")
        .count();
    assert_eq!(steps, 1);
}

#[test]
fn diverging_conditions_still_publish_facts_for_dead_bodies() {
    let checked = checked(
        r#"
fn main() {
    let a = [String::from("then"), String::from("else")];
    if panic("condition") { let left = a[0]; } else { let right = a[1]; }
    let b = [String::from("loop")];
    while panic("loop condition") { let body = b[0]; }
}
"#,
    );
    let own = &checked.facts.own;
    assert_eq!(rendered(own, &own.consumed), ["a[0]", "a[1]", "b[0]"]);
    assert!(own.consumed.is_subset(&own.owned_position));
}

#[test]
fn a_diverging_match_scrutinee_still_publishes_arm_facts() {
    let checked = checked(
        r#"
fn main() {
    let a = [String::from("arm")];
    match panic("scrutinee") { _ => { let arm = a[0]; } }
}
"#,
    );
    let own = &checked.facts.own;
    assert_eq!(rendered(own, &own.consumed), ["a[0]"]);
    assert!(own.consumed.is_subset(&own.owned_position));
}

#[test]
fn a_never_returning_method_makes_subsequent_moves_unreachable() {
    let checked = checked(
        r#"
struct Stop {}
impl Stop {
    fn stop(&self) -> ! { panic("stop") }
}
fn main() {
    let stopper = Stop {};
    let a = [String::from("value")];
    let first = a[0];
    stopper.stop();
    let dead = a[0];
}
"#,
    );
    let own = &checked.facts.own;
    assert_eq!(rendered(own, &own.consumed), ["a[0]", "a[0]"]);
    assert!(own.consumed.is_subset(&own.owned_position));
}
