//! Fuzz smoke tests for the Ravum semantic frontend (`ravum-lang-syntax` +
//! `ravum-lang-sema`).
//!
//! Objectifs (contrat de qualification, voir
//! `docs/qualification/FUZZ-MIRI-SEMA.md`) :
//!
//! * aucun **panic** sur une entrée arbitraire (le frontend est total) ;
//! * tout diagnostic porte un **code non vide**, un **message non vide** et un
//!   **span valide** (dans le fichier analysé, ou `Span::dummy()` pour un
//!   diagnostic synthétique) ;
//! * **déterminisme strict** : la même source produit exactement les mêmes
//!   diagnostics et les mêmes faits (types, résolutions, effets, chemins) ;
//! * reproduction : à l'échec, la graine, le profil et la source en hexadécimal
//!   sont affichés.
//!
//! Générateur : xorshift64* à graines fixes, mutations de templates
//! représentatifs (items, types, effets, modules, génériques). Budget :
//! `RAVUM_FUZZ_ITERS` (défaut 300 pour `fuzz_check_short`, 20 000 pour le mode
//! long `#[ignore]`).
//!
//! Ce test n'est **pas** exécuté sous Miri (trop lent) : voir la doc de
//! qualification pour le périmètre exact.

use ravum_lang_sema::syntax::{diag::DiagnosticBag, parser, source::SourceMap, span::Span};
use ravum_lang_sema::{check, CheckOptions, CheckResult, Profile};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// Générateur pseudo-aléatoire déterministe (xorshift64*)
// ---------------------------------------------------------------------------

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        // Une graine nulle est interdite pour xorshift.
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound <= 1 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

// ---------------------------------------------------------------------------
// Corpus et mutations
// ---------------------------------------------------------------------------

/// Programmes de départ représentatifs du sous-ensemble Core 1.0.
const TEMPLATES: &[&str] = &[
    r#"use std::io;
fn main() { io::println("hello"); }
"#,
    r#"struct Point { pub x: f64, pub y: f64 }
enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }
impl Point {
    fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y }; }
    fn norme(&self) -> f64 { return (self.x * self.x + self.y * self.y).sqrt(); }
}
fn aire(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.0 * r * r,
        Shape::Rect { w: w, h: h } => w * h,
        Shape::Empty => 0.0,
    }
}
fn main() { let mut p = Point::new(3.0, 4.0); let n = p.norme(); }
"#,
    r#"fn fib(n: u32) -> u64 {
    if n < 2 { return n as u64; }
    fib(n - 1) + fib(n - 2)
}
fn main() -> Result<(), Error> {
    let mut i: u32 = 0;
    while i <= 10 { io::println(&fib(i).to_string()); i += 1; }
    Ok(())
}
"#,
    r#"fn lire(p: &str) -> Result<String, Error> effects(fs, alloc) {
    let s = fs::read_to_string(p)?;
    return Result::Ok(s);
}
fn main() -> Result<(), Error> {
    let s = lire("a.txt")?;
    io::println(s.as_str());
    Ok(())
}
"#,
    r#"mod geo {
    pub struct Point { pub x: f64, pub y: f64 }
    impl Point {
        pub fn new(x: f64, y: f64) -> Point { Point { x: x, y: y } }
        pub fn somme(&self) -> f64 { self.x + self.y }
    }
}
fn main() {
    let p = geo::Point::new(1.0, 2.0);
    let s = p.somme();
}
"#,
    r#"const N: i32 = 10;
struct Boite<T> { pub contenu: T }
fn emballer<T>(x: T) -> Boite<T> { return Boite { contenu: x }; }
fn main() {
    let b = emballer(3);
    let v: Vec<i32> = Vec::new();
    let mut v = v;
    v.push(N);
    let o = Option::Some(1);
    let x = match o { Option::Some(v) => v, Option::None => 0 };
    let a = [1, 2, 3];
    let i: usize = 0;
    let z = a[i] as i64;
}
"#,
    r#"// Unicode autorisé dans les littéraux et commentaires.
