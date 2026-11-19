//! Lowering from a checked program (`ravum-lang-sema`) to IR v1.
//!
//! Fail-closed: any construct outside the IR v1 subset returns
//! [`LowerError::Unsupported`] naming the construct; nothing is approximated.
//! Types come from the checker's facts, never re-inferred here.

use std::collections::BTreeMap;

use ravum_lang_sema::syntax::ast::{
    self, Block as AstBlock, Expr, ExprKind, FnItem, ItemKind, PatKind, StmtKind,
};
use ravum_lang_sema::syntax::{NodeId, Span};
use ravum_lang_sema::{CallTarget, CheckedProgram, Resolution, Type};

use crate::ir::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// The program uses a construct IR v1 does not cover (fail-closed).
    Unsupported(String),
    /// A fact the checker should have produced is missing (internal error).
    MissingFact(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::Unsupported(s) => write!(f, "unsupported in IR v1: {s}"),
            LowerError::MissingFact(s) => write!(f, "missing checker fact: {s}"),
        }
    }
}

type R<T> = Result<T, LowerError>;

fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(LowerError::Unsupported(what.into()))
}

/// Map a checker type to an IR type.
pub fn map_ty(t: &Type) -> R<Ty> {
    Ok(match t {
        Type::I8 => Ty::I8,
        Type::I16 => Ty::I16,
        Type::I32 => Ty::I32,
        Type::I64 => Ty::I64,
        Type::Isize => Ty::Isize,
        Type::U8 => Ty::U8,
        Type::U16 => Ty::U16,
        Type::U32 => Ty::U32,
        Type::U64 => Ty::U64,
        Type::Usize => Ty::Usize,
        Type::F32 => Ty::F32,
        Type::F64 => Ty::F64,
        Type::Bool => Ty::Bool,
        Type::Unit | Type::Never => Ty::Unit,
        other => return unsupported(format!("type `{other}`")),
    })
}

/// Lower every function of `checked` into an IR module.
pub fn lower(checked: &CheckedProgram) -> R<Module> {
    let mut fns: Vec<(&ast::Item, &FnItem)> = Vec::new();
    for item in &checked.program.root.items {
        match &item.kind {
            ItemKind::Fn(f) => fns.push((item, f)),
            ItemKind::Use(_) => {}
            ItemKind::Struct(_) => return unsupported("struct"),
            ItemKind::Enum(_) => return unsupported("enum"),
            ItemKind::Const(_) => return unsupported("const"),
            ItemKind::Impl(_) => return unsupported("impl"),
            ItemKind::Mod(_) | ItemKind::ModDecl(_) => return unsupported("module"),
            ItemKind::Extern(_) => return unsupported("extern"),
        }
    }
    let ids: BTreeMap<NodeId, FuncId> = fns
        .iter()
        .enumerate()
        .map(|(i, (item, _))| (item.id, i as FuncId))
        .collect();
    let mut functions = Vec::new();
    for (item, f) in &fns {
        functions.push(FnLower::new(checked, &ids, item, f)?.run(f)?);
    }
    let m = Module {
        version: IR_VERSION.to_string(),
        functions,
    };
    m.validate().map_err(LowerError::MissingFact)?;
    Ok(m)
}

struct LoopCtx {
    break_to: BlockId,
    continue_to: BlockId,
}

struct FnLower<'a> {
    checked: &'a CheckedProgram,
    ids: &'a BTreeMap<NodeId, FuncId>,
    item: &'a ast::Item,
    locals: Vec<Ty>,
    blocks: Vec<Block>,
    cur: BlockId,
    /// (declaration span, name) → slot, per checker `Resolution::Local`.
    vars: BTreeMap<(Span, String), Local>,
    loops: Vec<LoopCtx>,
    ret: Ty,
    /// The current block follows a terminator: nothing emitted there runs.
    dead: bool,
}

impl<'a> FnLower<'a> {
    fn new(
        checked: &'a CheckedProgram,
        ids: &'a BTreeMap<NodeId, FuncId>,
        item: &'a ast::Item,
        f: &FnItem,
    ) -> R<Self> {
        if !f.generics.is_empty() {
            return unsupported("generic function");
        }
        let sig =
            checked.facts.fn_sigs.get(&item.id).ok_or_else(|| {
                LowerError::MissingFact(format!("signature of `{}`", f.name.name))
            })?;
        if sig.receiver.is_some() {
            return unsupported("method");
        }
        let ret = map_ty(&sig.ret)?;
        let mut me = FnLower {
            checked,
            ids,
            item,
            locals: Vec::new(),
            blocks: vec![Block {
                insts: Vec::new(),
                term: Term::Unreachable,
            }],
            cur: 0,
            vars: BTreeMap::new(),
            loops: Vec::new(),
            ret,
            dead: false,
        };
        for p in &sig.params {
            let ty = map_ty(&p.ty)?;
            let l = me.new_local(ty);
            me.vars.insert((item.span, p.name.clone()), l);
        }
        Ok(me)
    }

