//! E6 — type-checking paths that no test reached.
//!
//! Every test drives the real parser and checker on one program and pins the
//! errors the user reads: code and message, in emission order.

use ravum_lang_sema::facts::CheckOptions;
use ravum_lang_sema::syntax::{parser, source::SourceMap};

/// `(code, message)` of every error reported for `source`. Parse errors are
/// included: a fixture the parser refuses is pinned as such.
fn errors(source: &str) -> Vec<(String, String)> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let mut found: Vec<(String, String)> = parsed
        .diags
        .iter()
        .map(|diag| (diag.code.clone(), diag.message.clone()))
        .collect();
    if found.is_empty() {
        found = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default())
            .diags
            .iter()
            .filter(|diag| diag.code.starts_with("E-"))
            .map(|diag| (diag.code.clone(), diag.message.clone()))
            .collect();
    }
    found
}

/// The errors of `source` are exactly `expected`, in order.
fn assert_errors(source: &str, expected: &[(&str, &str)]) {
    let found = errors(source);
    let found: Vec<(&str, &str)> = found
        .iter()
        .map(|(code, message)| (code.as_str(), message.as_str()))
        .collect();
    assert_eq!(found, expected, "{source}");
}

#[test]
fn a_constant_cannot_negate_an_unsigned_value() {
    assert_errors(
        "const A: u32 = -1u32;\n\
         const B: i64 = -5i64;\n\
         const N: u8 = -1;\n\
         fn main() { }",
        &[
            ("E-TYP-MISMATCH", "cannot negate a value of type `u32`"),
            ("E-TYP-MISMATCH", "cannot negate a value of type `u8`"),
        ],
    );
}

#[test]
fn constant_logic_and_comparisons_are_folded() {
    assert_errors(
        "const C: bool = !true;\n\
         const F: bool = true || false;\n\
         const F2: bool = false && true;\n\
         const G: bool = 1 < 2;\n\
         const H: bool = 2 >= 1;\n\
         const H2: bool = 2 <= 1;\n\
         const H3: bool = 2 > 1;\n\
         const H4: bool = 2 == 1;\n\
         const H5: bool = 2 != 1;\n\
         const I: i32 = !5;\n\
         fn main() { let a: [i32; 3] = [0; 3]; }",
        &[],
    );
}

#[test]
fn a_constant_cannot_be_a_reference_or_negate_a_non_number() {
    assert_errors(
        "const E: &i32 = &1;\n\
         const D: f64 = !1.5;\n\
         const K: bool = -true;\n\
         fn main() { }",
        &[
            ("E-TYP-CONST", "references are not constant expressions"),
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "unary minus requires an integer constant",
            ),
        ],
    );
}

#[test]
fn constant_shifts_are_bounded() {
    assert_errors(
        "const I: i32 = 1 << 200;\n\
         const J: i32 = 1 << 3;\n\
         const K: i32 = 1 >> -1;\n\
         const L: i32 = 16 >> 2;\n\
         fn main() { }",
        &[
            ("E-TYP-CONST", "constant evaluation overflow"),
            ("E-TYP-CONST", "constant evaluation overflow"),
        ],
    );
}

#[test]
fn float_constants_are_typed_but_not_folded() {
    assert_errors(
        "const L: f64 = 1.5 + 2.5;\n\
         const M: f64 = -2.5;\n\
         const P: bool = 1.5 < 2.5;\n\
         fn main() { }",
        &[
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "unary minus requires an integer constant",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
            (
                "E-RES-CONST-NOT-CONST",
                "floating-point constants are not representable in Core 1.0 facts",
            ),
        ],
    );
}

#[test]
fn constant_casts_follow_the_cast_table() {
    assert_errors(
        "const O: i32 = true as i32;\n\
         const O2: u8 = false as u8;\n\
         const P: i32 = \"a\" as i32;\n\
         const Q: i8 = -128i8;\n\
         const R: i64 = 300 as i64;\n\
         const T: f64 = 3 as f64;\n\
         const U: bool = 1 as bool;\n\
         fn main() { }",
        &[
            (
                "E-TYP-CAST",
                "cast from `&text` to `i32` is not allowed in Core 1.0",
            ),
            (
                "E-TYP-CAST",
                "cast from `i32` to `bool` is not allowed in Core 1.0",
            ),
        ],
    );
}