fn main() {
    let texte = "héllo 😀";
    io::println(texte.as_str());
    let c = 'é';
    let n = c as u32;
}
"#,
    r#"fn main() {
    let mut total = 0;
    for i in 0..10 { total += i; }
    loop { break; }
    let mut k = 0;
    while k < 3 { k += 1; }
    let t = (1, "x", 2.5);
    let premier = t.0;
}
"#,
];

/// Fragments insérés par mutation (mots-clés, identifiants, ponctuation,
/// littéraux). Volontairement proches des tokens du langage.
const FRAGMENTS: &[&str] = &[
    "fn", "let", "mut", "struct", "enum", "impl", "use", "pub", "mod", "match", "if", "else",
    "while", "loop", "for", "in", "break", "continue", "return", "true", "false", "as", "test",
    "self", "Self", "crate", "super", "effects", "io", "fs", "math", "value", "Point", "Shape",
    "Option", "Result", "Vec", "String", "i32", "u8", "usize", "f64", "bool", "char", "&str",
    "fn ", "()", "->", "=>", "::", "..", ".", ",", ";", ":", "::", "{", "}", "(", ")", "[", "]",
    "+", "-", "*", "/", "%", "!", "&", "|", "^", "=", "==", "!=", "<", ">", "<=", ">=", "&&", "||",
    "?", "0", "1", "42", "1.0", "\"x\"", "'a'", " ", "\n", "\t", "",
];

fn mutate(rng: &mut Rng, base: &str) -> String {
    let mut chars: Vec<char> = base.chars().collect();
    let operations = 1 + rng.below(5);
    for _ in 0..operations {
        match rng.below(6) {
            // Insertion d'un fragment.
            0 => {
                let fragment = *rng.pick(FRAGMENTS);
                let at = rng.below(chars.len() + 1);
                for (offset, ch) in fragment.chars().enumerate() {
                    chars.insert((at + offset).min(chars.len()), ch);
                }
            }
            // Suppression d'une plage.
            1 if !chars.is_empty() => {
                let start = rng.below(chars.len());
                let len = 1 + rng.below((chars.len() - start).min(16));
                chars.drain(start..start + len);
            }
            // Remplacement d'un caractère par un fragment.
            2 if !chars.is_empty() => {
                let at = rng.below(chars.len());
                let fragment = *rng.pick(FRAGMENTS);
                let mut replacement = fragment.chars();
                if let Some(first) = replacement.next() {
                    chars[at] = first;
                }
                for ch in replacement {
                    chars.insert((at + 1).min(chars.len()), ch);
                }
            }
            // Duplication d'une plage.
            3 if !chars.is_empty() => {
                let start = rng.below(chars.len());
                let len = 1 + rng.below((chars.len() - start).min(32));
                let copy: Vec<char> = chars[start..start + len].to_vec();
                let at = rng.below(chars.len() + 1);
                for (offset, ch) in copy.into_iter().enumerate() {
                    chars.insert((at + offset).min(chars.len()), ch);
                }
            }
            // Troncature.
            4 if chars.len() > 1 => {
                let keep = 1 + rng.below(chars.len() - 1);
                chars.truncate(keep);
            }
            // Remplacement par un caractère ASCII arbitraire (bruit lexical).
            _ if !chars.is_empty() => {
                let at = rng.below(chars.len());
                chars[at] = (0x21 + rng.below(0x5e) as u8) as char;
            }
            _ => {}
        }
    }
    let mut out: String = chars.into_iter().collect();
    // Borne la taille pour garder le fuzz rapide (et exécutable sous contrainte
    // de temps) : les templates font tous moins de 2 000 caractères.
    if out.len() > 8_192 {
        out.truncate(8_192);
    }
    out
}

// ---------------------------------------------------------------------------
// Exécution et validation
// ---------------------------------------------------------------------------

/// Instantané comparable des diagnostics (déterminisme).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagSnap {
    code: String,
    severity: &'static str,
    message: String,
    span: (u32, u32, u32),
    notes: Vec<(String, Option<(u32, u32, u32)>)>,
}

