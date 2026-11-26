use super::*;
use ravum_lang_syntax::ast::BinOp;

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

/// EXPR-3.4: a shift amount must be provably non-negative at compile time.
/// That holds for an unsigned amount and for a constant integer expression
/// (literals combined with `+ - * / %`) whose value is non-negative. Any other
/// amount of a signed type is refused, since its sign is only known at run time.
pub(super) fn shift_amount_refused(amount: &Expr, amount_ty: &Type) -> bool {
    match literal_int_value(amount) {
        Some(value) => value < 0,
        None => amount_ty.is_signed_int(),
    }
}

/// The value of an integer expression made only of literals, or `None`.
/// Overflow and division by zero also give `None` (not provably constant).
fn literal_int_value(expr: &Expr) -> Option<i128> {
    match &expr.kind {
        ExprKind::Int { value, .. } => i128::try_from(*value).ok(),
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => literal_int_value(expr)?.checked_neg(),
        ExprKind::Binary { op, lhs, rhs } => {
            let (l, r) = (literal_int_value(lhs)?, literal_int_value(rhs)?);
            match op {
                BinOp::Add => l.checked_add(r),
                BinOp::Sub => l.checked_sub(r),
                BinOp::Mul => l.checked_mul(r),
                BinOp::Div => l.checked_div(r),
                BinOp::Rem => l.checked_rem(r),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn method_self_kind(ty: &Type) -> Option<MethodSelfKind> {
    Some(match ty {
        Type::String => MethodSelfKind::String,
        Type::Str => MethodSelfKind::StrRef,
        Type::Ref { inner, .. } if matches!(**inner, Type::Str) => MethodSelfKind::StrRef,
        Type::Slice(_) => MethodSelfKind::Slice,
        Type::Vec(_) => MethodSelfKind::Vec,
        Type::Nominal { def, .. } if *def == crate::builtins::PRELUDE_MAP_DEF => {
            MethodSelfKind::Map
        }
        Type::Nominal { def, .. } if *def == crate::builtins::PRELUDE_ARENA_DEF => {
            MethodSelfKind::Arena
        }
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
        Type::Nominal { def, args, .. }
            if *def == crate::builtins::PRELUDE_MAP_DEF && args.len() == 2 =>
        {
            vec![
                ("Self", ty.clone()),
                ("K", args[0].clone()),
                ("V", args[1].clone()),
            ]
        }
        Type::Nominal { def, args, .. }
            if *def == crate::builtins::PRELUDE_ARENA_DEF && args.len() == 1 =>
        {
            vec![("Self", ty.clone()), ("T", args[0].clone())]
        }
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

/// Substitutes the bound inference variables of `ty`.
///
/// The structural recursion stops past [`MAX_TYPE_DEPTH`]: the part below is
/// replaced by `Error`, which keeps the type deeper than the bound, so
/// `finalize_inference` still reports `E-TYP-DEPTH` on it. Links between
/// variables are followed in a loop and do not count as a level. The occurs
/// check (`var_occurs`) makes the chains acyclic, so a chain visits each
/// variable at most once; a longer walk is a forged cycle and yields `Error`.
pub(super) fn resolve_vars_impl(
    vars: &[Option<Type>],
    ty: &Type,
    mut unresolved: Option<&mut BTreeSet<usize>>,
    depth: usize,
) -> Type {
    if depth >= MAX_TYPE_DEPTH {
        return Type::Error;
    }
    let mut ty = ty;
    let mut links = 0;
    while let Some(index) = infer_var_index(ty) {
        match vars.get(index).and_then(Option::as_ref) {
            Some(_) if links >= vars.len() => return Type::Error,
            Some(bound) => {
                ty = bound;
                links += 1;
            }
            None => {
                return match unresolved.as_deref_mut() {
                    Some(set) => {
                        set.insert(index);
                        Type::Error
                    }
                    None => ty.clone(),
                };
            }
        }
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

/// RFC-UI-LANG-1 P5 (OWN-6.14): structural clonability. `Ok(alloc)` when
/// every component of `ty` is clonable (`alloc`: the clone may allocate,
/// effect `alloc`); `Err(why)` names the first component that is not.
/// Clonability is a property of the type: a *resource* of `std` or of the
/// runtime is never clonable, and neither is anything that contains one.
pub(super) fn clonability(
    world: &World,
    facts: &Facts,
    ty: &Type,
    seen: &mut Vec<NodeId>,
) -> Result<bool, String> {
    use crate::builtins as b;
    match ty {
        Type::String => Ok(true),
        Type::Vec(inner) => clonability(world, facts, inner, seen).map(|_| true),
        Type::Option(inner) | Type::Array { elem: inner, .. } => {
            clonability(world, facts, inner, seen)
        }
        Type::Result(ok, err) => {
            Ok(clonability(world, facts, ok, seen)? | clonability(world, facts, err, seen)?)
        }
        Type::Tuple(items) => {
            let mut alloc = false;
            for t in items {
                alloc |= clonability(world, facts, t, seen)?;
            }
            Ok(alloc)
        }
        Type::Ref { .. } => Err(format!(
            "`{ty}` is a reference (clone the value it points to)"
        )),
        Type::Param { name, .. } => Err(format!(
            "`{name}` is a type parameter: its clonability is unknown without trait bounds"
        )),
        Type::Slice(_) => Err(format!("`{ty}` is a borrowed view")),
        Type::Nominal { def, args, name } => {
            let resources = [
                b::PRELUDE_TCP_LISTENER_DEF,
                b::PRELUDE_TCP_STREAM_DEF,
                b::PRELUDE_JOIN_HANDLE_DEF,
                b::PRELUDE_CHANNEL_DEF,
                b::PRELUDE_SENDER_DEF,
                b::PRELUDE_RECEIVER_DEF,
                b::PRELUDE_HTTP_REQUEST_DEF,
                b::PRELUDE_HTTP_RESPONSE_DEF,
                b::PRELUDE_FUTURE_DEF,
            ];
            if resources.contains(def) {
                return Err(format!("`{name}` is a resource, never clonable"));
            }
            if *def == b::PRELUDE_ERROR_DEF {
                return Ok(true);
            }
            // HDL-7: a handle is a value (copied, no allocation); an arena is
            // clonable when its elements are (a new arena identifier).
            if *def == b::PRELUDE_HANDLE_DEF {
                return Ok(false);
            }
            if *def == b::PRELUDE_ARENA_DEF {
                let mut alloc = true;
                for a in args {
                    alloc |= clonability(world, facts, a, seen)?;
                }
                return Ok(alloc);
            }
            if *def == b::PRELUDE_MAP_DEF {
                let mut alloc = true;
                for a in args {
                    alloc |= clonability(world, facts, a, seen)?;
                }
                return Ok(alloc);
            }
            if *def == b::PRELUDE_CHANDLE_DEF {
                return Ok(false);
            }
            if seen.contains(def) {
                // A recursive type: its own clonability is being decided.
                return Ok(true);
            }
            seen.push(*def);
            let subst = nominal_subst(world, def, args);
            let mut alloc = false;
            let fields: Vec<(String, Type)> = if let Some(info) = facts.structs.get(def) {
                info.fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty.clone()))
                    .collect()
            } else if let Some(info) = facts.enums.get(def) {
                info.variants
                    .iter()
                    .flat_map(|v| {
                        v.tuple_types
                            .iter()
                            .map(move |t| (v.name.clone(), t.clone()))
                            .chain(v.fields.iter().map(|f| (f.name.clone(), f.ty.clone())))
                    })
                    .collect()
            } else {
                seen.pop();
                return Err(format!("`{name}` has no known layout"));
            };
            for (part, t) in fields {
                match clonability(world, facts, &substitute(&t, &subst), seen) {
                    Ok(a) => alloc |= a,
                    Err(why) => {
                        seen.pop();
                        return Err(format!("`{name}.{part}`: {why}"));
                    }
                }
            }
            seen.pop();
            Ok(alloc)
        }
        Type::Error | Type::Never => Ok(false),
        _ => Ok(false),
    }
}