    fn run(mut self, f: &FnItem) -> R<Function> {
        let params = self.locals.len() as u32;
        let v = self.block(&f.body)?;
        let term = if self.ret == Ty::Unit {
            Term::Return(None)
        } else {
            Term::Return(Some(v))
        };
        self.terminate(term);
        Ok(Function {
            name: f.name.name.clone(),
            params,
            ret: self.ret,
            locals: self.locals,
            blocks: self.blocks,
        })
    }

    // ---- helpers ---------------------------------------------------------

    fn new_local(&mut self, ty: Ty) -> Local {
        self.locals.push(ty);
        (self.locals.len() - 1) as Local
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(Block {
            insts: Vec::new(),
            term: Term::Unreachable,
        });
        (self.blocks.len() - 1) as BlockId
    }

    fn emit(&mut self, i: Inst) {
        if !self.dead {
            self.blocks[self.cur as usize].insts.push(i);
        }
    }

    /// Set the terminator of the current block unless it is dead.
    fn set_term(&mut self, t: Term) {
        if !self.dead {
            self.blocks[self.cur as usize].term = t;
        }
    }

    /// Terminate the current block and continue in a fresh (dead) block.
    fn terminate(&mut self, t: Term) {
        self.set_term(t);
        self.cur = self.new_block();
        self.dead = true;
    }

    fn switch_to(&mut self, b: BlockId) {
        self.cur = b;
        self.dead = false;
    }

    fn ty_of(&self, e: &Expr) -> R<Ty> {
        let t = self.checked.facts.expr_types.get(&e.id).ok_or_else(|| {
            LowerError::MissingFact(format!("type of expression at {:?}", e.span))
        })?;
        map_ty(t)
    }

    fn unit(&mut self) -> Local {
        let l = self.new_local(Ty::Unit);
        self.emit(Inst::Const {
            dst: l,
            value: Const::Unit,
        });
        l
    }