fn span_tuple(span: Span) -> (u32, u32, u32) {
    (span.file, span.start, span.end)
}

fn snapshot(bag: &DiagnosticBag) -> Vec<DiagSnap> {
    bag.iter()
        .map(|d| DiagSnap {
            code: d.code.clone(),
            severity: d.severity.as_str(),
            message: d.message.clone(),
            span: span_tuple(d.span),
            notes: d
                .notes
                .iter()
                .map(|n| (n.message.clone(), n.span.map(span_tuple)))
                .collect(),
        })
        .collect()
}

/// Instantané des faits produits par un programme accepté.
#[derive(Debug, Clone, PartialEq)]
struct FactsSnap {
    expr_types: Vec<(u64, ravum_lang_sema::Type)>,
    pat_types: Vec<(u64, ravum_lang_sema::Type)>,
    type_nodes: Vec<(u64, ravum_lang_sema::Type)>,
    item_paths: Vec<(u64, String)>,
    effects: Vec<(u64, Vec<String>)>,
    calls: Vec<(u64, String)>,
    resolutions: Vec<(u64, String)>,
    methods: Vec<(u64, String)>,
    main_fn: Option<u64>,
    test_fns: Vec<u64>,
}

#[derive(Debug, PartialEq)]
struct Outcome {
    parse_diags: Vec<DiagSnap>,
    check_diags: Vec<DiagSnap>,
    checked: bool,
    facts: Option<FactsSnap>,
}

fn snapshot_facts(result: &CheckResult) -> Option<FactsSnap> {
    let checked = result.checked.as_ref()?;
    let facts = &checked.facts;
    Some(FactsSnap {
        expr_types: facts
            .expr_types
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        pat_types: facts
            .pat_types
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        type_nodes: facts
            .type_nodes
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        item_paths: facts
            .item_paths
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        effects: facts
            .effects
            .iter()
            .map(|(k, set)| (*k, set.names().iter().map(|n| n.to_string()).collect()))
            .collect(),
        calls: facts
            .calls
            .iter()
            .map(|(k, target)| (*k, format!("{target:?}")))
            .collect(),
        resolutions: facts
            .resolutions
            .iter()
            .map(|(k, resolution)| (*k, format!("{resolution:?}")))
            .collect(),
        methods: facts
            .methods
            .iter()
            .map(|(k, target)| (*k, format!("{target:?}")))
            .collect(),
        main_fn: facts.main_fn,
        test_fns: facts.test_fns.clone(),
    })
}

