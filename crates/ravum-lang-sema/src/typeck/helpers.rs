use super::*;

// ---------------------------------------------------------------------------
// Aides libres
// ---------------------------------------------------------------------------

pub(super) fn expr_span_match(scrutinee: &Expr, arms: &[MatchArm]) -> Span {
    match arms.first() {
        Some(arm) => arm.span.join(scrutinee.span),
        None => scrutinee.span,
    }
}

pub(super) fn variant_pat_patterns(fields: &VariantPatFields) -> Vec<&Pat> {
    match fields {
        VariantPatFields::Tuple(pats) => pats.iter().collect(),
        VariantPatFields::Struct(fields) => fields.iter().map(|f| &f.pat).collect(),
    }
}

pub(super) fn int_type(ty: IntTy) -> Type {
    match ty {
        IntTy::I8 => Type::I8,
        IntTy::I16 => Type::I16,
        IntTy::I32 => Type::I32,
        IntTy::I64 => Type::I64,
        IntTy::I128 => Type::I128,
        IntTy::U8 => Type::U8,
        IntTy::U16 => Type::U16,
        IntTy::U32 => Type::U32,
        IntTy::U64 => Type::U64,
        IntTy::U128 => Type::U128,
        IntTy::Isize => Type::Isize,
        IntTy::Usize => Type::Usize,
    }
}

/// Bornes d'un entier : (magnitude du minimum, maximum, signé).
pub(super) fn int_bounds(ty: &Type) -> (u128, u128, bool) {
    let bits = ty.int_bits().unwrap_or(32);
    if ty.is_signed_int() {
        let min_magnitude = 1u128 << (bits - 1);
        let max = min_magnitude - 1;
        (min_magnitude, max, true)
    } else {
        let max = if bits == 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        (0, max, false)
    }
}

pub(super) fn int_value_fits(value: u128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (_, max, _) = int_bounds(ty);
    value <= max
}

/// G-6.37: `-value` fits `ty` when `ty` is signed and the magnitude does not
/// exceed the magnitude of its minimum (`-128` fits `i8`, `-129` does not, and
/// no negative pattern fits an unsigned type).
pub(super) fn negative_value_fits(value: u128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (min_magnitude, _, signed) = int_bounds(ty);
    signed && value <= min_magnitude
}

pub(super) fn int_i128_fits(value: i128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (min_magnitude, max, signed) = int_bounds(ty);
    if signed {
        value >= -(min_magnitude as i128) && value <= max as i128
    } else {
        value >= 0 && value as u128 <= max
    }
}

pub(super) fn int_literal_to_i128(value: u128, ty: &Type) -> i128 {
    let (_, max, signed) = int_bounds(ty);
    if signed && value > max {
        // Complément à deux : `-128i8` arrive ici avec la valeur 128.
        if value == 1u128 << 127 {
            i128::MIN
        } else {
            -(value as i128)
        }
    } else {
        value as i128
    }
}

pub(super) fn deref_type(ty: &Type) -> Type {
    let mut current = ty.clone();
    while let Type::Ref { inner, .. } = current {
        current = *inner;
    }
    current
}

pub(super) fn is_text_like(ty: &Type) -> bool {
    match ty {
        Type::String | Type::Str => true,
        Type::Ref { inner, .. } => matches!(**inner, Type::Str),
        _ => false,
    }
}

pub(super) fn cast_allowed(source: &Type, target: &Type) -> bool {
    let source_scalar =
        source.is_int() || source.is_float() || matches!(source, Type::Bool | Type::Char);
    if source == target && source_scalar {
        return true;
    }
    match (source, target) {
        (Type::Never, _) => true,
        (s, t) if s.is_int() && t.is_int() => true,
        (s, Type::F32) | (s, Type::F64) if s.is_int() || s == &Type::F32 || s == &Type::F64 => true,
        (Type::F32 | Type::F64, t) if t.is_int() => true,
        (Type::Bool, t) if t.is_int() => true,
        (Type::Char, t) if t.is_int() => true,
        _ => false,
    }
}

pub(super) fn is_untyped_num_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int { suffix: None, .. } | ExprKind::Float { suffix: None, .. } => true,
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => is_untyped_num_literal(expr),
        _ => false,
    }
}

pub(super) fn is_negative_int_literal(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } if matches!(expr.kind, ExprKind::Int { .. })
    )
}