#[test]
fn the_most_negative_literals_are_constants() {
    assert_errors(
        "const A: i128 = -170141183460469231731687303715884105728;\n\
         const B: i8 = -128;\n\
         const C: i8 = -129;\n\
         fn main() { }",
        &[(
            "E-TYP-LITERAL-RANGE",
            "integer literal `-129` does not fit in `i8`",
        )],
    );
}

#[test]
fn numeric_casts_follow_the_cast_table() {
    assert_errors(
        "fn main() { let a = 1.5f32 as f64; let b = 1.5f64 as f32; let c = 1.5 as i32; let d = true as u8; let e = 3 as f32; let f = 2.5f32 as u8; let g = 'a' as u32; let h = \"a\" as i32; let i = 1 as bool; }",
        &[
            ("E-TYP-CAST", "cast from `&text` to `i32` is not allowed in Core 1.0"),
            ("E-TYP-CAST", "cast from `i32` to `bool` is not allowed in Core 1.0"),
        ],
    );
}

#[test]
fn an_unknown_builtin_function_or_constructor_is_reported() {
    assert_errors(
        "fn main() { std::io::nothing(\"a\"); let o = Option::Nope(1); let v = Vec::nothing(); let m = std::math::nope(1.0); }",
        &[
            ("E-RES-UNKNOWN", "unknown name `nothing`"),
            ("E-RES-UNKNOWN", "unknown name `Nope`"),
            ("E-RES-UNKNOWN", "unknown name `nothing`"),
            ("E-RES-UNKNOWN", "unknown name `nope`"),
        ],
    );
}

#[test]
fn a_private_method_cannot_be_called_from_outside() {
    assert_errors(
        "mod a { pub struct S { n: i32 } impl S { fn hidden(&self) -> i32 { self.n } pub fn new() -> S { S { n: 1 } } pub fn shown(&self) -> i32 { self.hidden() } } }\n\
         fn main() { let s = a::S::new(); let x = s.hidden(); let y = a::S::hidden(&s); let z = s.shown(); }",
        &[
            ("E-RES-PRIVATE", "method `hidden` is private"),
            ("E-RES-UNKNOWN", "unknown name `hidden`"),
        ],
    );
}

#[test]
fn an_associated_function_binds_the_generics_of_its_impl() {
    assert_errors(
        "struct W<T> { v: T }\n\
         impl<T> W<T> { fn new(v: T) -> W<T> { W { v: v } } fn get(self) -> T { self.v } }\n\
         fn main() { let w = W::new(1); let x: i32 = w.get(); let u = W::new(true); let y: i32 = u.get(); }",
        &[
            ("E-TYP-MISMATCH", "let: expected `i32`, found `bool`"),
        ],
    );
}

#[test]
fn arguments_coerce_to_text_and_slice_parameters() {
    assert_errors(
        "fn t(s: &str) -> usize { s.len() }\n\
         fn sl(s: &[i32]) -> usize { s.len() }\n\
         fn main() { let owned = String::from(\"a\"); let a = t(\"x\"); let b = t(&owned); let c = t(owned.as_str()); let arr = [1, 2, 3]; let d = sl(&arr); let v: Vec<i32> = Vec::new(); let e = sl(&v); let f = sl(&arr[0]); }",
        &[
            ("E-TYP-MISMATCH", "argument 1 of `sl`: expected `&[i32]`, found `&Vec<i32>`"),
            ("E-TYP-MISMATCH", "argument 1 of `sl`: expected `&[i32]`, found `&i32`"),
        ],
    );
}

#[test]
fn a_receiver_reached_through_a_dereference_follows_its_reference() {
    assert_errors(
        "fn f(r: &mut Vec<i32>, s: &Vec<i32>) { (*r).push(1); (*s).push(2); r.push(3); s.push(4); }\n\
         fn main() { }",
        &[
            ("E-OWN-IMMUTABLE", "`push` takes `&mut self`; the receiver must be a mutable place (`let mut …` or a `&mut` borrow)"),
            ("E-OWN-IMMUTABLE", "`push` takes `&mut self`; the receiver must be a mutable place (`let mut …` or a `&mut` borrow)"),
        ],
    );
}

#[test]
fn an_uninferable_container_or_option_is_reported() {
    assert_errors(
        "fn main() { let v = Vec::new(); let x = None; let o = Ok(1); let e = Err(\"e\"); }",
        &[
            (
                "E-TYP-AMBIGUOUS",
                "cannot infer this type; add an explicit type annotation",
            ),
            (
                "E-TYP-AMBIGUOUS",
                "cannot infer this type; add an explicit type annotation",
            ),
            (
                "E-TYP-AMBIGUOUS",
                "cannot infer this type; add an explicit type annotation",
            ),
            (
                "E-TYP-AMBIGUOUS",
                "cannot infer this type; add an explicit type annotation",
            ),
        ],
    );
}

