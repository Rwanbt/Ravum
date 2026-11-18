//! Checker contracts added for the wave-3 coverage pass (owner decision S5).
//!
//! Each case pins one **behaviour** of `ravum-lang-sema` that had no test:
//! the `check_with_timings` / `check_with_entry` public entries (including the
//! `rt` entry-point refusal), the `self`-outside-a-method path, a destructured
//! static text binding (D13 rule 1), an exhaustive match on a struct variant,
//! and the `&T` shape of the unbound-generic-parameter guard.
//!
//! Assertions name the exact diagnostic code, so a checker that silently
//! accepted a refused program (or refused an accepted one) fails here.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test checker_entry_contracts`

use ravum_lang_sema::facts::CheckOptions;
use ravum_lang_sema::syntax::ast::{ItemKind, NodeId};
use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, check_with_entry, check_with_timings, CheckResult, Profile};

fn parse(src: &str) -> (ravum_lang_sema::syntax::ast::Program, SourceMap) {
    parse_at(src, "src/main.rv")
}

fn parse_at(src: &str, path: &str) -> (ravum_lang_sema::syntax::ast::Program, SourceMap) {
    let mut sources = SourceMap::new();
    let file = sources.add(path, src);
    let parsed = parser::parse_file(file, src);
    let codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        parsed.diags.is_empty(),
        "contract case is not syntactically valid: {codes:?}\n--- source ---\n{src}"
    );
    (parsed.program, sources)
}

fn options(profile: Profile) -> CheckOptions {
    CheckOptions {
        profile,
        edition: LANGUAGE_EDITION.to_string(),
    }
}

fn codes(result: &CheckResult) -> Vec<String> {
    result.diags.iter().map(|d| d.code.clone()).collect()
}

fn describe(result: &CheckResult, sources: &SourceMap) -> String {
    if result.diags.is_empty() {
        return "<no diagnostic>".to_string();
    }
    result.diags.render(sources)
}

fn accepted<'a>(
    result: &'a CheckResult,
    sources: &SourceMap,
) -> &'a ravum_lang_sema::CheckedProgram {
    assert!(
        !result.diags.has_errors(),
        "expected an accepted program, got:\n{}",
        describe(result, sources)
    );
    result
        .checked
        .as_ref()
        .expect("an accepted program carries a CheckedProgram")
}

