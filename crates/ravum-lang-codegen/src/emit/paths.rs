//! Array-element path planning for the emitters (RFC-OWN-ARRAY-ELEMENTS).
//!
//! Extracted from mit by E4 (P-6) with no behaviour change.

use super::*;

/// How the emitter lowers the element paths of one function body.
#[derive(Debug, Default)]
pub(super) struct ArrayMovePlan {
    /// Outermost expression of an element path -> lowering.
    paths: BTreeMap<NodeId, PathMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathMode {
    /// The element is moved out by value.
    Move,
    /// The element is read or borrowed immutably.
    Shared,
    /// The element is assigned or borrowed mutably.
    Exclusive,
    /// The assignment target re-initialises a moved element.
    Reinit,
}

/// One step of an expression path, with the expression it applies to.
pub(super) enum PathStep<'x> {
    Field { base: &'x Expr, name: &'x str },
    Tuple { base: &'x Expr, index: u32 },
    Index { base: &'x Expr, index: &'x Expr },
}

/// Collect the path from the deepest non-projection/non-index expression down to
/// `expr`, in application order (`a[0].0[1]` -> `[Index(0), Tuple(0), Index(1)]`
/// for the root expression `a`).
pub(super) fn collect_path_steps<'x>(expr: &'x Expr, out: &mut Vec<PathStep<'x>>) -> &'x Expr {
    match &expr.kind {
        ExprKind::Field { base, name } => {
            let root = collect_path_steps(base, out);
            out.push(PathStep::Field {
                base,
                name: name.name.as_str(),
            });
            root
        }
        ExprKind::TupleIndex { base, index } => {
            let root = collect_path_steps(base, out);
            out.push(PathStep::Tuple {
                base,
                index: *index,
            });
            root
        }
        ExprKind::Index { base, index } => {
            let root = collect_path_steps(base, out);
            out.push(PathStep::Index { base, index });
            root
        }
        _ => expr,
    }
}

/// The constant index of an integer literal, or `None` for a runtime index.
pub(super) fn constant_index(expr: &Expr) -> Option<usize> {
    match &expr.kind {
        ExprKind::Int { value, .. } => usize::try_from(*value).ok(),
        _ => None,
    }
}

/// A projection one Rust pattern can express, resolved against the checked types.
#[derive(Debug, Clone)]
pub(super) enum Slot {
    /// Element `at` of an array of `len` elements.
    Index { len: usize, at: usize },
    /// Element `at` of a tuple of `len` elements.
    Tuple { len: usize, at: usize },
    /// Field `name` of the struct at Rust path `path`, whose fields are `fields`.
    Field {
        path: String,
        fields: Vec<String>,
        name: String,
    },
}

/// An element path resolved against the checked types (see the section header).
pub(super) struct ElementPath<'x> {
    /// Deepest expression that is neither a projection nor an index.
    root: &'x Expr,
    /// The fixed-size array `steps[first]` indexes.
    array: &'x Expr,
    steps: Vec<PathStep<'x>>,
    /// Position in `steps` of the first index into a fixed-size array.
    first: usize,
    /// The constant run, starting at `steps[first]`.
    slots: Vec<Slot>,
}

impl<'x> ElementPath<'x> {
    /// The array expression the constant run applies to.
    fn array(&self) -> &'x Expr {
        self.array
    }

    /// The steps after the constant run.
    fn rest(&self) -> &[PathStep<'x>] {
        &self.steps[self.first + self.slots.len()..]
    }
}

/// `_, _, <inner>, ..` selecting position `at` out of `len`.
pub(super) fn positional_pattern(at: usize, len: usize, inner: &str, tuple: bool) -> String {
    let mut parts: Vec<&str> = vec!["_"; at];
    parts.push(inner);
    if at + 1 < len {
        parts.push("..");
    }
    let mut text = parts.join(", ");
    if tuple && len == 1 {
        text.push(',');
    }
    text
}

/// The irrefutable pattern that reaches the end of `slots` and binds `leaf`
/// there; every other position is `_` or `..`, which moves and borrows nothing.
pub(super) fn slot_pattern(slots: &[Slot], leaf: &str) -> String {
    let Some((slot, rest)) = slots.split_first() else {
        return leaf.to_string();
    };
    let inner = slot_pattern(rest, leaf);
    match slot {
        Slot::Index { len, at } => format!("[{}]", positional_pattern(*at, *len, &inner, false)),
        Slot::Tuple { len, at } => format!("({})", positional_pattern(*at, *len, &inner, true)),
        Slot::Field { path, name, .. } => format!("{path} {{ {name}: {inner}, .. }}"),
    }
}