#[test]
fn text_methods_are_available_on_borrowed_text() {
    assert_errors(
        "fn main() { let s: &str = \"abc\"; let n = s.len(); let e = s.is_empty(); let o = String::from(\"x\"); let m = o.as_str().len(); let b = s.nothing(); }",
        &[
            ("E-RES-UNKNOWN", "no builtin method `nothing` on type `text`"),
        ],
    );
}

#[test]
fn a_diverging_expression_can_be_cast() {
    assert_errors(
        "fn f() -> i32 { let x = (return 1) as i32; x }\n\
         fn main() { let r = f(); }",
        &[],
    );
}

#[test]
fn option_constructors_take_exactly_one_argument() {
    assert_errors(
        "fn main() { let a: Option<i32> = Some(); let b: Option<i32> = Some(1, 2); let d: Option<i32> = None(1); }",
        &[
            ("E-TYP-ARITY", "`Option::Some` expects exactly 1 argument"),
            ("E-TYP-ARITY", "`Option::Some` expects exactly 1 argument"),
            ("E-TYP-ARITY", "`Option::None` is a value, not a function"),
        ],
    );
}

#[test]
fn result_constructors_use_the_expected_type() {
    assert_errors(
        "fn f() -> Result<i32, String> { Ok(1) }\n\
         fn g() -> Result<i32, String> { Err(String::from(\"e\")) }\n\
         fn main() { let a = f(); let b = g(); }",
        &[],
    );
}

#[test]
fn compound_assignment_requires_matching_numeric_or_boolean_operands() {
    assert_errors(
        "fn main() { let mut b = true; b += 1; b &= true; b |= false; b ^= true; let mut x = 1; x += true; x <<= 1; x %= 2; x &= 3; let mut f = 1.5; f += 1.0; f %= 2.0; f &= 1.0; f <<= 1; }",
        &[
            ("E-TYP-MISMATCH", "operator `+=` is not defined for `bool` and `i32`"),
            ("E-TYP-MISMATCH", "operator `+=` is not defined for `i32` and `bool`"),
            ("E-TYP-MISMATCH", "operator `&=` requires two integers of the same type or two `bool`"),
            ("E-TYP-MISMATCH", "operator `<<=` requires integer operands, found `f64` and `i32`"),
        ],
    );
}

#[test]
fn text_supports_no_compound_assignment() {
    assert_errors(
        "fn main() { let mut s = String::from(\"a\"); s += \"b\"; s -= \"c\"; }",
        &[
            (
                "E-TYP-MISMATCH",
                "operator `+=` is not defined for `String` and `&text`",
            ),
            (
                "E-TYP-MISMATCH",
                "operator `-=` is not defined for `String` and `&text`",
            ),
        ],
    );
}

#[test]
fn a_method_is_not_a_field() {
    assert_errors(
        "struct P { n: i32 }\n\
         impl P { fn name(&self) -> i32 { self.n } }\n\
         fn main() { let p = P { n: 1 }; let a = p.name; let b = p.nothing; let c = p.n; }",
        &[
            ("E-TYP-FIELD", "no field `name` on type `P`"),
            ("E-TYP-FIELD", "no field `nothing` on type `P`"),
        ],
    );
}

/// TYP: the occurs check refuses an inference variable bound to a type that contains it.
#[test]
fn a_type_cannot_contain_itself() {
    assert_errors(
        "fn main() { let mut a = Vec::new(); a.push(a); }",
        &[
            (
                "E-TYP-AMBIGUOUS",
                "cannot infer this type; add an explicit type annotation",
            ),
            (
                "E-TYP-MISMATCH",
                "argument 1 of `push`: expected `$0`, found `Vec<$0>`",
            ),
        ],
    );
}

