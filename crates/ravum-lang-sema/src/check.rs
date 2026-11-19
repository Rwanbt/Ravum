//! Orchestration du vérificateur.
//!
//! Séquence : `resolve → typecheck → effects::infer_and_check →
//! own::check_ownership → profil`. Chaque phase peut être bloquante : dès qu'une
//! erreur est présente, le `CheckedProgram` n'est **pas** produit (le codegen ne
//! consomme jamais un programme partiellement vérifié). Les diagnostics sont
//! triés de façon déterministe avant d'être rendus.
//!
//! Une édition inconnue est refusée (`E-UNSUPPORTED-EDITION`) avant toute
//! analyse.

use crate::facts::{file_infos, CheckOptions, CheckResult, CheckedProgram, Facts};
use crate::resolve::World;
use crate::{effects, own, resolve, typeck};
use ravum_lang_syntax::ast::Program;
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag, Severity};
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use std::time::{Duration, Instant};

/// Wall-clock durations of the checker phases, for `ravum-lang --timings`.
///
/// Additive measurement only: no phase changes behaviour when timings are
/// collected. `profile` is folded into `effects` (the profile gate runs inside
/// `effects::infer_and_check`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    pub resolve: Duration,
    pub typeck: Duration,
    pub effects: Duration,
    pub own: Duration,
}

/// Vérifie un programme analysé par le frontend.
pub fn check(program: &Program, sources: &SourceMap, options: &CheckOptions) -> CheckResult {
    check_with_entry(program, sources, options, None)
}

/// `check`, with an explicit `rt` entry function (T6, `--rt-entry`).
///
/// The `rt` effects and trap rules apply to the functions reachable from
/// `rt_entry` instead of `main`; `None` is the historical whole-`main` behaviour.
pub fn check_with_entry(
    program: &Program,
    sources: &SourceMap,
    options: &CheckOptions,
    rt_entry: Option<ravum_lang_syntax::ast::NodeId>,
) -> CheckResult {
    check_entry_with_timings(program, sources, options, rt_entry).0
}

/// `check`, plus the wall-clock duration of each phase.
pub fn check_with_timings(
    program: &Program,
    sources: &SourceMap,
    options: &CheckOptions,
) -> (CheckResult, PhaseTimings) {
    check_entry_with_timings(program, sources, options, None)
}

/// `check_with_timings`, with the explicit `rt` entry function of T6.
/// Spans of the functions (free, in `impl`, in inline modules) that hold an
/// error of `diags`; `None` when some error lies outside every function.
fn faulty_functions(program: &Program, diags: &DiagnosticBag) -> Option<Vec<Span>> {
    use ravum_lang_syntax::ast::{Item, ItemKind};
    fn collect(items: &[Item], out: &mut Vec<Span>) {
        for item in items {
            match &item.kind {
                ItemKind::Fn(_) => out.push(item.span),
                ItemKind::Impl(imp) => collect(&imp.items, out),
                ItemKind::Mod(m) => collect(&m.items, out),
                _ => {}
            }
        }
    }
    let mut fns = Vec::new();
    collect(&program.root.items, &mut fns);
    let within = |outer: &Span, s: &Span| {
        outer.file == s.file && outer.start <= s.start && s.end <= outer.end
    };
    let mut faulty = Vec::new();
    for d in diags.iter().filter(|d| d.severity == Severity::Error) {
        let f = fns.iter().find(|f| within(f, &d.span))?;
        if !faulty.contains(f) {
            faulty.push(*f);
        }
    }
    Some(faulty)
}

/// Move into `diags` the diagnostics of `later` that are not inside one of
/// the `faulty` functions (DIAG-1.4: an early phase masks the later ones).
fn keep_outside(diags: &mut DiagnosticBag, later: DiagnosticBag, faulty: &[Span]) {
    for d in later.into_vec() {
        let masked = faulty
            .iter()
            .any(|f| f.file == d.span.file && f.start <= d.span.start && d.span.end <= f.end);
        if !masked {
            diags.push(d);
        }
    }
}

/// Effects, ownership and the `rt` gate on a program whose resolution and
/// typing hold errors in some functions. Their facts there are partial, so
/// the phases run under `catch_unwind`: should one fail on such facts, the
/// later diagnostics are omitted (as before DIAG-1.4 conformance) rather
/// than the check crashing.
fn later_phases_on_valid_functions(
    program: Program,
    facts: Facts,
    sources: &SourceMap,
    options: &CheckOptions,
    rt_entry: Option<ravum_lang_syntax::ast::NodeId>,
) -> Option<DiagnosticBag> {
    let files = file_infos(sources);
    let profile = options.profile;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut bag = DiagnosticBag::new();
        let mut checked = CheckedProgram {
            program,
            facts,
            files,
            profile,
        };
        let call_graph = effects::infer_and_check(
            &checked.program,
            &mut checked.facts,
            profile,
            rt_entry,
            &mut bag,
        );
        let own_facts = own::check_ownership(&checked, &mut bag);
        checked.facts.own = own_facts;
        if profile == crate::facts::Profile::Rt {
            let reachable = rt_entry.map(|entry| crate::effects::reachable_from(&call_graph, entry));
            crate::traps::check_rt_traps(&checked, &mut bag, reachable.as_ref());
        }
        bag
    }))
    .ok()
}

