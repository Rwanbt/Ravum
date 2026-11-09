//! Deterministic fuzz smoke tests for the Ravum lexer/parser/formatter.
//!
//! This file is a black-box harness over the public `ravum-lang-syntax` API
//! (`lexer::lex`, `parser::parse_file`, `fmt::format_source`). It uses a
//! hand-written xorshift64 generator with fixed seeds: no external crate, no
//! wall-clock or address-dependent input. The same `(seed, iteration, budget)`
//! always produces the same corpus and the same diagnostics.
//!
//! Invariants checked on every generated input:
//!
//! * `lex` (with and without trivia) never panics, token/diagnostic streams are
//!   deterministic, and every span is `file == 0`, `start <= end <= src.len()`;
//! * the last lexer token is `EOF` located exactly at `src.len()`;
//! * `parse_file` never panics, its AST and diagnostics are deterministic for
//!   the same input, and every diagnostic span is within the source;
//! * when parsing succeeds, `format_source` succeeds, its output re-parses
//!   without errors, formatting is idempotent, and the re-parsed AST is
//!   structurally identical (node ids and byte spans excluded);
//! * invalid UTF-8 byte sequences are rejected by the `&str` boundary (the
//!   lexer API cannot receive them); the harness also exercises the valid
//!   UTF-8 prefix and the lossy replacement of such inputs.
//!
//! Budgets:
//!
//! * default: 512 iterations, always runs with `cargo test`;
//! * long mode: `#[ignore]`d, run with
//!   `RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax \
//!      --test fuzz_lex_parse -- --ignored --nocapture`
//!   (default long budget is 20_000 when the variable is absent).
//!
//! `RAVUM_FUZZ_ITERS` overrides the budget of both tests when set. Failures
//! print the generator, seed, iteration and a hex dump (truncated at 16 KiB,
//! with the full length) before panicking.

use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::fmt::format_source;
use ravum_lang_syntax::lexer::lex;
use ravum_lang_syntax::parser::parse_file;
use ravum_lang_syntax::span::{FileId, Span};
use ravum_lang_syntax::token::Token;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

const FILE: FileId = 0;
const BASE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
const SHORT_DEFAULT_ITERS: u64 = 512;
const LONG_DEFAULT_ITERS: u64 = 20_000;
const HEX_CAP: usize = 16 * 1024;
/// In collect mode, keep at most this many failure reports (the rest are only
/// counted) so a systematic bug cannot flood the output.
const COLLECT_CAP: usize = 40;

