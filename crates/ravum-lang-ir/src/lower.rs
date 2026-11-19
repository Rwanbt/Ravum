//! Lowering from a checked program (`ravum-lang-sema`) to IR v1.1.
//!
//! Fail-closed: any construct outside the IR subset returns
//! [`LowerError::Unsupported`] naming the construct; nothing is approximated.
//! Types come from the checker's facts, never re-inferred here.
//!
//! # Memory discipline (IR v1.1)
//!
//! * Scalars live in IR locals (virtual registers).
//! * Aggregates (`Copy` structs, tuples, fixed arrays) live in memory: an
//!   aggregate-typed expression evaluates to a `Ptr` local holding its
//!   address. Binding, assignment, by-value arguments and returns **copy**
//!   the value field by field, so two variables never share storage.
//! * `&x` / `&mut x` is the address of a place in memory; references to
//!   scalar locals (registers) are refused.
//! * A function returning an aggregate takes a hidden first parameter: the
//!   address where it writes its result (`sret`).
//! * Every array index is checked against the length before the address is
//!   formed (`index out of bounds`), so every `Load`/`Store` the lowering
//!   emits stays inside the object it addresses.

use std::collections::BTreeMap;

use ravum_lang_sema::syntax::ast::{
    self, Block as AstBlock, Expr, ExprKind, FnItem, ItemKind, PatKind, StmtKind,
};
use ravum_lang_sema::syntax::{NodeId, Span};
use ravum_lang_sema::{CallTarget, CheckedProgram, MethodTarget, Resolution, Type};

use crate::ir::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// The program uses a construct the IR does not cover (fail-closed).
    Unsupported(String),
    /// A fact the checker should have produced is missing (internal error).
    MissingFact(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::Unsupported(s) => write!(f, "unsupported in IR v1.1: {s}"),
            LowerError::MissingFact(s) => write!(f, "missing checker fact: {s}"),
        }
    }
}

type R<T> = Result<T, LowerError>;

fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(LowerError::Unsupported(what.into()))
}

/// Largest number of scalar leaves an aggregate copy may unroll into.
const MAX_LEAVES: usize = 4096;

/// Map a checker type to the IR type of its *value*: scalars to themselves,
/// references and aggregates to `Ptr`.
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
        Type::Ref { .. } | Type::Nominal { .. } | Type::Tuple(_) | Type::Array { .. } => Ty::Ptr,
        other => return unsupported(format!("type `{other}`")),
    })
}

/// Memory layout of a value type.
#[derive(Debug, Clone)]
pub struct Layout {
    pub size: u32,
    pub align: u32,
}

fn round_up(x: u32, a: u32) -> u32 {
    if a <= 1 {
        x
    } else {
        x.div_ceil(a) * a
    }
}

/// Layouts of the program's aggregates, from the checker's struct facts.
pub struct Layouts<'a> {
    checked: &'a CheckedProgram,
}

impl<'a> Layouts<'a> {
    pub fn new(checked: &'a CheckedProgram) -> Self {
        Layouts { checked }
    }

    fn is_agg(t: &Type) -> bool {
        matches!(
            t,
            Type::Nominal { .. } | Type::Tuple(_) | Type::Array { .. }
        )
    }

    /// Fields of an aggregate: (offset, type), in declaration order.
    fn fields(&self, t: &Type) -> R<Vec<(u32, Type)>> {
        let tys: Vec<Type> = match t {
            Type::Nominal { def, args, .. } => {
                if !args.is_empty() {
                    return unsupported("generic struct");
                }
                let Some(info) = self.checked.facts.structs.get(def) else {
                    return unsupported("enum");
                };
                info.fields.iter().map(|f| f.ty.clone()).collect()
            }
            Type::Tuple(items) => items.clone(),
            Type::Array { elem, len } => {
                let l = self.layout(elem)?;
                let stride = round_up(l.size, l.align);
                return Ok((0..*len as u32)
                    .map(|i| (i * stride, (**elem).clone()))
                    .collect());
            }
            _ => return Err(LowerError::MissingFact("fields of a non-aggregate".into())),
        };
        let mut off = 0;
        let mut out = Vec::new();
        for ty in tys {
            let l = self.layout(&ty)?;
            off = round_up(off, l.align);
            out.push((off, ty));
            off += l.size;
        }
        Ok(out)
    }

