use super::*;

// ----------------------------------------------------------------------
// Pure helpers
// ----------------------------------------------------------------------

pub(super) fn join(a: &State, b: &State) -> State {
    let mut slots = BTreeMap::new();
    for (slot, sa) in &a.slots {
        match b.slots.get(slot) {
            Some(sb) => {
                slots.insert(*slot, join_slot(sa, sb));
            }
            None => {
                slots.insert(*slot, sa.clone());
            }
        }
    }
    for (slot, sb) in &b.slots {
        if !a.slots.contains_key(slot) {
            slots.insert(*slot, sb.clone());
        }
    }
    State { slots }
}

pub(super) fn join_all(states: &[State]) -> Option<State> {
    let mut iter = states.iter();
    let first = iter.next()?;
    let mut acc = first.clone();
    for st in iter {
        acc = join(&acc, st);
    }
    Some(acc)
}

pub(super) fn join_slot(a: &SlotState, b: &SlotState) -> SlotState {
    let mut partial = a.partial.clone();
    for (path, rec_b) in &b.partial {
        partial
            .entry(path.clone())
            .and_modify(|rec| {
                rec.used_since = rec.used_since || rec_b.used_since;
            })
            .or_insert_with(|| rec_b.clone());
    }
    SlotState {
        init: a.init && b.init,
        moved: a.moved && b.moved,
        maybe_moved: a.maybe_moved || b.maybe_moved || (a.moved != b.moved),
        move_span: a.move_span.or(b.move_span),
        used_since: a.used_since || b.used_since,
        partial,
    }
}

pub(super) fn drop_slot(state: &mut State, slot: SlotId) {
    state.slots.remove(&slot);
}

/// `inner` starts with `outer` (or equals it).
pub(super) fn steps_start_with(inner: &[Step], outer: &[Step]) -> bool {
    inner.len() >= outer.len() && inner.iter().zip(outer.iter()).all(|(a, b)| a == b)
}

/// Two projection steps denote the same position. An index computed from a
/// non-literal expression is compatible with every index (conservative,
/// Q-OWN-5); mismatched projection kinds are compatible too.
pub(super) fn step_compatible(a: &Step, b: &Step) -> bool {
    match (a, b) {
        (Step::Deref, Step::Deref) => true,
        (Step::Field(x), Step::Field(y)) => x == y,
        (Step::Tuple(x), Step::Tuple(y)) => x == y,
        (Step::Index(x), Step::Index(y)) => match (x, y) {
            (IndexKey::Lit(lx), IndexKey::Lit(ly)) => lx == ly,
            _ => true,
        },
        (Step::Deref, _) | (_, Step::Deref) => false,
        // Unknown/mismatched kinds: stay conservative.
        _ => true,
    }
}

pub(super) fn prefix_compatible(short: &[Step], long: &[Step]) -> bool {
    short.len() <= long.len()
        && short
            .iter()
            .zip(long.iter())
            .all(|(a, b)| step_compatible(a, b))
}

/// True when the first incompatible projection of two paths proves that the
/// places cannot overlap.
pub(super) fn diverges_disjoint(a: &[Step], b: &[Step]) -> bool {
    for (x, y) in a.iter().zip(b.iter()) {
        if !step_compatible(x, y) {
            return match (x, y) {
                (Step::Field(p), Step::Field(q)) => p != q,
                (Step::Tuple(p), Step::Tuple(q)) => p != q,
                (Step::Index(IndexKey::Lit(p)), Step::Index(IndexKey::Lit(q))) => p != q,
                (Step::Deref, _) | (_, Step::Deref) => true,
                // Opaque/different kinds: cannot prove disjointness.
                _ => false,
            };
        }
        // Two paths that agree on a non-literal index may denote the same
        // element, and reaching anything through such an index uses the whole
        // array at that level: a later divergence proves nothing (OWN-5.4).
        if matches!(x, Step::Index(IndexKey::Opaque)) || matches!(y, Step::Index(IndexKey::Opaque))
        {
            return false;
        }
    }
    false
}

pub(super) fn classify_partial(place: &[Step], moved: &[Step]) -> PartialRel {
    if place.len() == moved.len()
        && place
            .iter()
            .zip(moved.iter())
            .all(|(a, b)| step_compatible(a, b))
    {
        PartialRel::Exact
    } else if place.len() > moved.len() && prefix_compatible(moved, place) {
        PartialRel::Extension
    } else if place.len() < moved.len() && prefix_compatible(place, moved) {
        PartialRel::Ancestor
    } else if diverges_disjoint(place, moved) {
        PartialRel::Disjoint
    } else {
        // Overlapping but not comparable (e.g. opaque index): refuse.
        PartialRel::Extension
    }
}

