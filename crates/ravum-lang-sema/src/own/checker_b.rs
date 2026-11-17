use super::*;

impl<'a> OwnChecker<'a> {
    pub(super) fn eval_for(&mut self, pat: &Pat, iter_expr: &Expr, body: &Block) {
        self.eval_value(iter_expr);
        self.loans.truncate(self.loan_floor);
        let after_iter = self.state.clone();
        let iter_diverged = self.diverged;

        self.push_scope();
        let mut bindings: Vec<PatBinding> = Vec::new();
        self.collect_pat_bindings(pat, false, &mut bindings);
        let mut loop_slots: Vec<SlotId> = Vec::new();
        for b in &bindings {
            // A `for` item of type `&str` comes out of the iterable (a range or
            // a container of static texts): static by rule 2.
            let static_ref = is_borrowed_text(&b.ty);
            if b.ty.is_reference() && !static_ref {
                self.report_ref_escape(b.span, "stockée");
            }
            let slot = self.declare_slot(&b.name, b.ty.clone(), static_ref, b.span);
            loop_slots.push(slot);
        }

        let saved_emit = self.emit;
        let mut entry = after_iter.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            for slot in &loop_slots {
                self.state.slots.insert(*slot, SlotState::initialised());
            }
            self.diverged = iter_diverged;
            self.loans.truncate(self.loan_floor);
            self.loop_stack.push(LoopCtx::default());
            if !self.diverged {
                self.analyze_block(body, false);
            }
            let ctx = self.loop_stack.pop().unwrap_or_default();
            let mut next = entry.clone();
            if !self.diverged {
                next = join(&next, &self.state);
            }
            for c in &ctx.continues {
                next = join(&next, c);
            }
            for slot in &loop_slots {
                drop_slot(&mut next, *slot);
            }
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        self.emit = saved_emit;
        self.state = entry.clone();
        for slot in &loop_slots {
            self.state.slots.insert(*slot, SlotState::initialised());
        }
        self.diverged = iter_diverged;
        self.loans.truncate(self.loan_floor);
        self.loop_stack.push(LoopCtx::default());
        if !self.diverged {
            self.analyze_block(body, false);
        } else {
            self.analyze_dead(|checker| checker.analyze_block(body, false));
        }
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // Exit: zero iterations (`after_iter`) or any number of iterations then
        // normal termination (the fixpoint header), plus every `break`.
        let mut exit: Option<State> = if iter_diverged {
            None
        } else {
            Some(join(&after_iter, &entry))
        };
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        self.pop_scope();
        match exit {
            Some(mut st) => {
                for slot in &loop_slots {
                    drop_slot(&mut st, *slot);
                }
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }

    // ------------------------------------------------------------------
    // Calls
    // ------------------------------------------------------------------

    pub(super) fn eval_call(&mut self, expr: &Expr, args: &[Expr]) {
        let snapshot = self.loans.len();
        self.call_depth = self.call_depth.saturating_add(1);
        let target = self.facts.calls.get(&expr.id).cloned();
        match target {
            Some(CallTarget::UserFn { item }) => {
                let params: Vec<Type> = self
                    .facts
                    .fn_sigs
                    .get(&item)
                    .map(|sig| {
                        sig.params
                            .iter()
                            .filter(|p| !p.is_receiver)
                            .map(|p| p.ty.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if params.is_empty() {
                    // Signature unknown: do not invent a storing rule.
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.eval_call_arg(arg, params.get(i), false);
                    }
                }
            }
            Some(CallTarget::BuiltinFn { path }) => {
                if last_path_segment(&path) == "drop" {
                    // `drop(x)` is an explicit move (or a no-op copy).
                    for arg in args {
                        self.eval_value(arg);
                    }
                } else if builtin_ctor_stores_argument(&path) {
                    // `Option::Some` / `Result::Ok` / `Result::Err` place their
                    // argument inside an aggregate: a non-static reference
                    // escapes into it (`ownership.md` OWN-6.1, OWN-3.3). A
                    // `&'static str` literal is the documented exception
                    // (OWN-6.2) and stays accepted.
                    for arg in args {
                        self.eval_value(arg);
                        if self.is_nonstatic_ref(arg) {
                            self.report_ref_escape(arg.span, "stockée");
                        }
                    }
                } else {
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                }
            }
            Some(CallTarget::VariantCtor { .. }) => {
                for arg in args {
                    self.eval_value(arg);
                    if self.is_nonstatic_ref(arg) {
                        self.report_ref_escape(arg.span, "stockée");
                    }
                }
            }
            Some(CallTarget::Error) | None => {
                for arg in args {
                    self.eval_call_arg(arg, None, true);
                }
            }
        }
        self.loans.truncate(snapshot);
        self.call_depth = self.call_depth.saturating_sub(1);
        if self.facts.expr_types.get(&expr.id) == Some(&Type::Never) {
            self.diverged = true;
        }
    }

    pub(super) fn eval_method_call(&mut self, expr: &Expr, receiver: &Expr, args: &[Expr]) {
        let snapshot = self.loans.len();
        self.call_depth = self.call_depth.saturating_add(1);
        match self.facts.methods.get(&expr.id).cloned() {
            Some(MethodTarget::User { item }) => {
                let sig = self.facts.fn_sigs.get(&item);
                let receiver_kind = sig.and_then(|s| s.receiver);
                let params: Vec<Type> = sig
                    .map(|s| {
                        s.params
                            .iter()
                            .filter(|p| !p.is_receiver)
                            .map(|p| p.ty.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                match receiver_kind {
                    Some(Receiver::Shared) => self.borrow_receiver(receiver, false),
                    Some(Receiver::Exclusive) => self.borrow_receiver(receiver, true),
                    Some(Receiver::ByValue) => self.eval_value(receiver),
                    None => self.borrow_receiver(receiver, false),
                }
                if params.is_empty() {
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.eval_call_arg(arg, params.get(i), false);
                    }
                }
            }
            Some(MethodTarget::Builtin { path, .. }) => {
                let mutating = builtin_method_is_mutating(&path);
                if builtin_method_consumes_receiver(&path) {
                    // `spec/language/stdlib.md` STD-11.3: `unwrap`, `unwrap_or`,
                    // `ok_or`, `ok` and `err` take `self` **by value** ("Tous
                    // consomment le receveur"). A mobile receiver is therefore
                    // moved (`spec/language/ownership.md` OWN-5.8), and any later
                    // use is `E-OWN-USE-AFTER-MOVE` / `E-OWN-DOUBLE-MOVE`
                    // (OWN-5.1/5.2). A `Copy` receiver (`Option<i32>`,
                    // `Option<&'static str>`) is copied and stays `INIT`
                    // (OWN-2.3).
                    self.eval_value(receiver);
                } else {
                    self.borrow_receiver(receiver, mutating);
                }
                let stores_refs = builtin_method_stores_refs(&path);
                for arg in args {
                    self.eval_call_arg(arg, None, !stores_refs);
                }
            }
            Some(MethodTarget::Error) | None => {
                self.borrow_receiver(receiver, false);
                for arg in args {
                    self.eval_call_arg(arg, None, true);
                }
            }
        }
        self.loans.truncate(snapshot);
        self.call_depth = self.call_depth.saturating_sub(1);
        if self.facts.expr_types.get(&expr.id) == Some(&Type::Never) {
            self.diverged = true;
        }
    }

    pub(super) fn borrow_receiver(&mut self, receiver: &Expr, mutable: bool) {
        if let Some(mut place) = self.eval_place_expr(receiver) {
            self.auto_deref(&mut place);
            self.borrow_place(&place, receiver.span, mutable, receiver.id);
        } else {
            self.eval_value(receiver);
        }
    }

    pub(super) fn eval_call_arg(
        &mut self,
        arg: &Expr,
        param: Option<&Type>,
        allow_ref_temporaries: bool,
    ) {
        let arg_ty = self.expr_type(arg);
        if arg_ty.is_reference() {
            if self.is_static_ref(arg) {
                self.eval_value(arg);
                return;
            }
            if matches!(
                arg.kind,
                ExprKind::Unary {
                    op: UnOp::Ref | UnOp::RefMut,
                    ..
                }
            ) {
                self.eval_value(arg);
                return;
            }
            match param {
                Some(Type::Ref { mutable, .. }) => {
                    // Reborrow of a reference-typed place for the duration of
                    // the call; a reference rvalue (e.g. `s.as_str()`) is a
                    // temporary consumed by the call.
                    if let Some(mut place) = self.eval_place_expr(arg) {
                        self.auto_deref(&mut place);
                        self.borrow_place(&place, arg.span, *mutable, arg.id);
                    } else {
                        self.eval_value(arg);
                    }
                }
                // A known parameter that is not a reference: the callers that
                // pass a parameter type always forbid reference temporaries.
                Some(_) => {
                    self.report_ref_escape(arg.span, "stockée");
                    self.eval_value(arg);
                }
                None => {
                    if !allow_ref_temporaries {
                        self.report_ref_escape(arg.span, "stockée");
                    }
                    self.eval_value(arg);
                }
            }
        } else {
            self.eval_value(arg);
        }
    }

    // ------------------------------------------------------------------
    // Place consumption, reads, writes, borrows
    // ------------------------------------------------------------------

    pub(super) fn consume_place(&mut self, place: &Place, span: Span, node: NodeId) {
        let kind = if self.place_type(place).is_error_or_copy(self.facts) {
            UseKind::Copy
        } else {
            UseKind::Move
        };
        self.record_use(node, kind, place);
        // RFC-OWN-FACTS-2 steps C2/C3: this is the single site where the
        // checker takes the value of a place, so it is the single site that
        // decides "owned position" and "consumed".
        self.own.owned_position.insert(node);
        if kind == UseKind::Move {
            self.own.consumed.insert(node);
        }
        if !self.check_availability(place, span, Access::Move) {
            return;
        }
        if self.place_type(place).is_error_or_copy(self.facts) {
            self.mark_used(place.slot);
            return;
        }
        self.perform_move(place, span);
    }

    /// The value of `expr` — a projection of a temporary (`make()[i]`,
    /// `pair().0`), which is no place — is taken. There is no ownership state
    /// to update, but the decision is the same one `consume_place` records, and
    /// the backend needs it just as much (RFC-OWN-FACTS-2 steps C2/C3).
    pub(super) fn consume_temporary(&mut self, expr: &Expr) {
        let kind = if self.expr_type(expr).is_error_or_copy(self.facts) {
            UseKind::Copy
        } else {
            UseKind::Move
        };
        self.own.use_kind.insert(expr.id, kind);
        self.own.owned_position.insert(expr.id);
        if kind == UseKind::Move {
            self.own.consumed.insert(expr.id);
        }
    }

    pub(super) fn read_place(&mut self, place: &Place, span: Span, node: NodeId) {
        let kind = if self.place_type(place).is_error_or_copy(self.facts) {
            UseKind::Copy
        } else {
            UseKind::Borrow
        };
        self.record_use(node, kind, place);
        if self.check_availability(place, span, Access::Read) {
            self.mark_used(place.slot);
        }
    }

    pub(super) fn write_place(&mut self, place: &Place, span: Span, node: NodeId) {
        self.record_place(node, place);
        if !self.check_availability(place, span, Access::Write) {
            return;
        }
        match self.element_write(place) {
            ElementWrite::Plain => {}
            ElementWrite::Reinit => {
                self.own.element_reinit.insert(node);
            }
            ElementWrite::Blocked(moved_span) => {
                self.report_element_reinit(place, span, moved_span);
                return;
            }
        }
        if place.steps.is_empty() {
            let assign_once = self
                .slot_info(place.slot)
                .is_some_and(|info| info.assign_once);
            let assigned_before = self
                .state
                .slots
                .get(&place.slot)
                .is_some_and(|st| st.maybe_assigned);
            if assign_once && assigned_before {
                let name = self.display_place(place);
                self.emit_diag(
                    E_IMMUTABLE,
                    span,
                    format!("seconde affectation de la liaison immuable `{name}`"),
                    vec![(
                        "une liaison sans `mut` reçoit sa valeur une seule fois".to_string(),
                        None,
                    )],
                );
            }
            if let Some(st) = self.state.slots.get_mut(&place.slot) {
                st.maybe_assigned = true;
                st.init = true;
                st.moved = false;
                st.maybe_moved = false;
                st.move_span = None;
                st.used_since = true;
                st.partial.clear();
            }
            return;
        }
        if let Some(st) = self.state.slots.get_mut(&place.slot) {
            // Writing a projection: an exact re-assignment of a partially moved
            // projection restores it; writing an ancestor restores all its
            // sub-projections.
            let keys: Vec<Vec<Step>> = st.partial.keys().cloned().collect();
            for key in keys {
                if steps_start_with(&key, &place.steps) {
                    st.partial.remove(&key);
                }
            }
            st.used_since = true;
        }
    }

    pub(super) fn borrow_place(
        &mut self,
        place: &Place,
        span: Span,
        mutable: bool,
        node: NodeId,
    ) -> bool {
        self.record_use(
            node,
            if mutable {
                UseKind::BorrowMut
            } else {
                UseKind::Borrow
            },
            place,
        );
        let access = if mutable {
            Access::BorrowExclusive
        } else {
            Access::BorrowShared
        };
        if !self.check_availability(place, span, access) {
            return false;
        }
        self.mark_used(place.slot);
        self.loans.push(Loan {
            place: place.clone(),
            mutable,
            span,
        });
        true
    }

    /// Classify a write to a projection that sits under an array index
    /// (OWN-5.4).
    ///
    /// Arrays are lowered to Rust arrays, where a moved element is only
    /// re-initialised by assigning the whole array. A write that restores a
    /// moved place under an index is therefore accepted only when the backend
    /// can rebuild that array around the new value: every index of the target is
    /// a literal and no *other* part of the array has been moved.
    ///
    /// Called after `check_availability` accepted the write, so no moved record
    /// is an ancestor of the target here.
    pub(super) fn element_write(&self, place: &Place) -> ElementWrite {
        let Some(first_index) = place
            .steps
            .iter()
            .position(|step| matches!(step, Step::Index(_)))
        else {
            return ElementWrite::Plain;
        };
        let Some(st) = self.state.slots.get(&place.slot) else {
            return ElementWrite::Plain;
        };
        let array = &place.steps[..first_index];
        let mut restores: Option<Span> = None;
        let mut sibling: Option<Span> = None;
        for (moved, rec) in &st.partial {
            match classify_partial(&place.steps, moved) {
                PartialRel::Exact | PartialRel::Ancestor => {
                    restores.get_or_insert(rec.span);
                }
                PartialRel::Disjoint => {
                    if moved.len() > array.len() && prefix_compatible(array, moved) {
                        sibling.get_or_insert(rec.span);
                    }
                }
                PartialRel::Extension => {}
            }
        }
        let Some(restored_span) = restores else {
            return ElementWrite::Plain;
        };
        let opaque_index = place.steps[first_index..]
            .iter()
            .any(|step| matches!(step, Step::Index(IndexKey::Opaque)));
        if opaque_index {
            return ElementWrite::Blocked(restored_span);
        }
        match sibling {
            Some(span) => ElementWrite::Blocked(span),
            None => ElementWrite::Reinit,
        }
    }

    /// Record a value use for the backend (RFC-OWN-FACTS-1, step A).
    pub(super) fn record_use(&mut self, node: NodeId, kind: UseKind, place: &Place) {
        self.own.use_kind.insert(node, kind);
        self.own.place.insert(node, self.own_place(place));
    }

    /// Record a place without a value-use kind (writes).
    pub(super) fn record_place(&mut self, node: NodeId, place: &Place) {
        self.own.place.insert(node, self.own_place(place));
    }

    pub(super) fn own_place(&self, place: &Place) -> OwnPlace {
        let binding = self
            .slot_info(place.slot)
            .map(|info| info.decl_span)
            .unwrap_or_else(Span::dummy);
        let steps = place.steps.iter().map(projection).collect();
        OwnPlace { binding, steps }
    }

    pub(super) fn mark_used(&mut self, slot: SlotId) {
        if let Some(st) = self.state.slots.get_mut(&slot) {
            st.used_since = true;
            for rec in st.partial.values_mut() {
                rec.used_since = true;
            }
        }
    }

    pub(super) fn check_availability(&mut self, place: &Place, span: Span, access: Access) -> bool {
        let Some(st) = self.state.slots.get(&place.slot).cloned() else {
            // Unknown binding: a broken program; do not invent an error.
            return false;
        };

        let whole_reinit = access == Access::Write && place.steps.is_empty();

        if (st.moved || st.maybe_moved) && !whole_reinit {
            let is_effective_move = access == Access::Move && self.call_depth == 0;
            self.report_use_after_move(place, span, &st, is_effective_move);
            return false;
        }

        if !st.partial.is_empty() && !whole_reinit {
            let mut relation = PartialRel::Disjoint;
            let mut record: Option<PartialMove> = None;
            for (moved, rec) in &st.partial {
                match classify_partial(&place.steps, moved) {
                    PartialRel::Exact => {
                        relation = PartialRel::Exact;
                        record = Some(rec.clone());
                        break;
                    }
                    // `Exact` leaves the loop at once, so it is never the
                    // relation seen here.
                    PartialRel::Extension => {
                        relation = PartialRel::Extension;
                        record = Some(rec.clone());
                    }
                    PartialRel::Ancestor => {
                        if relation == PartialRel::Disjoint {
                            relation = PartialRel::Ancestor;
                            record = Some(rec.clone());
                        }
                    }
                    PartialRel::Disjoint => {}
                }
            }
            // Design corrigé : un champ peut être déplacé ; l'agrégat entier
            // reste utilisable uniquement s'il n'a subi aucun déplacement
            // partiel. Un autre champ reste utilisable.
            let allowed = match access {
                Access::Write => matches!(
                    relation,
                    PartialRel::Exact | PartialRel::Ancestor | PartialRel::Disjoint
                ),
                _ => relation == PartialRel::Disjoint,
            };
            if !allowed {
                match (relation, record) {
                    (PartialRel::Exact | PartialRel::Extension, Some(rec)) => {
                        let is_effective_move = access == Access::Move && self.call_depth == 0;
                        self.report_partial_use_after_move(place, span, &rec, is_effective_move);
                    }
                    (_, rec) => {
                        self.report_partial_move(place, span, rec.map(|r| r.span));
                    }
                }
                return false;
            }
        }

        let uninitialised = !st.init && !st.moved && !st.maybe_moved;
        if uninitialised && !whole_reinit {
            self.report_uninit(place, span);
            return false;
        }

        // Loans: exclusive loan forbids every use; shared loans forbid writes,
        // moves and exclusive borrows.
        let mut conflict: Option<(bool, Span)> = None;
        for loan in &self.loans {
            if places_overlap(&loan.place, place) {
                let bad = match access {
                    Access::Read | Access::BorrowShared => loan.mutable,
                    Access::Write | Access::Move | Access::BorrowExclusive => true,
                };
                if bad {
                    conflict = Some((loan.mutable, loan.span));
                    break;
                }
            }
        }
        if let Some((loan_mutable, loan_span)) = conflict {
            self.report_borrow_conflict(place, span, access, loan_mutable, loan_span);
            return false;
        }

        true
    }

    pub(super) fn perform_move(&mut self, place: &Place, span: Span) {
        if place.steps.is_empty() {
            if let Some(st) = self.state.slots.get_mut(&place.slot) {
                st.init = false;
                st.moved = true;
                st.maybe_moved = true;
                st.move_span = Some(span);
                st.used_since = false;
                st.partial.clear();
            }
            return;
        }

        let last_index = place.steps.len() - 1;
        let last = place.steps[last_index].clone();
        // Moving anything through a reference borrows memory this function does
        // not own, even when only a field/index is moved.
        if place.steps[..last_index]
            .iter()
            .any(|step| matches!(step, Step::Deref))
        {
            self.report_move_borrowed(span);
            return;
        }
        let before = self.type_at(place, last_index);
        match last {
            Step::Deref => {
                self.report_move_borrowed(span);
            }
            Step::Field(_) => {
                // Un champ de struct peut être déplacé et suivi individuellement
                // (design corrigé) ; l'usage de l'agrégat entier sera refusé.
                self.record_partial_move(place, span);
            }
            Step::Tuple(_) => {
                self.record_partial_move(place, span);
            }
            Step::Index(_) => match before {
                Type::Vec(_) => {
                    self.report_move_indexed(span);
                }
                Type::Slice(_) => {
                    self.report_move_borrowed(span);
                }
                _ => {
                    // Fixed-size array (or unknown): partial move.
                    self.record_partial_move(place, span);
                }
            },
        }
    }

    pub(super) fn record_partial_move(&mut self, place: &Place, span: Span) {
        if let Some(st) = self.state.slots.get_mut(&place.slot) {
            st.partial.insert(
                place.steps.clone(),
                PartialMove {
                    span,
                    used_since: false,
                },
            );
            st.used_since = false;
        }
    }

    // ------------------------------------------------------------------
    // Types and Copy classification
    // ------------------------------------------------------------------

    pub(super) fn place_type(&self, place: &Place) -> Type {
        self.type_at(place, place.steps.len())
    }

    /// Type obtained after applying the first `count` projection steps.
    pub(super) fn type_at(&self, place: &Place, count: usize) -> Type {
        let Some(info) = self.slot_info(place.slot) else {
            return Type::Error;
        };
        let mut ty = info.ty.clone();
        for step in place.steps.iter().take(count) {
            ty = match step {
                Step::Deref => match ty {
                    Type::Ref { inner, .. } => *inner,
                    _ => Type::Error,
                },
                Step::Field(name) => field_type(&ty, name, self.facts),
                Step::Tuple(index) => match ty {
                    Type::Tuple(items) => {
                        items.get(*index as usize).cloned().unwrap_or(Type::Error)
                    }
                    _ => Type::Error,
                },
                Step::Index(_) => match ty {
                    Type::Array { elem, .. } => *elem,
                    Type::Vec(elem) => *elem,
                    Type::Slice(elem) => *elem,
                    _ => Type::Error,
                },
            };
        }
        ty
    }

    // ------------------------------------------------------------------
    // Static `&str` provenance
    // ------------------------------------------------------------------

    pub(super) fn is_static_ref(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Str(_) => true,
            // `&e` always borrows a *place* (a variable, a temporary, or the
            // temporary holding a `&'static str` value). It never produces a
            // `'static` reference itself: `let r = &"a";` has type `&&str` and
            // stores a reference to a temporary, which must be rejected like any
            // other escaping borrow (F2 audit finding).
            ExprKind::Unary { op: UnOp::Ref, .. } => false,
            ExprKind::Path(path) => self
                .path_local_slot(path)
                .and_then(|slot| self.slot_info(slot))
                .is_some_and(|info| info.static_ref),
            // `self` is a parameter: never a `'static` reference.
            ExprKind::SelfValue => false,
            ExprKind::Block(block) | ExprKind::Unsafe(block) => block
                .tail
                .as_ref()
                .map(|tail| self.is_static_ref(tail))
                .unwrap_or(false),
            // A `&str` produced by `if`/`match` is static only when every
            // branch is itself static; this removes the F1 false positives
            // without weakening the non-escape rule.
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                then_block
                    .tail
                    .as_ref()
                    .map(|tail| self.is_static_ref(tail))
                    .unwrap_or(false)
                    && else_branch
                        .as_ref()
                        .map(|branch| self.is_static_ref(branch))
                        .unwrap_or(false)
            }
            ExprKind::Match { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|arm| self.is_static_ref(&arm.body))
            }
            // Interprocedural provenance (audit finding F1): a call to a user
            // function classified as always returning `&'static str` is static.
            // A builtin (notably `String::as_str`) is never accepted.
            ExprKind::Call { .. } => matches!(
                self.facts.calls.get(&expr.id),
                Some(CallTarget::UserFn { item }) if self.facts.static_str_fns.contains(item)
            ),
            // D13 rule 1 — projection preserves provenance. A `&str` read out of
            // an aggregate (`a[i]`, `s.champ`, `t.0`) is static *by
            // construction*: OWN-6.1/3.3 forbid storing a borrowed `&str` inside
            // an array, a tuple, a field, a `Vec` or an `Option`/`Result`, and
            // every storage site is checked (array/tuple/struct literals,
            // `Some`/`Ok`/`Err`, `push`/`insert`, assignment, `let`, `return`).
            ExprKind::Field { .. } | ExprKind::TupleIndex { .. } | ExprKind::Index { .. } => {
                is_borrowed_text(&self.expr_type(expr))
            }
            // D13 rule 1 — the payload of an `Option`/`Result` is read out of an
            // aggregate (static by rule 2); `str::as_str` is the identity on a
            // text reference. `String::as_str` stays refused: it borrows a local
            // `String` (OWN-6.3).
            ExprKind::MethodCall { receiver, args, .. } => {
                match self.facts.methods.get(&expr.id) {
                    Some(MethodTarget::Builtin { path, .. }) if path.as_str() == "str::as_str" => {
                        self.is_static_ref(receiver)
                    }
                    Some(MethodTarget::Builtin { path, .. })
                        if is_borrowed_text(&self.expr_type(expr)) =>
                    {
                        match path.as_str() {
                            "Option::unwrap" | "Result::unwrap" => true,
                            "Option::unwrap_or" | "Result::unwrap_or" => args
                                .first()
                                .map(|default| self.is_static_ref(default))
                                .unwrap_or(false),
                            _ => false,
                        }
                    }
                    // A user method obeys the same verified contract as a user
                    // function (its body proves every return static).
                    Some(MethodTarget::User { item }) => self.facts.static_str_fns.contains(item),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    pub(super) fn is_nonstatic_ref(&self, expr: &Expr) -> bool {
        self.expr_type(expr).is_reference() && !self.is_static_ref(expr)
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub(super) fn emit_diag(
        &mut self,
        code: &str,
        span: Span,
        message: String,
        notes: Vec<(String, Option<Span>)>,
    ) {
        if !self.emit || self.budget_hit {
            return;
        }
        self.emitted = self.emitted.saturating_add(1);
        if self.emitted > MAX_DIAGS_PER_FN {
            self.budget_hit = true;
            self.diags.push(Diagnostic::error(
                E_LIMIT,
                span,
                "trop d'erreurs d'emprunt dans cette fonction ; analyse interrompue",
            ));
            return;
        }
        let mut diag = Diagnostic::error(code, span, message);
        for (note, note_span) in notes {
            diag = diag.with_note(note, note_span);
        }
        self.diags.push(diag);
    }

    pub(super) fn report_uninit(&mut self, place: &Place, span: Span) {
        let name = self.display_place(place);
        let decl = self.slot_info(place.slot).map(|s| s.decl_span);
        self.emit_diag(
            E_UNINIT,
            span,
            format!("utilisation de `{name}` avant son initialisation"),
            vec![("déclarée ici, sans valeur initiale".to_string(), decl)],
        );
    }

    pub(super) fn report_use_after_move(
        &mut self,
        place: &Place,
        span: Span,
        st: &SlotState,
        is_move: bool,
    ) {
        let name = self.display_place(place);
        if is_move && st.moved && !st.used_since {
            let first = st.move_span;
            self.emit_diag(
                E_DOUBLE_MOVE,
                span,
                format!("déplacement multiple de `{name}`"),
                vec![("premier déplacement effectué ici".to_string(), first)],
            );
        } else {
            let first = st.move_span;
            let note = if st.maybe_moved && !st.moved {
                "déplacement possible sur un autre chemin (analyse conservatrice)"
            } else {
                "déplacement effectué ici"
            };
            self.emit_diag(
                E_USE_AFTER_MOVE,
                span,
                format!("utilisation de `{name}` après son déplacement"),
                vec![(note.to_string(), first)],
            );
        }
    }

    pub(super) fn report_partial_use_after_move(
        &mut self,
        place: &Place,
        span: Span,
        rec: &PartialMove,
        is_move: bool,
    ) {
        let name = self.display_place(place);
        if is_move && !rec.used_since {
            self.emit_diag(
                E_DOUBLE_MOVE,
                span,
                format!("déplacement multiple de `{name}`"),
                vec![(
                    "déplacement partiel effectué ici".to_string(),
                    Some(rec.span),
                )],
            );
        } else {
            self.emit_diag(
                E_USE_AFTER_MOVE,
                span,
                format!("utilisation de `{name}` après son déplacement"),
                vec![(
                    "déplacement partiel effectué ici".to_string(),
                    Some(rec.span),
                )],
            );
        }
    }

    pub(super) fn report_partial_move(
        &mut self,
        place: &Place,
        span: Span,
        moved_span: Option<Span>,
    ) {
        let name = self.display_place(place);
        self.emit_diag(
            E_PARTIAL_MOVE,
            span,
            format!("utilisation de l'agrégat `{name}` après un déplacement partiel"),
            vec![(
                "un champ ou un élément a déjà été déplacé".to_string(),
                moved_span,
            )],
        );
    }

    pub(super) fn report_element_reinit(&mut self, place: &Place, span: Span, moved_span: Span) {
        let name = self.display_place(place);
        self.emit_diag(
            E_PARTIAL_MOVE,
            span,
            format!(
                "ré-initialisation de `{name}` impossible : le tableau a subi un autre                  déplacement partiel"
            ),
            vec![
                ("déplacement partiel effectué ici".to_string(), Some(moved_span)),
                (
                    "ré-assigner le tableau entier pour le ré-initialiser".to_string(),
                    None,
                ),
            ],
        );
    }

    pub(super) fn report_move_indexed(&mut self, span: Span) {
        self.emit_diag(
            E_MOVE_INDEXED,
            span,
            "déplacement d'un élément de `Vec` par indexation interdit".to_string(),
            vec![(
                "utiliser `.pop()`, un motif ou une méthode sur `&mut self`".to_string(),
                None,
            )],
        );
    }

    pub(super) fn report_move_borrowed(&mut self, span: Span) {
        self.emit_diag(
            E_MOVE_BORROWED,
            span,
            "déplacement d'une valeur à travers une référence interdit".to_string(),
            vec![(
                "la valeur empruntée n'est pas possédée par cette fonction".to_string(),
                None,
            )],
        );
    }

    pub(super) fn report_borrow_conflict(
        &mut self,
        place: &Place,
        span: Span,
        access: Access,
        loan_mutable: bool,
        loan_span: Span,
    ) {
        let name = self.display_place(place);
        let message = match access {
            Access::Move => format!("déplacement de `{name}` pendant qu'il est emprunté"),
            Access::Read => format!("lecture de `{name}` pendant un emprunt exclusif"),
            Access::Write => format!("écriture de `{name}` pendant qu'il est emprunté"),
            Access::BorrowExclusive => {
                format!("emprunt exclusif de `{name}` en conflit avec un prêt actif")
            }
            Access::BorrowShared => {
                format!("emprunt partagé de `{name}` en conflit avec un emprunt exclusif")
            }
        };
        let kind = if loan_mutable {
            "un prêt exclusif est déjà actif ici"
        } else {
            "un prêt partagé est déjà actif ici"
        };
        self.emit_diag(
            E_BORROW_CONFLICT,
            span,
            message,
            vec![(kind.to_string(), Some(loan_span))],
        );
    }

    pub(super) fn report_ref_escape(&mut self, span: Span, action: &str) {
        self.emit_diag(
            E_REF_ESCAPE,
            span,
            format!("une référence ne peut pas être {action} en Core 1.0"),
            vec![(
                "une référence ne vit que le temps de l'instruction ; seule `&'static str` est stockable"
                    .to_string(),
                None,
            )],
        );
    }

    pub(super) fn display_place(&self, place: &Place) -> String {
        let Some(info) = self.slot_info(place.slot) else {
            return "<inconnu>".to_string();
        };
        let mut out = String::new();
        let mut derefs = 0;
        for step in &place.steps {
            if matches!(step, Step::Deref) {
                derefs += 1;
            }
        }
        for _ in 0..derefs {
            out.push('*');
        }
        out.push_str(&info.name);
        for step in &place.steps {
            match step {
                Step::Deref => {}
                Step::Field(name) => {
                    out.push('.');
                    out.push_str(name);
                }
                Step::Tuple(index) => {
                    out.push('.');
                    out.push_str(&index.to_string());
                }
                Step::Index(IndexKey::Lit(value)) => {
                    out.push('[');
                    out.push_str(&value.to_string());
                    out.push(']');
                }
                Step::Index(IndexKey::Opaque) => out.push_str("[_]"),
            }
        }
        out
    }
}
