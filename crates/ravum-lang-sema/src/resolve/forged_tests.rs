//! The resolver trusts the tables `World::build` fills: each namespace holds
//! only its own symbol kinds (debug-asserted at registration), every nominal
//! id is a struct or an enum of the world, every path has a segment. Each test
//! builds the world of a real program, breaks one table the way a defect
//! upstream would, and requires the resolver to refuse instead of guessing.

use super::*;
use crate::facts::Facts;
use ravum_lang_syntax::ast::{Ident, Path, PathRoot, PathSegment};
use ravum_lang_syntax::span::Span;
use std::collections::{BTreeMap, BTreeSet};

const SOURCE: &str = "\
mod m { pub fn f() -> i32 { return 1; } }
struct S { n: i32 }
enum E { A, B }
fn g() -> i32 { return 2; }
";

fn path(root: PathRoot, names: &[&str]) -> Path {
    Path {
        id: 5_000_000_001,
        root,
        segments: names
            .iter()
            .map(|name| PathSegment {
                name: Ident {
                    name: name.to_string(),
                    span: Span::dummy(),
                },
                args: Vec::new(),
            })
            .collect(),
        span: Span::dummy(),
    }
}

fn item(sym: Sym) -> ItemSym {
    ItemSym {
        sym,
        vis: Vis::Public(Span::dummy()),
        span: Span::dummy(),
    }
}

/// The world of `SOURCE` with wrong-namespace entries planted in the root and
/// in `m`: a struct among the values, a function among the modules.
fn with_broken_world(test: impl FnOnce(&World, ModuleId, &BTreeMap<&str, NodeId>)) {
    let parsed = ravum_lang_syntax::parser::parse_file(0, SOURCE);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    let mut world = World::build(&parsed.program);
    let root = world.root;
    let mut ids = BTreeMap::new();
    ids.insert("S", world.modules[root].types["S"].sym.item().expect("S"));
    ids.insert("E", world.modules[root].types["E"].sym.item().expect("E"));
    ids.insert("g", world.modules[root].values["g"].sym.item().expect("g"));
    let Sym::Module(m) = world.modules[root].modules["m"].sym else {
        panic!("expected the module `m`");
    };
    for module in [root, m] {
        let info = &mut world.modules[module];
        info.values
            .insert("bad".to_string(), item(Sym::Struct(ids["S"])));
        info.modules
            .insert("badm".to_string(), item(Sym::Fn(ids["g"])));
        info.types
            .insert("badt".to_string(), item(Sym::Fn(ids["g"])));
    }
    test(&world, m, &ids);
}

#[test]
fn value_paths_refuse_symbols_from_the_wrong_namespace() {
    with_broken_world(|world, _, _| {
        let root = world.root;
        for (root_kind, names) in [
            (PathRoot::Plain, vec!["bad"]),
            (PathRoot::Plain, vec!["badm", "x"]),
            (PathRoot::Plain, vec!["m", "badm", "x"]),
            (PathRoot::Plain, vec!["m", "bad"]),
            (PathRoot::Plain, vec!["m", "badm"]),
            (PathRoot::Plain, Vec::new()),
        ] {
            let p = path(root_kind, &names);
            assert!(
                resolve_value_path(world, root, &[], &p).is_err(),
                "accepted {names:?}"
            );
        }
    });
}

#[test]
fn type_paths_refuse_symbols_from_the_wrong_namespace() {
    with_broken_world(|world, _, ids| {
        let root = world.root;
        for names in [vec!["badm", "S"], vec!["m", "badm", "S"], Vec::new()] {
            let mut diags = DiagnosticBag::new();
            let ty = resolve_type_path(
                world,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                &mut diags,
                root,
                &[],
                None,
                &path(PathRoot::Plain, &names),
            );
            assert_eq!(ty, Type::Error, "accepted {names:?}");
        }
        let mut diags = DiagnosticBag::new();
        assert!(nominal_type(world, Sym::Fn(ids["g"]), &[], Span::dummy(), &mut diags).is_none());
        assert!(diags.has_errors());
    });
}