pub fn check_entry_with_timings(
    program: &Program,
    sources: &SourceMap,
    options: &CheckOptions,
    rt_entry: Option<ravum_lang_syntax::ast::NodeId>,
) -> (CheckResult, PhaseTimings) {
    let mut timings = PhaseTimings::default();
    let mut diags = DiagnosticBag::new();
    if !edition_supported(&options.edition) {
        diags.push(Diagnostic::error(
            "E-UNSUPPORTED-EDITION",
            Span::dummy(),
            format!(
                "edition `{}` is not supported; expected `{}`",
                options.edition,
                ravum_lang_syntax::LANGUAGE_EDITION
            ),
        ));
        diags.sort();
        return (CheckResult::failed(diags), timings);
    }

    let program = program.clone();
    let mut world = World::build(&program);
    let mut facts = Facts::default();

    let started = Instant::now();
    // `resolve_with_world`/`typecheck_with_world` return "no error"; where
    // the errors are is what decides below (DIAG-1.4).
    let _no_error = resolve::resolve_with_world(&mut world, &mut facts, &mut diags);
    timings.resolve = started.elapsed();
    // DIAG-1.4 (decision V1-D1): an early-phase error stops the later phases
    // on the faulty node only. The granularity used is the enclosing
    // function: later phases still run on the whole program, and their
    // diagnostics inside a function that already has an error are dropped.
    let resolve_faulty = faulty_functions(&program, &diags);
    if diags.has_errors() && resolve_faulty.is_none() {
        // An error outside any function (a type, an import...): every
        // function may depend on it; stop as before.
        diags.sort();
        return (CheckResult::failed(diags), timings);
    }
    let resolve_had_errors = diags.has_errors();

    let started = Instant::now();
    let mut typeck_diags = DiagnosticBag::new();
    let _no_error = if resolve_had_errors {
        // Facts are partial where resolution failed: never let that crash
        // the check (fail-closed: on a panic, report the resolution errors).
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            typeck::typecheck_with_world(
                &program,
                &mut facts,
                &world,
                Some(sources),
                &mut typeck_diags,
            )
        })) {
            Ok(t) => t,
            Err(_) => {
                diags.sort();
                return (CheckResult::failed(diags), timings);
            }
        }
    } else {
        typeck::typecheck_with_world(
            &program,
            &mut facts,
            &world,
            Some(sources),
            &mut typeck_diags,
        )
    };
    timings.typeck = started.elapsed();
    let resolve_faulty = resolve_faulty.unwrap_or_default();
    keep_outside(&mut diags, typeck_diags, &resolve_faulty);
    let faulty = faulty_functions(&program, &diags);
    if diags.has_errors() && faulty.is_none() {
        diags.sort();
        return (CheckResult::failed(diags), timings);
    }
    if diags.has_errors() {
        let faulty = faulty.unwrap_or_default();
        drop(world);
        let started = Instant::now();
        let later = later_phases_on_valid_functions(program, facts, sources, options, rt_entry);
        timings.effects = started.elapsed();
        if let Some(later) = later {
            keep_outside(&mut diags, later, &faulty);
        }
        diags.sort();
        return (CheckResult::failed(diags), timings);
    }

    // Le monde emprunte le programme : il doit disparaître avant le déplacement
    // du programme dans le `CheckedProgram`.
    drop(world);

    let mut checked = CheckedProgram {
        program,
        facts,
        files: file_infos(sources),
        profile: options.profile,
    };

    let started = Instant::now();
    let call_graph = effects::infer_and_check(
        &checked.program,
        &mut checked.facts,
        options.profile,
        rt_entry,
        &mut diags,
    );
    timings.effects = started.elapsed();

    let started = Instant::now();
    let own_facts = own::check_ownership(&checked, &mut diags);
    checked.facts.own = own_facts;
    timings.own = started.elapsed();

    // RFC-RT-TRAPS-1: intrinsic traps are checked for the `rt` profile only.
    // `core` and `pure` are unchanged (deliberate asymmetry, see the RFC).
    // With `--rt-entry` (T6) only the functions reachable from the entry are
    // checked: setup code (allocation, I/O) outside the real-time path stays
    // legal.
    if options.profile == crate::facts::Profile::Rt {
        let reachable = rt_entry.map(|entry| crate::effects::reachable_from(&call_graph, entry));
        crate::traps::check_rt_traps(&checked, &mut diags, reachable.as_ref());
    }

    diags.sort();
    if diags.has_errors() {
        (CheckResult::failed(diags), timings)
    } else {
        (
            CheckResult {
                checked: Some(checked),
                diags,
            },
            timings,
        )
    }
}