fn run_frontend(src: &str, profile: Profile) -> Outcome {
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", src);
    let parsed = parser::parse_file(file, src);
    let options = CheckOptions {
        profile,
        edition: ravum_lang_sema::syntax::LANGUAGE_EDITION.to_string(),
    };
    let result = check(&parsed.program, &sources, &options);
    Outcome {
        parse_diags: snapshot(&parsed.diags),
        check_diags: snapshot(&result.diags),
        checked: result.checked.is_some(),
        facts: snapshot_facts(&result),
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fail(seed: u64, iteration: usize, profile: Profile, src: &str, reason: &str) -> ! {
    panic!(
        "\n=== RAVUM SEMA FUZZ FAILURE ===\n\
         reason: {reason}\n\
         iteration: {iteration}\n\
         seed: {seed} (0x{seed:016x})\n\
         profile: {}\n\
         source_len: {} bytes\n\
         source_hex: {}\n\
         --- source ---\n{src}\n--- end source ---\n",
        profile.name(),
        src.len(),
        hex_dump(src.as_bytes()),
    )
}

/// Valide les invariants d'un instantané de diagnostics : codes/messages non
/// vides, spans dans le fichier (ou `Span::dummy()`).
fn validate_snapshot(
    diagnostics: &[DiagSnap],
    file: u32,
    src_len: usize,
    seed: u64,
    iteration: usize,
    profile: Profile,
    src: &str,
    phase: &str,
) {
    let check_span = |span: (u32, u32, u32), what: &str| {
        if span.0 == u32::MAX {
            return;
        }
        let valid = span.0 == file && span.1 <= span.2 && span.2 as usize <= src_len;
        if !valid {
            fail(
                seed,
                iteration,
                profile,
                src,
                &format!(
                    "{phase}: invalid {what} span {:?} (file={file}, len={src_len})",
                    Span::new(span.0, span.1, span.2)
                ),
            );
        }
    };
    for diag in diagnostics {
        if diag.code.is_empty() {
            fail(
                seed,
                iteration,
                profile,
                src,
                &format!("{phase}: empty diagnostic code"),
            );
        }
        if diag.message.is_empty() {
            fail(
                seed,
                iteration,
                profile,
                src,
                &format!("{phase}: empty diagnostic message for `{}`", diag.code),
            );
        }
        check_span(diag.span, "primary");
        for (message, note_span) in &diag.notes {
            if message.is_empty() {
                fail(
                    seed,
                    iteration,
                    profile,
                    src,
                    &format!("{phase}: empty note message for `{}`", diag.code),
                );
            }
            if let Some(span) = note_span {
                check_span(*span, "note");
            }
        }
    }
}

fn run_case(seed: u64, iteration: usize, profile: Profile, src: &str) {
    let first = match catch_unwind(AssertUnwindSafe(|| run_frontend(src, profile))) {
        Ok(outcome) => outcome,
        Err(_) => fail(seed, iteration, profile, src, "panic in parser or checker"),
    };
    validate_snapshot(
        &first.parse_diags,
        0,
        src.len(),
        seed,
        iteration,
        profile,
        src,
        "parse",
    );
    validate_snapshot(
        &first.check_diags,
        0,
        src.len(),
        seed,
        iteration,
        profile,
        src,
        "check",
    );

    // Déterminisme : même source => mêmes diagnostics et mêmes faits.
    let second = match catch_unwind(AssertUnwindSafe(|| run_frontend(src, profile))) {
        Ok(outcome) => outcome,
        Err(_) => fail(
            seed,
            iteration,
            profile,
            src,
            "panic on the second (determinism) run",
        ),
    };
    if first != second {
        fail(
            seed,
            iteration,
            profile,
            src,
            "non-deterministic outcome between two identical runs",
        );
    }
}

fn fuzz_round(seed_base: u64, iterations: usize) -> (u64, u64) {
    let started = std::time::Instant::now();
    let mut rng = Rng::new(seed_base);
    for iteration in 0..iterations {
        let seed = rng.next_u64();
        let mut case_rng = Rng::new(seed);
        let template = *case_rng.pick(TEMPLATES);
        let src = mutate(&mut case_rng, template);
        let profile = match case_rng.below(3) {
            0 => Profile::Core,
            1 => Profile::Pure,
            _ => Profile::Rt,
        };
        run_case(seed, iteration, profile, &src);
    }
    let elapsed = started.elapsed();
    (iterations as u64, elapsed.as_millis() as u64)
}

fn iterations(default: usize) -> usize {
    std::env::var("RAVUM_FUZZ_ITERS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn fuzz_check_short() {
    let started = std::time::Instant::now();
    let (iterations, _) = fuzz_round(0x0052_4156_554D_465A, iterations(300));
    eprintln!(
        "fuzz_check_short: {iterations} iterations in {:?}",
        started.elapsed()
    );
}

#[test]
#[ignore = "long fuzz run (budget via RAVUM_FUZZ_ITERS, default 20 000)"]
fn fuzz_check_long() {
    let budget = iterations(20_000);
    let started = std::time::Instant::now();
    let (iterations, millis) = fuzz_round(0x0052_4156_554D_465A_4C, budget);
    eprintln!(
        "fuzz_check_long: {iterations} iterations in {millis} ms ({:.0}/s)",
        iterations as f64 / (millis.max(1) as f64 / 1000.0)
    );
    eprintln!("fuzz_check_long total wall: {:?}", started.elapsed());
}