#[test]
fn use_paths_refuse_symbols_from_the_wrong_namespace() {
    with_broken_world(|world, _, _| {
        let root = world.root;
        assert!(matches!(
            resolve_use_single_name(world, root, "badm"),
            None | Some(UseTarget::Unknown)
        ));
        assert!(use_module_anchor(world, root, "badm").is_none());
        for names in [vec!["m", "badm", "x"], vec!["m", "badm"]] {
            let target = resolve_use_target(world, root, &path(PathRoot::Crate, &names));
            assert!(matches!(target, UseTarget::Unknown), "accepted {names:?}");
        }
    });
}

#[test]
fn use_targets_compare_builtins_by_path_and_keep_the_declaring_module() {
    assert!(same_use_target(
        &UseTarget::BuiltinModule("std::io"),
        &UseTarget::BuiltinModule("std::io")
    ));
    assert!(same_use_target(
        &UseTarget::BuiltinFn("drop"),
        &UseTarget::BuiltinFn("drop")
    ));
    assert!(!same_use_target(
        &UseTarget::BuiltinFn("drop"),
        &UseTarget::BuiltinModule("drop")
    ));
    assert!(matches!(
        finish_use_symbol(Sym::BuiltinModule("std::io"), Vis::Private, 0),
        UseTarget::BuiltinModule("std::io")
    ));
    assert!(matches!(
        finish_use_symbol(Sym::Module(1), Vis::Private, 0),
        UseTarget::Module(Sym::Module(1), _, 0)
    ));
}

#[test]
fn nominal_helpers_answer_nothing_for_an_id_outside_the_tables() {
    with_broken_world(|world, _, ids| {
        assert!(world.type_generics(9_999_999).is_empty());
        assert_eq!(type_of_symbol(world, Sym::Fn(ids["g"])), Type::Error);
        // Prelude constructors are found when no user enum declares them.
        assert_eq!(
            find_unqualified_variant(world, "Some").map(|(_, name)| name),
            Some("Option".to_string())
        );
        assert_eq!(
            find_unqualified_variant(world, "Err").map(|(_, name)| name),
            Some("Result".to_string())
        );
        let facts = Facts::default();
        let mut diags = DiagnosticBag::new();
        let ghost = Type::Nominal {
            def: 9_999_999,
            name: "Ghost".to_string(),
            args: Vec::new(),
        };
        // Called on an id of neither table (its callers start from them), the
        // copy analysis answers "not Copy" instead of an empty, Copy, type.
        assert!(!copy::nominal_is_copy(
            9_999_999,
            world,
            &facts,
            &mut BTreeMap::new(),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut diags
        ));
        assert!(!copy::type_is_copy(
            &ghost,
            world,
            &facts,
            &mut BTreeMap::new(),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut diags
        ));
    });
}

#[test]
fn item_paths_skip_builtin_symbols_and_associated_paths_skip_absent_enums() {
    let parsed = ravum_lang_syntax::parser::parse_file(0, SOURCE);
    let mut world = World::build(&parsed.program);
    let root = world.root;
    let e = world.modules[root].types["E"].sym.item().expect("E");
    world.modules[root]
        .values
        .insert("p".to_string(), item(Sym::BuiltinFn("drop")));
    world.modules[root]
        .types
        .insert("q".to_string(), item(Sym::BuiltinFn("drop")));
    world.modules[root]
        .modules
        .insert("r".to_string(), item(Sym::BuiltinModule("std::io")));
    let mut facts = Facts::default();
    fill_item_paths(&world, &mut facts);
    assert!(!facts
        .item_paths
        .values()
        .any(|p| p.ends_with("::p") || p.ends_with("::q")));
    world.enums.remove(&e);
    let ty = Type::Nominal {
        def: e,
        name: "E".to_string(),
        args: Vec::new(),
    };
    assert!(resolve_associated(&world, ty, "A").is_err());
}