/// The value of the aggregate reached by `slots[..depth]` with the place at the
/// end of `slots` replaced by `fresh`: every sibling is moved out of `array` and
/// put back unchanged.
pub(super) fn rebuild_text(slots: &[Slot], depth: usize, array: &str, fresh: &str) -> String {
    let Some(slot) = slots.get(depth) else {
        return fresh.to_string();
    };
    let keep = |sibling: Slot| {
        let mut path = slots[..depth].to_vec();
        path.push(sibling);
        format!(
            "{{ let {} = {array}; __rv_keep }}",
            slot_pattern(&path, "__rv_keep")
        )
    };
    let replaced = || rebuild_text(slots, depth + 1, array, fresh);
    match slot {
        Slot::Index { len, at } => {
            let items: Vec<String> = (0..*len)
                .map(|position| {
                    if position == *at {
                        replaced()
                    } else {
                        keep(Slot::Index {
                            len: *len,
                            at: position,
                        })
                    }
                })
                .collect();
            format!("[{}]", items.join(", "))
        }
        Slot::Tuple { len, at } => {
            let items: Vec<String> = (0..*len)
                .map(|position| {
                    if position == *at {
                        replaced()
                    } else {
                        keep(Slot::Tuple {
                            len: *len,
                            at: position,
                        })
                    }
                })
                .collect();
            let trailing = if *len == 1 { "," } else { "" };
            format!("({}{trailing})", items.join(", "))
        }
        Slot::Field { path, fields, name } => {
            let items: Vec<String> = fields
                .iter()
                .map(|field| {
                    let value = if field == name {
                        replaced()
                    } else {
                        keep(Slot::Field {
                            path: path.clone(),
                            fields: fields.clone(),
                            name: field.clone(),
                        })
                    };
                    format!("{field}: {value}")
                })
                .collect();
            format!("{path} {{ {} }}", items.join(", "))
        }
    }
}
/// Collect every expression node of a module (step B place cross-check).
pub(super) fn collect_module_exprs<'a>(
    module: &'a Module,
    out: &mut BTreeMap<NodeId, &'a Expr>,
    receivers: &mut BTreeSet<NodeId>,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(function) => collect_block_exprs(&function.body, out, receivers),
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    if let ItemKind::Fn(function) = &member.kind {
                        collect_block_exprs(&function.body, out, receivers);
                    }
                }
            }
            ItemKind::Mod(inner) => collect_module_exprs(inner, out, receivers),
            _ => {}
        }
    }
}

/// Collect the body of every function of a module (step-B path cross-check).
pub(super) fn collect_fn_bodies<'a>(module: &'a Module, out: &mut Vec<&'a Block>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(function) => out.push(&function.body),
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    if let ItemKind::Fn(function) = &member.kind {
                        out.push(&function.body);
                    }
                }
            }
            ItemKind::Mod(inner) => collect_fn_bodies(inner, out),
            _ => {}
        }
    }
}

pub(super) fn collect_block_exprs<'a>(
    block: &'a Block,
    out: &mut BTreeMap<NodeId, &'a Expr>,
    receivers: &mut BTreeSet<NodeId>,
) {
    for statement in &block.stmts {
        match &statement.kind {
            StmtKind::Let(let_statement) => {
                if let Some(init) = &let_statement.init {
                    collect_expr(init, out, receivers);
                }
            }
            StmtKind::Expr { expr, .. } => collect_expr(expr, out, receivers),
        }
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, out, receivers);
    }
}