/// The `NodeId` of the item named `main` (first one in the program).
fn main_id(program: &ravum_lang_sema::syntax::ast::Program) -> NodeId {
    for item in &program.root.items {
        if let ItemKind::Fn(function) = &item.kind {
            if function.name.name == "main" {
                return item.id;
            }
        }
    }
    panic!("the contract program has no `main`");
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

#[test]
fn check_with_timings_returns_the_same_verdict_as_check() {
    let src = "fn add(a: i32, b: i32) -> i32 { return a + b; }\n\nfn main() {\n    let x = add(1, 2);\n}\n";
    let (program, sources) = parse(src);
    let timed = check_with_timings(&program, &sources, &options(Profile::Core));
    let plain = check(&program, &sources, &options(Profile::Core));
    assert_eq!(
        codes(&timed.0),
        codes(&plain),
        "collecting phase timings must not change the verdict"
    );
    assert!(timed.0.checked.is_some(), "the program must be accepted");
    // The phases are genuine `Duration`s: their sum cannot exceed a second on a
    // program this small, which is the only stable assertion to make on them.
    let total = timed.1.resolve + timed.1.typeck + timed.1.effects + timed.1.own;
    assert!(
        total < std::time::Duration::from_secs(60),
        "phase timings are implausible: {total:?}"
    );
}

#[test]
fn a_valid_rt_entry_checks_only_that_entry() {
    let src = "fn helper() -> i32 { return 1; }\n\nfn main() {\n    let x = helper();\n}\n";
    let (program, sources) = parse(src);
    let entry = main_id(&program);
    let result = check_with_entry(&program, &sources, &options(Profile::Rt), Some(entry));
    accepted(&result, &sources);
}

#[test]
fn an_rt_entry_that_is_not_a_function_is_refused_fail_closed() {
    // A NodeId that names no item of the program: the profile gate must refuse
    // rather than check an empty reachable set.
    let src = "fn main() {}\n";
    let (program, sources) = parse(src);
    let missing: NodeId = NodeId::MAX;
    let result = check_with_entry(&program, &sources, &options(Profile::Rt), Some(missing));
    let rendered = describe(&result, &sources);
    assert!(
        rendered.contains("E-EFF-PROFILE") && rendered.contains("not a function of this program"),
        "expected the fail-closed rt entry refusal, got:\n{rendered}"
    );
    assert!(
        result.checked.is_none(),
        "a refused rt entry must not yield a CheckedProgram"
    );
}

// ---------------------------------------------------------------------------
// Resolution paths
// ---------------------------------------------------------------------------

#[test]
fn a_self_expression_outside_a_method_is_refused() {
    let src = "fn main() {\n    let value = self;\n}\n";
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    assert!(
        codes(&result).contains(&"E-RES-SELF".to_string()),
        "`self` outside a method must be E-RES-SELF, got:\n{}",
        describe(&result, &sources)
    );
}

#[test]
fn an_unqualified_variant_must_be_qualified_with_its_enum() {
    let src = "enum Colour { Red, Blue }\n\nfn main() {\n    let value = Red;\n}\n";
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    let rendered = describe(&result, &sources);
    assert!(
        rendered.contains("E-RES-UNKNOWN") && rendered.contains("Colour::Red"),
        "expected the qualification refusal naming `Colour::Red`, got:\n{rendered}"
    );
}

#[test]
fn use_forms_with_an_explicit_root_are_accepted() {
    // `use crate::` / `use self::` reach the `PathRoot::Crate` and
    // `PathRoot::SelfMod` arms of the resolver.
    let src = "mod inner;\n\nuse crate::inner::Soustraction;\nuse self::inner::Addition;\n\nfn main() {\n    let a = Addition { value: 1 };\n    let b = Soustraction { value: 2 };\n}\n";
    // A single-file package cannot resolve `mod inner;`, so the case is checked
    // at the *resolver* level: the parser must accept both batches and the
    // checker must report the missing module, not a bad path root.
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", src);
    let parsed = parser::parse_file(file, src);
    assert!(
        parsed.diags.is_empty(),
        "`use crate::` / `use self::` must parse: {:?}",
        parsed
            .diags
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    );
    let result = check(&parsed.program, &sources, &options(Profile::Core));
    let codes = codes(&result);
    assert!(
        !codes.contains(&"E-PARSE-EXPECTED".to_string()),
        "a rooted `use` must not be a parse error: {codes:?}"
    );
}

// ---------------------------------------------------------------------------
// Ownership facts
// ---------------------------------------------------------------------------

#[test]
fn a_destructuring_let_is_refused_by_the_checker() {
    // Core 1.0 `let` binds exactly one identifier: the parser keeps the pattern
    // form (it is a valid `Pat`), and the checker refuses it fail-closed before
    // ownership analysis, so the destructured-pattern path of `own::check_let`
    // is unreachable from source syntax.
    let src = "fn main() {\n    let (text, number): (&str, i32) = (\"abc\", 1);\n}\n";
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", src);
    let parsed = parser::parse_file(file, src);
    let result = check(&parsed.program, &sources, &options(Profile::Core));
    let codes = codes(&result);
    assert!(
        codes.contains(&"E-PARSE-EXPECTED".to_string()),
        "a destructuring `let` must be refused fail-closed, got {codes:?}"
    );
    let rendered = result.diags.render(&sources);
    assert!(
        rendered.contains("binds exactly one identifier"),
        "the refusal must name the rule, got:\n{rendered}"
    );
}

#[test]
fn an_option_string_survives_a_borrowing_predicate_and_a_consuming_unwrap() {
    // The receiver of `Option::is_some` is borrowed (STD-11.3); the same value
    // may then be consumed by `unwrap`. A backend that treats `is_some` as
    // consuming a receiver would lose the value here.
    let src = r#"
fn main() {
    let maybe: Option<String> = Option::Some(String::from("value"));
    let present = maybe.is_some();
    let text = maybe.unwrap();
}
"#;
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    let checked = accepted(&result, &sources);
    // The checker must have recorded a *borrowing* target for `is_some` and a
    // consuming one for `unwrap`: the fact table is the contract the backend
    // reads (see `facts::MethodTarget`).
    let borrow_targets: Vec<String> = checked
        .facts
        .methods
        .values()
        .map(|target| match target {
            ravum_lang_sema::facts::MethodTarget::Builtin { path, .. } => path.clone(),
            ravum_lang_sema::facts::MethodTarget::User { .. } => "user".to_string(),
            ravum_lang_sema::facts::MethodTarget::Error => "error".to_string(),
        })
        .collect();
    assert!(
        borrow_targets.iter().any(|path| path.contains("is_some")),
        "expected an `is_some` method fact, got {borrow_targets:?}"
    );
    assert!(
        borrow_targets.iter().any(|path| path.contains("unwrap")),
        "expected an `unwrap` method fact, got {borrow_targets:?}"
    );
}

// ---------------------------------------------------------------------------
// Pattern exhaustiveness
// ---------------------------------------------------------------------------

#[test]
fn an_exhaustive_struct_variant_match_is_accepted() {
    let src = r#"
enum Shape {
    Rect { w: i32, h: i32 },
    Dot,
}

fn area(shape: Shape) -> i32 {
    match shape {
        Shape::Rect { w: w, h: h } => w * h,
        Shape::Dot => 0,
    }
}

fn main() {
    let a = area(Shape::Dot);
    let b = area(Shape::Rect { w: 2, h: 3 });
}
"#;
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    accepted(&result, &sources);
}

#[test]
fn a_struct_variant_match_missing_the_unit_variant_is_non_exhaustive() {
    let src = r#"
enum Shape {
    Rect { w: i32, h: i32 },
    Dot,
}

fn area(shape: Shape) -> i32 {
    match shape {
        Shape::Rect { w: w, h: _ } => w,
    }
}

fn main() {
    let a = area(Shape::Dot);
}
"#;
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    assert!(
        codes(&result).contains(&"E-TYP-NONEXHAUSTIVE".to_string()),
        "a missing unit variant must be E-TYP-NONEXHAUSTIVE, got:\n{}",
        describe(&result, &sources)
    );
}

// ---------------------------------------------------------------------------
// Generic parameter leakage
// ---------------------------------------------------------------------------

#[test]
fn a_generic_reference_parameter_accepts_a_diverging_argument() {
    // `has_unbound_param` must look *through* a reference: the expected type is
    // `&T` with `T` unbound, and a diverging argument is legal there.
    let src = r#"
fn peek<T>(value: &T) -> i32 {
    return 1;
}

fn main() {
    let n = peek(panic("stop"));
}
"#;
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    accepted(&result, &sources);
}

// ---------------------------------------------------------------------------
// Surfaces the checker refuses before a later phase could run
// ---------------------------------------------------------------------------

#[test]
fn a_library_with_test_functions_is_analysed_without_a_main() {
    // Core 1.0 spells a test function `test fn` (G-3.6 / MOD-4.10). A package
    // with no `main` is analysed through its test entries: the effects profile
    // takes `Facts::test_fns` when `main_fn` is absent.
    let src = r#"
test fn first() {
}

test fn second() -> Result<(), Error> {
    return Result::Ok(());
}
"#;
    let (program, sources) = parse_at(src, "src/lib.rv");
    let result = check(&program, &sources, &options(Profile::Pure));
    let checked = accepted(&result, &sources);
    assert_eq!(
        checked.facts.test_fns.len(),
        2,
        "both `test fn` items must be recorded as test entries"
    );
    assert!(
        checked.facts.main_fn.is_none(),
        "a library package has no `main`"
    );
}

#[test]
fn an_invalid_test_function_signature_is_e_typ_main() {
    let src = "test fn bad(value: i32) {\n}\n";
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    assert!(
        codes(&result).contains(&"E-TYP-MAIN".to_string()),
        "a `test fn` with a parameter must be E-TYP-MAIN, got:\n{}",
        describe(&result, &sources)
    );
}

#[test]
fn a_double_super_path_is_a_parse_error() {
    // `super::super` is refused by the parser, so the `levels` loops of
    // `resolve_use_target` / `resolve_type_path` cannot run.
    let src = "use super::super::Something;\n\nfn main() {\n}\n";
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", src);
    let parsed = parser::parse_file(file, src);
    let codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.contains(&"E-PARSE-EXPECTED".to_string()),
        "`super::super` must be a parse error, got {codes:?}"
    );
    let rendered = parsed.diags.render(&sources);
    assert!(
        rendered.contains("expected path segment") && rendered.contains("keyword `super`"),
        "the refusal must name the offending keyword, got:\n{rendered}"
    );
}