pub(super) fn method_self_kind(ty: &Type) -> Option<MethodSelfKind> {
    Some(match ty {
        Type::String => MethodSelfKind::String,
        Type::Str => MethodSelfKind::StrRef,
        Type::Ref { inner, .. } if matches!(**inner, Type::Str) => MethodSelfKind::StrRef,
        Type::Slice(_) => MethodSelfKind::Slice,
        Type::Vec(_) => MethodSelfKind::Vec,
        Type::Option(_) => MethodSelfKind::Option,
        Type::Result(_, _) => MethodSelfKind::Result,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::Isize => {
            MethodSelfKind::SignedInt
        }
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::Usize => {
            MethodSelfKind::Int
        }
        Type::F64 => MethodSelfKind::F64,
        Type::F32 => MethodSelfKind::F32,
        Type::Bool => MethodSelfKind::Bool,
        Type::Char => MethodSelfKind::Char,
        _ => return None,
    })
}

pub(super) fn method_env_for(ty: &Type) -> Vec<(&'static str, Type)> {
    match ty {
        Type::Vec(elem) => vec![("Self", ty.clone()), ("T", (**elem).clone())],
        Type::Option(inner) => vec![("Self", ty.clone()), ("T", (**inner).clone())],
        Type::Result(ok, err) => vec![
            ("Self", ty.clone()),
            ("T", (**ok).clone()),
            ("E", (**err).clone()),
        ],
        Type::Slice(elem) => vec![("Self", ty.clone()), ("T", (**elem).clone())],
        _ => vec![("Self", ty.clone())],
    }
}

pub(super) fn nominal_def(ty: &Type) -> Option<NodeId> {
    match ty {
        Type::Nominal { def, .. } => Some(*def),
        _ => None,
    }
}

pub(super) fn nominal_subst(world: &World, def: &NodeId, args: &[Type]) -> BTreeMap<usize, Type> {
    let generics = world.type_generics(*def);
    let mut subst = BTreeMap::new();
    // A well-formed nominal has exactly one argument per parameter (E-TYP-ARGS
    // otherwise); `take` keeps the map bounded by the parameters anyway.
    for (index, arg) in args.iter().enumerate().take(generics.len()) {
        subst.insert(index, arg.clone());
    }
    subst
}

