//! Truth tables of the pure type-checking helpers: integer ranges, the cast
//! table, generic-parameter detection, constant casts and the recursion bound
//! of inference-variable resolution.

use super::helpers::*;
use super::VAR_BASE;
use crate::facts::ConstValue;
use crate::types::Type;

fn boxed(ty: Type) -> Box<Type> {
    Box::new(ty)
}

fn param() -> Type {
    Type::Param {
        name: "T".to_string(),
        index: 0,
    }
}

#[test]
fn integer_range_checks_refuse_non_integer_types() {
    assert!(!int_value_fits(1, &Type::Bool));
    assert!(!negative_value_fits(1, &Type::F64));
    assert!(!int_i128_fits(1, &Type::Char));
    assert!(int_value_fits(255, &Type::U8));
    assert!(!int_value_fits(256, &Type::U8));
    assert!(negative_value_fits(128, &Type::I8));
    assert!(!negative_value_fits(129, &Type::I8));
    assert!(!negative_value_fits(1, &Type::U8));
    assert!(int_i128_fits(-128, &Type::I8));
    assert!(!int_i128_fits(-129, &Type::I8));
    assert!(!int_i128_fits(-1, &Type::U32));
}

/// Two's complement: a literal above the signed maximum is the magnitude of a
/// negative value, and `1 << 127` is exactly `i128::MIN`.
#[test]
fn signed_literals_above_the_maximum_are_negative_magnitudes() {
    assert_eq!(int_literal_to_i128(1u128 << 127, &Type::I128), i128::MIN);
    assert_eq!(int_literal_to_i128(128, &Type::I8), -128);
    assert_eq!(int_literal_to_i128(127, &Type::I8), 127);
    assert_eq!(int_literal_to_i128(200, &Type::U8), 200);
}

#[test]
fn the_cast_table_admits_never_and_float_width_changes() {
    assert!(cast_allowed(&Type::Never, &Type::String));
    assert!(cast_allowed(&Type::F32, &Type::F64));
    assert!(cast_allowed(&Type::F64, &Type::F32));
    assert!(cast_allowed(&Type::F64, &Type::I32));
    assert!(cast_allowed(&Type::Bool, &Type::U8));
    assert!(!cast_allowed(&Type::I32, &Type::Bool));
    assert!(!cast_allowed(&Type::String, &Type::I32));
}

#[test]
fn borrowed_text_has_the_text_method_set() {
    let borrowed = Type::Ref {
        mutable: false,
        inner: boxed(Type::Str),
    };
    assert!(method_self_kind(&borrowed).is_some());
    assert_eq!(method_self_kind(&borrowed), method_self_kind(&Type::Str));
    assert_eq!(nominal_def(&Type::I32), None);
}

#[test]
fn a_generic_parameter_is_found_inside_every_type_constructor() {
    let containers = [
        Type::Ref {
            mutable: true,
            inner: boxed(param()),
        },
        Type::Array {
            elem: boxed(param()),
            len: 2,
        },
        Type::Slice(boxed(param())),
        Type::Tuple(vec![Type::I32, param()]),
        Type::Vec(boxed(param())),
        Type::Option(boxed(param())),
        Type::Result(boxed(Type::I32), boxed(param())),
        Type::Result(boxed(param()), boxed(Type::I32)),
        Type::Nominal {
            def: 3,
            name: "W".to_string(),
            args: vec![param()],
        },
    ];
    for ty in containers {
        assert!(has_unbound_param(&ty), "{ty:?}");
    }
    let closed = [
        Type::Ref {
            mutable: false,
            inner: boxed(Type::I32),
        },
        Type::Tuple(vec![Type::I32]),
        Type::Result(boxed(Type::I32), boxed(Type::Bool)),
        Type::Nominal {
            def: 3,
            name: "W".to_string(),
            args: vec![Type::I32],
        },
    ];
    for ty in closed {
        assert!(!has_unbound_param(&ty), "{ty:?}");
    }
}

/// A cycle among inference variables (which unification refuses) cannot make
/// resolution recurse without bound: past 64 levels the result is `Error`.
#[test]
fn resolution_of_a_cyclic_variable_chain_is_bounded() {
    let var = |index: usize| Type::Param {
        name: format!("${index}"),
        index: VAR_BASE + index,
    };
    let vars = vec![Some(var(1)), Some(var(0))];
    assert_eq!(resolve_vars_keep(&vars, &var(0)), Type::Error);
}

#[test]
fn text_constants_do_not_cast() {
    assert_eq!(
        cast_const_value(ConstValue::Str("a".to_string()), &Type::Str, &Type::I32),
        None
    );
    assert_eq!(
        cast_const_value(ConstValue::Bool(true), &Type::Bool, &Type::I32),
        Some(ConstValue::Int(1))
    );
    assert_eq!(
        cast_const_value(ConstValue::Bool(false), &Type::Bool, &Type::U8),
        Some(ConstValue::Int(0))
    );
    assert_eq!(
        cast_const_value(ConstValue::Bool(true), &Type::Bool, &Type::F64),
        None
    );
}