pub(super) fn places_overlap(a: &Place, b: &Place) -> bool {
    if a.slot != b.slot {
        return false;
    }
    let n = a.steps.len().min(b.steps.len());
    for i in 0..n {
        match (&a.steps[i], &b.steps[i]) {
            (Step::Deref, Step::Deref) => {}
            (Step::Field(x), Step::Field(y)) => {
                if x != y {
                    return false;
                }
            }
            (Step::Tuple(x), Step::Tuple(y)) => {
                if x != y {
                    return false;
                }
            }
            (Step::Index(x), Step::Index(y)) => {
                if x != y {
                    match (x, y) {
                        (IndexKey::Lit(lx), IndexKey::Lit(ly)) if lx != ly => return false,
                        _ => return true,
                    }
                }
            }
            (Step::Deref, _) | (_, Step::Deref) => return false,
            // Struct field vs tuple index (or other mismatched kinds): the
            // types cannot tell us, stay conservative.
            _ => return true,
        }
    }
    true
}

pub(super) fn last_path_segment(path: &str) -> &str {
    let last = path.rsplit("::").next().unwrap_or(path);
    last.rsplit('.').next().unwrap_or(last)
}

/// Builtin methods whose receiver is `self` **by value**. `stdlib.md` STD-11.4
/// closes the `Option`/`Result` method list, and STD-11.3 states that every one
/// of these consumes the receiver. `expect` does not exist in Core 1.0
/// (STD-11.4), so it is deliberately absent.
pub(super) fn builtin_method_consumes_receiver(path: &str) -> bool {
    matches!(
        path,
        "Option::unwrap"
            | "Option::unwrap_or"
            | "Option::ok_or"
            | "Result::unwrap"
            | "Result::unwrap_or"
            | "Result::ok"
            | "Result::err"
    )
}

/// Prelude constructors that store their argument inside an aggregate.
/// `Option::None` is a constant (no argument). Any reference stored there must
/// be a `&'static str` (`ownership.md` OWN-6.1/6.2).
pub(super) fn builtin_ctor_stores_argument(path: &str) -> bool {
    matches!(path, "Option::Some" | "Result::Ok" | "Result::Err")
}

pub(super) fn builtin_method_is_mutating(path: &str) -> bool {
    matches!(
        last_path_segment(path),
        "push"
            | "push_str"
            | "pop"
            | "insert"
            | "remove"
            | "clear"
            | "retain"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "reverse"
            | "truncate"
            | "extend"
            | "extend_from_slice"
            | "append"
            | "resize"
            | "reserve"
            | "reserve_exact"
            | "shrink_to_fit"
            | "swap"
            | "fill"
            | "drain"
            | "split_off"
            | "dedup"
            | "rotate_left"
            | "rotate_right"
            | "copy_from_slice"
    )
}

/// Builtin methods that store their argument *value* in the receiver, and can
/// therefore make a reference escape. Methods like `String::push_str`,
/// `Vec::extend` or `slice::fill` copy the pointee and must NOT be listed here:
/// `s.push_str(t.as_str())` is valid (`spec/language/ownership.md` OWN-6.3).
pub(super) fn builtin_method_stores_refs(path: &str) -> bool {
    matches!(last_path_segment(path), "push" | "insert")
}

pub(super) fn field_type(ty: &Type, name: &str, facts: &Facts) -> Type {
    match ty {
        Type::Nominal { def, .. } => {
            if let Some(info) = facts.structs.get(def) {
                info.fields
                    .iter()
                    .find(|f| f.name == name)
                    .map(|f| f.ty.clone())
                    .unwrap_or(Type::Error)
            } else {
                Type::Error
            }
        }
        _ => Type::Error,
    }
}

/// Copy classification (OWN-2.1/2.2). Nominal types use the `is_copy` flag
/// computed by the checker; missing facts are treated as mobile (conservative).
pub(crate) fn is_copy_type(ty: &Type, facts: &Facts) -> bool {
    match ty {
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 => true,
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => true,
        Type::Isize | Type::Usize | Type::F32 | Type::F64 => true,
        Type::Bool | Type::Char | Type::Unit | Type::Str => true,
        Type::Never => true,
        Type::Ref { .. } => true,
        Type::Array { elem, .. } => is_copy_type(elem, facts),
        Type::Tuple(items) => items.iter().all(|t| is_copy_type(t, facts)),
        Type::Slice(_) => true,
        Type::String | Type::Vec(_) => false,
        Type::Option(inner) => is_copy_type(inner, facts),
        Type::Result(ok, err) => is_copy_type(ok, facts) && is_copy_type(err, facts),
        Type::Nominal { def, .. } => {
            if let Some(info) = facts.structs.get(def) {
                info.is_copy
            } else if let Some(info) = facts.enums.get(def) {
                info.is_copy
            } else {
                false
            }
        }
        // A generic parameter could be `Copy` or not: assume the worst.
        Type::Param { .. } => false,
        // `Type::Error` must not cascade ownership errors.
        Type::Error => true,
    }
}

impl Type {
    /// Copy or `Error` (never penalise a program whose typing already failed).
    pub(super) fn is_error_or_copy(&self, facts: &Facts) -> bool {
        is_copy_type(self, facts)
    }
}
