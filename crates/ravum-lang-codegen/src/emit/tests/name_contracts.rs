use super::*;

#[test]
fn mangling_handles_identifiers_that_rust_forbids_even_in_raw_form() {
    for (source, expected) in [
        ("", "rv_unnamed"),
        ("_", "rv_kw__"),
        ("self", "rv_kw_self"),
        ("Self", "rv_kw_Self"),
        ("super", "rv_kw_super"),
        ("crate", "rv_kw_crate"),
        ("type", "r#type"),
        ("value_2", "value_2"),
    ] {
        assert_eq!(mangle_ident(source), expected);
    }
    assert_eq!(sanitize_segment("2module-9"), "_module_9");
    assert_eq!(sanitize_segment("_nested2"), "_nested2");
}

#[test]
fn literals_escape_controls_without_escaping_adjacent_printable_characters() {
    for (character, escaped) in [
        ('\0', r"\0"),
        ('\u{01}', r"\u{1}"),
        ('\u{1f}', r"\u{1f}"),
        (' ', " "),
        ('~', "~"),
        ('\u{7f}', r"\u{7f}"),
        ('é', "é"),
    ] {
        assert_eq!(escape_char(character), escaped);
        assert_eq!(escape_string(&character.to_string()), escaped);
    }
}

#[test]
fn fallback_recognizes_every_core_builtin_type_and_rejects_user_types() {
    for name in [
        "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "isize", "usize",
        "f32", "f64", "bool", "char", "String", "str", "Vec", "Option", "Result",
    ] {
        assert_eq!(builtin_type_name(name), Some(name));
    }
    assert_eq!(builtin_type_name("UserType"), None);
}

#[test]
fn integer_suffixes_include_wide_types_and_exclude_nonintegers() {
    for (ty, name) in [
        (STy::I8, "i8"),
        (STy::I16, "i16"),
        (STy::I32, "i32"),
        (STy::I64, "i64"),
        (STy::I128, "i128"),
        (STy::U8, "u8"),
        (STy::U16, "u16"),
        (STy::U32, "u32"),
        (STy::U64, "u64"),
        (STy::U128, "u128"),
        (STy::Isize, "isize"),
        (STy::Usize, "usize"),
    ] {
        assert_eq!(int_type_name(&ty), Some(name));
    }
    assert_eq!(int_type_name(&STy::Bool), None);
    assert_eq!(int_type_name(&STy::F64), None);
}