// ---------------------------------------------------------------------------
// Deterministic generator
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { BASE_SEED } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64 (Marsaglia): 13/7/17 shifts, period 2^64 - 1.
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn below(&mut self, upper: usize) -> usize {
        if upper == 0 {
            0
        } else {
            (self.next_u64() % upper as u64) as usize
        }
    }

    fn percent(&mut self, percent: u64) -> bool {
        percent > 0 && self.next_u64() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn env_budget(default: u64) -> u64 {
    std::env::var("RAVUM_FUZZ_ITERS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// `RAVUM_FUZZ_COLLECT=1` records failures and keeps fuzzing instead of aborting
/// on the first one. The test still fails at the end with the collected list.
/// This is an exploration aid; the default fast-fail behaviour is unchanged.
fn collect_mode() -> bool {
    std::env::var("RAVUM_FUZZ_COLLECT")
        .map(|value| {
            let value = value.trim();
            !value.is_empty() && value != "0"
        })
        .unwrap_or(false)
}

fn iteration_seed(base: u64, iteration: u64) -> u64 {
    base ^ iteration
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xD1B5_4A32_D192_ED03)
}

const ALPHABET: &[char] = &[
    'a', 'b', 'Z', '_', '0', '7', ' ', '\t', '\n', '\r', '(', ')', '{', '}', '[', ']', ';', ',',
    ':', '.', '-', '+', '*', '/', '%', '&', '|', '^', '!', '=', '<', '>', '?', '"', '\'', '\\',
    '#', '@', '$', 'é', 'λ', '😀', '\u{feff}', '\u{200b}', '\0',
];

const KEYWORDS: &[&str] = &[
    "fn", "struct", "enum", "impl", "const", "use", "mod", "pub", "let", "mut", "if", "else",
    "match", "while", "loop", "for", "in", "break", "continue", "return", "true", "false", "as",
    "test", "unsafe", "self", "Self", "crate", "super", "trait", "where", "extern", "async",
    "move", "type", "static", "effects", "_",
];

const IDENTS: &[&str] = &[
    "foo", "Bar", "x", "main", "Vec", "Option", "Result", "Error", "compute", "Point", "café", "é",
    "λ",
];

const PUNCT_ATOMS: &[&str] = &[
    "(", ")", "{", "}", "[", "]", ";", ",", ".", ":", "::", "..", "..=", "->", "=>", "+", "-", "*",
    "/", "%", "&", "&&", "|", "||", "^", "!", "=", "==", "!=", "<", "<=", ">", ">=", "<<", ">>",
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "?", "#", "@",
];

const LITERAL_ATOMS: &[&str] = &[
    "0",
    "1",
    "42",
    "1_000",
    "0xFF",
    "0o17",
    "0b1010",
    "10z",
    "1e9",
    "1.5e-3f64",
    "1.",
    "'a'",
    "'\\n'",
    "\"hi\"",
    "\"unterminated",
    "\"a\nb\"",
    "0xFF_FFu16",
];

const COMMENT_ATOMS: &[&str] = &[
    "// c\n",
    "/* c */",
    "/* unclosed",
    "/// doc\n",
    "//! inner\n",
    "// no newline at eof",
];

const SEED_PROGRAMS: &[&str] = &[
    "fn main() { io::println(\"hello\"); }\n",
    "struct Point { pub x: f64, y: f64 }\nimpl Point { fn norm(&self) -> f64 { self.x } }\n",
    "fn fib(n: u32) -> u64 { if n < 2 { return n as u64; } fib(n - 1) + fib(n - 2) }\nfn main() -> Result<(), Error> { Ok(()) }\n",
    "enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }\nfn f(s: Shape) -> f64 { match s { Shape::Circle(r) => r, Shape::Rect { w, h } => w, Shape::Empty => 0.0 } }\n",
    "test fn t() { let mut x: i32 = 0; while x < 10 { x += 1; } for i in 0..3 { continue; } }\n",
    "#[test]\nfn attributed() { }\n",
    "// comment\nfn f() { let s = \"a\\n\"; let c = 'x'; }\n",
    "fn log(s: &str) effects() { }\n",
];

fn gen_random_chars(rng: &mut Rng) -> String {
    let len = rng.below(160);
    let mut out = String::new();
    for _ in 0..len {
        out.push(*rng.pick(ALPHABET));
        if rng.percent(8) {
            out.push('\n');
        }
    }
    if rng.percent(12) {
        let ch = *rng.pick(&['a', '0', ' ', '/', '"', '(', '{']);
        let repeats = 64 + rng.below(512);
        for _ in 0..repeats {
            out.push(ch);
        }
    }
    out
}

fn gen_short_chars(rng: &mut Rng) -> String {
    let len = rng.below(12);
    let mut out = String::new();
    for _ in 0..len {
        out.push(*rng.pick(ALPHABET));
    }
    out
}

fn gen_token_soup(rng: &mut Rng) -> String {
    let count = rng.below(80);
    let mut out = String::new();
    for _ in 0..count {
        let atom = match rng.below(100) {
            0..=19 => *rng.pick(KEYWORDS),
            20..=34 => *rng.pick(IDENTS),
            35..=64 => *rng.pick(PUNCT_ATOMS),
            65..=79 => *rng.pick(LITERAL_ATOMS),
            80..=89 => *rng.pick(COMMENT_ATOMS),
            _ => *rng.pick(&[" ", "\n", "\t", "\r\n"]),
        };
        out.push_str(atom);
        if rng.percent(55) {
            out.push(' ');
        }
        if rng.percent(15) {
            out.push('\n');
        }
    }
    out
}

fn char_boundaries(text: &str) -> Vec<usize> {
    let mut bounds: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    bounds.push(text.len());
    bounds
}

fn random_range(rng: &mut Rng, bounds: &[usize]) -> (usize, usize) {
    let count = bounds.len();
    if count < 2 {
        return (0, 0);
    }
    let first = rng.below(count - 1);
    let second = first + 1 + rng.below(count - 1 - first);
    (bounds[first], bounds[second])
}

fn gen_mutated(rng: &mut Rng) -> String {
    let mut text = (*rng.pick(SEED_PROGRAMS)).to_string();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        if text.is_empty() {
            text.push(*rng.pick(ALPHABET));
        }
        let bounds = char_boundaries(&text);
        match rng.below(6) {
            0 => {
                let cut = bounds[rng.below(bounds.len())];
                text.truncate(cut);
            }
            1 => {
                let (start, end) = random_range(rng, &bounds);
                text.replace_range(start..end, "");
            }
            2 => {
                let (start, end) = random_range(rng, &bounds);
                let piece = text[start..end].to_string();
                let at = bounds[rng.below(bounds.len())];
                text.insert_str(at, &piece);
            }
            3 => {
                let (start, end) = random_range(rng, &bounds);
                let replacement = gen_short_chars(rng);
                text.replace_range(start..end, &replacement);
            }
            4 => {
                let at = bounds[rng.below(bounds.len())];
                let insert = gen_short_chars(rng);
                text.insert_str(at, &insert);
            }
            _ => {
                let atom = *rng.pick(PUNCT_ATOMS);
                let at = bounds[rng.below(bounds.len())];
                text.insert_str(at, atom);
            }
        }
    }
    text
}

const INTERESTING_BYTES: &[u8] = &[
    0x00, 0x09, 0x0a, 0x0d, 0x22, 0x27, 0x2f, 0x5c, 0x7f, 0x80, 0x9f, 0xc0, 0xc3, 0xa9, 0xe2, 0x80,
    0x8b, 0xef, 0xbb, 0xbf, 0xf0, 0x9f, 0x98, 0x80, 0xff,
];

fn gen_raw_bytes(rng: &mut Rng) -> Vec<u8> {
    let len = rng.below(96);
    let mut bytes = Vec::with_capacity(len + 16);
    for _ in 0..len {
        if rng.percent(35) {
            bytes.push(*rng.pick(INTERESTING_BYTES));
        } else {
            bytes.push((rng.next_u64() & 0xff) as u8);
        }
    }
    if rng.percent(30) {
        bytes.extend_from_slice("é😀\0\u{feff}".as_bytes());
    }
    bytes
}

// ---------------------------------------------------------------------------
// Invariant checks and failure reporting
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    label: &'static str,
    seed: u64,
    iteration: u64,
    generator: &'static str,
    src: &'a str,
    /// Raw bytes when `src` is a lossy/prefix view of a non-UTF-8 input.
    raw: Option<&'a [u8]>,
    /// In collect mode the detailed dump is deferred to the end of the run.
    quiet: bool,
}

