//! LEX-11.4: token classification is deterministic and depends on no parser
//! state (the lexer runs before the parser, on the source alone). Observable
//! consequence: a fragment lexes to the same kinds whatever it is embedded in
//! (expression, type, pattern, item), and lexing twice is identical. LEX-8.9
//! (`Dot` then integer, `t.0.1`) is the one documented exception and is
//! checked to stay local.

use ravum_lang_syntax::lexer::lex;
use ravum_lang_syntax::span::FileId;
use ravum_lang_syntax::token::TokenKind;

fn kinds(src: &str) -> Vec<TokenKind> {
    let (tokens, _) = lex(0 as FileId, src, false);
    tokens.into_iter().map(|t| t.kind).collect()
}

/// The kinds of `frag` inside `prefix frag suffix`, cut out by length.
fn kinds_in(prefix: &str, frag: &str, suffix: &str) -> Vec<TokenKind> {
    let all = kinds(&format!("{prefix} {frag} {suffix}"));
    let before = kinds(prefix).iter().filter(|k| !is_eof(k)).count();
    let own = kinds(frag).iter().filter(|k| !is_eof(k)).count();
    all[before..before + own].to_vec()
}

fn is_eof(k: &TokenKind) -> bool {
    format!("{k:?}") == "Eof"
}

#[test]
fn a_fragment_lexes_the_same_in_every_syntactic_context() {
    let fragments = [
        "Vec<Vec<i32>>",
        "a < b >> c",
        "x as u8 - 1",
        "&mut self",
        "'a' '\\n' \"s\"",
        "1.0e3 0x1F 7u8",
        "match r { Ok(v) => v, Err(_) => 0 }",
        "fn f() -> i32",
        "a::b::<c>",
        "..= .. ...",
    ];
    let contexts = [
        ("fn main() { let x =", "; }"),
        ("struct S { f:", " }"),
        ("fn g(p:", ") {}"),
        ("// c\n", ""),
        ("", ""),
    ];
    for frag in fragments {
        let alone: Vec<TokenKind> = kinds(frag).into_iter().filter(|k| !is_eof(k)).collect();
        assert_eq!(kinds(frag), kinds(frag), "{frag}: lexing twice differs");
        for (prefix, suffix) in contexts {
            assert_eq!(
                kinds_in(prefix, frag, suffix),
                alone,
                "{frag:?} lexes differently after {prefix:?}"
            );
        }
    }
}

#[test]
fn the_dot_integer_exception_stays_local() {
    // LEX-8.9: `t.0.1` is two field accesses, not `t` `.` `0.1`.
    let k: Vec<String> = kinds("t.0.1").iter().map(|k| format!("{k:?}")).collect();
    assert!(!k.iter().any(|s| s.contains("Float")), "{k:?}");
    // Outside that position `0.1` stays a float literal.
    let f: Vec<String> = kinds("let x = 0.1;")
        .iter()
        .map(|k| format!("{k:?}"))
        .collect();
    assert!(f.iter().any(|s| s.contains("Float")), "{f:?}");
}
