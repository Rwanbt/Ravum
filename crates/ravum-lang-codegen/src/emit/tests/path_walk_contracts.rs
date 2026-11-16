use super::*;
use crate::emit::paths::{positional_pattern, rebuild_text, slot_pattern, PathStep, Slot};
use ravum_lang_sema::syntax::parser;

#[test]
fn ownership_guard_walks_nested_functions_and_expression_children() {
    let source = r#"
mod outer {
    struct Demo { value: i32 }
    impl Demo {
        fn value(&self) -> i32 { 101 }
    }
    fn nested() -> i32 {
        let mut n = 102;
        n = -103;
        n += 104 as i32;
        let record = Demo { value: 105 };
        if n > 106 { n = 107; } else { n = 108; }
        match n { _ => 109 };
        while n < 110 { n = 111; }
        loop { let marker = 112; break; }
        for i in 113..114 { let marker = 115; }
        let result = { 116 };
        unsafe { let marker = 117; };
        let pair = (118, [119; 120]);
        let temporary = Result::Ok(121)?;
        return 122;
    }
}
fn main() { let check = 123; }
"#;
    let parsed = parser::parse_file(0, source);
    assert!(parsed.diags.is_empty(), "{:?}", parsed.diags);
    let mut bodies = Vec::new();
    collect_fn_bodies(&parsed.program.root, &mut bodies);
    assert_eq!(bodies.len(), 3);

    let mut expressions = BTreeMap::new();
    let mut receivers = BTreeSet::new();
    collect_module_exprs(&parsed.program.root, &mut expressions, &mut receivers);
    let mut markers: Vec<_> = expressions
        .values()
        .filter_map(|expr| match expr.kind {
            ExprKind::Int { value, .. } => Some(value),
            _ => None,
        })
        .collect();
    markers.sort_unstable();
    assert_eq!(markers, (101..=123).collect::<Vec<_>>());
}

#[test]
fn array_slot_requires_a_literal_index_strictly_below_length() {
    let mut builder = Builder::new();
    let array = builder.var(
        "a",
        Some(STy::Array {
            elem: Box::new(STy::String),
            len: 2,
        }),
    );
    let last = builder.int(1, Some(STy::Usize));
    let end = builder.int(2, Some(STy::Usize));
    let dynamic = builder.var("i", Some(STy::Usize));
    let program = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &program,
        options: &options,
        index: Index::build(&program),
        array_moves: Default::default(),
    };

    assert!(matches!(
        emitter.slot_of(&PathStep::Index {
            base: &array,
            index: &last
        }),
        Some(Slot::Index { len: 2, at: 1 })
    ));
    assert!(emitter
        .slot_of(&PathStep::Index {
            base: &array,
            index: &end
        })
        .is_none());
    assert!(emitter
        .slot_of(&PathStep::Index {
            base: &array,
            index: &dynamic
        })
        .is_none());
}

#[test]
fn array_tail_patterns_stop_at_the_last_index_and_tuples_keep_their_comma() {
    assert_eq!(positional_pattern(0, 3, "selected", false), "selected, ..");
    assert_eq!(
        positional_pattern(2, 3, "selected", false),
        "_, _, selected"
    );
    assert_eq!(positional_pattern(0, 1, "selected", true), "selected,");
}

#[test]
fn reinitialization_rebuilds_nested_aggregates_without_replacing_siblings() {
    let slots = [
        Slot::Index { len: 2, at: 0 },
        Slot::Tuple { len: 2, at: 1 },
        Slot::Field {
            path: "Packet".to_string(),
            fields: vec!["left".to_string(), "right".to_string()],
            name: "right".to_string(),
        },
    ];
    assert_eq!(
        slot_pattern(&slots, "selected"),
        "[(_, Packet { right: selected, .. }), ..]"
    );
    let expected = concat!(
        "[({ let [(__rv_keep, ..), ..] = a; __rv_keep }, ",
        "Packet { left: { let [(_, Packet { left: __rv_keep, .. }), ..] = a; __rv_keep }, ",
        "right: fresh }), { let [_, __rv_keep] = a; __rv_keep }]",
    );
    assert_eq!(rebuild_text(&slots, 0, "a", "fresh"), expected);
    assert_eq!(
        rebuild_text(&[Slot::Tuple { len: 1, at: 0 }], 0, "a", "fresh"),
        "(fresh,)"
    );
}
