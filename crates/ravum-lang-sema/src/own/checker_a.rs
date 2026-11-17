use super::*;

impl<'a> OwnChecker<'a> {
    pub(super) fn new(
        facts: &'a Facts,
        diags: &'a mut DiagnosticBag,
        own: &'a mut OwnFacts,
    ) -> Self {
        OwnChecker {
            facts,
            diags,
            slots: BTreeMap::new(),
            scopes: Vec::new(),
            state: State::default(),
            loans: Vec::new(),
            loan_floor: 0,
            next_slot: 0,
            emit: true,
            depth: 0,
            diverged: false,
            loop_stack: Vec::new(),
            emitted: 0,
            budget_hit: false,
            too_deep: false,
            call_depth: 0,
            own,
        }
    }

    // ------------------------------------------------------------------
    // Function entry
    // ------------------------------------------------------------------

    pub(super) fn check_fn(&mut self, item: &Item, f: &FnItem) {
        self.state = State::default();
        self.slots.clear();
        self.scopes.clear();
        self.loans.truncate(self.loan_floor);
        self.next_slot = 0;
        self.emit = true;
        self.depth = 0;
        self.diverged = false;
        self.loop_stack.clear();
        self.emitted = 0;
        self.budget_hit = false;
        self.too_deep = false;
        self.call_depth = 0;

        self.push_scope();
        let mut params: Vec<(String, Type)> = Vec::new();
        if let Some(sig) = self.facts.fn_sigs.get(&item.id) {
            for p in &sig.params {
                let name = if p.is_receiver {
                    "self".to_string()
                } else {
                    p.name.clone()
                };
                params.push((name, p.ty.clone()));
            }
        } else {
            // Fact table missing (checker not finished): fall back on the AST.
            for p in &f.params {
                match &p.kind {
                    ParamKind::Binding { name, ty, .. } => {
                        let ty = self
                            .facts
                            .type_nodes
                            .get(&ty.id)
                            .cloned()
                            .unwrap_or(Type::Error);
                        params.push((name.name.clone(), ty));
                    }
                    ParamKind::Receiver(r) => {
                        let ty = match r {
                            Receiver::ByValue => Type::Error,
                            Receiver::Shared => Type::Ref {
                                mutable: false,
                                inner: Box::new(Type::Error),
                            },
                            Receiver::Exclusive => Type::Ref {
                                mutable: true,
                                inner: Box::new(Type::Error),
                            },
                        };
                        params.push(("self".to_string(), ty));
                    }
                }
            }
        }
        for (name, ty) in params {
            let slot = self.declare_slot(&name, ty, false, item.span);
            self.state.slots.insert(slot, SlotState::initialised());
        }

        self.analyze_block(&f.body, true);

        self.pop_scope();
        self.loans.truncate(self.loan_floor);
    }

    // ------------------------------------------------------------------
    // Scopes and slots
    // ------------------------------------------------------------------

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub(super) fn pop_scope(&mut self) {
        if let Some(vars) = self.scopes.pop() {
            for (_, slot) in vars {
                self.state.slots.remove(&slot);
                self.slots.remove(&slot);
            }
        }
    }