pub(super) fn collect_expr<'a>(
    expr: &'a Expr,
    out: &mut BTreeMap<NodeId, &'a Expr>,
    receivers: &mut BTreeSet<NodeId>,
) {
    out.insert(expr.id, expr);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            collect_expr(callee, out, receivers);
            for arg in args {
                collect_expr(arg, out, receivers);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            receivers.insert(receiver.id);
            collect_expr(receiver, out, receivers);
            for arg in args {
                collect_expr(arg, out, receivers);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            collect_expr(base, out, receivers);
        }
        ExprKind::Index { base, index } => {
            collect_expr(base, out, receivers);
            collect_expr(index, out, receivers);
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => {
            collect_expr(expr, out, receivers);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, out, receivers);
            collect_expr(rhs, out, receivers);
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            collect_expr(target, out, receivers);
            collect_expr(value, out, receivers);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_expr(cond, out, receivers);
            collect_block_exprs(then_block, out, receivers);
            if let Some(else_branch) = else_branch {
                collect_expr(else_branch, out, receivers);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr(scrutinee, out, receivers);
            for arm in arms {
                collect_expr(&arm.body, out, receivers);
            }
        }
        ExprKind::While { cond, body } => {
            collect_expr(cond, out, receivers);
            collect_block_exprs(body, out, receivers);
        }
        ExprKind::Loop { body } => collect_block_exprs(body, out, receivers),
        ExprKind::For { iter, body, .. } => {
            collect_expr(iter, out, receivers);
            collect_block_exprs(body, out, receivers);
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => {
            collect_block_exprs(block, out, receivers);
        }
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                collect_expr(item, out, receivers);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            collect_expr(value, out, receivers);
            collect_expr(len, out, receivers);
        }
        ExprKind::StructLit { fields, .. } => {
            for field in fields {
                collect_expr(&field.value, out, receivers);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                collect_expr(start, out, receivers);
            }
            if let Some(end) = end {
                collect_expr(end, out, receivers);
            }
        }
        ExprKind::Try(inner) => collect_expr(inner, out, receivers),
        ExprKind::Return(Some(value)) => collect_expr(value, out, receivers),
        _ => {}
    }
}

/// An array place: a local binding, identified by the identity the checker
/// gave it (`OwnFacts::binding_identity`), and the field/tuple projections that
/// lead to the array (`a`, `h.items`, `t.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ArrayPlace {
    binding: BindingId,
    lead: Vec<String>,
}

impl ArrayPlace {
    /// True when one place contains the other, so a move out of one can leave
    /// the other partially moved.
    fn overlaps(&self, other: &ArrayPlace) -> bool {
        self.binding == other.binding
            && (self.lead.starts_with(&other.lead) || other.lead.starts_with(&self.lead))
    }
}

/// One element path found in a function body.
pub(super) struct PathAccess {
    /// Outermost expression of the path.
    node: NodeId,
    /// The array place, when the path is rooted at a local binding.
    place: Option<ArrayPlace>,
    /// The path's value is moved out.
    consumed: bool,
    /// The path is assigned or borrowed mutably.
    exclusive: bool,
    /// The constant run is not empty: a pattern reaches the element.
    constant: bool,
}

/// Walk of one function body that classifies its element paths. The walk only
/// *finds* the paths: whether a path is moved out of, and which binding it is
/// rooted at, are the checker's decisions, read from `OwnFacts`
/// (RFC-OWN-FACTS-2).
pub(super) struct PathPlanner<'e, 'a> {
    emitter: &'e Emitter<'a>,
    accesses: Vec<PathAccess>,
    reinits: Vec<NodeId>,
    /// Step-B guard (debug builds): decisions this walk derives from the syntax
    /// that the checker's facts do not confirm.
    disagreements: Vec<String>,
}

impl<'e, 'a> PathPlanner<'e, 'a> {
    fn new(emitter: &'e Emitter<'a>) -> Self {
        Self {
            emitter,
            accesses: Vec::new(),
            reinits: Vec::new(),
            disagreements: Vec::new(),
        }
    }

    /// Step-B guard (debug builds): the backend lowers the path `expr` (rooted
    /// at `root`), so the checker must have decided something about it. An
    /// absent decision is not "not consumed": it is a missing fact.
    ///
    /// * a place (`a[0]`, `p.items[1]`) carries a recorded place and a binding
    ///   identity for its root;
    /// * a projection of a temporary (`make()[i]`) is no place, and carries the
    ///   use the checker recorded when it took its value.
    fn cross_check(&mut self, expr: &Expr, root: &Expr) {
        let own = &self.emitter.facts().own;
        let location = self.emitter.expr_location(expr);
        if self.emitter.syntactic_place(expr).is_none() {
            if !own.use_kind.contains_key(&expr.id) {
                self.disagreements.push(format!(
                    "ownership facts disagree with the backend for expression {} ({location}): \
                     the checker recorded no decision for this projection of a temporary",
                    expr.id
                ));
            }
            return;
        }
        if !own.place.contains_key(&expr.id) {
            self.disagreements.push(format!(
                "ownership facts disagree with the backend for expression {} ({location}): the \
                 checker recorded no decision for this place",
                expr.id
            ));
        }
        // A place reached through an explicit dereference (`(*r)[0]`) is rooted
        // at that dereference, not at a binding: the planner tracks no array
        // place for it (`array_place` finds no identity and stops), so only a
        // root that names a binding owes one.
        let rooted_at_a_binding = matches!(root.kind, ExprKind::Path(_) | ExprKind::SelfValue);
        if rooted_at_a_binding && !own.binding_identity.contains_key(&root.id) {
            self.disagreements.push(format!(
                "ownership facts disagree with the backend for expression {} ({location}): the \
                 checker recorded no binding identity for the root of this place",
                expr.id
            ));
        }
    }

