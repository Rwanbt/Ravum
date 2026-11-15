use super::*;

/// Bound on the recursion depth of the coverage check (P0-H).
pub(super) const MAX_COVERAGE_DEPTH: u32 = 64;

/// One constructor of a sum type, with the payload types it introduces.
pub(super) struct CoverageCtor {
    kind: CoverageCtorKind,
    /// Payload types, in declaration order (empty for a unit constructor).
    payload: Vec<Type>,
    /// Field names, in declaration order, for a struct variant.
    fields: Option<Vec<String>>,
    /// Enum definition this constructor belongs to (`None` for `bool`,
    /// `Option` and `Result`).
    def: Option<NodeId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CoverageCtorKind {
    Bool(bool),
    Some,
    None,
    Ok,
    Err,
    Variant(usize),
    /// A tuple is a product with a single constructor and N payloads.
    Tuple,
}

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    /// `true` when the given patterns cover every value of `ty`.
    ///
    /// `None` in `pats` stands for a wildcard: it appears when a payload is
    /// absent (a unit variant) or when a struct pattern omits a field.
    pub(super) fn patterns_exhaustive(&self, ty: &Type, pats: &[Option<&Pat>], depth: u32) -> bool {
        if depth > MAX_COVERAGE_DEPTH {
            return false;
        }
        // G-6.36: codegen lowers a float sub-pattern to a guard, and a guarded
        // arm covers nothing. Such an arm must not count as coverage.
        let pats: Vec<Option<&Pat>> = pats
            .iter()
            .copied()
            .filter(|pat| pat.is_none_or(|pat| !pattern_contains_float(pat)))
            .collect();
        if pats.is_empty() {
            return false;
        }
        if pats
            .iter()
            .any(|pat| pat.is_none_or(|pat| self.pattern_irrefutable_for(pat, ty)))
        {
            return true;
        }
        match ty {
            Type::Bool => {
                let mut seen_true = false;
                let mut seen_false = false;
                for pat in pats.iter().flatten() {
                    if let PatKind::Lit(LitPat::Bool(value)) = &pat.kind {
                        if *value {
                            seen_true = true;
                        } else {
                            seen_false = true;
                        }
                    }
                }
                seen_true && seen_false
            }
            Type::Option(inner) => {
                let mut some_rows: Vec<Option<&Pat>> = Vec::new();
                let mut none_seen = false;
                for pat in pats.iter().flatten() {
                    match pattern_kind(pat) {
                        PatternKind::Some => some_rows.push(payload_1(pat)),
                        PatternKind::None => none_seen = true,
                        _ => {}
                    }
                }
                none_seen && self.patterns_exhaustive(inner, &some_rows, depth + 1)
            }
            Type::Result(ok, err) => {
                let mut ok_rows: Vec<Option<&Pat>> = Vec::new();
                let mut err_rows: Vec<Option<&Pat>> = Vec::new();
                for pat in pats.iter().flatten() {
                    match pattern_kind(pat) {
                        PatternKind::Ok => ok_rows.push(payload_1(pat)),
                        PatternKind::Err => err_rows.push(payload_1(pat)),
                        _ => {}
                    }
                }
                self.patterns_exhaustive(ok, &ok_rows, depth + 1)
                    && self.patterns_exhaustive(err, &err_rows, depth + 1)
            }
            Type::Nominal { def, args, .. } => {
                let Some(info) = self.facts.enums.get(def) else {
                    return false;
                };
                let subst = nominal_subst(self.world, def, args);
                for (index, variant) in info.variants.iter().enumerate() {
                    let payload: Vec<Type> = match variant.kind {
                        crate::facts::VariantKind::Unit => Vec::new(),
                        crate::facts::VariantKind::Tuple => variant
                            .tuple_types
                            .iter()
                            .map(|t| substitute(t, &subst))
                            .collect(),
                        crate::facts::VariantKind::Struct => variant
                            .fields
                            .iter()
                            .map(|f| substitute(&f.ty, &subst))
                            .collect(),
                    };
                    let mut rows: Vec<Vec<Option<&Pat>>> = Vec::new();
                    let mut seen_ctor = false;
                    for pat in pats.iter().flatten() {
                        if matches!(
                            self.variant_pats.get(&pat.id),
                            Some((ctor_def, ctor_index)) if ctor_def == def && *ctor_index == index
                        ) {
                            seen_ctor = true;
                            rows.push(self.variant_payload_row(pat, variant));
                        }
                    }
                    if !seen_ctor {
                        return false;
                    }
                    if !self.product_rows_exhaustive(&payload, &rows, depth + 1) {
                        return false;
                    }
                }
                true
            }
            Type::Tuple(items) => {
                let mut rows: Vec<Vec<Option<&Pat>>> = Vec::new();
                for pat in pats.iter().flatten() {
                    if let PatKind::Tuple(subs) = &pat.kind {
                        if subs.len() == items.len() {
                            rows.push(subs.iter().map(Some).collect());
                        }
                    }
                }
                self.product_rows_exhaustive(items, &rows, depth + 1)
            }
            _ => false,
        }
    }