#[test]
fn a_type_cannot_contain_itself_through_a_tuple_or_an_option() {
    assert_errors(
        "fn main() { let mut b = Vec::new(); b.push((b, 1)); let mut c = Vec::new(); c.push(Some(c)); let mut d = Vec::new(); d.push([d]); }",
        &[
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-MISMATCH", "argument 1 of `push`: expected `$0`, found `(Vec<$0>, i32)`"),
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-MISMATCH", "`Option::Some` produces `Option<Vec<$1>>`, expected `$1`"),
            ("E-TYP-MISMATCH", "argument 1 of `push`: expected `$1`, found `Option<Vec<$1>>`"),
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-MISMATCH", "argument 1 of `push`: expected `$2`, found `[Vec<$2>; 1]`"),
        ],
    );
}

#[test]
fn a_generic_function_over_a_slice_is_instantiated() {
    assert_errors(
        "fn count<T>(s: &[T]) -> usize { s.len() }\n\
         fn main() { let a = [1, 2, 3]; let n = count(&a); let b = [true]; let m = count(&b); }",
        &[
            (
                "E-TYP-MISMATCH",
                "argument 1 of `count`: expected `&[T]`, found `&[i32; 3]`",
            ),
            (
                "E-TYP-MISMATCH",
                "argument 1 of `count`: expected `&[T]`, found `&[bool; 1]`",
            ),
        ],
    );
}

#[test]
fn try_and_return_are_refused_in_a_constant() {
    assert_errors(
        "fn f() -> Option<i32> { Some(1) }\n\
         const A: i32 = f()?;\n\
         const B: i32 = return 1;\n\
         fn main() { }",
        &[
            (
                "E-TYP-CONST",
                "this expression is not allowed in a constant",
            ),
            (
                "E-TYP-CONST",
                "this expression is not allowed in a constant",
            ),
        ],
    );
}

#[test]
fn an_array_length_is_a_constant_expression() {
    assert_errors(
        "const N: usize = 3;\n\
         fn f() -> usize { 2 }\n\
         fn main() { let a = [0; N]; let b = [0; 1 + 2]; let c = [0; f()]; let d: [i32; 3] = a; let e: [i32; 3] = b; let g = [0; -1]; let h = [0; true]; }",
        &[
            ("E-TYP-ARRAY-REPEAT", "array repetition length must be a constant integer"),
            ("E-TYP-ARRAY-REPEAT", "array repetition length must be a constant integer"),
            ("E-TYP-MISMATCH", "let: expected `[i32; 3]`, found `[i32; 1]`"),
            ("E-TYP-ARRAY-REPEAT", "array repetition length must be a constant integer"),
            ("E-TYP-ARRAY-REPEAT", "array repetition length must be a constant integer"),
        ],
    );
}

#[test]
fn a_struct_literal_needs_a_struct() {
    assert_errors(
        "enum E { A, B }\n\
         struct W<T> { v: T }\n\
         fn main() { let p = Nope { a: 1 }; let e = E { a: 1 }; let w: W<i32> = W { v: 1 }; let x: W<bool> = W { v: 1 }; let y = W { v: 1, z: 2 }; let z = W { }; }",
        &[
            ("E-RES-UNKNOWN", "unknown name `Nope`"),
            ("E-RES-UNKNOWN", "`E` is not a struct"),
            ("E-TYP-MISMATCH", "field `v`: expected `bool`, found `i32`"),
            ("E-TYP-FIELD", "no field `z` in this aggregate"),
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-MISSING-FIELD", "missing field(s): v"),
        ],
    );
}

#[test]
fn a_diverging_field_value_types_a_generic_struct() {
    assert_errors(
        "struct W<T> { v: T }\n\
         fn f() -> W<i32> { W { v: return W { v: 1 } } }\n\
         fn main() { let w = f(); }",
        &[],
    );
}

#[test]
fn builtin_variant_patterns_take_their_exact_payload() {
    assert_errors(
        "fn f(o: Option<i32>, r: Result<i32, bool>) { match o { Some(a, b) => { } None(x) => { } Some => { } _ => { } } match r { Ok() => { } Err(a, b) => { } Ok => { } _ => { } } }\n\
         fn main() { }",
        &[
            ("E-TYP-ARITY", "`Some` pattern expects exactly 1 field"),
            ("E-TYP-ARITY", "`None` pattern takes no field, 1 given"),
            ("E-RES-UNKNOWN", "pattern does not name a variant or a constant"),
            ("E-TYP-ARITY", "`Result::Ok` pattern expects exactly 1 field"),
            ("E-TYP-ARITY", "`Result::Err` pattern expects exactly 1 field"),
            ("E-RES-UNKNOWN", "pattern does not name a variant or a constant"),
        ],
    );
}