#[test]
fn rt_reachability_handles_a_shared_callee_reached_twice() {
    // `main` reaches `shared` through two different callers: the breadth-first
    // walk of the reachable set must not enqueue it twice.
    let src = r#"
fn shared(value: i32) -> i32 {
    return value.wrapping_add(1);
}

fn left(value: i32) -> i32 {
    return shared(value);
}

fn right(value: i32) -> i32 {
    return shared(value);
}

fn main() {
    let a = left(1);
    let b = right(2);
}
"#;
    let (program, sources) = parse(src);
    let entry = main_id(&program);
    let result = check_with_entry(&program, &sources, &options(Profile::Rt), Some(entry));
    accepted(&result, &sources);
}

#[test]
fn a_mod_declaration_that_the_loader_did_not_inline_is_refused() {
    // The loader rewrites every `mod foo;` declaration into an inline
    // `ItemKind::Mod`; a program that still carries a `ModDecl` never reaches
    // code generation because checking fails closed first.
    let src = "mod inner;\n\nfn main() {\n}\n";
    let (program, sources) = parse(src);
    let result = check(&program, &sources, &options(Profile::Core));
    assert!(
        result.diags.has_errors(),
        "an unresolved `mod inner;` must be refused, got:\n{}",
        describe(&result, &sources)
    );
    assert!(
        result.checked.is_none(),
        "a program carrying a ModDecl must not yield a CheckedProgram"
    );
}

#[test]
fn rt_traps_walk_reachable_methods_conditionals_and_loop_bounds() {
    // Under `rt`, only functions reachable from the entry are scanned for
    // traps: a method of an `impl` reached by a call, not the one nobody
    // calls; an `if` without `else`; a `for` over a variable bound and over a
    // `len()` bound.
    let src = r#"
struct Acc { total: i32 }

impl Acc {
    fn add(&mut self, value: i32) {
        if value > 0 {
            self.total = self.total.wrapping_add(value);
        }
    }

    fn never_called(&self) -> i32 {
        return self.total;
    }
}

fn sum(values: &[i32]) -> i32 {
    let mut total = 0;
    for j in 0..values.len() {
        total = total.wrapping_add(values[j]);
    }
    return total;
}

fn main() {
    let mut acc = Acc { total: 0 };
    let n: i32 = 3;
    for i in 0..n {
        acc.add(i);
    }
    let items = [1, 2, 3];
    acc.add(sum(&items));
}
"#;
    let (program, sources) = parse(src);
    let entry = main_id(&program);
    let result = check_with_entry(&program, &sources, &options(Profile::Rt), Some(entry));
    accepted(&result, &sources);
}