pub(super) fn substitute(ty: &Type, subst: &BTreeMap<usize, Type>) -> Type {
    match ty {
        Type::Param { index, .. } => subst.get(index).cloned().unwrap_or_else(|| ty.clone()),
        Type::Ref { mutable, inner } => Type::Ref {
            mutable: *mutable,
            inner: Box::new(substitute(inner, subst)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(substitute(elem, subst)),
            len: *len,
        },
        Type::Slice(inner) => Type::Slice(Box::new(substitute(inner, subst))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(|t| substitute(t, subst)).collect()),
        Type::Vec(inner) => Type::Vec(Box::new(substitute(inner, subst))),
        Type::Option(inner) => Type::Option(Box::new(substitute(inner, subst))),
        Type::Result(ok, err) => Type::Result(
            Box::new(substitute(ok, subst)),
            Box::new(substitute(err, subst)),
        ),
        Type::Nominal { def, name, args } => Type::Nominal {
            def: *def,
            name: name.clone(),
            args: args.iter().map(|t| substitute(t, subst)).collect(),
        },
        other => other.clone(),
    }
}

pub(super) fn has_unbound_param(ty: &Type) -> bool {
    match ty {
        Type::Param { .. } => true,
        Type::Ref { inner, .. } => has_unbound_param(inner),
        Type::Array { elem, .. } => has_unbound_param(elem),
        Type::Slice(inner) => has_unbound_param(inner),
        Type::Tuple(items) => items.iter().any(has_unbound_param),
        Type::Vec(inner) => has_unbound_param(inner),
        Type::Option(inner) => has_unbound_param(inner),
        Type::Result(ok, err) => has_unbound_param(ok) || has_unbound_param(err),
        Type::Nominal { args, .. } => args.iter().any(has_unbound_param),
        _ => false,
    }
}

/// Résout les variables d'inférence connues ; les variables libres restent
/// telles quelles (elles peuvent être liées plus tard).
pub(super) fn resolve_vars_keep(vars: &[Option<Type>], ty: &Type) -> Type {
    resolve_vars_impl(vars, ty, None, 0)
}

/// Résout les variables d'inférence pour de bon : une variable libre devient
/// `Error` et son index est enregistré.
pub(super) fn resolve_vars_final(
    vars: &[Option<Type>],
    ty: &Type,
    unresolved: &mut BTreeSet<usize>,
) -> Type {
    resolve_vars_impl(vars, ty, Some(unresolved), 0)
}

/// Bound on the recursion of [`resolve_vars_impl`]. A written type is at most
/// `parser::MAX_DEPTH` (256) levels deep; a chain of bound inference variables
/// adds one level per link. The bound only stops a runaway substitution: the
/// old value (64) turned any type nested more than 64 levels into `Error`
/// without a diagnostic, and `build` then failed on a program `check` had
/// accepted.
const MAX_RESOLVE_DEPTH: usize = 1024;

pub(super) fn resolve_vars_impl(
    vars: &[Option<Type>],
    ty: &Type,
    mut unresolved: Option<&mut BTreeSet<usize>>,
    depth: usize,
) -> Type {
    if depth > MAX_RESOLVE_DEPTH {
        return Type::Error;
    }
    if let Some(index) = infer_var_index(ty) {
        return match vars.get(index).and_then(|v| v.clone()) {
            Some(bound) => resolve_vars_impl(vars, &bound, unresolved, depth + 1),
            None => match unresolved.as_deref_mut() {
                Some(set) => {
                    set.insert(index);
                    Type::Error
                }
                None => ty.clone(),
            },
        };
    }
    match ty {
        Type::Ref { mutable, inner } => Type::Ref {
            mutable: *mutable,
            inner: Box::new(resolve_vars_impl(vars, inner, unresolved, depth + 1)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(resolve_vars_impl(vars, elem, unresolved, depth + 1)),
            len: *len,
        },
        Type::Slice(inner) => Type::Slice(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Tuple(items) => Type::Tuple(
            items
                .iter()
                .map(|t| resolve_vars_impl(vars, t, unresolved.as_deref_mut(), depth + 1))
                .collect(),
        ),
        Type::Vec(inner) => Type::Vec(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Option(inner) => Type::Option(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Result(ok, err) => Type::Result(
            Box::new(resolve_vars_impl(
                vars,
                ok,
                unresolved.as_deref_mut(),
                depth + 1,
            )),
            Box::new(resolve_vars_impl(vars, err, unresolved, depth + 1)),
        ),
        Type::Nominal { def, name, args } => Type::Nominal {
            def: *def,
            name: name.clone(),
            args: args
                .iter()
                .map(|t| resolve_vars_impl(vars, t, unresolved.as_deref_mut(), depth + 1))
                .collect(),
        },
        other => other.clone(),
    }
}

pub(super) fn cast_const_value(
    value: ConstValue,
    source: &Type,
    target: &Type,
) -> Option<ConstValue> {
    match value {
        ConstValue::Int(v) => {
            if target.is_int() {
                Some(ConstValue::Int(truncate_int(v, target)))
            } else {
                None
            }
        }
        ConstValue::Bool(v) => {
            if target.is_int() {
                Some(ConstValue::Int(if v { 1 } else { 0 }))
            } else {
                None
            }
        }
        ConstValue::Char(v) => {
            if target.is_int() {
                Some(ConstValue::Int(v as i128))
            } else {
                None
            }
        }
        ConstValue::Str(_) => None,
    }
    .filter(|_| !matches!(source, Type::Never))
}

pub(super) fn truncate_int(value: i128, target: &Type) -> i128 {
    let bits = target.int_bits().unwrap_or(128).min(128);
    if bits == 128 {
        return value;
    }
    let mask = (1i128 << bits) - 1;
    let bits_value = value & mask;
    if target.is_signed_int() {
        let sign = 1i128 << (bits - 1);
        if bits_value & sign != 0 {
            bits_value - (1i128 << bits)
        } else {
            bits_value
        }
    } else {
        bits_value
    }
}

// ---------------------------------------------------------------------------
// Exhaustivité
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Exhaustivité
// ---------------------------------------------------------------------------
//
// G-6.22 / TYP-10.4 (MUST): a `match` must be exhaustive for `bool`, `Option`,
// `Result` and every enum; integers (and floats, G-6.36) require a `_` arm.
// Coverage is computed recursively over the constructor space: a sum type needs
// every constructor (descending into its payload), a product type (tuple) needs
// every combination. The previous engine only looked at the *outer* constructor
// of each arm, so `Some(Some(n))` was taken as covering `Some` (accepting a
// match that rustc rejects with E0004) while a fully enumerated `(bool, bool)`
// tuple was rejected (E-TYP-NONEXHAUSTIVE on a program rustc accepts).