    fn walk_block(&mut self, block: &Block) {
        for statement in &block.stmts {
            match &statement.kind {
                StmtKind::Let(declaration) => {
                    if let Some(init) = &declaration.init {
                        self.walk_expr(init, false);
                    }
                }
                StmtKind::Expr { expr, .. } => self.walk_expr(expr, false),
            }
        }
        if let Some(tail) = &block.tail {
            self.walk_expr(tail, false);
        }
    }

    /// The array place of a path rooted at a local binding through field/tuple
    /// projections only.
    fn array_place(&self, path: &ElementPath<'_>) -> Option<ArrayPlace> {
        let binding = *self
            .emitter
            .facts()
            .own
            .binding_identity
            .get(&path.root.id)?;
        let mut lead = Vec::new();
        for step in &path.steps[..path.first] {
            match step {
                PathStep::Field { name, .. } => lead.push((*name).to_string()),
                PathStep::Tuple { index, .. } => lead.push(index.to_string()),
                PathStep::Index { .. } => return None,
            }
        }
        Some(ArrayPlace { binding, lead })
    }

    /// Record `expr` (a field, tuple-index or index expression) when it is an
    /// element path, and walk the expressions evaluated inside it.
    fn walk_path(&mut self, expr: &Expr, exclusive: bool) {
        let mut steps = Vec::new();
        let root = collect_path_steps(expr, &mut steps);
        if !matches!(root.kind, ExprKind::Path(_) | ExprKind::SelfValue) {
            self.walk_expr(root, false);
        }
        for step in &steps {
            if let PathStep::Index { index, .. } = step {
                self.walk_expr(index, false);
            }
        }
        if cfg!(debug_assertions) {
            self.cross_check(expr, root);
        }
        let Some(path) = self.emitter.element_path(expr) else {
            return;
        };
        let own = &self.emitter.facts().own;
        let borrowed_mutably = matches!(own.use_kind.get(&expr.id), Some(UseKind::BorrowMut));
        self.accesses.push(PathAccess {
            node: expr.id,
            place: self.array_place(&path),
            // RFC-OWN-FACTS-2 step C3: the checker decided, where it evaluated
            // this expression, whether its value is moved out.
            consumed: own.consumed.contains(&expr.id),
            exclusive: exclusive || borrowed_mutably,
            constant: !path.slots.is_empty(),
        });
    }