impl Ctx<'_> {
    fn fail(&self, reason: impl AsRef<str>) -> ! {
        let reason = reason.as_ref();
        if !self.quiet {
            eprintln!("\n=== FUZZ FAILURE [{}] ===", self.label);
            eprintln!("generator:  {}", self.generator);
            eprintln!("seed:       0x{:016x}", self.seed);
            eprintln!("iteration:  {}", self.iteration);
            eprintln!("reason:     {reason}");
            eprintln!("input len:  {} bytes (str)", self.src.len());
            let bytes = self.raw.unwrap_or_else(|| self.src.as_bytes());
            eprintln!("raw len:    {} bytes", bytes.len());
            eprintln!("repro:      RAVUM_FUZZ_ITERS={} cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture", self.iteration + 1);
            eprintln!("hex dump (max {HEX_CAP} bytes):");
            eprintln!("{}", hex_dump(bytes, HEX_CAP));
        }
        panic!(
            "fuzz failure [{}/{}] seed=0x{:016x} iteration={}: {reason}",
            self.label, self.generator, self.seed, self.iteration
        );
    }
}

fn hex_dump(bytes: &[u8], cap: usize) -> String {
    let shown = bytes.len().min(cap);
    let mut out = String::new();
    for (offset, chunk) in bytes[..shown].chunks(16).enumerate() {
        out.push_str(&format!("{:08x}: ", offset * 16));
        for byte in chunk {
            out.push_str(&format!("{byte:02x} "));
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push('|');
        for &byte in chunk {
            out.push(if (0x20..=0x7e).contains(&byte) {
                byte as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    if bytes.len() > shown {
        out.push_str(&format!(
            "... truncated: {} bytes total, {} shown\n",
            bytes.len(),
            shown
        ));
    }
    out
}

fn same_diags(left: &DiagnosticBag, right: &DiagnosticBag) -> bool {
    left.len() == right.len() && left.iter().zip(right.iter()).all(|(a, b)| a == b)
}

fn codes(bag: &DiagnosticBag) -> Vec<String> {
    bag.iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

fn span_in_bounds(span: Span, len: usize) -> bool {
    span.file == FILE && span.start <= span.end && span.end as usize <= len
}

fn check_token_spans(ctx: &Ctx<'_>, tokens: &[Token], src_len: usize) {
    if tokens.is_empty() {
        ctx.fail("lexer returned an empty token stream (EOF is mandatory)");
    }
    for (index, token) in tokens.iter().enumerate() {
        if !span_in_bounds(token.span, src_len) {
            ctx.fail(format!(
                "token #{index} span out of bounds: {:?} (src len {src_len})",
                token.span
            ));
        }
    }
    let last = tokens.last().expect("non-empty checked above");
    if !last.is_eof() {
        ctx.fail("the last lexer token is not EOF");
    }
    if last.span.start != src_len as u32 || last.span.end != src_len as u32 {
        ctx.fail(format!(
            "EOF span must be ({src_len}, {src_len}), got {:?}",
            last.span
        ));
    }
}

fn check_diag_spans(ctx: &Ctx<'_>, bag: &DiagnosticBag, src_len: usize) {
    for diagnostic in bag.iter() {
        if !span_in_bounds(diagnostic.span, src_len) {
            ctx.fail(format!(
                "diagnostic {} span out of bounds: {:?} (src len {src_len})",
                diagnostic.code, diagnostic.span
            ));
        }
        for note in &diagnostic.notes {
            if let Some(span) = note.span {
                if !span_in_bounds(span, src_len) {
                    ctx.fail(format!(
                        "diagnostic {} note span out of bounds: {:?} (src len {src_len})",
                        diagnostic.code, span
                    ));
                }
            }
        }
    }
}

/// Span- and id-free `Debug` view of the AST. It is only used to compare the AST
/// of a source with the AST of its formatted form; ids/spans necessarily differ.
/// Known limitation: string literals containing `id: ` or `Span {` could be
/// sanitized too, but both sides are sanitized identically.
fn normalized_debug(program: &ravum_lang_syntax::ast::Program) -> String {
    let debug = format!("{program:?}");
    let bytes = debug.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < debug.len() {
        if debug[index..].starts_with("id: ") {
            out.push_str("id: 0");
            index += 4;
            while index < debug.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            continue;
        }
        if debug[index..].starts_with("Span {") {
            while index < debug.len() && bytes[index] != b'}' {
                index += 1;
            }
            if index < debug.len() {
                index += 1;
            }
            out.push_str("Span");
            continue;
        }
        let ch = debug[index..].chars().next().expect("non-empty suffix");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn check_source(ctx: &Ctx<'_>, stats: &mut Stats) {
    let src = ctx.src;
    let src_len = src.len();
    stats.inputs += 1;
    stats.bytes += src_len as u64;
    digest_input(stats, ctx.seed, ctx.iteration, src.as_bytes());

    // 1. Lexer: determinism, spanned bounds, EOF position (twice, no trivia).
    let (tokens_a, diags_a) = lex(FILE, src, false);
    let (tokens_b, diags_b) = lex(FILE, src, false);
    if tokens_a != tokens_b {
        ctx.fail("lexer token stream is not deterministic for the same input");
    }
    if !same_diags(&diags_a, &diags_b) {
        ctx.fail(format!(
            "lexer diagnostics are not deterministic: {:?} vs {:?}",
            codes(&diags_a),
            codes(&diags_b)
        ));
    }
    check_token_spans(ctx, &tokens_a, src_len);
    check_diag_spans(ctx, &diags_a, src_len);
    stats.tokens += tokens_a.len() as u64;
    stats.diagnostics += diags_a.len() as u64;
    digest_diags(stats, &diags_a);

    // 1b. Same lexer with trivia (comments kept): formatter input path.
    let (trivia_tokens, trivia_diags) = lex(FILE, src, true);
    check_token_spans(ctx, &trivia_tokens, src_len);
    check_diag_spans(ctx, &trivia_diags, src_len);
    if trivia_diags.has_errors() != diags_a.has_errors() {
        ctx.fail("keep_trivia changed whether the lexer reports errors");
    }

    // 2. Parser: determinism, span bounds.
    let parsed_a = parse_file(FILE, src);
    let parsed_b = parse_file(FILE, src);
    if parsed_a.program != parsed_b.program {
        ctx.fail("parser AST is not deterministic for the same input");
    }
    if !same_diags(&parsed_a.diags, &parsed_b.diags) {
        ctx.fail(format!(
            "parser diagnostics are not deterministic: {:?} vs {:?}",
            codes(&parsed_a.diags),
            codes(&parsed_b.diags)
        ));
    }
    check_diag_spans(ctx, &parsed_a.diags, src_len);
    stats.diagnostics += parsed_a.diags.len() as u64;
    digest_diags(stats, &parsed_a.diags);

    if parsed_a.diags.has_errors() {
        stats.parse_err += 1;
        return;
    }
    stats.parse_ok += 1;

    // 3. Clean parse => formatter must succeed, re-parse cleanly, be
    //    idempotent and preserve the AST.
    let formatted = match format_source(FILE, src) {
        Ok(formatted) => formatted,
        Err(error) => ctx.fail(format!(
            "parse succeeded but format_source returned Err: {:?}",
            codes(&error)
        )),
    };
    let reparsed = parse_file(FILE, &formatted);
    if reparsed.diags.has_errors() {
        ctx.fail(format!(
            "formatted output does not re-parse cleanly: {:?}\n--- formatted ---\n{}",
            codes(&reparsed.diags),
            formatted
        ));
    }
    match format_source(FILE, &formatted) {
        Ok(second) => {
            if second != formatted {
                ctx.fail("formatting is not idempotent: fmt(fmt(x)) != fmt(x)");
            }
        }
        Err(error) => ctx.fail(format!(
            "format_source failed on its own output: {:?}",
            codes(&error)
        )),
    }
    if normalized_debug(&parsed_a.program) != normalized_debug(&reparsed.program) {
        ctx.fail("formatting changed the parsed AST");
    }
    stats.fmt_ok += 1;
}

// ---------------------------------------------------------------------------
// Statistics and cross-run digest
// ---------------------------------------------------------------------------

struct FailureRecord {
    generator: &'static str,
    seed: u64,
    iteration: u64,
    reason: String,
    input: Vec<u8>,
}

struct Stats {
    label: &'static str,
    base_seed: u64,
    budget: u64,
    inputs: u64,
    parse_ok: u64,
    parse_err: u64,
    fmt_ok: u64,
    invalid_utf8: u64,
    tokens: u64,
    diagnostics: u64,
    bytes: u64,
    digest: u64,
    collect: bool,
    failures: Vec<FailureRecord>,
    dropped_failures: u64,
}

impl Stats {
    fn new(label: &'static str, base_seed: u64, budget: u64, collect: bool) -> Self {
        Self {
            label,
            base_seed,
            budget,
            inputs: 0,
            parse_ok: 0,
            parse_err: 0,
            fmt_ok: 0,
            invalid_utf8: 0,
            tokens: 0,
            diagnostics: 0,
            bytes: 0,
            digest: 0xcbf2_9ce4_8422_2325, // FNV-1a offset basis
            collect,
            failures: Vec::new(),
            dropped_failures: 0,
        }
    }

    fn print(&self, started: Instant) {
        eprintln!(
            "[fuzz:{}] base_seed=0x{:016x} budget={} inputs={} parse_ok={} parse_err={} \
             fmt_ok={} invalid_utf8_inputs={} tokens={} diagnostics={} bytes={} \
             digest=0x{:016x} failures={} dropped_failures={} elapsed_ms={}",
            self.label,
            self.base_seed,
            self.budget,
            self.inputs,
            self.parse_ok,
            self.parse_err,
            self.fmt_ok,
            self.invalid_utf8,
            self.tokens,
            self.diagnostics,
            self.bytes,
            self.digest,
            self.failures.len(),
            self.dropped_failures,
            started.elapsed().as_millis()
        );
    }
}

fn digest_bytes(stats: &mut Stats, bytes: &[u8]) {
    for &byte in bytes {
        stats.digest ^= byte as u64;
        stats.digest = stats.digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn digest_u64(stats: &mut Stats, value: u64) {
    digest_bytes(stats, &value.to_le_bytes());
}

fn digest_input(stats: &mut Stats, seed: u64, iteration: u64, src: &[u8]) {
    digest_u64(stats, seed);
    digest_u64(stats, iteration);
    digest_u64(stats, src.len() as u64);
    digest_bytes(stats, src);
}

fn digest_diags(stats: &mut Stats, bag: &DiagnosticBag) {
    digest_u64(stats, bag.len() as u64);
    for diagnostic in bag.iter() {
        digest_bytes(stats, diagnostic.code.as_bytes());
        digest_u64(stats, diagnostic.span.start as u64);
        digest_u64(stats, diagnostic.span.end as u64);
        digest_u64(stats, diagnostic.severity as u64);
        digest_bytes(stats, diagnostic.message.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// Case drivers
// ---------------------------------------------------------------------------

fn run_ctx(
    stats: &mut Stats,
    seed: u64,
    iteration: u64,
    generator: &'static str,
    src: &str,
    raw: Option<&[u8]>,
) {
    let quiet = stats.collect;
    let ctx = Ctx {
        label: stats.label,
        seed,
        iteration,
        generator,
        src,
        raw,
        quiet,
    };
    if !stats.collect {
        check_source(&ctx, stats);
        return;
    }
    // Collect mode: keep fuzzing after a property violation and report every
    // recorded failure at the end (the test still fails).
    let outcome = catch_unwind(AssertUnwindSafe(|| check_source(&ctx, &mut *stats)));
    if let Err(payload) = outcome {
        let reason = panic_payload_message(payload.as_ref());
        let input = ctx.raw.unwrap_or_else(|| ctx.src.as_bytes()).to_vec();
        if stats.failures.len() < COLLECT_CAP {
            stats.failures.push(FailureRecord {
                generator,
                seed,
                iteration,
                reason,
                input,
            });
        } else {
            stats.dropped_failures += 1;
        }
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn print_collected_failures(stats: &Stats) {
    eprintln!(
        "[fuzz:{}] COLLECTED {} failure(s), {} dropped (cap {} per run)",
        stats.label,
        stats.failures.len(),
        stats.dropped_failures,
        COLLECT_CAP
    );
    for (index, failure) in stats.failures.iter().enumerate() {
        eprintln!("\n=== COLLECTED FUZZ FAILURE #{} ===", index + 1);
        eprintln!("generator:  {}", failure.generator);
        eprintln!("seed:       0x{:016x}", failure.seed);
        eprintln!("iteration:  {}", failure.iteration);
        eprintln!("reason:     {}", failure.reason);
        eprintln!("input len:  {} bytes", failure.input.len());
        eprintln!("repro:      RAVUM_FUZZ_ITERS={} cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture", failure.iteration + 1);
        eprintln!("hex dump (max 4096 bytes):");
        eprintln!("{}", hex_dump(&failure.input, 4096));
    }
}

fn run_raw_bytes_case(stats: &mut Stats, seed: u64, iteration: u64, rng: &mut Rng) {
    let bytes = gen_raw_bytes(rng);
    match std::str::from_utf8(&bytes) {
        Ok(src) => run_ctx(stats, seed, iteration, "raw-bytes-utf8", src, None),
        Err(error) => {
            let valid_up_to = error.valid_up_to();
            let error_len = error.error_len();
            // The `&str` API boundary must reject the same sequence identically.
            match std::str::from_utf8(&bytes) {
                Ok(_) => {
                    let ctx = Ctx {
                        label: stats.label,
                        seed,
                        iteration,
                        generator: "raw-bytes-invalid",
                        src: "",
                        raw: Some(&bytes),
                        quiet: collect_mode(),
                    };
                    ctx.fail("bytes were invalid UTF-8 on first check but valid on the second");
                }
                Err(second) => {
                    if second.valid_up_to() != valid_up_to || second.error_len() != error_len {
                        let ctx = Ctx {
                            label: stats.label,
                            seed,
                            iteration,
                            generator: "raw-bytes-invalid",
                            src: "",
                            raw: Some(&bytes),
                            quiet: collect_mode(),
                        };
                        ctx.fail("UTF-8 rejection is not deterministic");
                    }
                }
            }
            stats.invalid_utf8 += 1;
            // Valid prefix (truncation exactly on a code-point boundary).
            match std::str::from_utf8(&bytes[..valid_up_to]) {
                Ok(prefix) => run_ctx(
                    stats,
                    seed,
                    iteration,
                    "raw-bytes-valid-prefix",
                    prefix,
                    Some(&bytes),
                ),
                Err(_) => {
                    let ctx = Ctx {
                        label: stats.label,
                        seed,
                        iteration,
                        generator: "raw-bytes-valid-prefix",
                        src: "",
                        raw: Some(&bytes),
                        quiet: collect_mode(),
                    };
                    ctx.fail("valid_up_to prefix is not valid UTF-8");
                }
            }
            // Lossy replacement, so the lexer still sees a `&str`.
            let lossy = String::from_utf8_lossy(&bytes).into_owned();
            run_ctx(
                stats,
                seed,
                iteration,
                "raw-bytes-lossy",
                &lossy,
                Some(&bytes),
            );
        }
    }
}

fn run_edge(stats: &mut Stats, generator: &'static str, src: &str) {
    run_ctx(
        stats,
        BASE_SEED ^ 0xED6E_6B1D_0000_0001,
        0,
        generator,
        src,
        None,
    );
}

fn edge_cases(stats: &mut Stats) {
    run_edge(stats, "edge-bom-start", "\u{feff}fn main() {}");
    run_edge(stats, "edge-bom-middle", "fn main() { let \u{feff}x = 1; }");
    run_edge(stats, "edge-nul-outside", "fn main() { \0 }");
    run_edge(
        stats,
        "edge-nul-in-string",
        "fn main() { let s = \"a\\0b\"; }",
    );
    run_edge(stats, "edge-crlf", "fn main() {\r\n    let x = 1;\r\n}\r\n");
    run_edge(stats, "edge-cr-only", "fn main() {\r}\r");
    run_edge(stats, "edge-attribute", "#[derive(Clone)]\nfn main() {}");
    run_edge(
        stats,
        "edge-closure",
        "fn main() { let f = |x: i32| x + 1; }",
    );
    run_edge(stats, "edge-unsafe", "fn main() { unsafe { } }");
    run_edge(stats, "edge-raw-pointer", "fn f(p: *const i32) {}");
    run_edge(
        stats,
        "edge-range",
        "fn main() { for i in 0..10 { let x = i; } }",
    );
    run_edge(
        stats,
        "edge-exponent",
        "fn main() { let x = 1e9; let y = 1.5e-3f64; }",
    );

    let long_ident = format!("fn main() {{ let {} = 1; }}", "a".repeat(100_000));
    run_edge(stats, "edge-long-ident", &long_ident);
    let long_string = format!("fn main() {{ let s = \"{}\"; }}", "é".repeat(50_000));
    run_edge(stats, "edge-long-string", &long_string);
    let long_comment = format!("// {}\nfn main() {{}}", "z".repeat(100_000));
    run_edge(stats, "edge-long-comment", &long_comment);
    let unterminated = format!("/* {}", "z".repeat(100_000));
    run_edge(stats, "edge-unterminated-comment", &unterminated);
    let huge_int = format!("fn main() {{ let x = {}; }}", "9".repeat(10_000));
    run_edge(stats, "edge-huge-int", &huge_int);
    let deep = format!(
        "fn main() {{ let x = {}1{}; }}",
        "(".repeat(600),
        ")".repeat(600)
    );
    run_edge(stats, "edge-deep-nesting", &deep);
    let mut many = String::from("fn main() {\n");
    for _ in 0..2_000 {
        many.push_str("let x = 1;\n");
    }
    many.push_str("}\n");
    run_edge(stats, "edge-many-statements", &many);

    // Every code-point boundary of a Unicode-heavy source is a valid `&str`
    // prefix; feed each one (frontier truncation).
    let unicode = "fn f() { let s = \"é😀λ\"; }\n// \u{200b}\n";
    for boundary in char_boundaries(unicode) {
        let prefix = &unicode[..boundary];
        run_ctx(
            stats,
            BASE_SEED ^ 0xB0_0000_0000_0000_00,
            boundary as u64,
            "edge-boundary-prefix",
            prefix,
            None,
        );
    }
}

fn fuzz(label: &'static str, base_seed: u64, iterations: u64) {
    let started = Instant::now();
    let collect = collect_mode();
    let mut stats = Stats::new(label, base_seed, iterations, collect);
    // In collect mode every caught failure would otherwise make the default
    // panic hook print a line; keep the output to the final consolidated report.
    let previous_hook = if collect {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        Some(previous)
    } else {
        None
    };
    edge_cases(&mut stats);
    for iteration in 0..iterations {
        if collect && iteration > 0 && iteration % 50_000 == 0 {
            eprintln!("[fuzz:{label}] progress {iteration}/{iterations}");
        }
        let seed = iteration_seed(base_seed, iteration);
        let mut rng = Rng::new(seed);
        match iteration % 4 {
            0 => {
                let src = gen_random_chars(&mut rng);
                run_ctx(&mut stats, seed, iteration, "random-chars", &src, None);
            }
            1 => {
                let src = gen_token_soup(&mut rng);
                run_ctx(&mut stats, seed, iteration, "token-soup", &src, None);
            }
            2 => {
                let src = gen_mutated(&mut rng);
                run_ctx(&mut stats, seed, iteration, "mutation", &src, None);
            }
            _ => run_raw_bytes_case(&mut stats, seed, iteration, &mut rng),
        }
    }
    if let Some(previous) = previous_hook {
        std::panic::set_hook(previous);
    }
    stats.print(started);
    if stats.collect && (!stats.failures.is_empty() || stats.dropped_failures > 0) {
        print_collected_failures(&stats);
        panic!(
            "collect mode: {} fuzz failure(s) recorded ({} dropped)",
            stats.failures.len(),
            stats.dropped_failures
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn fuzz_lex_parse_default_budget() {
    let budget = env_budget(SHORT_DEFAULT_ITERS);
    fuzz("short", BASE_SEED, budget);
}

/// Long run: `RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax \
/// --test fuzz_lex_parse -- --ignored --nocapture`.
#[test]
#[ignore = "long deterministic fuzz run; set RAVUM_FUZZ_ITERS and pass --ignored"]
fn fuzz_lex_parse_long_budget() {
    let budget = env_budget(LONG_DEFAULT_ITERS);
    fuzz("long", BASE_SEED ^ 0xA5A5_A5A5_A5A5_A5A5, budget);
}