    pub fn layout(&self, t: &Type) -> R<Layout> {
        if Self::is_agg(t) {
            if let Type::Array { elem, len } = t {
                let l = self.layout(elem)?;
                let stride = round_up(l.size, l.align);
                return Ok(Layout {
                    size: stride * (*len as u32),
                    align: l.align.max(1),
                });
            }
            let fields = self.fields(t)?;
            let mut align = 1;
            let mut end = 0;
            for (off, ty) in &fields {
                let l = self.layout(ty)?;
                align = align.max(l.align);
                end = end.max(off + l.size);
            }
            return Ok(Layout {
                size: round_up(end, align),
                align,
            });
        }
        match map_ty(t)? {
            Ty::Unit => unsupported("`()` stored in memory"),
            Ty::Ptr => unsupported("reference stored in memory"),
            s => Ok(Layout {
                size: s.size(),
                align: s.size(),
            }),
        }
    }

    /// Offset and type of field `name` of struct type `t`.
    fn named_field(&self, t: &Type, name: &str) -> R<(u32, Type)> {
        let Type::Nominal { def, .. } = t else {
            return Err(LowerError::MissingFact(format!(
                "field `{name}` of a non-struct"
            )));
        };
        let info = self
            .checked
            .facts
            .structs
            .get(def)
            .ok_or_else(|| LowerError::MissingFact("struct info".into()))?;
        let idx = info
            .fields
            .iter()
            .position(|f| f.name == name)
            .ok_or_else(|| LowerError::MissingFact(format!("field `{name}`")))?;
        Ok(self.fields(t)?[idx].clone())
    }

    fn leaves(&self, t: &Type) -> R<usize> {
        if Self::is_agg(t) {
            let mut n = 0;
            for (_, ty) in self.fields(t)? {
                n += self.leaves(&ty)?;
                if n > MAX_LEAVES {
                    return unsupported(format!("aggregate with more than {MAX_LEAVES} scalars"));
                }
            }
            Ok(n)
        } else {
            Ok(1)
        }
    }
}