    /// `exclusive` is true when `expr` is assigned or mutably borrowed.
    fn walk_expr(&mut self, expr: &Expr, exclusive: bool) {
        match &expr.kind {
            ExprKind::Field { .. } | ExprKind::TupleIndex { .. } | ExprKind::Index { .. } => {
                self.walk_path(expr, exclusive);
            }
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    self.walk_expr(item, false);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for field in fields {
                    self.walk_expr(&field.value, false);
                }
            }
            ExprKind::Call { callee, args } => {
                self.walk_expr(callee, false);
                for arg in args {
                    self.walk_expr(arg, false);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.walk_expr(receiver, false);
                for arg in args {
                    self.walk_expr(arg, false);
                }
            }
            ExprKind::Assign { target, value } => {
                self.walk_expr(value, false);
                self.walk_expr(target, true);
                if self.emitter.facts().own.element_reinit.contains(&target.id) {
                    self.reinits.push(target.id);
                }
            }
            ExprKind::AssignOp { target, value, .. } => {
                self.walk_expr(value, false);
                self.walk_expr(target, true);
            }
            ExprKind::Return(Some(value)) => self.walk_expr(value, false),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.walk_expr(cond, false);
                self.walk_block(then_block);
                if let Some(else_branch) = else_branch {
                    self.walk_expr(else_branch, false);
                }
            }
            ExprKind::Block(block) | ExprKind::Unsafe(block) => self.walk_block(block),
            ExprKind::Match { scrutinee, arms } => {
                self.walk_expr(scrutinee, false);
                for arm in arms {
                    self.walk_expr(&arm.body, false);
                }
            }
            ExprKind::While { cond, body } => {
                self.walk_expr(cond, false);
                self.walk_block(body);
            }
            ExprKind::Loop { body } => self.walk_block(body),
            ExprKind::For { iter, body, .. } => {
                self.walk_expr(iter, false);
                self.walk_block(body);
            }
            ExprKind::ArrayRepeat { value, len } => {
                self.walk_expr(value, false);
                self.walk_expr(len, false);
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.walk_expr(start, false);
                }
                if let Some(end) = end {
                    self.walk_expr(end, false);
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                self.walk_expr(inner, matches!(op, UnOp::RefMut));
            }
            ExprKind::Cast { expr: inner, .. } | ExprKind::Try(inner) => {
                self.walk_expr(inner, false);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs, false);
                self.walk_expr(rhs, false);
            }
            ExprKind::Return(None)
            | ExprKind::Break
            | ExprKind::Continue
            | ExprKind::Path(_)
            | ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::SelfValue => {}
        }
    }

    fn plan(&self) -> ArrayMovePlan {
        // Places an element is moved out of somewhere in the body: only those can
        // be partially moved when another of their elements is borrowed.
        let moved_from: Vec<&ArrayPlace> = self
            .accesses
            .iter()
            .filter(|access| access.consumed)
            .filter_map(|access| access.place.as_ref())
            .collect();
        let mut plan = ArrayMovePlan::default();
        for access in &self.accesses {
            let mode = if access.consumed {
                PathMode::Move
            } else {
                let partially_moved = access
                    .place
                    .as_ref()
                    .is_some_and(|place| moved_from.iter().any(|moved| moved.overlaps(place)));
                if !(access.constant && partially_moved) {
                    continue;
                }
                if access.exclusive {
                    PathMode::Exclusive
                } else {
                    PathMode::Shared
                }
            };
            plan.paths.insert(access.node, mode);
        }
        for node in &self.reinits {
            plan.paths.insert(*node, PathMode::Reinit);
        }
        plan
    }
}

impl<'a> Emitter<'a> {
    /// Plan the array-element paths of one function body (see [`ArrayMovePlan`]).
    pub(super) fn plan_array_moves(&self, body: &Block) -> ArrayMovePlan {
        let mut planner = PathPlanner::new(self);
        planner.walk_block(body);
        planner.plan()
    }

    /// Step-B guard: the disagreements between the syntactic walk of `body` and
    /// the checker's facts (see [`PathPlanner::cross_check`]).
    pub(super) fn path_fact_disagreements(&self, body: &Block) -> Vec<String> {
        let mut planner = PathPlanner::new(self);
        planner.walk_block(body);
        planner.disagreements
    }

    pub(super) fn path_mode(&self, node: NodeId) -> Option<PathMode> {
        self.array_moves.borrow().paths.get(&node).copied()
    }

    /// Element count of `base` when it is a fixed-size array (the indexed
    /// expression of an index step).
    pub(super) fn indexed_array_len(&self, base: &Expr) -> Option<usize> {
        match self.facts().expr_types.get(&base.id) {
            Some(Type::Array { len, .. }) => Some(*len),
            _ => None,
        }
    }

    /// The pattern position a step selects, when a Rust pattern can express it:
    /// a literal, in-range index of a fixed-size array, a tuple element or a
    /// struct field. A step through a reference, a `Vec`, a slice or a
    /// non-literal index has no pattern form.
    pub(super) fn slot_of(&self, step: &PathStep<'_>) -> Option<Slot> {
        match step {
            PathStep::Index { base, index } => {
                let len = self.indexed_array_len(base)?;
                let at = constant_index(index)?;
                (at < len).then_some(Slot::Index { len, at })
            }
            PathStep::Tuple { base, index } => match self.facts().expr_types.get(&base.id) {
                Some(Type::Tuple(items)) => {
                    let at = usize::try_from(*index).ok()?;
                    (at < items.len()).then_some(Slot::Tuple {
                        len: items.len(),
                        at,
                    })
                }
                _ => None,
            },
            PathStep::Field { base, name } => match self.facts().expr_types.get(&base.id) {
                Some(Type::Nominal { def, .. }) => {
                    let info = self.facts().structs.get(def)?;
                    Some(Slot::Field {
                        path: self.item_path(*def).ok()?,
                        fields: info
                            .fields
                            .iter()
                            .map(|field| mangle_ident(&field.name))
                            .collect(),
                        name: mangle_ident(name),
                    })
                }
                _ => None,
            },
        }
    }