fn edition_supported(edition: &str) -> bool {
    let edition = edition.trim();
    edition == ravum_lang_syntax::LANGUAGE_EDITION || edition == "core-1.0"
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::facts::{CheckOptions, CheckResult, Profile};
    use ravum_lang_syntax::parser;
    use ravum_lang_syntax::source::SourceMap;

    fn run(path: &str, src: &str, profile: Profile) -> (CheckResult, SourceMap) {
        let mut sources = SourceMap::new();
        let file = sources.add(path, src);
        let parsed = parser::parse_file(file, src);
        let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
        assert!(
            parsed.diags.is_empty(),
            "unexpected parse diagnostics {parse_codes:?}:\n{}",
            parsed.diags.render(&sources)
        );
        let options = CheckOptions {
            profile,
            edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
        };
        let result = check(&parsed.program, &sources, &options);
        (result, sources)
    }

    fn error(result: &CheckResult, code: &str) {
        assert!(
            result.checked.is_none(),
            "expected failure for {code}, got a checked program"
        );
        assert!(
            result.diags.iter().any(|d| d.code == code),
            "expected diagnostic {code}, got {:?}",
            result
                .diags
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>()
        );
    }

    fn ok(result: &CheckResult) -> CheckResult {
        if let Some(program) = &result.checked {
            let _ = program;
            return CheckResult {
                checked: result.checked.clone(),
                diags: result.diags.clone(),
            };
        }
        panic!(
            "expected success, got diagnostics {:?}",
            result
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn hello_program_is_valid() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn main() {
    io::println("hello");
    return;
}
"#,
            Profile::Core,
        );
        let checked = ok(&result).checked.unwrap();
        assert!(checked.facts.main_fn.is_some());
        assert!(!checked.facts.expr_types.is_empty());
        let main = checked.facts.main_fn.unwrap();
        assert!(checked.facts.effects[&main].contains(crate::facts::Effect::Io));
        assert_eq!(
            checked.facts.item_paths.get(&main).map(|s| s.as_str()),
            Some("crate::main")
        );
    }

    #[test]
    fn comprehensive_positive_program() {
        let (result, _) = run(
            "src/main.rv",
            r#"
struct Point { pub x: f64, pub y: f64 }
enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }

impl Point {
    fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y }; }
    fn norme(&self) -> f64 { return (self.x * self.x + self.y * self.y).sqrt(); }
    fn decale(&mut self, dx: f64) { self.x += dx; }
}

fn aire(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.0 * r * r,
        Shape::Rect { w: w, h: h } => w * h,
        Shape::Empty => 0.0,
    }
}

fn main() -> Result<(), Error> {
    let mut p = Point::new(3.0, 4.0);
    p.decale(1.0);
    let n = p.norme();
    let z = n as i32;
    let a = [1, 2, 3];
    let mut s = 0;
    for i in 0usize..3usize { s += a[i]; }
    let mut k = 0;
    while k < 3 { k += 1; }
    loop { break; }
    let mut v: Vec<i32> = Vec::new();
    v.push(s);
    let first = v[0];
    let o = Option::Some(1);
    let x = match o { Option::Some(v) => v, Option::None => 0 };
    let sh = Shape::Empty;
    let ar = aire(sh);
    io::println("hello");
    return Result::Ok(());
}
"#,
            Profile::Core,
        );
        ok(&result);
    }

    #[test]
    fn type_mismatch_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { let x: i32 = "text"; }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-MISMATCH");
    }

    #[test]
    fn wrong_arity_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn f(a: i32) { } fn main() { f(); }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-ARITY");
    }

    #[test]
    fn unknown_method_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { let s = String::from("x"); s.inconnue(); }"#,
            Profile::Core,
        );
        error(&result, "E-RES-UNKNOWN");
    }

    #[test]
    fn non_exhaustive_match_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"