    fn local_of_path(&self, path: &ast::Path) -> R<Local> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Local { name, decl, .. }) => self
                .vars
                .get(&(*decl, name.clone()))
                .copied()
                .ok_or_else(|| LowerError::MissingFact(format!("slot of `{name}`"))),
            Some(_) => unsupported("path to a non-local item used as a value"),
            None => Err(LowerError::MissingFact("path resolution".into())),
        }
    }

    // ---- statements and blocks ------------------------------------------

    fn block(&mut self, b: &AstBlock) -> R<Local> {
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Let(l) => {
                    let PatKind::Binding { name, .. } = &l.pat.kind else {
                        return unsupported("destructuring `let`");
                    };
                    let ty = self
                        .checked
                        .facts
                        .pat_types
                        .get(&l.pat.id)
                        .ok_or_else(|| LowerError::MissingFact(format!("type of `{}`", name.name)))
                        .and_then(map_ty)?;
                    let Some(init) = &l.init else {
                        return unsupported("`let` without initializer");
                    };
                    let v = self.expr(init)?;
                    let slot = self.new_local(ty);
                    self.emit(Inst::Copy { dst: slot, src: v });
                    self.vars.insert((l.pat.span, name.name.clone()), slot);
                }
                StmtKind::Expr { expr, .. } => {
                    self.expr(expr)?;
                }
            }
        }
        match &b.tail {
            Some(t) => self.expr(t),
            None => Ok(self.unit()),
        }
    }

    // ---- expressions -----------------------------------------------------

    fn expr(&mut self, e: &Expr) -> R<Local> {
        match &e.kind {
            ExprKind::Unit => Ok(self.unit()),
            ExprKind::Int { value, .. } => {
                let ty = self.ty_of(e)?;
                self.int_const(ty, *value as i128)
            }
            ExprKind::Float { text, .. } => {
                let ty = self.ty_of(e)?;
                self.float_const(ty, text, false)
            }
            ExprKind::Bool(b) => {
                let l = self.new_local(Ty::Bool);
                self.emit(Inst::Const {
                    dst: l,
                    value: Const::Bool(*b),
                });
                Ok(l)
            }
            ExprKind::Path(p) => self.local_of_path(p),
            ExprKind::Block(b) => self.block(b),
            ExprKind::Unary { op, expr } => {
                let ty = self.ty_of(e)?;
                // Fold `-literal` so that `-2147483648` is a valid i32.
                if *op == ast::UnOp::Neg {
                    match &expr.kind {
                        ExprKind::Int { value, .. } => {
                            return self.int_const(ty, -(*value as i128))
                        }
                        ExprKind::Float { text, .. } => return self.float_const(ty, text, true),
                        _ => {}
                    }
                }
                let op = match op {
                    ast::UnOp::Neg => UnOp::Neg,
                    ast::UnOp::Not => UnOp::Not,
                    _ => return unsupported("references"),
                };
                let src = self.expr(expr)?;
                let dst = self.new_local(ty);
                self.emit(Inst::Un { dst, op, ty, src });
                Ok(dst)
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(e, *op, lhs, rhs),
            ExprKind::Assign { target, value } => {
                let slot = self.place(target)?;
                let v = self.expr(value)?;
                self.emit(Inst::Copy { dst: slot, src: v });
                Ok(self.unit())
            }
            ExprKind::AssignOp { op, target, value } => {
                let slot = self.place(target)?;
                let ty = self.locals[slot as usize];
                let rhs = self.expr(value)?;
                let rhs_ty = self.ty_of(value)?;
                let r = self.arith(op.bin_op(), ty, slot, rhs, rhs_ty)?;
                self.emit(Inst::Copy { dst: slot, src: r });
                Ok(self.unit())
            }
            ExprKind::Cast { expr, .. } => {
                let from = self.ty_of(expr)?;
                let to = self.ty_of(e)?;
                let src = self.expr(expr)?;
                let dst = self.new_local(to);
                self.emit(Inst::Cast { dst, from, to, src });
                Ok(dst)
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let ty = self.ty_of(e)?;
                let result = self.new_local(ty);
                let c = self.expr(cond)?;
                let (bt, be, join) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: c,
                    then_to: bt,
                    else_to: be,
                });
                self.switch_to(bt);
                let v = self.block(then_block)?;
                self.emit(Inst::Copy {
                    dst: result,
                    src: v,
                });
                self.set_term(Term::Jump(join));
                self.switch_to(be);
                if let Some(eb) = else_branch {
                    let v = self.expr(eb)?;
                    self.emit(Inst::Copy {
                        dst: result,
                        src: v,
                    });
                }
                self.set_term(Term::Jump(join));
                self.switch_to(join);
                Ok(result)
            }
            ExprKind::While { cond, body } => {
                let (head, bodyb, exit) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Jump(head));
                self.switch_to(head);
                let c = self.expr(cond)?;
                self.set_term(Term::Branch {
                    cond: c,
                    then_to: bodyb,
                    else_to: exit,
                });
                self.switch_to(bodyb);
                self.loops.push(LoopCtx {
                    break_to: exit,
                    continue_to: head,
                });
                self.block(body)?;
                self.loops.pop();
                self.set_term(Term::Jump(head));
                self.switch_to(exit);
                Ok(self.unit())
            }
            ExprKind::Loop { body } => {
                let ty = self.ty_of(e)?;
                if ty != Ty::Unit {
                    return unsupported("`break` with a value");
                }
                let (head, exit) = (self.new_block(), self.new_block());
                self.set_term(Term::Jump(head));
                self.switch_to(head);
                self.loops.push(LoopCtx {
                    break_to: exit,
                    continue_to: head,
                });
                self.block(body)?;
                self.loops.pop();
                self.set_term(Term::Jump(head));
                self.switch_to(exit);
                Ok(self.unit())
            }
            ExprKind::For { pat, iter, body } => self.for_range(pat, iter, body),
            ExprKind::Break => {
                let to = self.loops.last().map(|l| l.break_to);
                let to = to.ok_or_else(|| LowerError::MissingFact("loop of `break`".into()))?;
                self.terminate(Term::Jump(to));
                Ok(self.unit())
            }
            ExprKind::Continue => {
                let to = self.loops.last().map(|l| l.continue_to);
                let to = to.ok_or_else(|| LowerError::MissingFact("loop of `continue`".into()))?;
                self.terminate(Term::Jump(to));
                Ok(self.unit())
            }
            ExprKind::Return(v) => {
                let t = match v {
                    Some(v) if self.ret != Ty::Unit => Term::Return(Some(self.expr(v)?)),
                    Some(v) => {
                        self.expr(v)?;
                        Term::Return(None)
                    }
                    None => Term::Return(None),
                };
                self.terminate(t);
                Ok(self.unit())
            }
            ExprKind::Call { callee, args } => self.call(e, callee, args),
            ExprKind::MethodCall { method, .. } => {
                unsupported(format!("method call `.{}()`", method.name))
            }
            ExprKind::Match { .. } => unsupported("match"),
            ExprKind::Tuple(_) | ExprKind::TupleIndex { .. } => unsupported("tuple"),
            ExprKind::Array(_) | ExprKind::ArrayRepeat { .. } | ExprKind::Index { .. } => {
                unsupported("array")
            }
            ExprKind::StructLit { .. } | ExprKind::Field { .. } => unsupported("struct"),
            ExprKind::Str(_) => unsupported("string"),
            ExprKind::Char(_) => unsupported("char"),
            ExprKind::Range { .. } => unsupported("range outside `for`"),
            ExprKind::Try(_) => unsupported("`?`"),
            ExprKind::Unsafe(_) => unsupported("unsafe"),
            ExprKind::Await(_) => unsupported("await"),
            ExprKind::SelfValue => unsupported("self"),
        }
    }

    fn place(&self, target: &Expr) -> R<Local> {
        match &target.kind {
            ExprKind::Path(p) => self.local_of_path(p),
            _ => unsupported("assignment to a non-local place"),
        }
    }

    fn int_const(&mut self, ty: Ty, value: i128) -> R<Local> {
        let Some(bits) = ty.int_bits() else {
            return unsupported(format!("integer literal of type {}", ty.name()));
        };
        let (min, max) = if ty.is_signed() {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        };
        if value < min || value > max {
            return Err(LowerError::MissingFact(format!(
                "literal {value} out of range for {}",
                ty.name()
            )));
        }
        let l = self.new_local(ty);
        self.emit(Inst::Const {
            dst: l,
            value: Const::Int { ty, value },
        });
        Ok(l)
    }

    fn float_const(&mut self, ty: Ty, text: &str, neg: bool) -> R<Local> {
        let clean: String = text.chars().filter(|c| *c != '_').collect();
        let value = match ty {
            Ty::F32 => {
                let v: f32 = clean
                    .parse()
                    .map_err(|_| LowerError::MissingFact(format!("float `{text}`")))?;
                Const::F32Bits(if neg { -v } else { v }.to_bits())
            }
            Ty::F64 => {
                let v: f64 = clean
                    .parse()
                    .map_err(|_| LowerError::MissingFact(format!("float `{text}`")))?;
                Const::F64Bits(if neg { -v } else { v }.to_bits())
            }
            _ => return unsupported("float literal of non-float type"),
        };
        let l = self.new_local(ty);
        self.emit(Inst::Const { dst: l, value });
        Ok(l)
    }

    fn binary(&mut self, e: &Expr, op: ast::BinOp, lhs: &Expr, rhs: &Expr) -> R<Local> {
        use ast::BinOp as B;
        if matches!(op, B::And | B::Or) {
            // Short-circuit (MACH O2): the right operand runs only if needed.
            let result = self.new_local(Ty::Bool);
            let l = self.expr(lhs)?;
            self.emit(Inst::Copy {
                dst: result,
                src: l,
            });
            let (rhs_b, join) = (self.new_block(), self.new_block());
            self.set_term(if op == B::And {
                Term::Branch {
                    cond: l,
                    then_to: rhs_b,
                    else_to: join,
                }
            } else {
                Term::Branch {
                    cond: l,
                    then_to: join,
                    else_to: rhs_b,
                }
            });
            self.switch_to(rhs_b);
            let r = self.expr(rhs)?;
            self.emit(Inst::Copy {
                dst: result,
                src: r,
            });
            self.set_term(Term::Jump(join));
            self.switch_to(join);
            return Ok(result);
        }
        let lt = self.ty_of(lhs)?;
        let rt = self.ty_of(rhs)?;
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        let _ = e;
        self.arith(op, lt, a, b, rt)
    }

    /// Emit a binary operation whose left operand has type `ty`.
    fn arith(&mut self, op: ast::BinOp, ty: Ty, a: Local, b: Local, b_ty: Ty) -> R<Local> {
        use ast::BinOp as B;
        let iop = match op {
            B::Add => BinOp::Add,
            B::Sub => BinOp::Sub,
            B::Mul => BinOp::Mul,
            B::Div => BinOp::Div,
            B::Rem => BinOp::Rem,
            B::BitAnd => BinOp::BitAnd,
            B::BitOr => BinOp::BitOr,
            B::BitXor => BinOp::BitXor,
            B::Shl => BinOp::Shl,
            B::Shr => BinOp::Shr,
            B::Eq => BinOp::Eq,
            B::Ne => BinOp::Ne,
            B::Lt => BinOp::Lt,
            B::Le => BinOp::Le,
            B::Gt => BinOp::Gt,
            B::Ge => BinOp::Ge,
            B::And | B::Or => unreachable!("handled by `binary`"),
        };
        let rhs = if matches!(iop, BinOp::Shl | BinOp::Shr) {
            // IR v1: the shift amount is always a `u64` slot (negative
            // amounts are refused by the checker, D-19-1).
            if !b_ty.is_int() {
                return unsupported("non-integer shift amount");
            }
            let c = self.new_local(Ty::U64);
            self.emit(Inst::Cast {
                dst: c,
                from: b_ty,
                to: Ty::U64,
                src: b,
            });
            c
        } else {
            b
        };
        let out_ty = if matches!(
            iop,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        ) {
            Ty::Bool
        } else {
            ty
        };
        let dst = self.new_local(out_ty);
        self.emit(Inst::Bin {
            dst,
            op: iop,
            ty,
            lhs: a,
            rhs,
        });
        Ok(dst)
    }

    fn for_range(&mut self, pat: &ast::Pat, iter: &Expr, body: &AstBlock) -> R<Local> {
        let ExprKind::Range {
            start: Some(start),
            end: Some(end),
            inclusive: false,
        } = &iter.kind
        else {
            return unsupported("`for` over something other than `a..b`");
        };
        let PatKind::Binding { name, .. } = &pat.kind else {
            return unsupported("`for` pattern");
        };
        let ty = self.ty_of(start)?;
        if !ty.is_int() {
            return unsupported("non-integer range");
        }
        let i = self.new_local(ty);
        let s = self.expr(start)?;
        self.emit(Inst::Copy { dst: i, src: s });
        let end_v = self.expr(end)?;
        let end_slot = self.new_local(ty);
        self.emit(Inst::Copy {
            dst: end_slot,
            src: end_v,
        });
        let (head, bodyb, step, exit) = (
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
        );
        self.set_term(Term::Jump(head));
        self.switch_to(head);
        let c = self.new_local(Ty::Bool);
        self.emit(Inst::Bin {
            dst: c,
            op: BinOp::Lt,
            ty,
            lhs: i,
            rhs: end_slot,
        });
        self.set_term(Term::Branch {
            cond: c,
            then_to: bodyb,
            else_to: exit,
        });
        self.switch_to(bodyb);
        // The loop variable is a fresh binding per iteration (a copy of `i`).
        let var = self.new_local(ty);
        self.emit(Inst::Copy { dst: var, src: i });
        self.vars.insert((pat.span, name.name.clone()), var);
        self.loops.push(LoopCtx {
            break_to: exit,
            continue_to: step,
        });
        self.block(body)?;
        self.loops.pop();
        self.set_term(Term::Jump(step));
        self.switch_to(step);
        // `i < end` holds here, so `i + 1` cannot overflow.
        let one = self.int_const(ty, 1)?;
        let next = self.new_local(ty);
        self.emit(Inst::Bin {
            dst: next,
            op: BinOp::Add,
            ty,
            lhs: i,
            rhs: one,
        });
        self.emit(Inst::Copy { dst: i, src: next });
        self.set_term(Term::Jump(head));
        self.switch_to(exit);
        Ok(self.unit())
    }

    fn call(&mut self, e: &Expr, callee: &Expr, args: &[Expr]) -> R<Local> {
        match self.checked.facts.calls.get(&e.id) {
            Some(CallTarget::UserFn { item }) => {
                let func = *self.ids.get(item).ok_or_else(|| {
                    LowerError::Unsupported("call to a function outside the root module".into())
                })?;
                let mut vals = Vec::new();
                // MACH O1: arguments are evaluated left to right.
                for a in args {
                    vals.push(self.expr(a)?);
                }
                let ty = self.ty_of(e)?;
                let dst = self.new_local(ty);
                self.emit(Inst::Call {
                    dst: Some(dst),
                    func,
                    args: vals,
                });
                Ok(dst)
            }
            Some(CallTarget::BuiltinFn { path }) if path.ends_with("panic") => match args {
                [Expr {
                    kind: ExprKind::Str(msg),
                    ..
                }] => {
                    self.terminate(Term::Trap(msg.clone()));
                    Ok(self.unit())
                }
                _ => unsupported("`panic` with a non-literal message"),
            },
            Some(CallTarget::BuiltinFn { path }) => unsupported(format!("builtin `{path}`")),
            Some(other) => unsupported(format!("call target {other:?}")),
            None => {
                let _ = (callee, self.item);
                Err(LowerError::MissingFact("call target".into()))
            }
        }
    }
}