    /// Resolve an expression as an element path: a chain of projections and
    /// indexes that goes through at least one fixed-size array index.
    pub(super) fn element_path<'x>(&self, expr: &'x Expr) -> Option<ElementPath<'x>> {
        let mut steps = Vec::new();
        let root = collect_path_steps(expr, &mut steps);
        let (first, array) = steps.iter().enumerate().find_map(|(at, step)| match step {
            PathStep::Index { base, .. } if self.indexed_array_len(base).is_some() => {
                Some((at, *base))
            }
            _ => None,
        })?;
        let mut slots = Vec::new();
        for step in &steps[first..] {
            match self.slot_of(step) {
                Some(slot) => slots.push(slot),
                None => break,
            }
        }
        Some(ElementPath {
            root,
            array,
            steps,
            first,
            slots,
        })
    }

    /// Lower a planned element path, or `None` when the expression keeps its
    /// plain form.
    pub(super) fn element_path_expr(
        &self,
        expr: &Expr,
        indent: usize,
    ) -> Result<Option<String>, CodegenError> {
        match self.path_mode(expr.id) {
            // A re-initialised target is lowered by its assignment.
            None | Some(PathMode::Reinit) => Ok(None),
            Some(PathMode::Move) => self.element_move_expr(expr, indent).map(Some),
            Some(PathMode::Shared) => self.element_borrow_expr(expr, false, indent).map(Some),
            Some(PathMode::Exclusive) => self.element_borrow_expr(expr, true, indent).map(Some),
        }
    }

    pub(super) fn planned_path<'x>(&self, expr: &'x Expr) -> Result<ElementPath<'x>, CodegenError> {
        self.element_path(expr).ok_or_else(|| {
            err(format!(
                "internal: expression {} was planned as an array-element path but is not one",
                expr.id
            ))
        })
    }

    /// Move the value of an element path out of its array.
    ///
    /// The constant run is one irrefutable pattern applied to the array *place*
    /// (`let [_, __rv_el, ..] = a;`): rustc records a move of exactly that
    /// element, so the other elements stay usable and every later use is checked
    /// by rustc itself. What follows the constant run works on the moved value:
    /// a non-literal index consumes its array with `into_iter().nth(i)` and
    /// keeps the exact rustc bounds-panic text ("index out of bounds: the len is
    /// N but the index is I", measured in `tests/backend_audit.rs`); the value
    /// being indexed is always evaluated before its index expression.
    pub(super) fn element_move_expr(
        &self,
        expr: &Expr,
        indent: usize,
    ) -> Result<String, CodegenError> {
        let path = self.planned_path(expr)?;
        let id = expr.id;
        let array = self.expr(path.array(), indent)?;
        let mut out = String::from("({ ");
        let mut current = if path.slots.is_empty() {
            out.push_str(&format!("let __rv_root_{id} = {array}; "));
            format!("__rv_root_{id}")
        } else {
            let binding = format!("__rv_el_{id}");
            out.push_str(&format!(
                "let {} = {array}; ",
                slot_pattern(&path.slots, &binding)
            ));
            binding
        };
        for (level, step) in path.rest().iter().enumerate() {
            match step {
                PathStep::Field { name, .. } => {
                    current.push_str(&format!(".{}", mangle_ident(name)));
                }
                PathStep::Tuple { index, .. } => current.push_str(&format!(".{index}")),
                PathStep::Index { base, index } => {
                    let selected = format!("__rv_sel_{id}_{level}");
                    match (self.indexed_array_len(base), constant_index(index)) {
                        (Some(len), Some(at)) if at < len => {
                            out.push_str(&format!(
                                "let {} = {current}; ",
                                slot_pattern(&[Slot::Index { len, at }], &selected)
                            ));
                            current = selected;
                        }
                        // A runtime index, or a literal one past the end: Core
                        // reports the latter at run time, like a copied or
                        // borrowed element (`a[3]` of a `[T; 1]`), so it takes
                        // the same bounds-checked form instead of a plain `a[3]`
                        // that rustc refuses for a non-`Copy` element (E0508).
                        (Some(len), _) => {
                            let index_name = format!("__rv_idx_{id}_{level}");
                            out.push_str(&format!(
                                "let {index_name}: usize = {}; ",
                                self.expr(index, indent)?
                            ));
                            out.push_str(&format!(
                                "let {selected} = match {current}.into_iter().nth({index_name}) {{ \
                                 Some(__rv_nth_{id}_{level}) => __rv_nth_{id}_{level}, \
                                 None => panic!(\"index out of bounds: the len is {{}} but the index is {{}}\", {len}usize, {index_name}) }}; "
                            ));
                            current = selected;
                        }
                        // Not a fixed-size array level: keep the plain form so
                        // rustc reports it instead of a pattern it cannot match.
                        _ => {
                            current = format!("{current}[{}]", self.expr(index, indent)?);
                        }
                    }
                }
            }
        }
        out.push_str(&current);
        out.push_str(" })");
        Ok(out)
    }

    /// Borrow an element of an array that is partially moved somewhere in the
    /// function: `(*{ let [_, ref __rv_el, ..] = a; __rv_el })`.
    ///
    /// A plain `a[1]` borrows the whole array, which rustc refuses once another
    /// element has been moved; a `ref` binding borrows that element only. The
    /// result is a place expression, so reads, `&`, `&mut`, method calls and
    /// assignments keep working, and the steps after the constant run are
    /// applied to it unchanged.
    pub(super) fn element_borrow_expr(
        &self,
        expr: &Expr,
        exclusive: bool,
        indent: usize,
    ) -> Result<String, CodegenError> {
        let path = self.planned_path(expr)?;
        let binding = format!("__rv_el_{}", expr.id);
        let leaf = if exclusive {
            format!("ref mut {binding}")
        } else {
            format!("ref {binding}")
        };
        let mut text = format!(
            "(*{{ let {} = {}; {binding} }})",
            slot_pattern(&path.slots, &leaf),
            self.expr(path.array(), indent)?
        );
        for step in path.rest() {
            match step {
                PathStep::Field { name, .. } => text.push_str(&format!(".{}", mangle_ident(name))),
                PathStep::Tuple { index, .. } => text.push_str(&format!(".{index}")),
                PathStep::Index { index, .. } => {
                    text.push_str(&format!("[{}]", self.expr(index, indent)?));
                }
            }
        }
        Ok(text)
    }

    /// Re-initialise a moved place under an array index by rebuilding the array
    /// around the new value (OWN-5.4): `{ let new = <value>; a = [new, <a[1]>]; }`.
    ///
    /// Rust only re-initialises an array element by assigning the whole array.
    /// The checker accepts such a write only when every other part of the array
    /// is still initialised (`OwnFacts::element_reinit`), so each of them can be
    /// moved out and back. The value is evaluated first, as in a plain
    /// assignment.
    pub(super) fn element_reinit_expr(
        &self,
        target: &Expr,
        value: &Expr,
        indent: usize,
    ) -> Result<String, CodegenError> {
        let path = self.planned_path(target)?;
        // A literal index past the end of a fixed-size array stops the constant
        // run (`a[3] = v` with `a: [T; 1]`): Core reports it at run time, after
        // evaluating the value, like the move it re-initialises.
        if let Some(PathStep::Index { base, index }) = path.rest().first() {
            if let (Some(len), Some(at)) = (self.indexed_array_len(base), constant_index(index)) {
                return Ok(format!(
                    "{{ let _ = {}; panic!(\"index out of bounds: the len is {{}} but the index is {{}}\", {len}usize, {at}usize); }}",
                    self.expr(value, indent)?
                ));
            }
        }
        if !path.rest().is_empty() {
            return Err(err(format!(
                "internal: re-initialised array element {} is not reachable by a constant path",
                target.id
            )));
        }
        let array = self.expr(path.array(), indent)?;
        let fresh = format!("__rv_new_{}", target.id);
        let rebuilt = rebuild_text(&path.slots, 0, &array, &fresh);
        Ok(format!(
            "{{ let {fresh} = {}; {array} = {rebuilt}; }}",
            self.expr(value, indent)?
        ))
    }
}