enum E { A, B }
fn f(e: E) -> i32 { match e { E::A => 1 } }
fn main() { }
"#,
            Profile::Core,
        );
        error(&result, "E-TYP-NONEXHAUSTIVE");
    }

    #[test]
    fn invalid_main_signatures_are_reported() {
        let (result, _) = run("src/main.rv", r#"fn main(x: i32) { }"#, Profile::Core);
        error(&result, "E-TYP-MAIN");

        let (result, _) = run(
            "src/main.rv",
            r#"fn main() -> i32 { return 0; }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-MAIN");

        let (result, _) = run("src/main.rv", r#"fn pas_main() { }"#, Profile::Core);
        error(&result, "E-TYP-MAIN-MISSING");

        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { } fn main() { }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-MAIN-DUPLICATE");
    }

    #[test]
    fn invalid_try_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn f() { let x = io::read_line()?; }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-TRY");
    }

    #[test]
    fn valid_try_is_accepted() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn f() -> Result<(), Error> {
    let x = io::read_line()?;
    return Result::Ok(());
}
fn main() -> Result<(), Error> { return f(); }
"#,
            Profile::Core,
        );
        ok(&result);
    }

    #[test]
    fn literal_out_of_range_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { let x: u8 = 300; }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-LITERAL-RANGE");
    }

    #[test]
    fn negative_min_i8_literal_is_accepted() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { let x: i8 = -128; }"#,
            Profile::Core,
        );
        ok(&result);
    }

    #[test]
    fn unknown_effect_is_reported() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() -> () effects(bogus) { }"#,
            Profile::Core,
        );
        error(&result, "E-EFF-UNKNOWN");
    }

    /// CONC-6.1: `.await` turns a `Future<T>` into `T` inside an `async fn`,
    /// is refused outside one, and refuses a value that is not a future.
    #[test]
    fn await_is_typed_and_confined_to_async_fns() {
        let ok = r#"async fn two() -> i64 effects(async) { 2 }
async fn four() -> i64 effects(async) { let x: i64 = two().await; x * 2 }
fn main() -> () effects(async) { let v: i64 = task::block_on(four()); drop(v); }"#;
        let (result, _) = run("src/main.rv", ok, Profile::Core);
        assert!(result.checked.is_some(), "{:?}", result.diags);
        let outside = r#"async fn two() -> i64 effects(async) { 2 }
fn main() -> () effects(async) { let v: i64 = two().await; drop(v); }"#;
        let (result, _) = run("src/main.rv", outside, Profile::Core);
        error(&result, "E-CONC-AWAIT-OUTSIDE");
        let not_future = r#"async fn f() -> i64 effects(async) { let x: i64 = 1; x.await }
fn main() { }"#;
        let (result, _) = run("src/main.rv", not_future, Profile::Core);
        error(&result, "E-TYP-MISMATCH");
        let reference = r#"async fn f(s: &str) -> i64 effects(async) { 1 }
fn main() { }"#;
        let (result, _) = run("src/main.rv", reference, Profile::Core);
        error(&result, "E-CONC-REF-ACROSS-AWAIT");
    }

    /// D-19-9: `async` is producible since RFC-E11-CONCURRENCY phase 2, so a
    /// declared `effects(async)` is accepted under `core` and refused at the
    /// entry point of `pure` (no effect group is unsupported any more).
    #[test]
    fn async_effect_is_accepted_under_core_and_refused_under_pure() {
        let source = r#"fn main() -> () effects(async) { drop(task::yield_now()); }"#;
        let (core, _) = run("src/main.rv", source, Profile::Core);
        assert!(core.checked.is_some(), "async refused under core");
        let (pure, _) = run("src/main.rv", source, Profile::Pure);
        error(&pure, "E-EFF-PROFILE");
    }

    /// RFC-E11-NET-HTTP NET-1.1: `net` is producible under `core` and is
    /// refused at the entry point of `pure`.
    #[test]
    fn net_effect_is_accepted_under_core_and_refused_under_pure() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() -> () effects(net) { }"#,
            Profile::Core,
        );
        assert!(result.checked.is_some(), "{:?}", result.diags);
        let src = r#"fn main() -> () effects(io, net, alloc) {
    let r: Result<TcpListener, Error> = net::listen("127.0.0.1:0");
    io::println(if r.is_ok() { "ok" } else { "err" });
}"#;
        let (result, _) = run("src/main.rv", src, Profile::Core);
        assert!(result.checked.is_some(), "{:?}", result.diags);
        let (result, _) = run("src/main.rv", src, Profile::Pure);
        assert!(result.checked.is_none(), "pure must refuse net");
    }

    #[test]
    fn private_item_is_not_accessible() {
        let (result, _) = run(
            "src/main.rv",
            r#"
mod a { fn cache() -> i32 { return 1; } }
fn main() { let x = a::cache(); }
"#,
            Profile::Core,
        );
        error(&result, "E-RES-PRIVATE");
    }

    #[test]
    fn unused_import_is_a_warning() {
        let (result, _) = run(
            "src/main.rv",
            r#"
use std::fs;
fn main() { }
"#,
            Profile::Core,
        );
        let checked = ok(&result).checked.unwrap();
        let _ = checked;
        // Le warning est conservé même en cas de succès.
        let (result2, _) = run(
            "src/main.rv",
            r#"
use std::fs;
fn main() { }
"#,
            Profile::Core,
        );
        assert!(result2
            .diags
            .iter()
            .any(|d| d.code == "W-RES-UNUSED-IMPORT"));
    }

    #[test]
    fn pure_profile_rejects_io() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { io::println("x"); }"#,
            Profile::Pure,
        );
        error(&result, "E-EFF-PROFILE");
    }

    #[test]
    fn pure_profile_accepts_pure_main() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn carre(x: i32) -> i32 { return x * x; } fn main() { let n = carre(3); }"#,
            Profile::Pure,
        );
        ok(&result);
    }

    #[test]
    fn recursive_effects_reach_fixpoint_and_are_declared() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn a() -> () effects(io) { b(); }