#[test]
fn a_covering_match_is_accepted() {
    assert_errors(
        "enum E { A, B(Option<i32>), C }\n\
         fn f(e: E, o: Option<Option<i32>>, r: Result<Option<i32>, bool>, t: (bool, Option<i32>)) -> i32 { let a = match e { E::A => 1, E::B(Some(_)) => 2, E::B(None) => 3, E::C => 4 }; let b = match o { Some(Some(_)) => 1, Some(None) => 2, None => 3 }; let c = match r { Ok(Some(1)) => 1, Ok(_) => 2, Err(true) => 3, Err(false) => 4 }; let d = match t { (true, Some(_)) => 1, (true, None) => 2, (false, _) => 3 }; a + b + c + d }\n\
         fn main() { }",
        &[],
    );
}

#[test]
fn builtin_unit_variants_are_values() {
    assert_errors(
        "fn main() { let x: Option<i32> = None; let y: Option<i32> = Option::None; let z = Option::None; let w: i32 = None; }",
        &[
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-AMBIGUOUS", "cannot infer this type; add an explicit type annotation"),
            ("E-TYP-MISMATCH", "let: expected `i32`, found `Option<$1>`"),
        ],
    );
}

/// A bare path matches a unit variant only: `check` accepted `E::B` for a
/// variant with a payload and rustc refused the generated Rust (E0532, E0533).
#[test]
fn user_variant_patterns_take_their_exact_payload() {
    assert_errors(
        "enum E { A, B(i32), C { x: i32 } }\n\
         fn f(e: E) { match e { E::A(1) => { } E::B => { } E::B(a, b) => { } E::C => { } E::C(z) => { } E::C { y } => { } E::D => { } _ => { } } }\n\
         fn main() { }",
        &[
            ("E-TYP-ARITY", "variant `E::A` expects 0 field(s), found 1"),
            ("E-TYP-ARITY", "variant `E::B` expects 1 field(s), found 0"),
            ("E-TYP-ARITY", "variant `E::B` expects 1 field(s), found 2"),
            ("E-TYP-ARITY", "variant `E::C` expects 1 field(s), found 0"),
            ("E-TYP-ARITY", "variant `E::C` expects 0 field(s), found 1"),
            ("E-TYP-MISSING-FIELD", "missing field(s): x"),
            ("E-TYP-FIELD", "no field `y` in this variant"),
            ("E-RES-UNKNOWN", "unknown name `D`"),
        ],
    );
}

#[test]
fn unit_and_payload_variants_match_with_their_own_pattern_form() {
    assert_errors(
        "enum E { A, B(i32), C { x: i32 } }\n\
         fn f(e: E) -> i32 { match e { E::A => 0, E::B(n) => n, E::C { x } => x } }\n\
         fn main() { let r = f(E::A); }",
        &[],
    );
}

#[test]
fn assignment_requires_a_mutable_place() {
    assert_errors(
        "struct P { n: i32 }\n\
         fn f(p: &P, q: &mut P, c: i32) { p.n = 1; (*p).n = 2; q.n = 3; (*q).n = 4; c = 2; }\n\
         fn main() { let t = (1, 2); t.0 = 3; let mut u = (1, 2); u.0 = 3; let a = [1, 2]; a[0] = 3; let x = 1; *x = 2; }",
        &[
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-TYP-MISMATCH", "cannot dereference a value of type `i32`"),
        ],
    );
}

#[test]
fn a_loop_variable_is_immutable() {
    assert_errors(
        "fn main() { for i in 0..3 { i = 5; } }",
        &[("E-OWN-IMMUTABLE", "cannot assign to an immutable place")],
    );
}

/// `let e: T;` without `mut` takes its value from one assignment. `check`
/// accepted a second one and rustc refused the generated Rust (E0384).
#[test]
fn a_binding_declared_without_a_value_is_assigned_once() {
    assert_errors(
        "fn main() { let e: i32; e = 2; e = 3; let r = e; }",
        &[(
            "E-OWN-IMMUTABLE",
            "seconde affectation de la liaison immuable `e`",
        )],
    );
}

#[test]
fn a_binding_declared_without_a_value_cannot_be_assigned_in_a_loop() {
    assert_errors(
        "fn main() { let e: i32; let mut i = 0; while i < 2 { e = i; i = i + 1; } }",
        &[(
            "E-OWN-IMMUTABLE",
            "seconde affectation de la liaison immuable `e`",
        )],
    );
}

