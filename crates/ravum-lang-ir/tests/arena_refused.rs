//! STD-21.12 (RFC-UI-LANG-2 P1): path C refuses `Arena` / `Handle` without
//! approximation until P2 brings them to the IR.

#[test]
fn arena_programs_are_refused_by_the_ir() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../conformance/cases/run/arena-handles/src/main.rv"
    ))
    .unwrap();
    let e = ravum_lang_ir::compile_source(&src).unwrap_err();
    let text = format!("{e:?}");
    assert!(text.contains("nsupported"), "{text}");
    let only_handle = "fn id(h: Handle<i64>) -> Handle<i64> { h }\nfn main() -> () {}\n";
    let e = ravum_lang_ir::compile_source(only_handle).unwrap_err();
    assert!(format!("{e:?}").contains("nsupported"), "{e:?}");
}