    /// A pattern that matches every value of its type: a wildcard, a binding,
    /// or a tuple whose elements are all irrefutable (G-6.38, TYP-4).
    pub(super) fn pattern_irrefutable_for(&self, pat: &Pat, ty: &Type) -> bool {
        match &pat.kind {
            PatKind::Wild | PatKind::Binding { .. } => true,
            PatKind::Tuple(items) => match ty {
                Type::Tuple(types) if types.len() == items.len() => items
                    .iter()
                    .zip(types.iter())
                    .all(|(item, ty)| self.pattern_irrefutable_for(item, ty)),
                Type::Unit => items.is_empty(),
                _ => false,
            },
            _ => false,
        }
    }

    /// Coverage of a product type: every combination of the columns must be
    /// covered. The rows are specialized column by column; the first column
    /// with a finite constructor set is decomposed completely, which is the
    /// standard (sound and complete) decision procedure for products.
    pub(super) fn product_rows_exhaustive(
        &self,
        tys: &[Type],
        rows: &[Vec<Option<&Pat>>],
        depth: u32,
    ) -> bool {
        if depth > MAX_COVERAGE_DEPTH {
            return false;
        }
        if tys.is_empty() {
            // The empty product (unit): any row covers it.
            return !rows.is_empty();
        }
        if rows.iter().any(|row| {
            row.iter()
                .zip(tys.iter())
                .all(|(pat, ty)| pat.is_none_or(|pat| self.pattern_irrefutable_for(pat, ty)))
        }) {
            return true;
        }
        if rows.is_empty() {
            return false;
        }
        for (column, ty) in tys.iter().enumerate() {
            let Some(ctors) = self.coverage_ctors(ty) else {
                continue;
            };
            for ctor in &ctors {
                let mut sub_rows: Vec<Vec<Option<&Pat>>> = Vec::new();
                for row in rows {
                    if let Some(specialized) = self.specialize_row(row, column, ty, ctor) {
                        sub_rows.push(specialized);
                    }
                }
                let mut new_tys = ctor.payload.clone();
                new_tys.extend(tys[..column].iter().cloned());
                new_tys.extend(tys[column + 1..].iter().cloned());
                if !self.product_rows_exhaustive(&new_tys, &sub_rows, depth + 1) {
                    // This constructor leaves part of the product uncovered,
                    // and the decomposition is complete: the product is not
                    // exhaustive.
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// The finite constructor set of a sum type, if it has one. `None` means
    /// the type has no enumerable constructors and therefore requires `_`
    /// (integers, floats, text, references, ...).
    pub(super) fn coverage_ctors(&self, ty: &Type) -> Option<Vec<CoverageCtor>> {
        match ty {
            Type::Bool => Some(vec![
                CoverageCtor {
                    kind: CoverageCtorKind::Bool(true),
                    payload: Vec::new(),
                    fields: None,
                    def: None,
                },
                CoverageCtor {
                    kind: CoverageCtorKind::Bool(false),
                    payload: Vec::new(),
                    fields: None,
                    def: None,
                },
            ]),
            Type::Option(inner) => Some(vec![
                CoverageCtor {
                    kind: CoverageCtorKind::Some,
                    payload: vec![(**inner).clone()],
                    fields: None,
                    def: None,
                },
                CoverageCtor {
                    kind: CoverageCtorKind::None,
                    payload: Vec::new(),
                    fields: None,
                    def: None,
                },
            ]),
            Type::Result(ok, err) => Some(vec![
                CoverageCtor {
                    kind: CoverageCtorKind::Ok,
                    payload: vec![(**ok).clone()],
                    fields: None,
                    def: None,
                },
                CoverageCtor {
                    kind: CoverageCtorKind::Err,
                    payload: vec![(**err).clone()],
                    fields: None,
                    def: None,
                },
            ]),
            Type::Nominal { def, args, .. } => {
                let info = self.facts.enums.get(def)?;
                let subst = nominal_subst(self.world, def, args);
                let mut ctors = Vec::with_capacity(info.variants.len());
                for (index, variant) in info.variants.iter().enumerate() {
                    let (payload, fields) = match variant.kind {
                        crate::facts::VariantKind::Unit => (Vec::new(), None),
                        crate::facts::VariantKind::Tuple => (
                            variant
                                .tuple_types
                                .iter()
                                .map(|t| substitute(t, &subst))
                                .collect(),
                            None,
                        ),
                        crate::facts::VariantKind::Struct => (
                            variant
                                .fields
                                .iter()
                                .map(|f| substitute(&f.ty, &subst))
                                .collect(),
                            Some(variant.fields.iter().map(|f| f.name.clone()).collect()),
                        ),
                    };
                    ctors.push(CoverageCtor {
                        kind: CoverageCtorKind::Variant(index),
                        payload,
                        fields,
                        def: Some(*def),
                    });
                }
                Some(ctors)
            }
            // A tuple is a product with one constructor: it must be decomposed
            // as a column so nested tuples (`((bool, bool), bool)`) are covered.
            Type::Tuple(items) => Some(vec![CoverageCtor {
                kind: CoverageCtorKind::Tuple,
                payload: items.clone(),
                fields: None,
                def: None,
            }]),
            _ => None,
        }
    }

    /// Specialize one row on `column` for `ctor`: `None` when the pattern does
    /// not match this constructor (the row is dropped), otherwise the row with
    /// the column replaced by the constructor payload patterns.
    pub(super) fn specialize_row<'a>(
        &self,
        row: &[Option<&'a Pat>],
        column: usize,
        ty: &Type,
        ctor: &CoverageCtor,
    ) -> Option<Vec<Option<&'a Pat>>> {
        let payload: Vec<Option<&Pat>> = match row.get(column).copied().flatten() {
            None => vec![None; ctor.payload.len()],
            Some(pat) => {
                if self.pattern_irrefutable_for(pat, ty) {
                    vec![None; ctor.payload.len()]
                } else {
                    self.pattern_payloads_for_ctor(pat, ctor)?
                }
            }
        };
        let mut out = payload;
        for (index, pat) in row.iter().enumerate() {
            if index != column {
                out.push(*pat);
            }
        }
        Some(out)
    }

    /// Payload patterns of `pat` when it matches `ctor`; `None` when it does
    /// not match. The returned vector is aligned with `ctor.payload`.
    pub(super) fn pattern_payloads_for_ctor<'a>(
        &self,
        pat: &'a Pat,
        ctor: &CoverageCtor,
    ) -> Option<Vec<Option<&'a Pat>>> {
        match ctor.kind {
            CoverageCtorKind::Bool(value) => match &pat.kind {
                PatKind::Lit(LitPat::Bool(found)) if *found == value => Some(Vec::new()),
                _ => None,
            },
            CoverageCtorKind::Some => match pattern_kind(pat) {
                PatternKind::Some => Some(vec![payload_1(pat)]),
                _ => None,
            },
            CoverageCtorKind::None => match pattern_kind(pat) {
                PatternKind::None => Some(Vec::new()),
                _ => None,
            },
            CoverageCtorKind::Ok => match pattern_kind(pat) {
                PatternKind::Ok => Some(vec![payload_1(pat)]),
                _ => None,
            },
            CoverageCtorKind::Err => match pattern_kind(pat) {
                PatternKind::Err => Some(vec![payload_1(pat)]),
                _ => None,
            },
            CoverageCtorKind::Tuple => match &pat.kind {
                PatKind::Tuple(items) if items.len() == ctor.payload.len() => {
                    Some(items.iter().map(Some).collect())
                }
                _ => None,
            },
            CoverageCtorKind::Variant(index) => {
                if !matches!(
                    self.variant_pats.get(&pat.id),
                    Some((def, ctor_index))
                        if Some(*def) == ctor.def && *ctor_index == index
                ) {
                    return None;
                }
                match &pat.kind {
                    PatKind::Path(_) => Some(Vec::new()),
                    PatKind::Variant { fields, .. } => Some(match fields {
                        VariantPatFields::Tuple(items) => (0..ctor.payload.len())
                            .map(|i| items.get(i).map(Some).unwrap_or(None))
                            .collect(),
                        VariantPatFields::Struct(field_pats) => ctor
                            .fields
                            .as_ref()
                            .map(|names| {
                                names
                                    .iter()
                                    .map(|name| {
                                        field_pats
                                            .iter()
                                            .find(|field| field.name.name == *name)
                                            .map(|field| Some(&field.pat))
                                            .unwrap_or(None)
                                    })
                                    .collect()
                            })
                            .unwrap_or_else(|| vec![None; ctor.payload.len()]),
                    }),
                    _ => Some(vec![None; ctor.payload.len()]),
                }
            }
        }
    }

    /// Payload row of a variant pattern, in declaration order.
    pub(super) fn variant_payload_row<'a>(
        &self,
        pat: &'a Pat,
        variant: &crate::facts::VariantInfo,
    ) -> Vec<Option<&'a Pat>> {
        match &pat.kind {
            PatKind::Path(_) => Vec::new(),
            PatKind::Variant { fields, .. } => match fields {
                VariantPatFields::Tuple(items) => (0..variant.tuple_types.len())
                    .map(|i| items.get(i).map(Some).unwrap_or(None))
                    .collect(),
                VariantPatFields::Struct(field_pats) => variant
                    .fields
                    .iter()
                    .map(|decl| {
                        field_pats
                            .iter()
                            .find(|field| field.name.name == decl.name)
                            .map(|field| Some(&field.pat))
                            .unwrap_or(None)
                    })
                    .collect(),
            },
            _ => Vec::new(),
        }
    }
}
