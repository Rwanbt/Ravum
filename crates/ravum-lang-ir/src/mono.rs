//! Monomorphization support (IR v1.2): generic functions are lowered once
//! per set of type arguments, found by matching the declared parameter and
//! return types against the checker's types at the call.

use std::collections::BTreeMap;

use ravum_lang_sema::Type;

/// Generic parameter index → concrete type.
pub type Subst = BTreeMap<usize, Type>;

/// Replace every `Param` of `ty` bound in `s`.
pub fn substitute(ty: &Type, s: &Subst) -> Type {
    match ty {
        Type::Param { index, .. } => s.get(index).cloned().unwrap_or_else(|| ty.clone()),
        Type::Ref { mutable, inner } => Type::Ref {
            mutable: *mutable,
            inner: Box::new(substitute(inner, s)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(substitute(elem, s)),
            len: *len,
        },
        Type::Slice(inner) => Type::Slice(Box::new(substitute(inner, s))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(|t| substitute(t, s)).collect()),
        Type::Vec(inner) => Type::Vec(Box::new(substitute(inner, s))),
        Type::Option(inner) => Type::Option(Box::new(substitute(inner, s))),
        Type::Result(ok, err) => {
            Type::Result(Box::new(substitute(ok, s)), Box::new(substitute(err, s)))
        }
        Type::Nominal { def, name, args } => Type::Nominal {
            def: *def,
            name: name.clone(),
            args: args.iter().map(|t| substitute(t, s)).collect(),
        },
        other => other.clone(),
    }
}

/// Bind the `Param`s of `pattern` from the concrete `actual`. Returns
/// `false` on a shape mismatch or a parameter bound to two types.
pub fn bind(pattern: &Type, actual: &Type, s: &mut Subst) -> bool {
    match (pattern, actual) {
        (Type::Param { index, .. }, t) => match s.get(index) {
            Some(prev) => prev == t,
            None => {
                s.insert(*index, t.clone());
                true
            }
        },
        (Type::Ref { inner: a, .. }, Type::Ref { inner: b, .. })
        | (Type::Slice(a), Type::Slice(b))
        | (Type::Vec(a), Type::Vec(b))
        | (Type::Option(a), Type::Option(b)) => bind(a, b, s),
        (Type::Array { elem: a, len: n }, Type::Array { elem: b, len: m }) => {
            n == m && bind(a, b, s)
        }
        (Type::Tuple(a), Type::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| bind(x, y, s))
        }
        (Type::Result(a1, a2), Type::Result(b1, b2)) => bind(a1, b1, s) && bind(a2, b2, s),
        (
            Type::Nominal {
                def: d1, args: a, ..
            },
            Type::Nominal {
                def: d2, args: b, ..
            },
        ) => d1 == d2 && a.len() == b.len() && a.iter().zip(b).all(|(x, y)| bind(x, y, s)),
        // `!` coerces to any type: it binds nothing.
        (_, Type::Never) => true,
        (a, b) => a == b,
    }
}

/// Whether `ty` still contains a generic parameter.
pub fn has_param(ty: &Type) -> bool {
    match ty {
        Type::Param { .. } => true,
        Type::Ref { inner, .. }
        | Type::Slice(inner)
        | Type::Vec(inner)
        | Type::Option(inner)
        | Type::Array { elem: inner, .. } => has_param(inner),
        Type::Tuple(items) => items.iter().any(has_param),
        Type::Result(a, b) => has_param(a) || has_param(b),
        Type::Nominal { args, .. } => args.iter().any(has_param),
        _ => false,
    }
}