#[test]
fn a_binding_declared_without_a_value_is_assigned_once_per_path() {
    assert_errors(
        "fn f(c: bool) -> i32 { let e: i32; if c { e = 1; } else { e = 2; } e }\n\
         fn g(c: bool) -> i32 { let e: i32; if c { e = 1; } e = 2; e }\n\
         fn main() { let a = f(true); let b = g(true); }",
        &[(
            "E-OWN-IMMUTABLE",
            "seconde affectation de la liaison immuable `e`",
        )],
    );
}

#[test]
fn a_mutable_binding_declared_without_a_value_is_assigned_freely() {
    assert_errors(
        "fn main() { let mut e: i32; e = 2; e = 3; e += 1; let r = e; }",
        &[],
    );
}

/// `check` treated such a binding as mutable everywhere; rustc refuses the
/// field write, the element write, the compound assignment (E0594, E0384).
#[test]
fn nothing_else_is_mutable_about_a_binding_declared_without_a_value() {
    assert_errors(
        "struct P { n: i32 }\n\
         fn main() { let p: P; p = P { n: 1 }; p.n = 2; let a: [i32; 2]; a = [1, 2]; a[0] = 3; let x: i32; x = 1; x += 1; }",
        &[
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
        ],
    );
}

/// `check` did not look at the place under `&mut`; rustc refuses the
/// generated Rust (E0596).
#[test]
fn an_immutable_place_cannot_be_borrowed_exclusively() {
    assert_errors(
        "fn g(a: &mut i32) { }\n\
         struct P { n: i32 }\n\
         fn h(p: &P, c: i32) { g(&mut p.n); g(&mut c); }\n\
         fn main() { let x = 1; g(&mut x); let t = (1, 2); g(&mut t.0); let d: i32; d = 1; g(&mut d); }",
        &[
            ("E-OWN-IMMUTABLE", "cannot borrow an immutable place as `&mut`"),
            ("E-OWN-IMMUTABLE", "cannot borrow an immutable place as `&mut`"),
            ("E-OWN-IMMUTABLE", "cannot borrow an immutable place as `&mut`"),
            ("E-OWN-IMMUTABLE", "cannot borrow an immutable place as `&mut`"),
            ("E-OWN-IMMUTABLE", "cannot borrow an immutable place as `&mut`"),
        ],
    );
}

#[test]
fn a_mutable_place_or_a_temporary_can_be_borrowed_exclusively() {
    assert_errors(
        "fn g(a: &mut i32) { }\n\
         fn one() -> i32 { 1 }\n\
         struct P { n: i32 }\n\
         fn h(q: &mut P) { g(&mut q.n); g(&mut (*q).n); }\n\
         fn main() { let mut x = 1; g(&mut x); let mut t = (1, 2); g(&mut t.0); g(&mut one()); g(&mut 3); let mut p = P { n: 1 }; h(&mut p); g(&mut p.n); }",
        &[],
    );
}

#[test]
fn a_match_must_cover_every_enum_constructor() {
    assert_errors(
        "enum E { A, B(Option<i32>), C }\n\
         fn f(e: E) -> i32 { match e { E::A => 1, E::B(Some(_)) => 2 } }\n\
         fn main() { }",
        &[(
            "E-TYP-NONEXHAUSTIVE",
            "non-exhaustive match: the arms do not cover every value of `E`",
        )],
    );
}

#[test]
fn a_match_must_cover_nested_options_and_results() {
    assert_errors(
        "fn f(o: Option<Option<i32>>) -> i32 { match o { Some(Some(_)) => 1, None => 2 } }\n\
         fn g(r: Result<Option<i32>, bool>) -> i32 { match r { Ok(Some(1)) => 1, Ok(_) => 2 } }\n\
         fn main() { }",
        &[
            ("E-TYP-NONEXHAUSTIVE", "non-exhaustive match: the arms do not cover every value of `Option<Option<i32>>`"),
            ("E-TYP-NONEXHAUSTIVE", "non-exhaustive match: the arms do not cover every value of `Result<Option<i32>, bool>`"),
        ],
    );
}

#[test]
fn a_match_must_cover_tuples_and_unit() {
    assert_errors(
        "fn f(t: (bool, Option<i32>)) -> i32 { match t { (true, Some(_)) => 1, (false, _) => 2 } }\n\
         fn g(u: ()) -> i32 { match u { () => 1 } }\n\
         fn main() { }",
        &[
            ("E-TYP-NONEXHAUSTIVE", "non-exhaustive match: the arms do not cover every value of `(bool, Option<i32>)`"),
            ("E-TYP-MISMATCH", "tuple pattern cannot match type `()`"),
        ],
    );
}