fn b() { a(); io::println("x"); }
fn main() { a(); }
"#,
            Profile::Core,
        );
        let checked = ok(&result).checked.unwrap();
        let main = checked.facts.main_fn.unwrap();
        assert!(checked.facts.effects[&main].contains(crate::facts::Effect::Io));
        // `a` déclare io : OK. Si on retire la déclaration d'un effet présent,
        // E-EFF-UNDECLARED.
        let (result, _) = run(
            "src/main.rv",
            r#"
fn a() -> () effects() { io::println("x"); }
fn main() { a(); }
"#,
            Profile::Core,
        );
        error(&result, "E-EFF-UNDECLARED");
    }

    #[test]
    fn struct_field_visibility_is_checked() {
        let (result, _) = run(
            "src/main.rv",
            r#"
mod m { pub struct S { secret: i32 } pub fn make() -> S { return S { secret: 1 }; } }
fn main() { let s = m::make(); let x = s.secret; }
"#,
            Profile::Core,
        );
        error(&result, "E-RES-PRIVATE");
    }

    fn codes(result: &CheckResult) -> Vec<String> {
        result.diags.iter().map(|d| d.code.clone()).collect()
    }

    /// DIAG-1.4 (V1-D1): a typing error in one function does not hide an
    /// ownership error in another.
    #[test]
    fn a_type_error_does_not_hide_an_ownership_error_elsewhere() {
        let src = "fn bad() -> i64 effects() {\n    return true;\n}\n\nfn moved() -> () effects(alloc) {\n    let s = String::new();\n    let t = s;\n    let u = s;\n    drop(t);\n    drop(u);\n}\n\nfn main() -> () effects(alloc) {\n    moved();\n}\n";
        let (result, _) = run("src/main.rv", src, Profile::Core);
        let c = codes(&result);
        assert!(result.checked.is_none());
        assert!(c.contains(&"E-TYP-MISMATCH".to_string()), "{c:?}");
        assert!(c.contains(&"E-OWN-DOUBLE-MOVE".to_string()), "{c:?}");
    }

    /// Negative: in the *same* function the early phase still masks the
    /// later one (DIAG-1.4, F-META-011).
    #[test]
    fn a_type_error_still_masks_an_ownership_error_in_the_same_function() {
        let src = "fn both() -> i64 effects(alloc) {\n    let s = String::new();\n    let t = s;\n    let u = s;\n    drop(t);\n    drop(u);\n    return true;\n}\n\nfn main() -> () effects(alloc) {\n    let _x = both();\n}\n";
        let (result, _) = run("src/main.rv", src, Profile::Core);
        let c = codes(&result);
        assert!(c.contains(&"E-TYP-MISMATCH".to_string()), "{c:?}");
        assert!(!c.iter().any(|x| x.starts_with("E-OWN")), "{c:?}");
    }

    /// A resolution error in one function does not hide a typing error in
    /// another.
    #[test]
    fn a_resolution_error_does_not_hide_a_type_error_elsewhere() {
        let src = "fn a() -> () effects() {\n    inconnu();\n}\n\nfn b() -> i64 effects() {\n    return true;\n}\n\nfn main() -> () effects() {\n    a();\n}\n";
        let (result, _) = run("src/main.rv", src, Profile::Core);
        let c = codes(&result);
        assert!(c.contains(&"E-RES-UNKNOWN".to_string()), "{c:?}");
        assert!(c.contains(&"E-TYP-MISMATCH".to_string()), "{c:?}");
    }

    /// Negative: an error outside every function (a field type) still stops
    /// the later phases for the whole package.
    #[test]
    fn an_error_outside_functions_still_stops_everything() {
        let src = "struct S {\n    x: Inconnu,\n}\n\nfn b() -> i64 effects() {\n    return true;\n}\n\nfn main() -> () effects() {\n}\n";
        let (result, _) = run("src/main.rv", src, Profile::Core);
        let c = codes(&result);
        assert!(!c.contains(&"E-TYP-MISMATCH".to_string()), "{c:?}");
        assert!(!c.is_empty());
    }

    #[test]
    fn unknown_name_is_reported() {
        let (result, _) = run("src/main.rv", r#"fn main() { inconnu(); }"#, Profile::Core);
        error(&result, "E-RES-UNKNOWN");
    }

    #[test]
    fn cast_rules_are_enforced() {
        let (result, _) = run(
            "src/main.rv",
            r#"fn main() { let x = 1.0 as char; }"#,
            Profile::Core,
        );
        error(&result, "E-TYP-CAST");
    }

    #[test]
    fn unsupported_edition_is_reported() {
        let mut sources = SourceMap::new();
        let file = sources.add("src/main.rv", "fn main() { }");
        let parsed = parser::parse_file(file, "fn main() { }");
        let options = CheckOptions {
            profile: Profile::Core,
            edition: "core-9.9".to_string(),
        };
        let result = check(&parsed.program, &sources, &options);
        error(&result, "E-UNSUPPORTED-EDITION");
    }
}

#[cfg(test)]
mod fact_coverage_tests {
    use super::check;
    use crate::facts::CheckOptions;
    use ravum_lang_syntax::ast::{
        Expr, ExprKind, ItemKind, MatchArm, Module, NodeId, PatKind, Program, StmtKind,
    };
    use ravum_lang_syntax::parser;
    use ravum_lang_syntax::source::SourceMap;
    use std::collections::BTreeSet;

    fn collect_ids(
        program: &Program,
    ) -> (
        BTreeSet<NodeId>,
        BTreeSet<NodeId>,
        BTreeSet<NodeId>,
        BTreeSet<NodeId>,
    ) {
        let mut exprs = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut pats = BTreeSet::new();
        let mut types = BTreeSet::new();
        walk_module(&program.root, &mut exprs, &mut paths, &mut pats, &mut types);
        (exprs, paths, pats, types)
    }