    pub(super) fn declare_slot(
        &mut self,
        name: &str,
        ty: Type,
        static_ref: bool,
        span: Span,
    ) -> SlotId {
        let id = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        let binding = self.own.intern_binding(span, name);
        self.slots.insert(
            id,
            SlotInfo {
                name: name.to_string(),
                ty,
                static_ref,
                decl_span: span,
                binding,
            },
        );
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_string(), id));
        }
        id
    }

    pub(super) fn lookup(&self, name: &str) -> Option<SlotId> {
        for scope in self.scopes.iter().rev() {
            for (n, slot) in scope.iter().rev() {
                if n == name {
                    return Some(*slot);
                }
            }
        }
        None
    }

    pub(super) fn slot_info(&self, slot: SlotId) -> Option<&SlotInfo> {
        self.slots.get(&slot)
    }

    pub(super) fn expr_type(&self, expr: &Expr) -> Type {
        self.facts
            .expr_types
            .get(&expr.id)
            .cloned()
            .unwrap_or(Type::Error)
    }

    pub(super) fn path_local_slot(&self, expr: &Expr) -> Option<SlotId> {
        if let Some(Resolution::Local { name, .. }) = self.facts.resolutions.get(&expr.id) {
            return self.lookup(name);
        }
        if let ExprKind::Path(path) = &expr.kind {
            if path.segments.len() == 1 {
                if let Some(seg) = path.segments.first() {
                    return self.lookup(&seg.name.name);
                }
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // Block / statement / CFG
    // ------------------------------------------------------------------

    pub(super) fn enter(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.too_deep = true;
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(super) fn analyze_block(&mut self, block: &Block, fn_body: bool) {
        if !self.enter() {
            return;
        }
        self.push_scope();
        self.loans.truncate(self.loan_floor);
        let mut reached = 0;
        for stmt in &block.stmts {
            if self.diverged {
                break;
            }
            self.analyze_stmt(stmt);
            reached += 1;
        }
        if !self.diverged {
            if let Some(tail) = &block.tail {
                self.loans.truncate(self.loan_floor);
                self.eval_value(tail);
                if fn_body && self.is_nonstatic_ref(tail) {
                    self.report_ref_escape(tail.span, "retournée");
                }
                self.loans.truncate(self.loan_floor);
            }
        } else {
            // The rest of the block is unreachable, but the backend still emits
            // it: its ownership decisions are recorded like any other.
            self.analyze_dead(|checker| {
                for stmt in &block.stmts[reached..] {
                    checker.diverged = false;
                    checker.analyze_stmt(stmt);
                }
                if let Some(tail) = &block.tail {
                    checker.diverged = false;
                    checker.loans.truncate(checker.loan_floor);
                    checker.eval_value(tail);
                }
            });
        }
        self.pop_scope();
        self.leave();
    }

    /// Record the ownership decisions of unreachable code (RFC-OWN-FACTS-2).
    ///
    /// Code after a `return`, `break` or `continue` has no effect on the
    /// ownership state and reports no diagnostic, but the backend lowers it like
    /// any other code and reads the same facts. `analyze` therefore runs
    /// silently on a scratch copy of the state, and the checker comes back
    /// exactly as it was, still diverged.
    pub(super) fn analyze_dead(&mut self, analyze: impl FnOnce(&mut Self)) {
        let state = self.state.clone();
        let loans = self.loans.clone();
        let loop_stack = self.loop_stack.clone();
        let emit = self.emit;
        self.emit = false;
        self.diverged = false;
        analyze(self);
        self.state = state;
        self.loans = loans;
        self.loop_stack = loop_stack;
        self.emit = emit;
        self.diverged = true;
    }

    pub(super) fn analyze_stmt(&mut self, stmt: &Stmt) {
        if !self.enter() {
            return;
        }
        self.loans.truncate(self.loan_floor);
        match &stmt.kind {
            StmtKind::Let(l) => self.check_let(l, stmt.span),
            StmtKind::Expr { expr, .. } => self.eval_value(expr),
        }
        self.loans.truncate(self.loan_floor);
        self.leave();
    }

    pub(super) fn check_let(&mut self, l: &ravum_lang_syntax::ast::LetStmt, stmt_span: Span) {
        let mut init_static = false;
        let has_init = l.init.is_some();
        if let Some(init) = &l.init {
            self.eval_value(init);
            init_static = self.is_static_ref(init);
        }

        if let PatKind::Binding { name, .. } = &l.pat.kind {
            let ty = self
                .facts
                .pat_types
                .get(&l.pat.id)
                .cloned()
                .or_else(|| l.init.as_ref().map(|e| self.expr_type(e)))
                .unwrap_or(Type::Error);
            let static_ref = ty.is_reference() && init_static;
            if ty.is_reference() && !static_ref {
                self.report_ref_escape(stmt_span, "stockée");
            }
            let slot = self.declare_slot(&name.name, ty, static_ref, l.pat.span);
            let st = if has_init {
                SlotState::initialised()
            } else {
                SlotState::default()
            };
            self.state.slots.insert(slot, st);
            return;
        }

        // Destructuring pattern: the initialiser was consumed as a whole.
        let mut bindings: Vec<PatBinding> = Vec::new();
        self.collect_pat_bindings(&l.pat, false, &mut bindings);
        for b in bindings {
            // A `&str` extracted from a destructured aggregate is static by
            // construction (D13 rule 1 + rule 2); every other reference keeps
            // the Core 1.0 refusal.
            let static_ref = b.nested && is_borrowed_text(&b.ty);
            if b.ty.is_reference() && !static_ref {
                self.report_ref_escape(b.span, "stockée");
            }
            let slot = self.declare_slot(&b.name, b.ty, static_ref, b.span);
            let st = if has_init {
                SlotState::initialised()
            } else {
                SlotState::default()
            };
            self.state.slots.insert(slot, st);
        }
    }

    pub(super) fn collect_pat_bindings(&self, pat: &Pat, nested: bool, out: &mut Vec<PatBinding>) {
        match &pat.kind {
            PatKind::Binding { name, .. } => {
                let ty = self
                    .facts
                    .pat_types
                    .get(&pat.id)
                    .cloned()
                    .unwrap_or(Type::Error);
                out.push(PatBinding {
                    name: name.name.clone(),
                    ty,
                    span: pat.span,
                    nested,
                });
            }
            PatKind::Tuple(items) => {
                for item in items {
                    // Every component of a destructured value is read *out* of
                    // an aggregate (D13 rule 1, projection).
                    self.collect_pat_bindings(item, true, out);
                }
            }
            PatKind::Variant { fields, .. } => match fields {
                ravum_lang_syntax::ast::VariantPatFields::Tuple(items) => {
                    for item in items {
                        self.collect_pat_bindings(item, true, out);
                    }
                }
                ravum_lang_syntax::ast::VariantPatFields::Struct(fields) => {
                    for f in fields {
                        self.collect_pat_bindings(&f.pat, true, out);
                    }
                }
            },
            PatKind::Wild | PatKind::Lit(_) | PatKind::Path(_) => {}
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    /// Analyse a construct that owns statements (block, branch, loop). The
    /// loans active on entry belong to the enclosing expression and stay in
    /// force inside; the construct only ends the temporaries it creates.
    fn nested(&mut self, analyse: impl FnOnce(&mut Self)) {
        let outer = self.loan_floor;
        self.loan_floor = self.loans.len();
        analyse(self);
        self.loans.truncate(self.loan_floor);
        self.loan_floor = outer;
    }

    pub(super) fn eval_value(&mut self, expr: &Expr) {
        if !self.enter() {
            return;
        }
        match &expr.kind {
            ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_) => {}
            ExprKind::SelfValue | ExprKind::Path(_) => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span, expr.id);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span, expr.id);
                } else {
                    let base_ty = self.expr_type(base);
                    self.eval_value(base);
                    self.consume_temporary(expr);
                    if base_ty.is_reference() && !self.expr_type(expr).is_error_or_copy(self.facts)
                    {
                        // Moving a field/index out of a borrowed temporary.
                        self.report_move_borrowed(expr.span);
                    }
                }
            }
            ExprKind::Index { base, index } => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span, expr.id);
                } else {
                    let base_ty = self.expr_type(base);
                    self.eval_value(base);
                    self.eval_value(index);
                    self.consume_temporary(expr);
                    if base_ty.is_reference() {
                        if !self.expr_type(expr).is_error_or_copy(self.facts) {
                            self.report_move_borrowed(expr.span);
                        }
                    } else if matches!(base_ty, Type::Vec(_))
                        && !self.expr_type(expr).is_error_or_copy(self.facts)
                    {
                        self.report_move_indexed(expr.span);
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => match op {
                UnOp::Ref => self.eval_borrow(inner, false, expr.span),
                UnOp::RefMut => self.eval_borrow(inner, true, expr.span),
                UnOp::Deref => {
                    if let Some(place) = self.eval_place_expr(expr) {
                        self.consume_place(&place, expr.span, expr.id);
                    } else {
                        let inner_ty = self.expr_type(inner);
                        self.eval_value(inner);
                        self.consume_temporary(expr);
                        if inner_ty.is_reference()
                            && !self.expr_type(expr).is_error_or_copy(self.facts)
                        {
                            self.report_move_borrowed(expr.span);
                        }
                    }
                }
                UnOp::Neg | UnOp::Not => self.eval_value(inner),
            },
            ExprKind::Binary { op, lhs, rhs } => {
                self.eval_value(lhs);
                if matches!(op, BinOp::And | BinOp::Or) {
                    let after_lhs = self.state.clone();
                    let lhs_diverged = self.diverged;
                    self.eval_value(rhs);
                    if self.diverged {
                        self.state = after_lhs;
                        self.diverged = lhs_diverged;
                    } else if !lhs_diverged {
                        self.state = join(&after_lhs, &self.state);
                    }
                } else {
                    self.eval_value(rhs);
                }
            }
            ExprKind::Assign { target, value } => {
                self.eval_value(value);
                let target_ty = self.expr_type(target);
                if target_ty.is_reference() && self.is_nonstatic_ref(value) {
                    self.report_ref_escape(value.span, "stockée");
                }
                if let Some(place) = self.eval_place_expr(target) {
                    self.write_place(&place, target.span, target.id);
                }
            }
            ExprKind::AssignOp { target, value, .. } => {
                let place = self.eval_place_expr(target);
                if let Some(p) = &place {
                    self.read_place(p, target.span, target.id);
                }
                self.eval_value(value);
                if let Some(p) = place {
                    self.write_place(&p, target.span, target.id);
                }
            }
            ExprKind::Cast { expr: inner, .. } => self.eval_value(inner),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.nested(|c| c.eval_if(cond, then_block, else_branch.as_deref())),
            ExprKind::Match { scrutinee, arms } => self.nested(|c| c.eval_match(scrutinee, arms)),
            ExprKind::While { cond, body } => self.nested(|c| c.eval_while(cond, body)),
            ExprKind::Loop { body } => self.nested(|c| c.eval_loop(body)),
            ExprKind::For { pat, iter, body } => self.nested(|c| c.eval_for(pat, iter, body)),
            ExprKind::Block(block) => self.nested(|c| c.analyze_block(block, false)),
            ExprKind::Unsafe(block) => self.nested(|c| c.analyze_block(block, false)),
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    self.eval_value(item);
                    if self.is_nonstatic_ref(item) {
                        self.report_ref_escape(item.span, "stockée");
                    }
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                let ty = self.expr_type(value);
                if !ty.is_error_or_copy(self.facts) {
                    self.emit_diag(
                        E_NOT_COPY,
                        value.span,
                        "répétition d'un tableau avec une valeur mobile".to_string(),
                        vec![(
                            "la valeur serait dupliquée ; elle doit être `Copy`".to_string(),
                            None,
                        )],
                    );
                    if let Some(place) = self.eval_place_expr(value) {
                        self.read_place(&place, value.span, value.id);
                    } else {
                        self.eval_value(value);
                    }
                } else {
                    self.eval_value(value);
                }
                self.eval_value(len);
            }
            ExprKind::StructLit { fields, .. } => {
                for field in fields {
                    self.eval_value(&field.value);
                    if self.is_nonstatic_ref(&field.value) {
                        self.report_ref_escape(field.value.span, "stockée");
                    }
                }
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.eval_value(start);
                }
                if let Some(end) = end {
                    self.eval_value(end);
                }
            }
            ExprKind::Try(inner) => {
                // `?` may leave the function on the error path; the normal path
                // simply continues with the payload.
                self.eval_value(inner);
            }
            ExprKind::Call { callee, args } => {
                if !matches!(callee.kind, ExprKind::Path(_)) {
                    self.eval_value(callee);
                }
                self.eval_call(expr, args);
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let _ = method;
                self.eval_method_call(expr, receiver, args);
            }
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    self.eval_value(value);
                    if self.is_nonstatic_ref(value) {
                        self.report_ref_escape(value.span, "retournée");
                    }
                }
                self.diverged = true;
            }
            ExprKind::Break => {
                let st = self.state.clone();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.breaks.push(st);
                }
                self.diverged = true;
            }
            ExprKind::Continue => {
                let st = self.state.clone();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continues.push(st);
                }
                self.diverged = true;
            }
        }
        self.leave();
    }

    pub(super) fn eval_borrow(&mut self, operand: &Expr, mutable: bool, span: Span) {
        if let Some(place) = self.eval_place_expr(operand) {
            self.borrow_place(&place, span, mutable, operand.id);
        } else {
            // Borrow of a temporary rvalue: allowed, dies with the instruction.
            self.eval_value(operand);
        }
    }

    /// Evaluate an expression as a place without consuming it.
    pub(super) fn eval_place_expr(&mut self, expr: &Expr) -> Option<Place> {
        if !self.enter() {
            return None;
        }
        let result = match &expr.kind {
            ExprKind::SelfValue | ExprKind::Path(_) => {
                let slot = match &expr.kind {
                    ExprKind::SelfValue => self.lookup("self"),
                    _ => self.path_local_slot(expr),
                };
                // RFC-OWN-FACTS-2 step C4: the scope lookup above is the only
                // place where a path becomes a binding; publish its outcome.
                if let Some(info) = slot.and_then(|slot| self.slot_info(slot)) {
                    let binding = info.binding;
                    self.own.binding_identity.insert(expr.id, binding);
                }
                slot.map(|slot| Place {
                    slot,
                    steps: Vec::new(),
                })
            }
            ExprKind::Field { base, name } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.auto_deref(&mut place);
                    place.steps.push(Step::Field(name.name.clone()));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::TupleIndex { base, index } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.auto_deref(&mut place);
                    place.steps.push(Step::Tuple(*index));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::Index { base, index } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.eval_value(index);
                    self.auto_deref(&mut place);
                    let key = match &index.kind {
                        ExprKind::Int { value, .. } => IndexKey::Lit(*value),
                        _ => IndexKey::Opaque,
                    };
                    place.steps.push(Step::Index(key));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr: inner,
            } => {
                if let Some(mut place) = self.eval_place_expr(inner) {
                    place.steps.push(Step::Deref);
                    Some(place)
                } else {
                    None
                }
            }
            _ => None,
        };
        self.leave();
        result
    }

    pub(super) fn auto_deref(&self, place: &mut Place) {
        let mut guard = 0;
        while self.place_type(place).is_reference() && guard < 16 {
            place.steps.push(Step::Deref);
            guard += 1;
        }
    }

    pub(super) fn eval_if(&mut self, cond: &Expr, then_block: &Block, else_branch: Option<&Expr>) {
        self.eval_value(cond);
        // Condition borrows/temporaries end before the branches execute; branch
        // statements are distinct instructions.
        self.loans.truncate(self.loan_floor);
        let after_cond = self.state.clone();
        let cond_diverged = self.diverged;

        let mut states: Vec<State> = Vec::new();
        self.state = after_cond.clone();
        self.diverged = cond_diverged;
        if !self.diverged {
            self.analyze_block(then_block, false);
        } else {
            self.analyze_dead(|checker| checker.analyze_block(then_block, false));
        }
        if !self.diverged {
            states.push(self.state.clone());
        }

        self.state = after_cond.clone();
        self.diverged = cond_diverged;
        if let Some(branch) = else_branch {
            if !self.diverged {
                self.eval_value(branch);
            } else {
                self.analyze_dead(|checker| checker.eval_value(branch));
            }
        }
        if !self.diverged {
            states.push(self.state.clone());
        }

        match join_all(&states) {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.state = after_cond;
                self.diverged = true;
            }
        }
    }

    pub(super) fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) {
        self.eval_value(scrutinee);
        // The scrutinee is consumed; loans it created expire before the arms so
        // that a subsequent `match v.pop() { ... v.push(..) }` stays accepted.
        // Any reference that could escape from the scrutinee would already be
        // rejected as a ref escape.
        self.loans.truncate(self.loan_floor);
        let base = self.state.clone();
        let base_diverged = self.diverged;

        let mut states: Vec<State> = Vec::new();
        for arm in arms {
            self.state = base.clone();
            self.diverged = base_diverged;
            if self.diverged {
                self.analyze_dead(|checker| {
                    checker.push_scope();
                    checker.declare_arm_bindings(arm, scrutinee);
                    checker.eval_value(&arm.body);
                    checker.pop_scope();
                });
                continue;
            }
            self.push_scope();
            let arm_slots = self.declare_arm_bindings(arm, scrutinee);
            self.eval_value(&arm.body);
            if !self.diverged {
                let mut st = self.state.clone();
                for slot in &arm_slots {
                    drop_slot(&mut st, *slot);
                }
                states.push(st);
            }
            self.pop_scope();
        }

        match join_all(&states) {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.state = base;
                self.diverged = true;
            }
        }
    }

    /// Declare the bindings of a `match` arm, initialised, in the current scope.
    fn declare_arm_bindings(&mut self, arm: &MatchArm, scrutinee: &Expr) -> Vec<SlotId> {
        let mut bindings: Vec<PatBinding> = Vec::new();
        self.collect_pat_bindings(&arm.pat, false, &mut bindings);
        let mut arm_slots: Vec<SlotId> = Vec::new();
        for b in bindings {
            // An arm binding of type `&str` either copies the scrutinee
            // (top-level pattern: its own provenance decides) or reads a
            // payload out of it (nested: static by rule 2).
            let static_ref = is_borrowed_text(&b.ty) && (b.nested || self.is_static_ref(scrutinee));
            if b.ty.is_reference() && !static_ref {
                self.report_ref_escape(b.span, "stockée");
            }
            let slot = self.declare_slot(&b.name, b.ty, static_ref, b.span);
            arm_slots.push(slot);
            self.state.slots.insert(slot, SlotState::initialised());
        }
        arm_slots
    }

    pub(super) fn eval_while(&mut self, cond: &Expr, body: &Block) {
        let saved_emit = self.emit;
        let pre = self.state.clone();
        let mut entry = pre.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            self.diverged = false;
            self.loans.truncate(self.loan_floor);
            self.eval_value(cond);
            self.loans.truncate(self.loan_floor);
            let cond_diverged = self.diverged;
            self.loop_stack.push(LoopCtx::default());
            if !cond_diverged {
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
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        // Final pass: emit diagnostics from the stabilised header state.
        self.emit = saved_emit;
        self.state = entry;
        self.diverged = false;
        self.loans.truncate(self.loan_floor);
        self.eval_value(cond);
        self.loans.truncate(self.loan_floor);
        let cond_diverged = self.diverged;
        let after_cond = self.state.clone();
        self.loop_stack.push(LoopCtx::default());
        if !cond_diverged {
            self.analyze_block(body, false);
        } else {
            self.analyze_dead(|checker| checker.analyze_block(body, false));
        }
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // Exit: either the (at least one) condition evaluation fails, or a
        // `break` was taken. The failing-condition state comes from the
        // stabilised header, not from the body exit.
        let mut exit: Option<State> = if cond_diverged {
            None
        } else {
            Some(after_cond)
        };
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        match exit {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }

    pub(super) fn eval_loop(&mut self, body: &Block) {
        let saved_emit = self.emit;
        let pre = self.state.clone();
        let mut entry = pre.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            self.diverged = false;
            self.loans.truncate(self.loan_floor);
            self.loop_stack.push(LoopCtx::default());
            self.analyze_block(body, false);
            let ctx = self.loop_stack.pop().unwrap_or_default();
            let mut next = entry.clone();
            if !self.diverged {
                next = join(&next, &self.state);
            }
            for c in &ctx.continues {
                next = join(&next, c);
            }
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        self.emit = saved_emit;
        self.state = entry;
        self.diverged = false;
        self.loans.truncate(self.loan_floor);
        self.loop_stack.push(LoopCtx::default());
        self.analyze_block(body, false);
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // `loop` always executes its body at least once; the only normal exit is
        // `break`.
        let mut exit: Option<State> = None;
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        match exit {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }
}