/// Lower every function of `checked` into an IR module.
pub fn lower(checked: &CheckedProgram) -> R<Module> {
    let mut fns: Vec<(&ast::Item, &FnItem)> = Vec::new();
    for item in &checked.program.root.items {
        match &item.kind {
            ItemKind::Fn(f) => fns.push((item, f)),
            ItemKind::Use(_) => {}
            ItemKind::Struct(s) => {
                let info = checked.facts.structs.get(&item.id);
                if !info.is_some_and(|i| i.is_copy) {
                    return unsupported(format!("non-`Copy` struct `{}`", s.name.name));
                }
            }
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
    let layouts = Layouts::new(checked);
    let mut functions = Vec::new();
    for (item, f) in &fns {
        functions.push(FnLower::new(checked, &layouts, &ids, item, f)?.run(f)?);
    }
    let m = Module {
        version: IR_VERSION.to_string(),
        functions,
        state: None,
    };
    m.validate().map_err(LowerError::MissingFact)?;
    Ok(m)
}

struct LoopCtx {
    break_to: BlockId,
    continue_to: BlockId,
}

/// Where a value lives.
enum Place {
    /// A scalar local (register).
    Reg(Local),
    /// Memory at `addr + off`, holding a value of type `ty`.
    Mem { addr: Local, off: u32, ty: Type },
}

struct FnLower<'a> {
    checked: &'a CheckedProgram,
    layouts: &'a Layouts<'a>,
    ids: &'a BTreeMap<NodeId, FuncId>,
    locals: Vec<Ty>,
    stack: Vec<StackSlot>,
    blocks: Vec<Block>,
    cur: BlockId,
    /// (declaration span, name) → (slot, checker type). Aggregate variables
    /// hold the address of their own storage.
    vars: BTreeMap<(Span, String), (Local, Type)>,
    loops: Vec<LoopCtx>,
    ret: Type,
    /// Hidden result address when the function returns an aggregate.
    sret: Option<Local>,
    /// The current block follows a terminator: nothing emitted there runs.
    dead: bool,
}

impl<'a> FnLower<'a> {
    fn new(
        checked: &'a CheckedProgram,
        layouts: &'a Layouts<'a>,
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
        let mut me = FnLower {
            checked,
            layouts,
            ids,
            locals: Vec::new(),
            stack: Vec::new(),
            blocks: vec![Block {
                insts: Vec::new(),
                term: Term::Unreachable,
            }],
            cur: 0,
            vars: BTreeMap::new(),
            loops: Vec::new(),
            ret: sig.ret.clone(),
            sret: None,
            dead: false,
        };
        map_ty(&sig.ret)?;
        if matches!(sig.ret, Type::Ref { .. }) {
            return unsupported("function returning a reference");
        }
        if Layouts::is_agg(&sig.ret) {
            layouts.layout(&sig.ret)?;
            me.sret = Some(me.new_local(Ty::Ptr));
        }
        for p in &sig.params {
            if let Type::Ref { inner, .. } = &p.ty {
                if !Layouts::is_agg(inner) {
                    return unsupported("reference to a scalar parameter");
                }
                layouts.layout(inner)?;
            } else if Layouts::is_agg(&p.ty) {
                layouts.layout(&p.ty)?;
            }
            let ty = map_ty(&p.ty)?;
            let l = me.new_local(ty);
            me.vars
                .insert((item.span, p.name.clone()), (l, p.ty.clone()));
        }
        Ok(me)
    }

    fn run(mut self, f: &FnItem) -> R<Function> {
        let params = self.locals.len() as u32;
        let v = self.block(&f.body)?;
        let term = self.return_term(v)?;
        self.terminate(term);
        let ret = if self.sret.is_some() {
            Ty::Unit
        } else {
            map_ty(&self.ret)?
        };
        Ok(Function {
            name: f.name.name.clone(),
            params,
            ret,
            locals: self.locals,
            blocks: self.blocks,
            stack: self.stack,
        })
    }

    fn return_term(&mut self, v: Local) -> R<Term> {
        if let Some(out) = self.sret {
            if !self.dead {
                let ty = self.ret.clone();
                self.copy(out, 0, v, 0, &ty)?;
            }
            return Ok(Term::Return(None));
        }
        Ok(if map_ty(&self.ret)? == Ty::Unit {
            Term::Return(None)
        } else {
            Term::Return(Some(v))
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

    fn set_term(&mut self, t: Term) {
        if !self.dead {
            self.blocks[self.cur as usize].term = t;
        }
    }

    fn terminate(&mut self, t: Term) {
        self.set_term(t);
        self.cur = self.new_block();
        self.dead = true;
    }

    fn switch_to(&mut self, b: BlockId) {
        self.cur = b;
        self.dead = false;
    }

    fn type_of(&self, e: &Expr) -> R<Type> {
        self.checked
            .facts
            .expr_types
            .get(&e.id)
            .cloned()
            .ok_or_else(|| LowerError::MissingFact(format!("type of expression at {:?}", e.span)))
    }

    fn ty_of(&self, e: &Expr) -> R<Ty> {
        map_ty(&self.type_of(e)?)
    }

    fn unit(&mut self) -> Local {
        let l = self.new_local(Ty::Unit);
        self.emit(Inst::Const {
            dst: l,
            value: Const::Unit,
        });
        l
    }

    /// A fresh stack slot for a value of type `t`; returns its address.
    fn alloc(&mut self, t: &Type) -> R<Local> {
        let l = self.layouts.layout(t)?;
        self.layouts.leaves(t)?;
        self.stack.push(StackSlot {
            size: l.size.max(1),
            align: l.align.max(1),
        });
        let slot = (self.stack.len() - 1) as u32;
        let dst = self.new_local(Ty::Ptr);
        self.emit(Inst::StackAddr { dst, slot });
        Ok(dst)
    }

    fn u64_const(&mut self, v: u64) -> Local {
        let l = self.new_local(Ty::U64);
        self.emit(Inst::Const {
            dst: l,
            value: Const::Int {
                ty: Ty::U64,
                value: v as i128,
            },
        });
        l
    }

    /// `addr + off` as a fresh pointer (or `addr` itself when `off == 0`).
    fn offset(&mut self, addr: Local, off: u32) -> Local {
        if off == 0 {
            return addr;
        }
        let k = self.u64_const(off as u64);
        let dst = self.new_local(Ty::Ptr);
        self.emit(Inst::PtrAdd {
            dst,
            base: addr,
            index: k,
            scale: 1,
        });
        dst
    }

    /// Copy a value of type `t` from `src + so` to `dst + doff`, leaf by leaf.
    fn copy(&mut self, dst: Local, doff: u32, src: Local, so: u32, t: &Type) -> R<()> {
        if Layouts::is_agg(t) {
            for (off, fty) in self.layouts.fields(t)? {
                self.copy(dst, doff + off, src, so + off, &fty)?;
            }
            return Ok(());
        }
        let ty = map_ty(t)?;
        let tmp = self.new_local(ty);
        self.emit(Inst::Load {
            dst: tmp,
            ty,
            addr: src,
            offset: so,
        });
        self.emit(Inst::Store {
            ty,
            addr: dst,
            offset: doff,
            src: tmp,
        });
        Ok(())
    }

    /// Store the value `v` (a scalar local, or the address of an aggregate)
    /// of type `t` at `dst + off`.
    fn store_value(&mut self, dst: Local, off: u32, v: Local, t: &Type) -> R<()> {
        if Layouts::is_agg(t) {
            return self.copy(dst, off, v, 0, t);
        }
        let ty = map_ty(t)?;
        if ty == Ty::Unit {
            return Ok(());
        }
        if ty == Ty::Ptr {
            return unsupported("reference stored in memory");
        }
        self.emit(Inst::Store {
            ty,
            addr: dst,
            offset: off,
            src: v,
        });
        Ok(())
    }

    /// A private copy of the aggregate at `v`, in a fresh slot.
    fn snapshot(&mut self, v: Local, t: &Type) -> R<Local> {
        let dst = self.alloc(t)?;
        self.copy(dst, 0, v, 0, t)?;
        Ok(dst)
    }

    // ---- places ----------------------------------------------------------

    fn var_of_path(&self, path: &ast::Path) -> R<(Local, Type)> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Local { name, decl, .. }) => self
                .vars
                .get(&(*decl, name.clone()))
                .cloned()
                .ok_or_else(|| LowerError::MissingFact(format!("slot of `{name}`"))),
            Some(_) => unsupported("path to a non-local item used as a value"),
            None => Err(LowerError::MissingFact("path resolution".into())),
        }
    }

    /// Memory address and offset of an aggregate-valued base expression,
    /// following references (auto-deref of `&State` / `&mut State`).
    fn base_mem(&mut self, base: &Expr) -> R<(Local, u32, Type)> {
        let bt = self.type_of(base)?;
        if let Type::Ref { inner, .. } = &bt {
            let p = self.expr(base)?;
            return Ok((p, 0, (**inner).clone()));
        }
        match self.place(base)? {
            Place::Mem { addr, off, ty } => Ok((addr, off, ty)),
            Place::Reg(_) => Err(LowerError::MissingFact("aggregate in a register".into())),
        }
    }

    fn place(&mut self, e: &Expr) -> R<Place> {
        match &e.kind {
            ExprKind::Path(p) => {
                let (l, t) = self.var_of_path(p)?;
                if Layouts::is_agg(&t) {
                    Ok(Place::Mem {
                        addr: l,
                        off: 0,
                        ty: t,
                    })
                } else {
                    Ok(Place::Reg(l))
                }
            }
            ExprKind::Field { base, name } => {
                let (addr, off, bt) = self.base_mem(base)?;
                let (foff, fty) = self.layouts.named_field(&bt, &name.name)?;
                Ok(Place::Mem {
                    addr,
                    off: off + foff,
                    ty: fty,
                })
            }
            ExprKind::TupleIndex { base, index } => {
                let (addr, off, bt) = self.base_mem(base)?;
                let fields = self.layouts.fields(&bt)?;
                let (foff, fty) = fields
                    .get(*index as usize)
                    .cloned()
                    .ok_or_else(|| LowerError::MissingFact("tuple index".into()))?;
                Ok(Place::Mem {
                    addr,
                    off: off + foff,
                    ty: fty,
                })
            }
            ExprKind::Index { base, index } => {
                let (addr, off, bt) = self.base_mem(base)?;
                let Type::Array { elem, len } = &bt else {
                    return unsupported("indexing something other than a fixed array");
                };
                let (elem, len) = ((**elem).clone(), *len);
                let el = self.layouts.layout(&elem)?;
                let stride = round_up(el.size, el.align);
                let it = self.ty_of(index)?;
                let iv = self.expr(index)?;
                let i = self.new_local(Ty::U64);
                self.emit(Inst::Cast {
                    dst: i,
                    from: it,
                    to: Ty::U64,
                    src: iv,
                });
                // Bounds check (EXPR-7.6) before the address exists.
                let n = self.u64_const(len as u64);
                let ok = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: ok,
                    op: BinOp::Lt,
                    ty: Ty::U64,
                    lhs: i,
                    rhs: n,
                });
                let (inb, oob) = (self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: ok,
                    then_to: inb,
                    else_to: oob,
                });
                self.switch_to(oob);
                self.set_term(Term::Trap("index out of bounds".into()));
                self.switch_to(inb);
                let base_addr = self.offset(addr, off);
                let dst = self.new_local(Ty::Ptr);
                self.emit(Inst::PtrAdd {
                    dst,
                    base: base_addr,
                    index: i,
                    scale: stride,
                });
                Ok(Place::Mem {
                    addr: dst,
                    off: 0,
                    ty: elem,
                })
            }
            ExprKind::Unary {
                op: ast::UnOp::Deref,
                expr,
            } => {
                let t = self.type_of(expr)?;
                let Type::Ref { inner, .. } = t else {
                    return Err(LowerError::MissingFact("deref of a non-reference".into()));
                };
                let p = self.expr(expr)?;
                Ok(Place::Mem {
                    addr: p,
                    off: 0,
                    ty: *inner,
                })
            }
            _ => {
                // An rvalue: aggregates are already in memory.
                let t = self.type_of(e)?;
                let v = self.expr(e)?;
                if Layouts::is_agg(&t) {
                    Ok(Place::Mem {
                        addr: v,
                        off: 0,
                        ty: t,
                    })
                } else {
                    Ok(Place::Reg(v))
                }
            }
        }
    }

    /// Read a place: a scalar value, or the address of an aggregate.
    fn read(&mut self, p: Place) -> R<Local> {
        match p {
            Place::Reg(l) => Ok(l),
            Place::Mem { addr, off, ty } => {
                if Layouts::is_agg(&ty) {
                    return Ok(self.offset(addr, off));
                }
                let t = map_ty(&ty)?;
                let dst = self.new_local(t);
                self.emit(Inst::Load {
                    dst,
                    ty: t,
                    addr,
                    offset: off,
                });
                Ok(dst)
            }
        }
    }

    /// Write `v` (scalar, or address of an aggregate to copy) into a place.
    fn write(&mut self, p: Place, v: Local) -> R<()> {
        match p {
            Place::Reg(l) => {
                self.emit(Inst::Copy { dst: l, src: v });
                Ok(())
            }
            Place::Mem { addr, off, ty } => self.store_value(addr, off, v, &ty),
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
                        .cloned()
                        .ok_or_else(|| {
                            LowerError::MissingFact(format!("type of `{}`", name.name))
                        })?;
                    let Some(init) = &l.init else {
                        return unsupported("`let` without initializer");
                    };
                    if matches!(ty, Type::Ref { .. }) {
                        return unsupported("`let` binding a reference");
                    }
                    let v = self.expr(init)?;
                    let slot = if Layouts::is_agg(&ty) {
                        self.snapshot(v, &ty)?
                    } else {
                        let slot = self.new_local(map_ty(&ty)?);
                        self.emit(Inst::Copy { dst: slot, src: v });
                        slot
                    };
                    self.vars
                        .insert((l.pat.span, name.name.clone()), (slot, ty));
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
            ExprKind::Path(_)
            | ExprKind::Field { .. }
            | ExprKind::TupleIndex { .. }
            | ExprKind::Index { .. } => {
                let p = self.place(e)?;
                self.read(p)
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Unary { op, expr } => match op {
                ast::UnOp::Ref | ast::UnOp::RefMut => match self.place(expr)? {
                    Place::Mem { addr, off, .. } => Ok(self.offset(addr, off)),
                    Place::Reg(_) => unsupported("reference to a scalar local"),
                },
                ast::UnOp::Deref => {
                    let p = self.place(e)?;
                    self.read(p)
                }
                ast::UnOp::Neg | ast::UnOp::Not => {
                    let ty = self.ty_of(e)?;
                    // Fold `-literal` so that `-2147483648` is a valid i32.
                    if *op == ast::UnOp::Neg {
                        match &expr.kind {
                            ExprKind::Int { value, .. } => {
                                return self.int_const(ty, -(*value as i128))
                            }
                            ExprKind::Float { text, .. } => {
                                return self.float_const(ty, text, true)
                            }
                            _ => {}
                        }
                    }
                    let op = if *op == ast::UnOp::Neg {
                        UnOp::Neg
                    } else {
                        UnOp::Not
                    };
                    let src = self.expr(expr)?;
                    let dst = self.new_local(ty);
                    self.emit(Inst::Un { dst, op, ty, src });
                    Ok(dst)
                }
            },
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),
            ExprKind::Assign { target, value } => {
                // Evaluate the value first (into a private copy for an
                // aggregate), then write it into the place.
                let v = self.expr(value)?;
                let vt = self.type_of(value)?;
                let v = if Layouts::is_agg(&vt) {
                    self.snapshot(v, &vt)?
                } else {
                    v
                };
                let p = self.place(target)?;
                self.write(p, v)?;
                Ok(self.unit())
            }
            ExprKind::AssignOp { op, target, value } => {
                let rhs_ty = self.ty_of(value)?;
                let ty = self.ty_of(target)?;
                let (cur, p) = match self.place(target)? {
                    Place::Reg(l) => (l, Place::Reg(l)),
                    Place::Mem { addr, off, ty: t } => {
                        let v = self.read(Place::Mem {
                            addr,
                            off,
                            ty: t.clone(),
                        })?;
                        (v, Place::Mem { addr, off, ty: t })
                    }
                };
                let rhs = self.expr(value)?;
                let r = self.arith(op.bin_op(), ty, cur, rhs, rhs_ty)?;
                self.write(p, r)?;
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
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                let t = self.type_of(e)?;
                let dst = self.alloc(&t)?;
                let fields = self.layouts.fields(&t)?;
                for (item, (off, fty)) in items.iter().zip(fields) {
                    let v = self.expr(item)?;
                    self.store_value(dst, off, v, &fty)?;
                }
                Ok(dst)
            }
            ExprKind::ArrayRepeat { value, .. } => {
                let t = self.type_of(e)?;
                let dst = self.alloc(&t)?;
                let v = self.expr(value)?;
                for (off, fty) in self.layouts.fields(&t)? {
                    self.store_value(dst, off, v, &fty)?;
                }
                Ok(dst)
            }
            ExprKind::StructLit { fields, .. } => {
                let t = self.type_of(e)?;
                let dst = self.alloc(&t)?;
                // Field initializers run in source order (left to right).
                for fi in fields {
                    let v = self.expr(&fi.value)?;
                    let (off, fty) = self.layouts.named_field(&t, &fi.name.name)?;
                    self.store_value(dst, off, v, &fty)?;
                }
                Ok(dst)
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let t = self.type_of(e)?;
                let agg = Layouts::is_agg(&t);
                let result = if agg {
                    self.alloc(&t)?
                } else {
                    self.new_local(map_ty(&t)?)
                };
                let c = self.expr(cond)?;
                let (bt, be, join) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: c,
                    then_to: bt,
                    else_to: be,
                });
                self.switch_to(bt);
                let v = self.block(then_block)?;
                self.join_value(result, v, &t, agg)?;
                self.set_term(Term::Jump(join));
                self.switch_to(be);
                if let Some(eb) = else_branch {
                    let v = self.expr(eb)?;
                    self.join_value(result, v, &t, agg)?;
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
                if self.ty_of(e)? != Ty::Unit {
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
                let to = self
                    .loops
                    .last()
                    .map(|l| l.break_to)
                    .ok_or_else(|| LowerError::MissingFact("loop of `break`".into()))?;
                self.terminate(Term::Jump(to));
                Ok(self.unit())
            }
            ExprKind::Continue => {
                let to = self
                    .loops
                    .last()
                    .map(|l| l.continue_to)
                    .ok_or_else(|| LowerError::MissingFact("loop of `continue`".into()))?;
                self.terminate(Term::Jump(to));
                Ok(self.unit())
            }
            ExprKind::Return(v) => {
                let val = match v {
                    Some(v) => self.expr(v)?,
                    None => self.unit(),
                };
                let t = self.return_term(val)?;
                self.terminate(t);
                Ok(self.unit())
            }
            ExprKind::Call { args, .. } => self.call(e, args),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call(e, receiver, &method.name, args),
            ExprKind::Match { .. } => unsupported("match"),
            ExprKind::Str(_) => unsupported("string"),
            ExprKind::Char(_) => unsupported("char"),
            ExprKind::Range { .. } => unsupported("range outside `for`"),
            ExprKind::Try(_) => unsupported("`?`"),
            ExprKind::Unsafe(_) => unsupported("unsafe"),
            ExprKind::Await(_) => unsupported("await"),
            ExprKind::SelfValue => unsupported("self"),
        }
    }

    fn join_value(&mut self, result: Local, v: Local, t: &Type, agg: bool) -> R<()> {
        if self.dead {
            return Ok(());
        }
        if agg {
            self.copy(result, 0, v, 0, t)
        } else {
            self.emit(Inst::Copy {
                dst: result,
                src: v,
            });
            Ok(())
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
        let bad = || LowerError::MissingFact(format!("float `{text}`"));
        let value = match ty {
            Ty::F32 => {
                let v: f32 = clean.parse().map_err(|_| bad())?;
                Const::F32Bits(if neg { -v } else { v }.to_bits())
            }
            Ty::F64 => {
                let v: f64 = clean.parse().map_err(|_| bad())?;
                Const::F64Bits(if neg { -v } else { v }.to_bits())
            }
            _ => return unsupported("float literal of non-float type"),
        };
        let l = self.new_local(ty);
        self.emit(Inst::Const { dst: l, value });
        Ok(l)
    }

    fn binary(&mut self, op: ast::BinOp, lhs: &Expr, rhs: &Expr) -> R<Local> {
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
            let (then_to, else_to) = if op == B::And {
                (rhs_b, join)
            } else {
                (join, rhs_b)
            };
            self.set_term(Term::Branch {
                cond: l,
                then_to,
                else_to,
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
        if lt == Ty::Ptr || rt == Ty::Ptr {
            return unsupported("operator on an aggregate or a reference");
        }
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
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
            // The shift amount is always a `u64` slot (negative amounts are
            // refused by the checker, D-19-1).
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
        let cmp = matches!(
            iop,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        );
        let dst = self.new_local(if cmp { Ty::Bool } else { ty });
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
        let name = match &pat.kind {
            PatKind::Binding { name, .. } => Some(name.name.clone()),
            PatKind::Wild => None,
            _ => return unsupported("`for` pattern"),
        };
        let st = self.type_of(start)?;
        let ty = map_ty(&st)?;
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
        if let Some(name) = name {
            self.vars.insert((pat.span, name), (var, st));
        }
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

    /// IR v1.2: the scalar builtin methods whose semantics map exactly onto
    /// one IR operation. Anything else (user methods, `checked_*` returning
    /// `Option`, `round`/`min`/`max`/`powf` whose NaN or tie rules would need
    /// more care, `to_string`) stays `Unsupported`.
    fn method_call(&mut self, e: &Expr, recv: &Expr, name: &str, args: &[Expr]) -> R<Local> {
        let path = match self.checked.facts.methods.get(&e.id) {
            Some(MethodTarget::Builtin { path, .. }) => path.clone(),
            _ => return unsupported(format!("method call `.{name}()`")),
        };
        let un = match path.as_str() {
            "int::abs" | "f64::abs" => Some(UnOp::Abs),
            "int::wrapping_neg" => Some(UnOp::WrappingNeg),
            "int::saturating_neg" => Some(UnOp::SaturatingNeg),
            "f64::sqrt" => Some(UnOp::Sqrt),
            "f64::floor" => Some(UnOp::Floor),
            "f64::ceil" => Some(UnOp::Ceil),
            _ => None,
        };
        let bin = match path.as_str() {
            "int::wrapping_add" => Some(BinOp::WrappingAdd),
            "int::wrapping_sub" => Some(BinOp::WrappingSub),
            "int::wrapping_mul" => Some(BinOp::WrappingMul),
            "int::saturating_add" => Some(BinOp::SaturatingAdd),
            "int::saturating_sub" => Some(BinOp::SaturatingSub),
            "int::saturating_mul" => Some(BinOp::SaturatingMul),
            _ => None,
        };
        let ty = self.ty_of(recv)?;
        match (un, bin, args) {
            (Some(op), None, []) if crate::ir::un_op_accepts(op, ty) => {
                let src = self.expr(recv)?;
                let dst = self.new_local(ty);
                self.emit(Inst::Un { dst, op, ty, src });
                Ok(dst)
            }
            (None, Some(op), [arg])
                if crate::ir::bin_op_accepts(op, ty) && self.ty_of(arg)? == ty =>
            {
                let lhs = self.expr(recv)?;
                let rhs = self.expr(arg)?;
                let dst = self.new_local(ty);
                self.emit(Inst::Bin {
                    dst,
                    op,
                    ty,
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            _ => unsupported(format!("method call `.{name}()` (`{path}`)")),
        }
    }

    fn call(&mut self, e: &Expr, args: &[Expr]) -> R<Local> {
        match self.checked.facts.calls.get(&e.id) {
            Some(CallTarget::UserFn { item }) => {
                let func = *self.ids.get(item).ok_or_else(|| {
                    LowerError::Unsupported("call to a function outside the root module".into())
                })?;
                let rt = self.type_of(e)?;
                let mut vals = Vec::new();
                let sret = if Layouts::is_agg(&rt) {
                    let out = self.alloc(&rt)?;
                    vals.push(out);
                    Some(out)
                } else {
                    None
                };
                // MACH O1: arguments are evaluated left to right; a by-value
                // aggregate is copied at once, so later arguments cannot
                // change it.
                for a in args {
                    let at = self.type_of(a)?;
                    let v = self.expr(a)?;
                    vals.push(if Layouts::is_agg(&at) {
                        self.snapshot(v, &at)?
                    } else {
                        v
                    });
                }
                if let Some(out) = sret {
                    self.emit(Inst::Call {
                        dst: None,
                        func,
                        args: vals,
                    });
                    return Ok(out);
                }
                let dst = self.new_local(map_ty(&rt)?);
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
            None => Err(LowerError::MissingFact("call target".into())),
        }
    }
}