    fn walk_module(
        module: &Module,
        exprs: &mut BTreeSet<NodeId>,
        paths: &mut BTreeSet<NodeId>,
        pats: &mut BTreeSet<NodeId>,
        types: &mut BTreeSet<NodeId>,
    ) {
        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(f) => {
                    types.insert(f.ret.id);
                    for p in &f.params {
                        if let ravum_lang_syntax::ast::ParamKind::Binding { ty, .. } = &p.kind {
                            types.insert(ty.id);
                        }
                    }
                    walk_block(&f.body, exprs, paths, pats, types);
                }
                ItemKind::Struct(s) => {
                    for field in &s.fields {
                        types.insert(field.ty.id);
                    }
                }
                ItemKind::Enum(e) => {
                    for v in &e.variants {
                        match &v.fields {
                            ravum_lang_syntax::ast::VariantDefFields::Tuple(ts) => {
                                for t in ts {
                                    types.insert(t.id);
                                }
                            }
                            ravum_lang_syntax::ast::VariantDefFields::Struct(fs) => {
                                for f in fs {
                                    types.insert(f.ty.id);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ItemKind::Const(c) => {
                    types.insert(c.ty.id);
                    walk_expr(&c.value, exprs, paths, pats, types);
                }
                ItemKind::Impl(imp) => {
                    types.insert(imp.self_ty.id);
                    for (_, f) in imp.methods() {
                        types.insert(f.ret.id);
                        walk_block(&f.body, exprs, paths, pats, types);
                    }
                }
                ItemKind::Mod(inner) => walk_module(inner, exprs, paths, pats, types),
                _ => {}
            }
        }
    }

    fn walk_block(
        block: &ravum_lang_syntax::ast::Block,
        exprs: &mut BTreeSet<NodeId>,
        paths: &mut BTreeSet<NodeId>,
        pats: &mut BTreeSet<NodeId>,
        types: &mut BTreeSet<NodeId>,
    ) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StmtKind::Let(l) => {
                    walk_pat(&l.pat, paths, pats);
                    if let Some(init) = &l.init {
                        walk_expr(init, exprs, paths, pats, types);
                    }
                    if let Some(ty) = &l.ty {
                        types.insert(ty.id);
                    }
                }
                StmtKind::Expr { expr, .. } => walk_expr(expr, exprs, paths, pats, types),
            }
        }
        if let Some(tail) = &block.tail {
            walk_expr(tail, exprs, paths, pats, types);
        }
    }