#[test]
fn unwrap_or_on_static_text_is_static_only_with_a_static_default() {
    assert_errors(
        "fn f(o: Option<&str>) -> &str { o.unwrap_or(\"x\") }\n\
         fn g(o: Option<&str>, d: &String) -> &str { o.unwrap_or(d.as_str()) }\n\
         fn main() { let a = f(None); }",
        &[(
            "E-OWN-REF-ESCAPE",
            "une référence ne peut pas être retournée en Core 1.0",
        )],
    );
}

#[test]
fn a_user_method_returning_static_text_propagates() {
    assert_errors(
        "struct P { n: i32 }\n\
         impl P { fn name(&self) -> &str { \"p\" } }\n\
         fn f(p: &P) -> &str { p.name() }\n\
         fn main() { let p = P { n: 1 }; let s = f(&p); }",
        &[],
    );
}

#[test]
fn a_tail_return_of_static_text_is_static() {
    assert_errors(
        "fn f(c: bool) -> &str { if c { return \"a\"; } return \"b\" }\n\
         fn main() { let s = f(true); }",
        &[],
    );
}

#[test]
fn a_let_initialised_with_non_static_text_cannot_be_returned() {
    assert_errors(
        "fn f(s: &String) -> &str { let t = s.as_str(); t }\n\
         fn main() { }",
        &[
            (
                "E-OWN-REF-ESCAPE",
                "une référence ne peut pas être stockée en Core 1.0",
            ),
            (
                "E-OWN-REF-ESCAPE",
                "une référence ne peut pas être retournée en Core 1.0",
            ),
        ],
    );
}

/// Conservative refusal: the static-text analysis does not follow a tuple
/// scrutinee into its pattern bindings (recorded as an open question).
#[test]
fn text_bound_by_a_tuple_pattern_is_not_returned_as_static() {
    assert_errors(
        "fn f(c: bool) -> &str { let t = (\"a\", \"b\"); match t { (x, y) => if c { x } else { y } } }\n\
         fn main() { let s = f(true); }",
        &[
            ("E-OWN-REF-ESCAPE", "une référence ne peut pas être retournée en Core 1.0"),
        ],
    );
}

#[test]
fn text_bound_by_a_struct_variant_pattern_is_not_returned_as_static() {
    assert_errors(
        "enum E { A { s: &str } }\n\
         fn f() -> &str { let e = E::A { s: \"x\" }; match e { E::A { s } => s } }\n\
         fn main() { let s = f(); }",
        &[(
            "E-OWN-REF-ESCAPE",
            "une référence ne peut pas être retournée en Core 1.0",
        )],
    );
}

#[test]
fn a_deferred_or_diverging_let_in_a_static_text_function() {
    assert_errors(
        "fn f(c: bool) -> &str { let x: i32; x = 1; let y = if c { return \"a\"; } else { 2 }; \"b\" }\n\
         fn main() { let s = f(true); }",
        &[],
    );
}

/// check => build: a diverging callee used to be accepted, and the backend
/// then failed with "call expression has an unresolved target".
#[test]
fn a_diverging_callee_is_not_callable() {
    assert_errors(
        "fn main() { (return)(); }",
        &[("E-TYP-CALL", "value of type `!` is not callable")],
    );
}

/// A unit variant called with an empty argument list is the variant itself.
#[test]
fn a_unit_variant_called_without_arguments_is_accepted() {
    assert_errors(
        "enum E { A, B(i32) } fn main() { let e = E::A(); drop(e); }",
        &[],
    );
}

/// OWN-7.4: `self` taken by value without `mut` is an immutable place.
#[test]
fn a_by_value_self_cannot_be_borrowed_mutably() {
    assert_errors(
        "struct P { }\n\
         impl P { fn f(self) { let r = &mut self; } }\n\
         fn main() { }",
        &[(
            "E-OWN-IMMUTABLE",
            "cannot borrow an immutable place as `&mut`",
        )],
    );
}

/// A constant, local or reached through a module, is never a mutable place.
#[test]
fn a_constant_is_not_an_assignable_place() {
    assert_errors(
        "const C: i32 = 1;\n\
         mod m { pub const D: i32 = 1; }\n\
         fn main() { C = 2; m::D = 2; }",
        &[
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
            ("E-OWN-IMMUTABLE", "cannot assign to an immutable place"),
        ],
    );
}

#[test]
fn assigning_through_an_unknown_dereference_reports_only_the_unknown_name() {
    assert_errors(
        "fn main() { *nothing = 1; }",
        &[("E-RES-UNKNOWN", "unknown name `nothing`")],
    );
}

/// The hint "is a method, call it" exists for builtin methods only; a user
/// method used without a call is reported as a missing field.
#[test]
fn a_method_of_a_user_type_used_without_a_call_is_not_a_field() {
    assert_errors(
        "struct P { }\n\
         impl P { fn m(&self) { } }\n\
         fn main() { let p = P { }; let x = p.m; }",
        &[("E-TYP-FIELD", "no field `m` on type `P`")],
    );
}

/// D13: a function in a module returning only literals returns static text.
#[test]
fn a_static_text_function_inside_a_module_is_recognised() {
    assert_errors(
        "mod m { pub fn f() -> &str { \"x\" } }\n\
         fn main() { io::println(m::f()); }",
        &[],
    );
}

/// Text reached through `self` is a borrow, never static.
#[test]
fn text_returned_through_self_is_not_static() {
    assert_errors(
        "struct S { n: String }\n\
         impl S { fn f(&self) -> &str { \"x\" } fn g(&self) -> &str { self.f() } fn h(&self) -> &str { self.n.as_str() } }\n\
         fn main() { }",
        &[
            ("E-OWN-REF-ESCAPE", "une référence ne peut pas être retournée en Core 1.0"),
        ],
    );
}

/// `typeck::typecheck` is the public re-entry point: it rebuilds the world and
/// types a program whose resolution facts are already filled.
#[test]
fn the_public_typecheck_entry_types_a_resolved_program() {
    let source = "fn f(a: i32) -> i32 { a + 1 }\nfn main() { let x = f(1); drop(x); }";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture checks");
    let mut diags = ravum_lang_sema::syntax::diag::DiagnosticBag::new();
    assert!(ravum_lang_sema::typeck::typecheck(&mut checked, &mut diags));
    assert!(!diags.has_errors());
}

#[test]
fn a_builtin_module_called_with_arguments_is_not_callable() {
    assert_errors(
        "fn main() { std::io(1); }",
        &[("E-TYP-CALL", "a module is not callable")],
    );
}

#[test]
fn a_private_associated_function_is_refused_from_outside_its_module() {
    assert_errors(
        "pub mod m { pub struct S { } impl S { fn new() -> S { S { } } } }\n\
         fn main() { let s = m::S::new(); drop(s); }",
        &[("E-RES-PRIVATE", "function `new` is private")],
    );
}

#[test]
fn an_associated_function_of_another_type_does_not_take_the_expected_type() {
    assert_errors(
        "struct A { }\n\
         struct B { }\n\
         impl A { fn new() -> A { A { } } }\n\
         fn main() { let b: B = A::new(); drop(b); }",
        &[("E-TYP-MISMATCH", "let: expected `B`, found `A`")],
    );
}

#[test]
fn an_open_range_in_for_is_refused() {
    assert_errors(
        "fn main() { for i in ..3 { drop(i); } for j in ..=3 { drop(j); } }",
        &[
            (
                "E-TYP-RANGE",
                "`for` requires an explicit `start..end` range",
            ),
            (
                "E-TYP-RANGE",
                "`for` requires an explicit `start..end` range",
            ),
            (
                "E-TYP-RANGE",
                "inclusive ranges (`..=`) are not supported in Core 1.0",
            ),
        ],
    );
}

#[test]
fn a_generic_unit_variant_without_context_is_ambiguous() {
    assert_errors(
        "enum O<T> { N, S(T) }\n\
         fn main() { let o = O::N(); drop(o); }",
        &[(
            "E-TYP-AMBIGUOUS",
            "cannot infer this type; add an explicit type annotation",
        )],
    );
}

#[test]
fn ok_without_its_argument_is_an_arity_error() {
    assert_errors(
        "fn main() { let r: Result<i32, i32> = Ok(); drop(r); }",
        &[
            ("E-TYP-ARITY", "`Result::Ok` expects exactly 1 argument"),
            (
                "E-TYP-MISMATCH",
                "let: expected `Result<i32, i32>`, found `Result<<error>, i32>`",
            ),
        ],
    );
}