    fn walk_pat(
        pat: &ravum_lang_syntax::ast::Pat,
        paths: &mut BTreeSet<NodeId>,
        pats: &mut BTreeSet<NodeId>,
    ) {
        pats.insert(pat.id);
        match &pat.kind {
            PatKind::Tuple(items) => {
                for item in items {
                    walk_pat(item, paths, pats);
                }
            }
            PatKind::Path(p) => {
                paths.insert(p.id);
            }
            PatKind::Variant { path, fields } => {
                paths.insert(path.id);
                match fields {
                    ravum_lang_syntax::ast::VariantPatFields::Tuple(items) => {
                        for item in items {
                            walk_pat(item, paths, pats);
                        }
                    }
                    ravum_lang_syntax::ast::VariantPatFields::Struct(fields) => {
                        for field in fields {
                            walk_pat(&field.pat, paths, pats);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn walk_expr(
        expr: &Expr,
        exprs: &mut BTreeSet<NodeId>,
        paths: &mut BTreeSet<NodeId>,
        pats: &mut BTreeSet<NodeId>,
        types: &mut BTreeSet<NodeId>,
    ) {
        exprs.insert(expr.id);
        match &expr.kind {
            ExprKind::Path(p) => {
                paths.insert(p.id);
            }
            ExprKind::Call { callee, args } => {
                walk_expr(callee, exprs, paths, pats, types);
                for a in args {
                    walk_expr(a, exprs, paths, pats, types);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                walk_expr(receiver, exprs, paths, pats, types);
                for a in args {
                    walk_expr(a, exprs, paths, pats, types);
                }
            }
            ExprKind::Field { base, .. } => walk_expr(base, exprs, paths, pats, types),
            ExprKind::TupleIndex { base, .. } => walk_expr(base, exprs, paths, pats, types),
            ExprKind::Index { base, index } => {
                walk_expr(base, exprs, paths, pats, types);
                walk_expr(index, exprs, paths, pats, types);
            }
            ExprKind::Unary { expr, .. } => walk_expr(expr, exprs, paths, pats, types),
            ExprKind::Binary { lhs, rhs, .. } => {
                walk_expr(lhs, exprs, paths, pats, types);
                walk_expr(rhs, exprs, paths, pats, types);
            }
            ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
                walk_expr(target, exprs, paths, pats, types);
                walk_expr(value, exprs, paths, pats, types);
            }
            ExprKind::Cast { expr, ty } => {
                walk_expr(expr, exprs, paths, pats, types);
                types.insert(ty.id);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                walk_expr(cond, exprs, paths, pats, types);
                walk_block(then_block, exprs, paths, pats, types);
                if let Some(e) = else_branch {
                    walk_expr(e, exprs, paths, pats, types);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                walk_expr(scrutinee, exprs, paths, pats, types);
                for arm in arms {
                    walk_match_arm(arm, exprs, paths, pats, types);
                }
            }
            ExprKind::While { cond, body } => {
                walk_expr(cond, exprs, paths, pats, types);
                walk_block(body, exprs, paths, pats, types);
            }
            ExprKind::Loop { body } => walk_block(body, exprs, paths, pats, types),
            ExprKind::For { pat, iter, body } => {
                walk_pat(pat, paths, pats);
                walk_expr(iter, exprs, paths, pats, types);
                walk_block(body, exprs, paths, pats, types);
            }
            ExprKind::Block(b) => walk_block(b, exprs, paths, pats, types),
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    walk_expr(item, exprs, paths, pats, types);
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                walk_expr(value, exprs, paths, pats, types);
                walk_expr(len, exprs, paths, pats, types);
            }
            ExprKind::StructLit { path, fields } => {
                paths.insert(path.id);
                for f in fields {
                    walk_expr(&f.value, exprs, paths, pats, types);
                }
            }
            ExprKind::Range { start, end, .. } => {
                // A range reaching this pass is a closed `for` clause (typeck refuses
                // the open and inclusive forms); visit whichever bounds are present.
                for bound in start.iter().chain(end.iter()) {
                    walk_expr(bound, exprs, paths, pats, types);
                }
            }
            ExprKind::Try(inner) | ExprKind::Await(inner) => {
                walk_expr(inner, exprs, paths, pats, types)
            }
            ExprKind::Return(Some(v)) => walk_expr(v, exprs, paths, pats, types),
            _ => {}
        }
    }

    fn walk_match_arm(
        arm: &MatchArm,
        exprs: &mut BTreeSet<NodeId>,
        paths: &mut BTreeSet<NodeId>,
        pats: &mut BTreeSet<NodeId>,
        types: &mut BTreeSet<NodeId>,
    ) {
        walk_pat(&arm.pat, paths, pats);
        walk_expr(&arm.body, exprs, paths, pats, types);
    }

    fn try_file(path: &str, src: &str) {
        let mut sources = SourceMap::new();
        let file = sources.add(path, src);
        let parsed = parser::parse_file(file, src);
        assert!(
            parsed.diags.is_empty(),
            "parse errors: {:?}",
            parsed.diags.as_slice()
        );
        let options = CheckOptions::default();
        let result = check(&parsed.program, &sources, &options);
        let checked = match result.checked {
            Some(c) => c,
            None => panic!(
                "check failed: {:?}",
                result
                    .diags
                    .iter()
                    .map(|d| (d.code.clone(), d.message.clone()))
                    .collect::<Vec<_>>()
            ),
        };
        let (exprs, paths, pats, types) = collect_ids(&parsed.program);
        let missing_exprs: Vec<NodeId> = exprs
            .iter()
            .copied()
            .filter(|id| !checked.facts.expr_types.contains_key(id))
            .collect();
        let missing_paths: Vec<(NodeId, String)> = paths
            .iter()
            .copied()
            .filter(|id| !checked.facts.resolutions.contains_key(id))
            .map(|id| (id, "?".to_string()))
            .collect();
        let missing_pats: Vec<NodeId> = pats
            .iter()
            .copied()
            .filter(|id| !checked.facts.pat_types.contains_key(id))
            .collect();
        let missing_types: Vec<NodeId> = types
            .iter()
            .copied()
            .filter(|id| !checked.facts.type_nodes.contains_key(id))
            .collect();
        assert!(
            missing_exprs.is_empty(),
            "missing expr_types for {missing_exprs:?}"
        );
        assert!(
            missing_paths.is_empty(),
            "missing resolutions for {missing_paths:?}"
        );
        assert!(
            missing_pats.is_empty(),
            "missing pat_types for {missing_pats:?}"
        );
        assert!(
            missing_types.is_empty(),
            "missing type_nodes for {missing_types:?}"
        );
    }

    #[test]
    fn fib_example_facts_are_complete() {
        let src = include_str!("../../../examples/fib/src/main.rv");
        try_file("src/main.rv", src);
    }

    #[test]
    fn calculator_example_facts_are_complete() {
        let src = include_str!("../../../examples/calculator/src/main.rv");
        try_file("src/main.rv", src);
    }
}

#[cfg(test)]
mod inference_and_rules_tests {
    use super::check;
    use crate::facts::{CallTarget, CheckOptions, CheckResult, Profile, Resolution};
    use ravum_lang_syntax::parser;
    use ravum_lang_syntax::source::SourceMap;

    fn run(path: &str, src: &str) -> (CheckResult, SourceMap) {
        let mut sources = SourceMap::new();
        let file = sources.add(path, src);
        let parsed = parser::parse_file(file, src);
        assert!(
            parsed.diags.is_empty(),
            "parse: {:?}",
            parsed
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
        let options = CheckOptions {
            profile: Profile::Core,
            edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
        };
        (check(&parsed.program, &sources, &options), sources)
    }

    fn expect_code(result: &CheckResult, code: &str) {
        assert!(
            result.diags.iter().any(|d| d.code == code),
            "expected {code}, got {:?}",
            result
                .diags
                .iter()
                .map(|d| d.code.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn vec_inference_from_push_is_accepted() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn main() -> Result<(), Error> {
    let mut out = Vec::new();
    let mut i: usize = 0;
    while i < 3 {
        out.push(i as f64 * 0.5);
        i += 1;
    }
    io::println(&out[0].to_string());
    Ok(())
}
"#,
        );
        if let Some(checked) = &result.checked {
            let main = checked.facts.main_fn.unwrap();
            let effects = &checked.facts.effects[&main];
            assert!(effects.contains(crate::facts::Effect::Alloc));
            assert!(effects.contains(crate::facts::Effect::Io));
        } else {
            panic!(
                "expected success: {:?}",
                result
                    .diags
                    .iter()
                    .map(|d| (d.code.clone(), d.message.clone()))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn unconstrained_none_is_ambiguous() {
        let (result, _) = run("src/main.rv", r#"fn main() { let x = None; }"#);
        expect_code(&result, "E-TYP-AMBIGUOUS");
    }

    #[test]
    fn none_with_context_is_accepted() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn f(o: Option<i32>) -> i32 { match o { Option::Some(x) => x, Option::None => 0 } }
fn main() { let o: Option<i32> = None; let n = f(o); }
"#,
        );
        assert!(
            result.checked.is_some(),
            "expected success: {:?}",
            result
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn string_ref_to_str_coercion_is_applied() {
        // TYP-13.1: `&String` coerces to `&str`.
        let (result, _) = run(
            "src/main.rv",
            r#"
fn takes(s: &str) -> usize { return s.len(); }
fn main() {
    let a = String::from("ab");
    let n = takes(&a);
}
"#,
        );
        assert!(
            result.checked.is_some(),
            "expected success: {:?}",
            result
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn owned_string_is_not_implicitly_coerced_to_str() {
        // TYP-8.5 (MUST): `String -> &str` is explicit (`.as_str()`).
        // TYP-13.1 (MUST): the only implicit text adjustment is
        // `&String -> &str`, never an owned `String`. This was accepted and the
        // generated Rust failed with E0308 (`examples/stats`,
        // `conformance/cases/run/stats` used to depend on it; both are being
        // migrated).
        let (result, _) = run(
            "src/main.rv",
            r#"
fn takes(s: &str) -> usize { return s.len(); }
fn main() {
    let b = String::from("cd");
    let m = takes(b);
}
"#,
        );
        assert!(
            result.checked.is_none(),
            "an owned `String` must not be coerced to `&str`: {:?}",
            result
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            result.diags.iter().any(|d| d.code == "E-TYP-MISMATCH"),
            "expected E-TYP-MISMATCH, got {:?}",
            result
                .diags
                .iter()
                .map(|d| d.code.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn assert_eq_unifies_literals_and_text() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn main() { }
test fn t() {
    let s = String::from("ravum");
    assert_eq(s.len(), 5);
    let value: i64 = 42;
    assert_eq(value, 42);
    assert_eq(s.to_uppercase(), "RAVUM");
}
"#,
        );
        assert!(
            result.checked.is_some(),
            "expected success: {:?}",
            result
                .diags
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn index_literal_is_usize_but_int_variable_is_rejected() {
        let (ok_result, _) = run(
            "src/main.rv",
            r#"fn main() { let a = [1, 2, 3]; let x = a[0]; }"#,
        );
        assert!(ok_result.checked.is_some(), "literal index must be usize");

        let (bad, _) = run(
            "src/main.rv",
            r#"fn main() { let a = [1, 2, 3]; let i: i32 = 0; let x = a[i]; }"#,
        );
        expect_code(&bad, "E-TYP-INDEX");
    }

    #[test]
    fn assignment_to_immutable_local_is_rejected() {
        let (result, _) = run("src/main.rv", r#"fn main() { let x = 1; x = 2; }"#);
        expect_code(&result, "E-OWN-IMMUTABLE");
    }

    #[test]
    fn canonical_prelude_constructor_facts() {
        let (result, _) = run(
            "src/main.rv",
            r#"
fn main() {
    let o = Option::Some(1);
    let n: Option<i32> = None;
    let r: Result<i32, Error> = Result::Ok(2);
}
"#,
        );
        let checked = result.checked.expect("checked program");
        assert!(checked
            .facts
            .resolutions
            .values()
            .any(|r| matches!(r, Resolution::BuiltinFn { path } if path == "Option::Some")));
        assert!(checked
            .facts
            .resolutions
            .values()
            .any(|r| matches!(r, Resolution::BuiltinConst { path } if path == "Option::None")));
        assert!(checked
            .facts
            .calls
            .values()
            .any(|c| matches!(c, CallTarget::BuiltinFn { path } if path == "Result::Ok")));
    }
}
