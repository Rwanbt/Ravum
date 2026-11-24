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

use ravum_lang_sema::facts::ConstValue;
use ravum_lang_sema::syntax::ast::{
    self, Block as AstBlock, Expr, ExprKind, FnItem, ItemKind, PatKind, StmtKind,
};
use ravum_lang_sema::syntax::{NodeId, Span};
use ravum_lang_sema::{
    CallTarget, CheckedProgram, ConstInfo, MethodTarget, Resolution, Type, VariantKind,
};

use crate::ir::*;
use crate::mono::{substitute, Subst};
use ravum_lang_sema::contracts::{self, Shown};

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
        // IR v1.3: heap objects, and borrows of them, are handles.
        Type::String | Type::Vec(_) => Ty::Obj,
        Type::Ref { inner, .. } if matches!(**inner, Type::String | Type::Str | Type::Vec(_)) => {
            Ty::Obj
        }
        Type::Ref { .. }
        | Type::Nominal { .. }
        | Type::Tuple(_)
        | Type::Array { .. }
        | Type::Option(_)
        | Type::Result(..) => Ty::Ptr,
        other => return unsupported(format!("type `{other}`")),
    })
}

/// Fields `(offset, type)` of one aggregate or variant.
type FieldList = Vec<(u32, Type)>;

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
            Type::Nominal { .. }
                | Type::Tuple(_)
                | Type::Array { .. }
                | Type::Option(_)
                | Type::Result(..)
        )
    }

    /// Payload types of each variant when `t` is an enum (`Option<T>`: `None`
    /// is variant 0, `Some` variant 1), `None` for any other type.
    fn variants(&self, t: &Type) -> R<Option<Vec<Vec<Type>>>> {
        match t {
            Type::Option(inner) => Ok(Some(vec![vec![], vec![(**inner).clone()]])),
            // `Ok` is variant 0, `Err` variant 1.
            Type::Result(ok, err) => Ok(Some(vec![vec![(**ok).clone()], vec![(**err).clone()]])),
            Type::Nominal { def, args, .. } => {
                let Some(info) = self.checked.facts.enums.get(def) else {
                    return Ok(None);
                };
                if args.len() != info.generics.len() {
                    return Err(LowerError::MissingFact("generic enum arity".into()));
                }
                // A generic enum is laid out per instance, like a generic
                // struct. A non-`Copy` payload (a `String`) is a handle: the
                // enum is moved word for word and the checker forbids reuse
                // of a moved value, so the handle has one owner.
                let subst: crate::mono::Subst = args.iter().cloned().enumerate().collect();
                let vs: Vec<Vec<Type>> = info
                    .variants
                    .iter()
                    .map(|v| {
                        let tys: Vec<Type> = match v.kind {
                            VariantKind::Struct => v.fields.iter().map(|f| f.ty.clone()).collect(),
                            _ => v.tuple_types.clone(),
                        };
                        tys.iter()
                            .map(|t| crate::mono::substitute(t, &subst))
                            .collect()
                    })
                    .collect();
                if vs.iter().flatten().any(crate::mono::has_param) {
                    return unsupported("generic enum with an unresolved type argument");
                }
                Ok(Some(vs))
            }
            _ => Ok(None),
        }
    }

    /// Enum layout: a `u32` tag at offset 0, then every variant's payload laid
    /// out like a tuple from the same offset. Returns the layout and, per
    /// variant, its fields `(offset, type)`.
    fn enum_layout(&self, vs: &[Vec<Type>]) -> R<(Layout, Vec<FieldList>)> {
        let mut align = 4;
        for v in vs {
            for t in v {
                align = align.max(self.layout(t)?.align);
            }
        }
        let start = round_up(4, align);
        let mut end = start;
        let mut out = Vec::new();
        for v in vs {
            let mut off = start;
            let mut fs = Vec::new();
            for t in v {
                let l = self.layout(t)?;
                off = round_up(off, l.align);
                fs.push((off, t.clone()));
                off += l.size;
            }
            end = end.max(off);
            out.push(fs);
        }
        Ok((
            Layout {
                size: round_up(end, align),
                align,
            },
            out,
        ))
    }

    /// Fields `(offset, type)` of variant `v` of enum type `t`.
    fn variant_fields(&self, t: &Type, v: usize) -> R<Vec<(u32, Type)>> {
        let vs = self
            .variants(t)?
            .ok_or_else(|| LowerError::MissingFact("variant of a non-enum".into()))?;
        let (_, fs) = self.enum_layout(&vs)?;
        fs.get(v)
            .cloned()
            .ok_or_else(|| LowerError::MissingFact("variant index".into()))
    }

    /// Fields of an aggregate: (offset, type), in declaration order. An enum
    /// is copied as raw words (its payload depends on the tag), so its
    /// "fields" are the aligned words that cover it.
    fn fields(&self, t: &Type) -> R<Vec<(u32, Type)>> {
        if let Some(vs) = self.variants(t)? {
            let (l, _) = self.enum_layout(&vs)?;
            let (w, wt) = match l.align {
                8.. => (8, Type::U64),
                4 => (4, Type::U32),
                2 => (2, Type::U16),
                _ => (1, Type::U8),
            };
            return Ok((0..l.size / w).map(|i| (i * w, wt.clone())).collect());
        }
        let tys: Vec<Type> = match t {
            // The builtin `Error` (STD-7.5): `struct Error { message: String }`.
            Type::Nominal { def, name, .. }
                if name == "Error" && !self.checked.facts.structs.contains_key(def) =>
            {
                vec![Type::String]
            }
            Type::Nominal { def, args, .. } => {
                let Some(info) = self.checked.facts.structs.get(def) else {
                    return Err(LowerError::MissingFact("struct info".into()));
                };
                if args.len() != info.generics.len() {
                    return Err(LowerError::MissingFact("generic struct arity".into()));
                }
                // A generic struct is laid out per instance: its fields with
                // the type arguments substituted (IR v1.2 monomorphization).
                let subst: crate::mono::Subst = args.iter().cloned().enumerate().collect();
                let tys: Vec<Type> = info
                    .fields
                    .iter()
                    .map(|f| crate::mono::substitute(&f.ty, &subst))
                    .collect();
                if tys.iter().any(crate::mono::has_param) {
                    return unsupported("generic struct with an unresolved type argument");
                }
                tys
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
            if let Some(vs) = self.variants(t)? {
                return Ok(self.enum_layout(&vs)?.0);
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
            // `()` occupies no memory: a `Result<(), E>` payload, a unit
            // field. Loads and stores of it are elided.
            Ty::Unit => Ok(Layout { size: 0, align: 1 }),
            Ty::Ptr => unsupported("reference stored in memory"),
            s => Ok(Layout {
                size: s.size(),
                align: s.size(),
            }),
        }
    }

    /// Offset and type of field `name` of struct type `t`.
    fn named_field(&self, t: &Type, name: &str) -> R<(u32, Type)> {
        if let Type::Nominal { def, name: n, .. } = t {
            if n == "Error" && name == "message" && !self.checked.facts.structs.contains_key(def) {
                return Ok((0, Type::String));
            }
        }
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
    let mut invariants: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    let mut method_owner: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for item in &checked.program.root.items {
        match &item.kind {
            ItemKind::Fn(f) => fns.push((item, f)),
            ItemKind::Use(_) => {}
            ItemKind::Struct(s) => {
                if !checked.facts.structs.contains_key(&item.id) {
                    return Err(LowerError::MissingFact(format!("struct `{}`", s.name.name)));
                }
            }
            ItemKind::Enum(e) => {
                // Non-`Copy` and generic enums are laid out per use
                // (`Layouts::variants`); only the facts must exist.
                if !checked.facts.enums.contains_key(&item.id) {
                    return Err(LowerError::MissingFact(format!("enum `{}`", e.name.name)));
                }
            }
            ItemKind::Const(c) => match checked.facts.consts.get(&item.id) {
                Some(ConstInfo {
                    value: ConstValue::Int(_) | ConstValue::Bool(_),
                    ..
                }) => {}
                _ => return unsupported(format!("const `{}` of this type", c.name.name)),
            },
            ItemKind::Impl(imp) => {
                if !imp.generics.is_empty() {
                    return unsupported("generic impl");
                }
                let owner = match checked.facts.type_nodes.get(&imp.self_ty.id) {
                    Some(Type::Nominal { def, .. }) => Some(*def),
                    _ => None,
                };
                for sub in &imp.items {
                    match &sub.kind {
                        ItemKind::Fn(f) => {
                            if let Some(def) = owner {
                                method_owner.insert(sub.id, def);
                                if f.name.name.starts_with(ast::INVARIANT_METHOD_PREFIX) {
                                    invariants.entry(def).or_default().push(sub.id);
                                }
                            }
                            fns.push((sub, f))
                        }
                        _ => return unsupported("non-method item in an impl"),
                    }
                }
            }
            ItemKind::Mod(_) | ItemKind::ModDecl(_) => return unsupported("module"),
            ItemKind::Extern(_) => return unsupported("extern"),
        }
    }
    // Generic functions are lowered per instance, on demand (`FnTable`).
    let by_id: BTreeMap<NodeId, (&ast::Item, &FnItem)> =
        fns.iter().map(|(i, f)| (i.id, (*i, *f))).collect();
    let plain: Vec<(&ast::Item, &FnItem)> = fns
        .into_iter()
        .filter(|(_, f)| f.generics.is_empty())
        .collect();
    let table = std::cell::RefCell::new(FnTable {
        plain: plain
            .iter()
            .enumerate()
            .map(|(i, (item, _))| (item.id, i as FuncId))
            .collect(),
        generic: by_id
            .iter()
            .filter(|(_, (_, f))| !f.generics.is_empty())
            .map(|(id, _)| *id)
            .collect(),
        next: plain.len() as FuncId,
        invariants,
        method_owner,
        ..FnTable::default()
    });
    let layouts = Layouts::new(checked);
    let mut functions = Vec::new();
    for (item, f) in &plain {
        functions.push(FnLower::new(checked, &layouts, &table, item, f, Subst::new())?.run(f)?);
    }
    let mut done = 0;
    loop {
        let next = table.borrow().queue.get(done).cloned();
        let Some((item_id, subst)) = next else { break };
        done += 1;
        if done > MAX_INSTANCES {
            return unsupported(format!("more than {MAX_INSTANCES} generic instances"));
        }
        let (item, f) = by_id[&item_id];
        functions.push(FnLower::new(checked, &layouts, &table, item, f, subst)?.run(f)?);
    }
    // STD (stdlib.md §13 table): `main() -> Result<(), Error>` returning
    // `Err(e)` prints `error: <message>` on stderr. Its body takes a hidden
    // result address, so the entry `main` becomes a parameterless wrapper
    // that calls it (renamed `main$body`) and reports an `Err`.
    if let Some((item, _)) = plain.iter().find(|(_, f)| f.name.name == "main") {
        let ret = checked
            .facts
            .fn_sigs
            .get(&item.id)
            .map(|s| s.ret.clone())
            .ok_or_else(|| LowerError::MissingFact("signature of `main`".into()))?;
        if matches!(&ret, Type::Result(ok, err) if **ok == Type::Unit && is_error(err)) {
            let id = table.borrow().plain[&item.id];
            functions[id as usize].name = "main$body".into();
            functions.push(main_wrapper(&layouts, &ret, id)?);
        }
    }
    let m = Module {
        version: IR_VERSION.to_string(),
        functions,
        state: None,
        migrate_from: None,
    };
    m.validate().map_err(LowerError::MissingFact)?;
    Ok(m)
}

/// Largest number of generic instances one program may need.
const MAX_INSTANCES: usize = 1024;

/// Function ids: plain functions first, then generic instances in creation
/// order (the order they are lowered and pushed in).
#[derive(Default)]
struct FnTable {
    plain: BTreeMap<NodeId, FuncId>,
    generic: std::collections::BTreeSet<NodeId>,
    instances: BTreeMap<(NodeId, String), FuncId>,
    queue: Vec<(NodeId, Subst)>,
    next: FuncId,
    /// RFC-INVARIANTS-1: struct → its synthetic invariant methods (in
    /// clause order), and method item → the struct of its `impl`.
    invariants: BTreeMap<NodeId, Vec<NodeId>>,
    method_owner: BTreeMap<NodeId, NodeId>,
}

impl FnTable {
    fn instance(&mut self, item: NodeId, subst: &Subst) -> FuncId {
        let key = (item, format!("{subst:?}"));
        if let Some(id) = self.instances.get(&key) {
            return *id;
        }
        let id = self.next;
        self.next += 1;
        self.instances.insert(key, id);
        self.queue.push((item, subst.clone()));
        id
    }
}

struct LoopCtx {
    break_to: BlockId,
    continue_to: BlockId,
}

/// Where a value lives.
enum Place {
    /// A scalar local (register).
    Reg(Local),
    /// IR v1.3: element `i` of the heap vector `h` (scalar elements).
    VecElem { h: Local, i: Local, ty: Ty },
    /// T14: (part of) an aggregate element of a `Vec`. The vector holds one
    /// handle per element, to a word vector `o` with one word per scalar
    /// leaf; this place is the leaves `base..` of type `ty` in `o`.
    VecAgg { o: Local, base: u32, ty: Type },
    /// Memory at `addr + off`, holding a value of type `ty`.
    Mem { addr: Local, off: u32, ty: Type },
}

struct FnLower<'a> {
    checked: &'a CheckedProgram,
    layouts: &'a Layouts<'a>,
    ids: &'a std::cell::RefCell<FnTable>,
    /// Type arguments of this instance (empty for a plain function).
    subst: Subst,
    locals: Vec<Ty>,
    stack: Vec<StackSlot>,
    blocks: Vec<Block>,
    cur: BlockId,
    /// (declaration span, name) → (slot, checker type). Aggregate variables
    /// hold the address of their own storage.
    vars: BTreeMap<(Span, String), (Local, Type)>,
    /// `let` scalars whose address is taken (`&x`, `&mut x`): they live in a
    /// stack slot from their declaration on, `mem_vars` holds those slots.
    borrowed: std::collections::BTreeSet<(Span, String)>,
    mem_vars: std::collections::BTreeSet<Local>,
    /// `&mut v[i]` of an aggregate `Vec` element: a private copy is lent to
    /// the call and stored back after it (Core 1.0 cannot keep a borrow past
    /// a call). `wb_marks` opens one frame per user call being lowered.
    writebacks: Vec<(Local, u32, Type, Local)>,
    wb_marks: Vec<usize>,
    loops: Vec<LoopCtx>,
    ret: Type,
    /// Hidden result address when the function returns an aggregate.
    sret: Option<Local>,
    /// The current block follows a terminator: nothing emitted there runs.
    dead: bool,
    /// IR name: the plain name of a free function, `S::m` for a method.
    name: String,
    /// RFC-CONTRACTS-1 clauses, and the parameters' entry copies that
    /// `ensures` reads (CON-3: their value on entry).
    contracts: Vec<ast::Contract>,
    entry_params: Vec<((Span, String), (Local, Type))>,
    /// Re-entrancy guard: an `ensures` is lowered once per return point.
    in_ensures: bool,
    /// Amendment 1: the parameters a violation message shows (by their
    /// declared, unsubstituted type), and how `result` is shown.
    shown_params: Vec<((Span, String), Shown)>,
    shown_result: Option<Shown>,
    /// RFC-INVARIANTS-1 INV-3: a `&mut self` method of a struct with
    /// invariants: (struct, receiver address), checked at every return.
    inv_receiver: Option<(NodeId, Local)>,
}

impl<'a> FnLower<'a> {
    fn new(
        checked: &'a CheckedProgram,
        layouts: &'a Layouts<'a>,
        ids: &'a std::cell::RefCell<FnTable>,
        item: &'a ast::Item,
        f: &FnItem,
        subst: Subst,
    ) -> R<Self> {
        if (0..f.generics.len()).any(|i| !subst.contains_key(&i)) {
            return unsupported("generic function without its type arguments");
        }
        let sig0 =
            checked.facts.fn_sigs.get(&item.id).ok_or_else(|| {
                LowerError::MissingFact(format!("signature of `{}`", f.name.name))
            })?;
        let mut sig = sig0.clone();
        sig.ret = substitute(&sig.ret, &subst);
        for p in &mut sig.params {
            p.ty = substitute(&p.ty, &subst);
        }
        let sig = &sig;
        // A method: the receiver is the first parameter (`self` by value, or
        // `&self`/`&mut self` as an address); its IR name is its path
        // (`S::m`) so that two types' methods never share a name.
        let path = sig
            .rust_path
            .strip_prefix("crate::")
            .unwrap_or(&sig.rust_path);
        let mut name = if path.contains("::") {
            path.to_string()
        } else {
            f.name.name.clone()
        };
        if !subst.is_empty() {
            let args: Vec<String> = subst.values().map(|t| t.to_string()).collect();
            name = format!("{name}<{}>", args.join(", "));
        }
        let mut me = FnLower {
            checked,
            layouts,
            ids,
            subst,
            locals: Vec::new(),
            stack: Vec::new(),
            blocks: vec![Block {
                insts: Vec::new(),
                term: Term::Unreachable,
            }],
            cur: 0,
            vars: BTreeMap::new(),
            borrowed: std::collections::BTreeSet::new(),
            mem_vars: std::collections::BTreeSet::new(),
            writebacks: Vec::new(),
            wb_marks: Vec::new(),
            loops: Vec::new(),
            ret: sig.ret.clone(),
            sret: None,
            dead: false,
            name,
            contracts: f.contracts.clone(),
            entry_params: Vec::new(),
            in_ensures: false,
            shown_params: sig0
                .params
                .iter()
                .filter(|p| !p.is_receiver)
                .filter_map(|p| {
                    contracts::shown(&p.ty, true).map(|s| ((item.span, p.name.clone()), s))
                })
                .collect(),
            shown_result: contracts::shown(&sig0.ret, true),
            inv_receiver: None,
        };
        map_ty(&sig.ret)?;
        // Core 1.0 returns only `&'static str` (E-OWN-REF-ESCAPE otherwise):
        // a heap handle. A pointer return stays refused.
        if matches!(sig.ret, Type::Ref { .. }) && map_ty(&sig.ret)? != Ty::Obj {
            return unsupported("function returning a reference");
        }
        if Layouts::is_agg(&sig.ret) {
            layouts.layout(&sig.ret)?;
            me.sret = Some(me.new_local(Ty::Ptr));
        }
        for p in &sig.params {
            if let Type::Ref { inner, .. } = &p.ty {
                if map_ty(&p.ty)? == Ty::Obj {
                    // `&String`/`&mut Vec<T>`: the handle itself.
                } else if !Layouts::is_agg(inner) && !plain_scalar(map_ty(inner)?) {
                    return unsupported("reference to a scalar parameter");
                } else {
                    layouts.layout(inner)?;
                }
            } else if Layouts::is_agg(&p.ty) {
                layouts.layout(&p.ty)?;
            }
            let ty = map_ty(&p.ty)?;
            let l = me.new_local(ty);
            me.vars
                .insert((item.span, p.name.clone()), (l, p.ty.clone()));
        }
        if matches!(sig.receiver, Some(ast::Receiver::Exclusive)) {
            let t = ids.borrow();
            if let Some(def) = t.method_owner.get(&item.id) {
                if t.invariants.contains_key(def) {
                    me.inv_receiver = Some((*def, 0));
                }
            }
        }
        Ok(me)
    }

    fn run(mut self, f: &FnItem) -> R<Function> {
        let params = self.locals.len() as u32;
        if f.is_async && !self.contracts.is_empty() {
            return unsupported("contracts on an `async fn`");
        }
        // CON-3: `ensures` sees the parameters as they were on entry.
        if self
            .contracts
            .iter()
            .any(|c| c.kind == ast::ContractKind::Ensures)
        {
            let keys: Vec<_> = self
                .vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, (l, t)) in keys {
                if matches!(t, Type::Ref { .. }) {
                    continue; // a reference is read as is
                }
                let c = if Layouts::is_agg(&t) {
                    self.snapshot(l, &t)?
                } else {
                    let c = self.new_local(map_ty(&t)?);
                    self.emit(Inst::Copy { dst: c, src: l });
                    c
                };
                self.entry_params.push((k, (c, t)));
            }
        }
        // CON-5: `requires`, in order, after the arguments.
        let reqs: Vec<ast::Contract> = self
            .contracts
            .iter()
            .filter(|c| c.kind == ast::ContractKind::Requires)
            .cloned()
            .collect();
        for c in &reqs {
            self.contract_check(c)?;
        }
        self.borrowed = borrowed_locals(self.checked, &f.body);
        let v = self.block(&f.body)?;
        let term = self.return_term(v)?;
        self.terminate(term);
        let ret = if self.sret.is_some() {
            Ty::Unit
        } else {
            map_ty(&self.ret)?
        };
        Ok(Function {
            name: self.name.clone(),
            params,
            ret,
            locals: self.locals,
            blocks: prune_unreachable(self.blocks),
            stack: self.stack,
        })
    }

    /// Evaluate one clause; continue when true, trap with its message
    /// otherwise (CON-5/CON-6).
    fn contract_check(&mut self, c: &ast::Contract) -> R<()> {
        let ok = self.expr(&c.expr)?;
        let (good, bad) = (self.new_block(), self.new_block());
        self.set_term(Term::Branch {
            cond: ok,
            then_to: good,
            else_to: bad,
        });
        self.switch_to(bad);
        self.contract_note(c.kind)?;
        let kind = match c.kind {
            ast::ContractKind::Requires => "requires",
            ast::ContractKind::Ensures => "ensures",
        };
        self.set_term(Term::Trap(format!("contract violated: {kind} {}", c.text)));
        self.switch_to(good);
        Ok(())
    }

    /// RFC-INVARIANTS-1: call each invariant method of struct `def` on the
    /// value at `addr`, in order; trap on the first false one, the note
    /// showing the fields (same text as the `rustc` build).
    fn check_invariants(&mut self, def: NodeId, addr: Local) -> R<()> {
        let methods = self
            .ids
            .borrow()
            .invariants
            .get(&def)
            .cloned()
            .unwrap_or_default();
        if methods.is_empty() {
            return Ok(());
        }
        let info = self
            .checked
            .facts
            .structs
            .get(&def)
            .ok_or_else(|| LowerError::MissingFact("invariant struct".into()))?;
        let texts: Vec<String> = info.invariants.iter().map(|(_, t)| t.clone()).collect();
        let fields: Vec<(String, Type)> = info
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        let st = Type::Nominal {
            def,
            name: info.name.clone(),
            args: Vec::new(),
        };
        for (m, text) in methods.iter().zip(texts) {
            let func = *self
                .ids
                .borrow()
                .plain
                .get(m)
                .ok_or_else(|| LowerError::MissingFact("invariant method".into()))?;
            let ok = self.new_local(Ty::Bool);
            self.emit(Inst::Call {
                dst: Some(ok),
                func,
                args: vec![addr],
            });
            let (good, bad) = (self.new_block(), self.new_block());
            self.set_term(Term::Branch {
                cond: ok,
                then_to: good,
                else_to: bad,
            });
            self.switch_to(bad);
            let mut first = true;
            for (name, fty) in &fields {
                let Some(s) = contracts::shown(fty, true) else {
                    continue;
                };
                let (off, _) = self.layouts.named_field(&st, name)?;
                let ty = map_ty(fty)?;
                let v = self.new_local(ty);
                self.emit(Inst::Load {
                    dst: v,
                    ty,
                    addr,
                    offset: off,
                });
                let sep = if first { " (" } else { ", " };
                first = false;
                self.emit(Inst::Print {
                    stream: Stream::Note,
                    arg: PrintArg::Text(format!("{sep}self.{name} = ")),
                });
                let quoted = s == Shown::Text;
                if quoted {
                    self.emit(Inst::Print {
                        stream: Stream::Note,
                        arg: PrintArg::Text("\"".into()),
                    });
                }
                self.emit(Inst::Print {
                    stream: Stream::Note,
                    arg: if quoted {
                        PrintArg::Obj(v)
                    } else {
                        PrintArg::Value { ty, src: v }
                    },
                });
                if quoted {
                    self.emit(Inst::Print {
                        stream: Stream::Note,
                        arg: PrintArg::Text("\"".into()),
                    });
                }
            }
            if !first {
                self.emit(Inst::Print {
                    stream: Stream::Note,
                    arg: PrintArg::Text(")".into()),
                });
            }
            self.set_term(Term::Trap(format!("contract violated: invariant {text}")));
            self.switch_to(good);
        }
        Ok(())
    }

    /// Amendment 1: ` (a = 1, s = "x", result = 2)` written to the trap note,
    /// the same text as the `rustc` build's message.
    fn contract_note(&mut self, kind: ast::ContractKind) -> R<()> {
        let ensures = kind == ast::ContractKind::Ensures;
        let mut shown: Vec<(String, Local, Type, Shown)> = Vec::new();
        for (key, s) in self.shown_params.clone() {
            if ensures
                && s == Shown::Text
                && !matches!(self.vars.get(&key), Some((_, Type::Ref { .. })))
            {
                continue; // owned `String`: consumed by the body
            }
            let Some((l, t)) = self.vars.get(&key).cloned() else {
                return Err(LowerError::MissingFact(format!("parameter `{}`", key.1)));
            };
            shown.push((key.1, l, t, s));
        }
        if ensures {
            if let Some(s) = self.shown_result {
                let key = self
                    .vars
                    .keys()
                    .find(|(_, n)| n == "result")
                    .cloned()
                    .ok_or_else(|| LowerError::MissingFact("`result`".into()))?;
                let (l, t) = self.vars[&key].clone();
                shown.push(("result".into(), l, t, s));
            }
        }
        if shown.is_empty() {
            return Ok(());
        }
        let note = |me: &mut Self, arg| {
            me.emit(Inst::Print {
                stream: Stream::Note,
                arg,
            })
        };
        note(self, PrintArg::Text(" (".into()));
        for (i, (name, l, t, s)) in shown.into_iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            note(self, PrintArg::Text(format!("{sep}{name} = ")));
            match s {
                Shown::Scalar => {
                    let ty = map_ty(&t)?;
                    note(self, PrintArg::Value { ty, src: l });
                }
                Shown::Text => {
                    note(self, PrintArg::Text("\"".into()));
                    note(self, PrintArg::Obj(l));
                    note(self, PrintArg::Text("\"".into()));
                }
            }
        }
        note(self, PrintArg::Text(")".into()));
        Ok(())
    }

    /// CON-6: every `ensures`, `result` bound to `v`, parameters read at
    /// their entry value.
    fn check_ensures(&mut self, v: Local) -> R<()> {
        let ens: Vec<ast::Contract> = self
            .contracts
            .iter()
            .filter(|c| c.kind == ast::ContractKind::Ensures)
            .cloned()
            .collect();
        if ens.is_empty() || self.dead || self.in_ensures {
            return Ok(());
        }
        self.in_ensures = true;
        let saved: Vec<_> = self
            .entry_params
            .iter()
            .map(|(k, _)| (k.clone(), self.vars.get(k).cloned()))
            .collect();
        for (k, e) in self.entry_params.clone() {
            self.vars.insert(k, e);
        }
        let ret = self.ret.clone();
        for c in &ens {
            self.vars
                .insert((c.span, "result".to_string()), (v, ret.clone()));
            let r = self.contract_check(c);
            self.vars.remove(&(c.span, "result".to_string()));
            if let Err(e) = r {
                self.in_ensures = false;
                return Err(e);
            }
        }
        for (k, old) in saved {
            match old {
                Some(o) => {
                    self.vars.insert(k, o);
                }
                None => {
                    self.vars.remove(&k);
                }
            }
        }
        self.in_ensures = false;
        Ok(())
    }

    fn return_term(&mut self, v: Local) -> R<Term> {
        self.check_ensures(v)?;
        if let Some((def, recv)) = self.inv_receiver {
            if !self.dead {
                self.check_invariants(def, recv)?;
            }
        }
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
            .map(|t| substitute(t, &self.subst))
            .ok_or_else(|| LowerError::MissingFact(format!("type of expression at {:?}", e.span)))
    }

    /// Id of the function `item` called by `e` with arguments `args` (and a
    /// receiver type first for a method): a plain function, or the instance
    /// whose type arguments match the call's types.
    fn func_for_call(&self, e: &Expr, item: NodeId, arg_types: &[Type]) -> R<FuncId> {
        let t = self.ids.borrow();
        if let Some(id) = t.plain.get(&item) {
            return Ok(*id);
        }
        if !t.generic.contains(&item) {
            return Err(LowerError::Unsupported(
                "call to a function outside the root module".into(),
            ));
        }
        drop(t);
        let sig = self
            .checked
            .facts
            .fn_sigs
            .get(&item)
            .ok_or_else(|| LowerError::MissingFact("generic signature".into()))?;
        let mut s = Subst::new();
        let mut ok = sig.params.len() == arg_types.len();
        for (p, a) in sig.params.iter().zip(arg_types) {
            ok &= crate::mono::bind(&p.ty, a, &mut s);
        }
        ok &= crate::mono::bind(&sig.ret, &self.type_of(e)?, &mut s);
        let complete =
            (0..sig.generics.len()).all(|i| s.get(&i).is_some_and(|t| !crate::mono::has_param(t)));
        if !ok || !complete {
            return unsupported("generic call whose type arguments cannot be inferred");
        }
        Ok(self.ids.borrow_mut().instance(item, &s))
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

    /// Transfer a value leaf by leaf. For an owned heap field this transfers
    /// its handle; the checker prevents reuse of the moved value.
    fn copy(&mut self, dst: Local, doff: u32, src: Local, so: u32, t: &Type) -> R<()> {
        if Layouts::is_agg(t) {
            for (off, fty) in self.layouts.fields(t)? {
                self.copy(dst, doff + off, src, so + off, &fty)?;
            }
            return Ok(());
        }
        let ty = map_ty(t)?;
        if ty == Ty::Unit {
            return Ok(());
        }
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
            Place::VecAgg { .. } => unsupported("an array inside a `Vec` element"),
            Place::Reg(_) | Place::VecElem { .. } => {
                Err(LowerError::MissingFact("aggregate in a register".into()))
            }
        }
    }

    /// Whether `e` denotes (part of) an aggregate element of a `Vec`.
    fn vec_rooted(&self, e: &Expr) -> R<bool> {
        Ok(match &e.kind {
            ExprKind::Index { base, .. } => self.vec_agg_elem(base)?.is_some(),
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                Layouts::is_agg(&self.type_of(e)?) && self.vec_rooted(base)?
            }
            _ => false,
        })
    }

    fn vec_agg_place(&mut self, e: &Expr) -> R<(Local, u32, Type)> {
        match self.place(e)? {
            Place::VecAgg { o, base, ty } => Ok((o, base, ty)),
            _ => Err(LowerError::MissingFact("`Vec` element place".into())),
        }
    }

    /// The field at offset `foff` (type `fty`) of the element part `bt`
    /// starting at leaf `b` of `o`.
    fn vec_sub(&mut self, o: Local, b: u32, bt: &Type, foff: u32, fty: Type) -> R<Place> {
        let mut j = b;
        for (off, t) in self.layouts.fields(bt)? {
            if off == foff && t == fty {
                break;
            }
            j += self.vec_leaves(&t)?.len() as u32;
        }
        if Layouts::is_agg(&fty) {
            return Ok(Place::VecAgg {
                o,
                base: j,
                ty: fty,
            });
        }
        let ty = map_ty(&fty)?;
        if ty == Ty::Unit {
            return unsupported("a unit field of a `Vec` element");
        }
        let i = self.u64_const(j as u64);
        Ok(Place::VecElem { h: o, i, ty })
    }

    /// The scalar leaves (offset, IR type) of a `Vec` element type, in the
    /// order of its word vector. References are refused.
    fn vec_leaves(&self, t: &Type) -> R<Vec<(u32, Ty)>> {
        if Layouts::is_agg(t) {
            // An enum (`Option` and `Result` included) is stored as the raw
            // words that cover it, exactly as it is copied (`Layouts::fields`).
            let mut out = Vec::new();
            for (off, ft) in self.layouts.fields(t)? {
                for (o, l) in self.vec_leaves(&ft)? {
                    out.push((off + o, l));
                }
            }
            return Ok(out);
        }
        Ok(match map_ty(t)? {
            Ty::Unit => Vec::new(),
            Ty::Ptr => return unsupported("`Vec` of aggregates holding a reference"),
            l => vec![(0, l)],
        })
    }

    /// The element type of `e` when it is a `Vec` of aggregates.
    fn vec_agg_elem(&self, e: &Expr) -> R<Option<Type>> {
        let t = match self.type_of(e)? {
            Type::Vec(t) => *t,
            Type::Ref { inner, .. } => match *inner {
                Type::Vec(t) => *t,
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        if !Layouts::is_agg(&t) {
            return Ok(None);
        }
        self.vec_leaves(&t)?;
        Ok(Some(t))
    }

    /// A copy, in a fresh slot, of the element part at leaf `base` of `o`.
    fn vec_materialize(&mut self, o: Local, base: u32, ty: &Type) -> R<Local> {
        let dst = self.alloc(ty)?;
        for (j, (off, lt)) in self.vec_leaves(ty)?.into_iter().enumerate() {
            let i = self.u64_const(base as u64 + j as u64);
            let tmp = self.new_local(lt);
            self.heap(Some(tmp), HeapOp::VecGet, Some(o), Some(i));
            self.emit(Inst::Store {
                ty: lt,
                addr: dst,
                offset: off,
                src: tmp,
            });
        }
        Ok(dst)
    }

    /// Store the aggregate at `src` into the element part at `base` of `o`.
    fn vec_store(&mut self, o: Local, base: u32, ty: &Type, src: Local) -> R<()> {
        for (j, (off, lt)) in self.vec_leaves(ty)?.into_iter().enumerate() {
            let i = self.u64_const(base as u64 + j as u64);
            let tmp = self.new_local(lt);
            self.emit(Inst::Load {
                dst: tmp,
                ty: lt,
                addr: src,
                offset: off,
            });
            self.emit(Inst::VecSet { h: o, i, x: tmp });
        }
        Ok(())
    }

    fn place(&mut self, e: &Expr) -> R<Place> {
        match &e.kind {
            ExprKind::Path(p) if self.unit_variant(p).is_none() && self.const_of(p).is_none() => {
                let (l, t) = self.var_of_path(p)?;
                if Layouts::is_agg(&t) || self.mem_vars.contains(&l) {
                    Ok(Place::Mem {
                        addr: l,
                        off: 0,
                        ty: t,
                    })
                } else {
                    Ok(Place::Reg(l))
                }
            }
            ExprKind::SelfValue => {
                let (l, t) = match self.checked.facts.resolutions.get(&e.id) {
                    Some(Resolution::Local { name, decl, .. }) => self
                        .vars
                        .get(&(*decl, name.clone()))
                        .cloned()
                        .ok_or_else(|| LowerError::MissingFact("slot of `self`".into()))?,
                    _ => return Err(LowerError::MissingFact("resolution of `self`".into())),
                };
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
            ExprKind::Field { base, name } if self.vec_rooted(base)? => {
                let (o, b, bt) = self.vec_agg_place(base)?;
                let (foff, fty) = self.layouts.named_field(&bt, &name.name)?;
                self.vec_sub(o, b, &bt, foff, fty)
            }
            ExprKind::TupleIndex { base, index } if self.vec_rooted(base)? => {
                let (o, b, bt) = self.vec_agg_place(base)?;
                let (foff, fty) = self
                    .layouts
                    .fields(&bt)?
                    .get(*index as usize)
                    .cloned()
                    .ok_or_else(|| LowerError::MissingFact("tuple index".into()))?;
                self.vec_sub(o, b, &bt, foff, fty)
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
            ExprKind::Index { base, index } if self.vec_agg_elem(base)?.is_some() => {
                let ty = self.vec_agg_elem(base)?.expect("checked by the guard");
                let h = self.expr(base)?;
                let it = self.ty_of(index)?;
                let iv = self.expr(index)?;
                let i = self.new_local(Ty::U64);
                self.emit(Inst::Cast {
                    dst: i,
                    from: it,
                    to: Ty::U64,
                    src: iv,
                });
                // The element's handle: the heap checks the bounds against
                // the element count, with the language's own message.
                let o = self.new_local(Ty::Obj);
                self.heap(Some(o), HeapOp::VecGet, Some(h), Some(i));
                Ok(Place::VecAgg { o, base: 0, ty })
            }
            ExprKind::Index { base, index } if self.is_vec(base)? => {
                let elem = self.vec_elem(base)?;
                let h = self.expr(base)?;
                let it = self.ty_of(index)?;
                let iv = self.expr(index)?;
                let i = self.new_local(Ty::U64);
                self.emit(Inst::Cast {
                    dst: i,
                    from: it,
                    to: Ty::U64,
                    src: iv,
                });
                Ok(Place::VecElem { h, i, ty: elem })
            }
            ExprKind::Index { base, index } if self.vec_rooted(base)? => {
                // `v[i].z[j]`: an array of scalars inside a `Vec` element.
                let (o, b, bt) = self.vec_agg_place(base)?;
                let Type::Array { elem, len } = &bt else {
                    return unsupported("indexing something other than a fixed array");
                };
                let (elem, len) = ((**elem).clone(), *len);
                if Layouts::is_agg(&elem) {
                    return unsupported("an array of aggregates inside a `Vec` element");
                }
                let ty = map_ty(&elem)?;
                if !plain_scalar(ty) && ty != Ty::Obj {
                    return unsupported("an array of this type inside a `Vec` element");
                }
                let it = self.ty_of(index)?;
                let iv = self.expr(index)?;
                let j = self.new_local(Ty::U64);
                self.emit(Inst::Cast {
                    dst: j,
                    from: it,
                    to: Ty::U64,
                    src: iv,
                });
                // Bounds check (EXPR-7.6) against the array, not the vector.
                let n = self.u64_const(len as u64);
                let ok = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: ok,
                    op: BinOp::Lt,
                    ty: Ty::U64,
                    lhs: j,
                    rhs: n,
                });
                let (inb, oob) = (self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: ok,
                    then_to: inb,
                    else_to: oob,
                });
                self.switch_to(oob);
                self.set_term(Term::TrapBounds { len: n, index: j });
                self.switch_to(inb);
                let first = self.u64_const(b as u64);
                let i = self.new_local(Ty::U64);
                self.emit(Inst::Bin {
                    dst: i,
                    op: BinOp::Add,
                    ty: Ty::U64,
                    lhs: first,
                    rhs: j,
                });
                Ok(Place::VecElem { h: o, i, ty })
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
                self.set_term(Term::TrapBounds { len: n, index: i });
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
                if matches!(*inner, Type::String | Type::Str | Type::Vec(_)) {
                    return unsupported("explicit dereference of a heap object borrow");
                }
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
            Place::VecAgg { o, base, ty } => self.vec_materialize(o, base, &ty),
            Place::VecElem { h, i, ty } => {
                let dst = self.new_local(ty);
                self.emit(Inst::Heap {
                    dst: Some(dst),
                    op: HeapOp::VecGet,
                    a: Some(h),
                    b: Some(i),
                });
                Ok(dst)
            }
            Place::Mem { addr, off, ty } => {
                if Layouts::is_agg(&ty) {
                    return Ok(self.offset(addr, off));
                }
                let t = map_ty(&ty)?;
                if t == Ty::Unit {
                    return Ok(self.unit());
                }
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
            Place::VecElem { h, i, .. } => {
                self.emit(Inst::VecSet { h, i, x: v });
                Ok(())
            }
            Place::VecAgg { o, base, ty } => self.vec_store(o, base, &ty, v),
        }
    }

    /// A `Vec` aggregate element read as a private copy in memory (an
    /// `Option` method or `?` only reads it); other places unchanged.
    fn in_memory(&mut self, p: Place) -> R<Place> {
        Ok(match p {
            Place::VecAgg { o, base, ty } => Place::Mem {
                addr: self.vec_materialize(o, base, &ty)?,
                off: 0,
                ty,
            },
            p => p,
        })
    }

    /// Whether `e` is a `Vec<T>` or a borrow of one.
    fn is_vec(&self, e: &Expr) -> R<bool> {
        Ok(match self.type_of(e)? {
            Type::Vec(_) => true,
            Type::Ref { inner, .. } => matches!(*inner, Type::Vec(_)),
            _ => false,
        })
    }

    /// IR type of the elements of the vector `e` (scalars or handles only).
    fn vec_elem(&self, e: &Expr) -> R<Ty> {
        let t = match self.type_of(e)? {
            Type::Vec(t) => *t,
            Type::Ref { inner, .. } => match *inner {
                Type::Vec(t) => *t,
                _ => return Err(LowerError::MissingFact("element of a non-vector".into())),
            },
            _ => return Err(LowerError::MissingFact("element of a non-vector".into())),
        };
        if Layouts::is_agg(&t) || matches!(t, Type::Ref { .. }) {
            return unsupported("`Vec` of aggregates or references");
        }
        map_ty(&t)
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
                        .map(|t| substitute(t, &self.subst))
                        .ok_or_else(|| {
                            LowerError::MissingFact(format!("type of `{}`", name.name))
                        })?;
                    let Some(init) = &l.init else {
                        return unsupported("`let` without initializer");
                    };
                    // Only a text binding (`let s: &str = "…"`) is a reference
                    // the checker lets a `let` hold: a heap handle.
                    if matches!(ty, Type::Ref { .. }) && map_ty(&ty)? != Ty::Obj {
                        return unsupported("`let` binding a reference");
                    }
                    let v = self.expr(init)?;
                    let key = (l.pat.span, name.name.clone());
                    let slot = if Layouts::is_agg(&ty) {
                        self.snapshot(v, &ty)?
                    } else if self.borrowed.contains(&key) && plain_scalar(map_ty(&ty)?) {
                        let addr = self.alloc(&ty)?;
                        self.store_value(addr, 0, v, &ty)?;
                        self.mem_vars.insert(addr);
                        addr
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
            ExprKind::Path(p) if self.const_of(p).is_some() => {
                let Some(v) = self.const_of(p) else {
                    unreachable!()
                };
                let ty = self.ty_of(e)?;
                self.const_value(ty, &v)
            }
            ExprKind::Path(p) if self.unit_variant(p).is_some() => {
                let v = self.unit_variant(p).unwrap_or(0);
                let t = self.type_of(e)?;
                self.construct(&t, v, Vec::new())
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
                    Place::Mem { addr, off, ty } if map_ty(&ty)? == Ty::Obj => {
                        self.read(Place::Mem { addr, off, ty })
                    }
                    Place::Mem { addr, off, .. } => Ok(self.offset(addr, off)),
                    // `&s` / `&mut v` of a heap object: the handle itself.
                    Place::Reg(l) if self.locals[l as usize] == Ty::Obj => Ok(l),
                    // `&v[i]` of a `Vec<String>`: the element is the handle.
                    p @ Place::VecElem { ty: Ty::Obj, .. } => self.read(p),
                    // `&v[i]` of an aggregate element: a private copy is a
                    // sound shared borrow; `&mut` would need its address.
                    p @ Place::VecAgg { .. } if *op == ast::UnOp::Ref => self.read(p),
                    Place::VecAgg { o, base, ty } => self.lend_mut(o, base, ty),
                    Place::Reg(_) | Place::VecElem { .. } => {
                        unsupported("reference to a scalar local")
                    }
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
                // `*r = v` through a `&mut String`/`&mut Vec<T>`: the borrow
                // is the object's handle, so the object takes `v`'s contents
                // in place (every alias of the object sees the new value).
                if let ExprKind::Unary {
                    op: ast::UnOp::Deref,
                    expr: inner,
                } = &target.kind
                {
                    if let Type::Ref { inner: t, .. } = self.type_of(inner)? {
                        if matches!(*t, Type::String | Type::Vec(_)) {
                            let v = self.expr(value)?;
                            let h = self.expr(inner)?;
                            self.heap(None, HeapOp::Replace, Some(h), Some(v));
                            return Ok(self.unit());
                        }
                    }
                }
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
                    Place::VecElem { h, i, ty: t } => {
                        let v = self.read(Place::VecElem { h, i, ty: t })?;
                        (v, Place::VecElem { h, i, ty: t })
                    }
                    Place::VecAgg { .. } => {
                        return Err(LowerError::MissingFact(
                            "compound assignment of an aggregate".into(),
                        ))
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
            ExprKind::StructLit { path, fields } if self.variant_of(path).is_some() => {
                let Some((enum_item, v)) = self.variant_of(path) else {
                    unreachable!()
                };
                let t = self.type_of(e)?;
                let names: Vec<String> = self
                    .checked
                    .facts
                    .enums
                    .get(&enum_item)
                    .and_then(|i| i.variants.get(v))
                    .map(|vi| vi.fields.iter().map(|f| f.name.clone()).collect())
                    .ok_or_else(|| LowerError::MissingFact("variant fields".into()))?;
                // Initializers run in source order; stored by declaration order.
                let mut vals: Vec<Option<(Local, Type)>> = vec![None; names.len()];
                for fi in fields {
                    let i = names
                        .iter()
                        .position(|n| *n == fi.name.name)
                        .ok_or_else(|| LowerError::MissingFact("variant field".into()))?;
                    let ft = self.type_of(&fi.value)?;
                    let val = self.expr(&fi.value)?;
                    // A private copy: a later initializer must not see it change.
                    let val = if Layouts::is_agg(&ft) {
                        self.snapshot(val, &ft)?
                    } else {
                        val
                    };
                    vals[i] = Some((val, ft));
                }
                let vals = vals
                    .into_iter()
                    .map(|v| v.ok_or_else(|| LowerError::MissingFact("missing field".into())))
                    .collect::<R<Vec<_>>>()?;
                self.construct(&t, v, vals.into_iter().map(|(l, _)| l).collect())
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
                // RFC-INVARIANTS-1 INV-2: a constructed value is checked.
                if let Type::Nominal { def, .. } = &t {
                    self.check_invariants(*def, dst)?;
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
            ExprKind::Match { scrutinee, arms } => self.match_expr(e, scrutinee, arms),
            ExprKind::Str(t) => {
                // A literal `&str`: a fresh heap string (IR v1.3).
                let dst = self.new_local(Ty::Obj);
                self.emit(Inst::HeapText {
                    dst,
                    text: t.clone(),
                });
                Ok(dst)
            }
            ExprKind::Char(_) => unsupported("char"),
            ExprKind::Range { .. } => unsupported("range outside `for`"),
            ExprKind::Try(inner) => self.try_expr(inner),
            ExprKind::Unsafe(_) => unsupported("unsafe"),
            ExprKind::Await(_) => unsupported("await"),
            ExprKind::SelfValue => {
                let p = self.place(e)?;
                self.read(p)
            }
        }
    }

    // ---- enums and `match` (IR v1.2) ------------------------------------

    /// `Some((enum, variant))` when `path` names a user enum variant.
    fn variant_of(&self, path: &ast::Path) -> Option<(NodeId, usize)> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Variant { enum_item, variant }) => Some((*enum_item, *variant)),
            _ => None,
        }
    }

    /// The value of a `const` item named by `path`.
    fn const_of(&self, path: &ast::Path) -> Option<ConstValue> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Item { item }) => {
                self.checked.facts.consts.get(item).map(|c| c.value.clone())
            }
            _ => None,
        }
    }

    fn const_value(&mut self, ty: Ty, v: &ConstValue) -> R<Local> {
        match v {
            ConstValue::Int(n) => self.int_const(ty, *n),
            ConstValue::Bool(b) => {
                let l = self.new_local(Ty::Bool);
                self.emit(Inst::Const {
                    dst: l,
                    value: Const::Bool(*b),
                });
                Ok(l)
            }
            _ => unsupported("const of this type"),
        }
    }

    /// Variant index when `path` is a unit enum value (`E::A`, `None`).
    fn unit_variant(&self, path: &ast::Path) -> Option<usize> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Variant { variant, .. }) => Some(*variant),
            Some(Resolution::BuiltinConst { path }) if path == "Option::None" => Some(0),
            _ => None,
        }
    }

    /// Evaluate call arguments left to right; aggregates are snapshotted so a
    /// later argument cannot change an earlier one.
    fn eval_args(&mut self, args: &[Expr]) -> R<Vec<Local>> {
        let mut out = Vec::new();
        for a in args {
            let t = self.type_of(a)?;
            let v = self.expr(a)?;
            out.push(if Layouts::is_agg(&t) {
                self.snapshot(v, &t)?
            } else {
                v
            });
        }
        Ok(out)
    }

    fn tag_const(&mut self, v: usize) -> Local {
        let l = self.new_local(Ty::U32);
        self.emit(Inst::Const {
            dst: l,
            value: Const::Int {
                ty: Ty::U32,
                value: v as i128,
            },
        });
        l
    }

    /// A fresh enum value of type `t`: tag `v`, payload `vals`.
    fn construct(&mut self, t: &Type, v: usize, vals: Vec<Local>) -> R<Local> {
        let fields = self.layouts.variant_fields(t, v)?;
        if fields.len() != vals.len() {
            return Err(LowerError::MissingFact("variant arity".into()));
        }
        let dst = self.alloc(t)?;
        let tag = self.tag_const(v);
        self.emit(Inst::Store {
            ty: Ty::U32,
            addr: dst,
            offset: 0,
            src: tag,
        });
        for ((off, ft), val) in fields.into_iter().zip(vals) {
            self.store_value(dst, off, val, &ft)?;
        }
        Ok(dst)
    }

    /// `match`: arms are tried in order (O7); the first whose pattern
    /// matches binds its variables (by copy) and runs.
    fn match_expr(&mut self, e: &Expr, scrutinee: &Expr, arms: &[ast::MatchArm]) -> R<Local> {
        let t = self.type_of(e)?;
        let agg = Layouts::is_agg(&t);
        let result = if agg {
            self.alloc(&t)?
        } else {
            self.new_local(map_ty(&t)?)
        };
        let st = self.type_of(scrutinee)?;
        if matches!(st, Type::Ref { .. }) {
            return unsupported("`match` on a reference");
        }
        let place = match self.place(scrutinee)? {
            Place::Reg(l) => Place::Reg(l),
            Place::Mem { addr, off, ty } => Place::Mem { addr, off, ty },
            p @ Place::VecElem { .. } => {
                let v = self.read(p)?;
                Place::Reg(v)
            }
            Place::VecAgg { o, base, ty } => Place::Mem {
                addr: self.vec_materialize(o, base, &ty)?,
                off: 0,
                ty,
            },
        };
        let join = self.new_block();
        for arm in arms {
            let next = self.new_block();
            self.pat_test(&arm.pat, &place, &st, next)?;
            self.pat_bind(&arm.pat, &place, &st)?;
            let v = self.expr(&arm.body)?;
            self.join_value(result, v, &t, agg)?;
            self.set_term(Term::Jump(join));
            self.switch_to(next);
        }
        // The checker proved exhaustiveness; reaching here is a compiler bug,
        // reported as a fault, never a value.
        self.set_term(Term::Unreachable);
        self.switch_to(join);
        Ok(result)
    }

    /// Sub-place of `p` at `off` with type `ty` (`p` is in memory).
    fn sub_place(p: &Place, off: u32, ty: Type) -> R<Place> {
        match p {
            Place::Mem { addr, off: o, .. } => Ok(Place::Mem {
                addr: *addr,
                off: o + off,
                ty,
            }),
            Place::Reg(_) | Place::VecElem { .. } | Place::VecAgg { .. } => {
                Err(LowerError::MissingFact("field of a register".into()))
            }
        }
    }

    /// Continue in the current block when `pat` matches the value at `p`;
    /// jump to `fail` otherwise.
    fn pat_test(&mut self, pat: &ast::Pat, p: &Place, t: &Type, fail: BlockId) -> R<()> {
        match &pat.kind {
            PatKind::Wild | PatKind::Binding { .. } => Ok(()),
            PatKind::Lit(lit) => {
                let ty = map_ty(t)?;
                let k = match lit {
                    ast::LitPat::Int { value, neg, .. } => {
                        let v = *value as i128;
                        self.int_const(ty, if *neg { -v } else { v })?
                    }
                    ast::LitPat::Float { text, .. } => self.float_const(ty, text, false)?,
                    ast::LitPat::Bool(b) => {
                        let l = self.new_local(Ty::Bool);
                        self.emit(Inst::Const {
                            dst: l,
                            value: Const::Bool(*b),
                        });
                        l
                    }
                    _ => return unsupported("char or string pattern"),
                };
                let v = self.read_place(p)?;
                let ok = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: ok,
                    op: BinOp::Eq,
                    ty,
                    lhs: v,
                    rhs: k,
                });
                self.branch_or_fail(ok, fail);
                Ok(())
            }
            PatKind::Tuple(items) => {
                let fields = self.layouts.fields(t)?;
                for (item, (off, ft)) in items.iter().zip(fields) {
                    let sp = Self::sub_place(p, off, ft.clone())?;
                    self.pat_test(item, &sp, &ft, fail)?;
                }
                Ok(())
            }
            PatKind::Path(path) if self.const_of(path).is_some() => {
                let Some(c) = self.const_of(path) else {
                    unreachable!()
                };
                let ty = map_ty(t)?;
                let k = self.const_value(ty, &c)?;
                let v = self.read_place(p)?;
                let ok = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: ok,
                    op: BinOp::Eq,
                    ty,
                    lhs: v,
                    rhs: k,
                });
                self.branch_or_fail(ok, fail);
                Ok(())
            }
            PatKind::Path(path) => {
                let v = self.pat_variant(path)?;
                self.tag_test(p, v, fail)
            }
            PatKind::Variant { path, fields } => {
                let v = self.pat_variant(path)?;
                self.tag_test(p, v, fail)?;
                for (sub, off, ft) in self.variant_subpats(path, fields, t, v)? {
                    let sp = Self::sub_place(p, off, ft.clone())?;
                    self.pat_test(sub, &sp, &ft, fail)?;
                }
                Ok(())
            }
        }
    }

    /// Bind the variables of a pattern that matched the value at `p`.
    fn pat_bind(&mut self, pat: &ast::Pat, p: &Place, t: &Type) -> R<()> {
        match &pat.kind {
            PatKind::Binding { name, .. } => {
                let slot = if Layouts::is_agg(t) {
                    let Place::Mem { addr, off, .. } = p else {
                        return Err(LowerError::MissingFact("aggregate in a register".into()));
                    };
                    let a = self.offset(*addr, *off);
                    self.snapshot(a, t)?
                } else {
                    let v = self.read_place(p)?;
                    let slot = self.new_local(map_ty(t)?);
                    self.emit(Inst::Copy { dst: slot, src: v });
                    slot
                };
                self.vars
                    .insert((pat.span, name.name.clone()), (slot, t.clone()));
                Ok(())
            }
            PatKind::Tuple(items) => {
                let fields = self.layouts.fields(t)?;
                for (item, (off, ft)) in items.iter().zip(fields) {
                    let sp = Self::sub_place(p, off, ft.clone())?;
                    self.pat_bind(item, &sp, &ft)?;
                }
                Ok(())
            }
            PatKind::Variant { path, fields } => {
                let v = self.pat_variant(path)?;
                for (sub, off, ft) in self.variant_subpats(path, fields, t, v)? {
                    let sp = Self::sub_place(p, off, ft.clone())?;
                    self.pat_bind(sub, &sp, &ft)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn read_place(&mut self, p: &Place) -> R<Local> {
        match p {
            Place::Reg(l) => Ok(*l),
            Place::VecAgg { o, base, ty } => self.vec_materialize(*o, *base, ty),
            Place::VecElem { h, i, ty } => self.read(Place::VecElem {
                h: *h,
                i: *i,
                ty: *ty,
            }),
            Place::Mem { addr, off, ty } => self.read(Place::Mem {
                addr: *addr,
                off: *off,
                ty: ty.clone(),
            }),
        }
    }

    fn pat_variant(&self, path: &ast::Path) -> R<usize> {
        match self.checked.facts.resolutions.get(&path.id) {
            Some(Resolution::Variant { variant, .. }) => Ok(*variant),
            Some(Resolution::BuiltinConst { path }) if path == "Option::None" => Ok(0),
            Some(Resolution::BuiltinFn { path }) if path == "Option::Some" => Ok(1),
            Some(Resolution::BuiltinFn { path }) if path == "Result::Ok" => Ok(0),
            Some(Resolution::BuiltinFn { path }) if path == "Result::Err" => Ok(1),
            Some(Resolution::BuiltinFn { path }) => unsupported(format!("`{path}` pattern")),
            Some(Resolution::Item { .. }) => unsupported("constant pattern"),
            _ => Err(LowerError::MissingFact("pattern resolution".into())),
        }
    }

    /// Sub-patterns of a variant pattern with the offset and type of the
    /// payload field each one matches.
    fn variant_subpats<'p>(
        &self,
        path: &ast::Path,
        fields: &'p ast::VariantPatFields,
        t: &Type,
        v: usize,
    ) -> R<Vec<(&'p ast::Pat, u32, Type)>> {
        let layout = self.layouts.variant_fields(t, v)?;
        match fields {
            ast::VariantPatFields::Tuple(pats) => Ok(pats
                .iter()
                .zip(layout)
                .map(|(p, (off, ft))| (p, off, ft))
                .collect()),
            ast::VariantPatFields::Struct(fps) => {
                let Some((enum_item, _)) = self.variant_of(path) else {
                    return Err(LowerError::MissingFact("struct variant".into()));
                };
                let names: Vec<String> = self
                    .checked
                    .facts
                    .enums
                    .get(&enum_item)
                    .and_then(|i| i.variants.get(v))
                    .map(|vi| vi.fields.iter().map(|f| f.name.clone()).collect())
                    .ok_or_else(|| LowerError::MissingFact("variant fields".into()))?;
                fps.iter()
                    .map(|fp| {
                        let i = names
                            .iter()
                            .position(|n| *n == fp.name.name)
                            .ok_or_else(|| LowerError::MissingFact("variant field".into()))?;
                        let (off, ft) = layout[i].clone();
                        Ok((&fp.pat, off, ft))
                    })
                    .collect()
            }
        }
    }

    fn tag_test(&mut self, p: &Place, v: usize, fail: BlockId) -> R<()> {
        let Place::Mem { addr, off, .. } = p else {
            return Err(LowerError::MissingFact("enum in a register".into()));
        };
        let tag = self.new_local(Ty::U32);
        self.emit(Inst::Load {
            dst: tag,
            ty: Ty::U32,
            addr: *addr,
            offset: *off,
        });
        let k = self.tag_const(v);
        let ok = self.new_local(Ty::Bool);
        self.emit(Inst::Bin {
            dst: ok,
            op: BinOp::Eq,
            ty: Ty::U32,
            lhs: tag,
            rhs: k,
        });
        self.branch_or_fail(ok, fail);
        Ok(())
    }

    fn branch_or_fail(&mut self, ok: Local, fail: BlockId) {
        let cont = self.new_block();
        self.set_term(Term::Branch {
            cond: ok,
            then_to: cont,
            else_to: fail,
        });
        self.switch_to(cont);
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
        if lt == Ty::Obj && rt == Ty::Obj && matches!(op, ast::BinOp::Eq | ast::BinOp::Ne) {
            let (a, b) = (self.expr(lhs)?, self.expr(rhs)?);
            if self.is_vec(lhs)? {
                return unsupported("`==` on vectors");
            }
            let eq = self.new_local(Ty::Bool);
            self.emit(Inst::Heap {
                dst: Some(eq),
                op: HeapOp::StrEq,
                a: Some(a),
                b: Some(b),
            });
            if op == ast::BinOp::Eq {
                return Ok(eq);
            }
            let ne = self.new_local(Ty::Bool);
            self.emit(Inst::Un {
                dst: ne,
                op: UnOp::Not,
                ty: Ty::Bool,
                src: eq,
            });
            return Ok(ne);
        }
        if lt == Ty::Obj || rt == Ty::Obj {
            return unsupported("operator on a string or a vector");
        }
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
        if matches!(self.type_of(iter)?, Type::Vec(_)) {
            return self.for_vec(pat, iter, body);
        }
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

    /// `for x in v` over an owned `Vec<T>` (EXPR-6.4): the vector is
    /// consumed, its elements are visited in order by index. The length is
    /// read once: the body cannot reach `v`, which the loop owns.
    fn for_vec(&mut self, pat: &ast::Pat, iter: &Expr, body: &AstBlock) -> R<Local> {
        let name = match &pat.kind {
            PatKind::Binding { name, .. } => Some(name.name.clone()),
            PatKind::Wild => None,
            _ => return unsupported("`for` pattern"),
        };
        let Type::Vec(et) = self.type_of(iter)? else {
            return Err(LowerError::MissingFact("`for` over a non-vector".into()));
        };
        let et = *et;
        let agg = self.vec_agg_elem(iter)?.is_some();
        let elem = if agg { Ty::Obj } else { self.vec_elem(iter)? };
        let h = self.expr(iter)?;
        let len = self.new_local(Ty::U64);
        self.heap(Some(len), HeapOp::VecLen, Some(h), None);
        let i = self.u64_const(0);
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
            ty: Ty::U64,
            lhs: i,
            rhs: len,
        });
        self.set_term(Term::Branch {
            cond: c,
            then_to: bodyb,
            else_to: exit,
        });
        self.switch_to(bodyb);
        let raw = self.new_local(elem);
        self.heap(Some(raw), HeapOp::VecGet, Some(h), Some(i));
        let var = if agg {
            self.vec_materialize(raw, 0, &et)?
        } else {
            raw
        };
        if let Some(name) = name {
            self.vars.insert((pat.span, name), (var, et));
        }
        self.loops.push(LoopCtx {
            break_to: exit,
            continue_to: step,
        });
        self.block(body)?;
        self.loops.pop();
        self.set_term(Term::Jump(step));
        self.switch_to(step);
        // `i < len <= u64::MAX` holds here, so `i + 1` cannot overflow.
        let one = self.u64_const(1);
        let next = self.new_local(Ty::U64);
        self.emit(Inst::Bin {
            dst: next,
            op: BinOp::Add,
            ty: Ty::U64,
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
            Some(MethodTarget::User { item }) => {
                let item = *item;
                return self.user_method_call(e, recv, item, args);
            }
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
        if let Some(r) = self.heap_method(e, recv, &path, args)? {
            return Ok(r);
        }
        if let Some(m) = path.strip_prefix("Option::") {
            return self.option_method(e, recv, m, args);
        }
        if let Some(m) = path.strip_prefix("Result::") {
            let m = match m {
                "is_ok" => "is_some",
                "is_err" => "is_none",
                other => other,
            };
            return self.option_method(e, recv, m, args);
        }
        if let Some(m) = path.strip_prefix("int::checked_") {
            return self.checked_method(e, recv, m, args, ty);
        }
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

    /// `int::checked_*` → `Option<T>`: `None` exactly when the plain
    /// operation would trap (overflow, division or remainder by zero), else
    /// `Some(result)`. The operation itself only runs on the `Some` path.
    fn checked_method(
        &mut self,
        e: &Expr,
        recv: &Expr,
        m: &str,
        args: &[Expr],
        ty: Ty,
    ) -> R<Local> {
        if !ty.is_int() {
            return unsupported("`checked_*` on a non-integer");
        }
        let t = self.type_of(e)?;
        let a = self.expr(recv)?;
        let b = match (m, args) {
            ("neg", []) => None,
            (_, [arg]) if self.ty_of(arg)? == ty => Some(self.expr(arg)?),
            _ => return unsupported(format!("`checked_{m}` call shape")),
        };
        let boolean = |me: &mut Self, op: BinOp, ty: Ty, l: Local, r: Local| {
            let d = me.new_local(Ty::Bool);
            me.emit(Inst::Bin {
                dst: d,
                op,
                ty,
                lhs: l,
                rhs: r,
            });
            d
        };
        let (bad, op): (Local, Option<BinOp>) = match (m, b) {
            ("add", Some(b)) => (
                boolean(self, BinOp::OverflowsAdd, ty, a, b),
                Some(BinOp::WrappingAdd),
            ),
            ("sub", Some(b)) => (
                boolean(self, BinOp::OverflowsSub, ty, a, b),
                Some(BinOp::WrappingSub),
            ),
            ("mul", Some(b)) => (
                boolean(self, BinOp::OverflowsMul, ty, a, b),
                Some(BinOp::WrappingMul),
            ),
            ("div" | "rem", Some(b)) => {
                let zero = self.int_const(ty, 0)?;
                let mut bad = boolean(self, BinOp::Eq, ty, b, zero);
                if ty.is_signed() {
                    let min = self.int_const(ty, crate::interp::range(ty).0)?;
                    let m1 = self.int_const(ty, -1)?;
                    let x = boolean(self, BinOp::Eq, ty, a, min);
                    let y = boolean(self, BinOp::Eq, ty, b, m1);
                    let both = boolean(self, BinOp::BitAnd, Ty::Bool, x, y);
                    bad = boolean(self, BinOp::BitOr, Ty::Bool, bad, both);
                }
                (bad, Some(if m == "div" { BinOp::Div } else { BinOp::Rem }))
            }
            ("neg", None) if ty.is_signed() => {
                let min = self.int_const(ty, crate::interp::range(ty).0)?;
                (boolean(self, BinOp::Eq, ty, a, min), None)
            }
            _ => return unsupported(format!("`checked_{m}`")),
        };
        let result = self.alloc(&t)?;
        let (none_b, some_b, join) = (self.new_block(), self.new_block(), self.new_block());
        self.set_term(Term::Branch {
            cond: bad,
            then_to: none_b,
            else_to: some_b,
        });
        self.switch_to(none_b);
        let none = self.construct(&t, 0, Vec::new())?;
        self.copy(result, 0, none, 0, &t)?;
        self.set_term(Term::Jump(join));
        self.switch_to(some_b);
        let v = self.new_local(ty);
        match (op, b) {
            (Some(op), Some(b)) => self.emit(Inst::Bin {
                dst: v,
                op,
                ty,
                lhs: a,
                rhs: b,
            }),
            _ => self.emit(Inst::Un {
                dst: v,
                op: UnOp::WrappingNeg,
                ty,
                src: a,
            }),
        }
        let some = self.construct(&t, 1, vec![v])?;
        self.copy(result, 0, some, 0, &t)?;
        self.set_term(Term::Jump(join));
        self.switch_to(join);
        Ok(result)
    }

    /// The `&str` argument of `io::print*`: a string literal, or the text of a
    /// scalar, `&x.to_string()` / `x.to_string().as_str()`. Any other string
    /// is `Unsupported` (no `String` in the IR).
    fn print_arg(&mut self, a: &Expr) -> R<PrintArg> {
        if let ExprKind::Str(t) = &a.kind {
            return Ok(PrintArg::Text(t.clone()));
        }
        let to_string = match &a.kind {
            ExprKind::Unary {
                op: ast::UnOp::Ref,
                expr,
            } => Some(&**expr),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } if method.name == "as_str" && args.is_empty() => Some(&**receiver),
            _ => None,
        };
        if let Some(Expr {
            id,
            kind:
                ExprKind::MethodCall {
                    receiver,
                    method,
                    args,
                },
            ..
        }) = to_string
        {
            let scalar_text = matches!(
                self.checked.facts.methods.get(id),
                Some(MethodTarget::Builtin { path, .. })
                    if matches!(path.as_str(), "int::to_string" | "float::to_string" | "bool::to_string")
            );
            if method.name == "to_string" && args.is_empty() && scalar_text {
                let ty = self.ty_of(receiver)?;
                let src = self.expr(receiver)?;
                return Ok(PrintArg::Value { ty, src });
            }
        }
        // IR v1.3: any other `&str` is a heap string handle.
        if self.ty_of(a)? == Ty::Obj {
            let h = self.expr(a)?;
            return Ok(PrintArg::Obj(h));
        }
        unsupported("`io::print*` of this argument")
    }

    fn heap(&mut self, dst: Option<Local>, op: HeapOp, a: Option<Local>, b: Option<Local>) {
        self.emit(Inst::Heap { dst, op, a, b });
    }

    /// IR v1.3 `String`/`str`/`Vec` methods and scalar `to_string`;
    /// `Ok(None)` when `path` is not one of them.
    fn heap_method(
        &mut self,
        e: &Expr,
        recv: &Expr,
        path: &str,
        args: &[Expr],
    ) -> R<Option<Local>> {
        let scalar_text = matches!(
            path,
            "int::to_string" | "float::to_string" | "bool::to_string"
        );
        let known = scalar_text
            || matches!(
                path,
                "String::push_str"
                    | "String::len"
                    | "str::len"
                    | "String::clone"
                    | "String::as_str"
                    | "str::as_str"
                    | "String::is_empty"
                    | "str::is_empty"
                    | "Vec::push"
                    | "Vec::len"
                    | "Vec::is_empty"
                    | "Vec::clone"
                    | "Vec::pop"
            );
        if !known {
            return Ok(None);
        }
        let h = self.expr(recv)?;
        if scalar_text {
            let ty = self.ty_of(recv)?;
            let dst = self.new_local(Ty::Obj);
            self.emit(Inst::HeapOfValue { dst, ty, src: h });
            return Ok(Some(dst));
        }
        let len = |me: &mut Self, op: HeapOp| {
            let n = me.new_local(Ty::Usize);
            me.heap(Some(n), op, Some(h), None);
            n
        };
        let r = match (path, args) {
            ("String::push_str", [a]) => {
                let b = self.expr(a)?;
                self.heap(None, HeapOp::StrPush, Some(h), Some(b));
                self.unit()
            }
            ("String::len" | "str::len", []) => len(self, HeapOp::StrLen),
            ("Vec::len", []) => len(self, HeapOp::VecLen),
            ("String::is_empty" | "str::is_empty" | "Vec::is_empty", []) => {
                let op = if path == "Vec::is_empty" {
                    HeapOp::VecLen
                } else {
                    HeapOp::StrLen
                };
                let n = len(self, op);
                let z = self.int_const(Ty::Usize, 0)?;
                let d = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: d,
                    op: BinOp::Eq,
                    ty: Ty::Usize,
                    lhs: n,
                    rhs: z,
                });
                d
            }
            // `clone` of a `Vec` of aggregates: the checker allows it only for
            // `Copy` elements, whose words hold no handle, so cloning each
            // element's word vector is a deep copy.
            ("Vec::clone", []) if self.vec_agg_elem(recv)?.is_some() => {
                let out = self.new_local(Ty::Obj);
                self.heap(Some(out), HeapOp::VecNew, None, None);
                let len = self.new_local(Ty::U64);
                self.heap(Some(len), HeapOp::VecLen, Some(h), None);
                let i = self.u64_const(0);
                let (head, body, exit) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Jump(head));
                self.switch_to(head);
                let c = self.new_local(Ty::Bool);
                self.emit(Inst::Bin {
                    dst: c,
                    op: BinOp::Lt,
                    ty: Ty::U64,
                    lhs: i,
                    rhs: len,
                });
                self.set_term(Term::Branch {
                    cond: c,
                    then_to: body,
                    else_to: exit,
                });
                self.switch_to(body);
                let elem = self.new_local(Ty::Obj);
                self.heap(Some(elem), HeapOp::VecGet, Some(h), Some(i));
                let copy = self.new_local(Ty::Obj);
                self.heap(Some(copy), HeapOp::VecClone, Some(elem), None);
                self.heap(None, HeapOp::VecPush, Some(out), Some(copy));
                let one = self.u64_const(1);
                let next = self.new_local(Ty::U64);
                self.emit(Inst::Bin {
                    dst: next,
                    op: BinOp::Add,
                    ty: Ty::U64,
                    lhs: i,
                    rhs: one,
                });
                self.emit(Inst::Copy { dst: i, src: next });
                self.set_term(Term::Jump(head));
                self.switch_to(exit);
                out
            }
            ("String::clone" | "Vec::clone", []) => {
                let op = if path == "Vec::clone" {
                    HeapOp::VecClone
                } else {
                    HeapOp::StrClone
                };
                let d = self.new_local(Ty::Obj);
                self.heap(Some(d), op, Some(h), None);
                d
            }
            ("String::as_str" | "str::as_str", []) => h,
            ("Vec::push", [a]) if self.vec_agg_elem(recv)?.is_some() => {
                let ty = self.vec_agg_elem(recv)?.expect("checked by the guard");
                let src = self.expr(a)?;
                let o = self.new_local(Ty::Obj);
                self.heap(Some(o), HeapOp::VecNew, None, None);
                for (off, lt) in self.vec_leaves(&ty)? {
                    let tmp = self.new_local(lt);
                    self.emit(Inst::Load {
                        dst: tmp,
                        ty: lt,
                        addr: src,
                        offset: off,
                    });
                    self.heap(None, HeapOp::VecPush, Some(o), Some(tmp));
                }
                self.heap(None, HeapOp::VecPush, Some(h), Some(o));
                self.unit()
            }
            ("Vec::push", [a]) => {
                self.vec_elem(recv)?;
                let x = self.expr(a)?;
                self.heap(None, HeapOp::VecPush, Some(h), Some(x));
                self.unit()
            }
            ("Vec::pop", []) if self.vec_agg_elem(recv)?.is_some() => {
                let ty = self.vec_agg_elem(recv)?.expect("checked by the guard");
                let t = self.type_of(e)?;
                let (present, o) = (self.new_local(Ty::Bool), self.new_local(Ty::Obj));
                self.emit(Inst::VecPop {
                    present,
                    value: o,
                    h,
                });
                let result = self.alloc(&t)?;
                let (sb, nb, join) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: present,
                    then_to: sb,
                    else_to: nb,
                });
                self.switch_to(sb);
                let value = self.vec_materialize(o, 0, &ty)?;
                let some = self.construct(&t, 1, vec![value])?;
                self.copy(result, 0, some, 0, &t)?;
                self.set_term(Term::Jump(join));
                self.switch_to(nb);
                let none = self.construct(&t, 0, Vec::new())?;
                self.copy(result, 0, none, 0, &t)?;
                self.set_term(Term::Jump(join));
                self.switch_to(join);
                result
            }
            ("Vec::pop", []) => {
                let elem = self.vec_elem(recv)?;
                let t = self.type_of(e)?;
                let (present, value) = (self.new_local(Ty::Bool), self.new_local(elem));
                self.emit(Inst::VecPop { present, value, h });
                let result = self.alloc(&t)?;
                let (sb, nb, join) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: present,
                    then_to: sb,
                    else_to: nb,
                });
                self.switch_to(sb);
                let some = self.construct(&t, 1, vec![value])?;
                self.copy(result, 0, some, 0, &t)?;
                self.set_term(Term::Jump(join));
                self.switch_to(nb);
                let none = self.construct(&t, 0, Vec::new())?;
                self.copy(result, 0, none, 0, &t)?;
                self.set_term(Term::Jump(join));
                self.switch_to(join);
                result
            }
            _ => return Err(LowerError::MissingFact(format!("arity of `{path}`"))),
        };
        Ok(Some(r))
    }

    /// `x?` (EXPR, `check_try`): on `None`/`Err(e)` return `None`/`Err(e)`
    /// from the function (same error type: no conversion), else the payload.
    fn try_expr(&mut self, inner: &Expr) -> R<Local> {
        let it = self.type_of(inner)?;
        let (good, bad) = match it {
            Type::Option(_) => (1, 0),
            Type::Result(..) => (0, 1),
            _ => return Err(LowerError::MissingFact("`?` on a non-Option/Result".into())),
        };
        let p = self.place(inner)?;
        let Place::Mem { addr, off, .. } = self.in_memory(p)? else {
            return Err(LowerError::MissingFact("`?` operand in a register".into()));
        };
        let tag = self.new_local(Ty::U32);
        self.emit(Inst::Load {
            dst: tag,
            ty: Ty::U32,
            addr,
            offset: off,
        });
        let k = self.tag_const(good);
        let ok = self.new_local(Ty::Bool);
        self.emit(Inst::Bin {
            dst: ok,
            op: BinOp::Eq,
            ty: Ty::U32,
            lhs: tag,
            rhs: k,
        });
        let (gb, bb) = (self.new_block(), self.new_block());
        self.set_term(Term::Branch {
            cond: ok,
            then_to: gb,
            else_to: bb,
        });
        // Early return.
        self.switch_to(bb);
        let ret = self.ret.clone();
        let vals = match &it {
            Type::Result(..) => {
                let (foff, ft) = self.layouts.variant_fields(&it, bad)?[0].clone();
                let v = self.read(Place::Mem {
                    addr,
                    off: off + foff,
                    ty: ft.clone(),
                })?;
                vec![if Layouts::is_agg(&ft) {
                    self.snapshot(v, &ft)?
                } else {
                    v
                }]
            }
            _ => Vec::new(),
        };
        let r = self.construct(&ret, bad, vals)?;
        let t = self.return_term(r)?;
        self.terminate(t);
        // The payload.
        self.switch_to(gb);
        let (foff, ft) = self.layouts.variant_fields(&it, good)?[0].clone();
        self.read(Place::Mem {
            addr,
            off: off + foff,
            ty: ft,
        })
    }

    /// `Option::{is_some, is_none, unwrap, unwrap_or}` (and the `Result`
    /// forms); `unwrap` panics with the STD-11.2 text, as every engine does.
    fn option_method(&mut self, e: &Expr, recv: &Expr, m: &str, args: &[Expr]) -> R<Local> {
        let ot = self.type_of(recv)?;
        // The "present" variant: `Some` (1) for `Option`, `Ok` (0) for
        // `Result` — `is_ok`/`is_err` map onto `is_some`/`is_none`.
        let good = match ot {
            Type::Option(_) => 1,
            Type::Result(..) => 0,
            _ => return unsupported(format!("`{m}` on `{ot}`")),
        };
        let p = self.place(recv)?;
        let p = self.in_memory(p)?;
        let Place::Mem { addr, off, .. } = p else {
            return Err(LowerError::MissingFact("option in a register".into()));
        };
        let tag = self.new_local(Ty::U32);
        self.emit(Inst::Load {
            dst: tag,
            ty: Ty::U32,
            addr,
            offset: off,
        });
        let one = self.tag_const(good);
        let some = self.new_local(Ty::Bool);
        self.emit(Inst::Bin {
            dst: some,
            op: BinOp::Eq,
            ty: Ty::U32,
            lhs: tag,
            rhs: one,
        });
        match (m, args) {
            ("is_some", []) => Ok(some),
            ("is_none", []) => {
                let d = self.new_local(Ty::Bool);
                self.emit(Inst::Un {
                    dst: d,
                    op: UnOp::Not,
                    ty: Ty::Bool,
                    src: some,
                });
                Ok(d)
            }
            ("unwrap_or", [default]) => {
                let t = self.type_of(e)?;
                let agg = Layouts::is_agg(&t);
                let result = if agg {
                    self.alloc(&t)?
                } else {
                    self.new_local(map_ty(&t)?)
                };
                // Receiver, then argument (O1): the default is evaluated
                // whatever the tag, as a call argument would be.
                let d = self.expr(default)?;
                let (sb, nb, join) = (self.new_block(), self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: some,
                    then_to: sb,
                    else_to: nb,
                });
                self.switch_to(sb);
                let (foff, ft) = self.layouts.variant_fields(&ot, good)?[0].clone();
                let v = self.read(Place::Mem {
                    addr,
                    off: off + foff,
                    ty: ft,
                })?;
                self.join_value(result, v, &t, agg)?;
                self.set_term(Term::Jump(join));
                self.switch_to(nb);
                self.join_value(result, d, &t, agg)?;
                self.set_term(Term::Jump(join));
                self.switch_to(join);
                Ok(result)
            }
            ("unwrap", []) => {
                // STD-11.2 (MUST): fixed texts, `Err` content not formatted.
                let text = if good == 1 {
                    "called Option::unwrap on None"
                } else {
                    "called Result::unwrap on an Err value"
                };
                let (sb, nb) = (self.new_block(), self.new_block());
                self.set_term(Term::Branch {
                    cond: some,
                    then_to: sb,
                    else_to: nb,
                });
                self.switch_to(nb);
                self.set_term(Term::Trap(text.into()));
                self.switch_to(sb);
                let (foff, ft) = self.layouts.variant_fields(&ot, good)?[0].clone();
                self.read(Place::Mem {
                    addr,
                    off: off + foff,
                    ty: ft,
                })
            }
            _ => unsupported(format!("`Option::{m}`")),
        }
    }

    /// A call to a user method: the receiver becomes the first argument — a
    /// copy for `self`, the place's address for `&self`/`&mut self`
    /// (auto-reference), or the reference itself when it already is one.
    fn user_method_call(&mut self, e: &Expr, recv: &Expr, item: NodeId, args: &[Expr]) -> R<Local> {
        let mut arg_types = vec![self.type_of(recv)?];
        for a in args {
            arg_types.push(self.type_of(a)?);
        }
        let func = match self.func_for_call(e, item, &arg_types) {
            Ok(f) => f,
            // A method takes `&self` where the receiver is a place: retry
            // with the receiver auto-referenced.
            Err(_) => {
                arg_types[0] = Type::Ref {
                    mutable: false,
                    inner: Box::new(arg_types[0].clone()),
                };
                self.func_for_call(e, item, &arg_types)?
            }
        };
        let sig = self
            .checked
            .facts
            .fn_sigs
            .get(&item)
            .ok_or_else(|| LowerError::MissingFact("method signature".into()))?;
        let by_ref = !matches!(sig.receiver, Some(ast::Receiver::ByValue));
        let rt = self.type_of(recv)?;
        self.wb_marks.push(self.writebacks.len());
        let first = if by_ref {
            if matches!(rt, Type::Ref { .. }) {
                self.expr(recv)?
            } else {
                match self.place(recv)? {
                    Place::Mem { addr, off, .. } => self.offset(addr, off),
                    Place::VecAgg { o, base, ty }
                        if matches!(sig.receiver, Some(ast::Receiver::Shared)) =>
                    {
                        self.vec_materialize(o, base, &ty)?
                    }
                    Place::VecAgg { o, base, ty } => self.lend_mut(o, base, ty)?,
                    Place::Reg(_) | Place::VecElem { .. } => {
                        return unsupported("method on a scalar receiver")
                    }
                }
            }
        } else {
            let (v, t) = if let Type::Ref { inner, .. } = &rt {
                // `x.m()` with `x: &S` and `fn m(self)`: a copy of `*x`.
                (self.expr(recv)?, (**inner).clone())
            } else {
                (self.expr(recv)?, rt.clone())
            };
            if Layouts::is_agg(&t) {
                self.snapshot(v, &t)?
            } else {
                v
            }
        };
        let mut vals = vec![first];
        vals.extend(self.eval_args(args)?);
        let r = self.call_func(e, func, vals)?;
        self.store_back()?;
        Ok(r)
    }

    /// Lend a private copy of a `Vec` aggregate element as `&mut`; it is
    /// stored back once the enclosing user call returns.
    fn lend_mut(&mut self, o: Local, base: u32, ty: Type) -> R<Local> {
        if self.wb_marks.is_empty() {
            return unsupported("`&mut` borrow of an aggregate `Vec` element outside a call");
        }
        let addr = self.vec_materialize(o, base, &ty)?;
        self.writebacks.push((o, base, ty, addr));
        Ok(addr)
    }

    /// Close the innermost call frame: store every copy it lent back into
    /// its vector, in lending order.
    fn store_back(&mut self) -> R<()> {
        let mark = self
            .wb_marks
            .pop()
            .ok_or_else(|| LowerError::MissingFact("write-back frame".into()))?;
        let lent: Vec<_> = self.writebacks.drain(mark..).collect();
        for (o, base, ty, addr) in lent {
            self.vec_store(o, base, &ty, addr)?;
        }
        Ok(())
    }

    /// Emit a call to `func` with already-evaluated `vals`, adding the hidden
    /// result address when the call returns an aggregate.
    fn call_func(&mut self, e: &Expr, func: FuncId, vals: Vec<Local>) -> R<Local> {
        let rt = self.type_of(e)?;
        if Layouts::is_agg(&rt) {
            let out = self.alloc(&rt)?;
            let mut all = vec![out];
            all.extend(vals);
            self.emit(Inst::Call {
                dst: None,
                func,
                args: all,
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

    fn call(&mut self, e: &Expr, args: &[Expr]) -> R<Local> {
        match self.checked.facts.calls.get(&e.id) {
            Some(CallTarget::UserFn { item }) => {
                let arg_types = args
                    .iter()
                    .map(|a| self.type_of(a))
                    .collect::<R<Vec<_>>>()?;
                let func = self.func_for_call(e, *item, &arg_types)?;
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
                self.wb_marks.push(self.writebacks.len());
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
                    self.store_back()?;
                    return Ok(out);
                }
                let dst = self.new_local(map_ty(&rt)?);
                self.emit(Inst::Call {
                    dst: Some(dst),
                    func,
                    args: vals,
                });
                self.store_back()?;
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
            Some(CallTarget::VariantCtor { variant, .. }) => {
                let variant = *variant;
                let vals = self.eval_args(args)?;
                let t = self.type_of(e)?;
                self.construct(&t, variant, vals)
            }
            Some(CallTarget::BuiltinFn { path })
                if matches!(
                    path.as_str(),
                    "std::io::println" | "std::io::print" | "std::io::eprintln"
                ) =>
            {
                let [arg] = args else {
                    return Err(LowerError::MissingFact("print arity".into()));
                };
                let stream = if path == "std::io::eprintln" {
                    Stream::Stderr
                } else {
                    Stream::Stdout
                };
                let arg = self.print_arg(arg)?;
                self.emit(Inst::Print { stream, arg });
                if path != "std::io::print" {
                    self.emit(Inst::Print {
                        stream,
                        arg: PrintArg::Text("\n".into()),
                    });
                }
                Ok(self.unit())
            }
            Some(CallTarget::BuiltinFn { path }) if path.starts_with("std::math::") => {
                let name = &path["std::math::".len()..];
                let un = match name {
                    "abs" => Some(UnOp::Abs),
                    "sqrt" => Some(UnOp::Sqrt),
                    "floor" => Some(UnOp::Floor),
                    "ceil" => Some(UnOp::Ceil),
                    "round" => Some(UnOp::Round),
                    _ => None,
                };
                let bin = match name {
                    "min" => Some(BinOp::MathMin),
                    "max" => Some(BinOp::MathMax),
                    _ => None,
                };
                // `pow` and the libm functions stay outside the verifiable
                // core (MACH-5.4): refused, never approximated.
                match (un, bin, args) {
                    (Some(op), None, [a]) if self.ty_of(a)? == Ty::F64 => {
                        let src = self.expr(a)?;
                        let dst = self.new_local(Ty::F64);
                        self.emit(Inst::Un {
                            dst,
                            op,
                            ty: Ty::F64,
                            src,
                        });
                        Ok(dst)
                    }
                    (None, Some(op), [a, b])
                        if self.ty_of(a)? == Ty::F64 && self.ty_of(b)? == Ty::F64 =>
                    {
                        let lhs = self.expr(a)?;
                        let rhs = self.expr(b)?;
                        let dst = self.new_local(Ty::F64);
                        self.emit(Inst::Bin {
                            dst,
                            op,
                            ty: Ty::F64,
                            lhs,
                            rhs,
                        });
                        Ok(dst)
                    }
                    _ => unsupported(format!("builtin `{path}`")),
                }
            }
            Some(CallTarget::BuiltinFn { path })
                if matches!(
                    path.as_str(),
                    "String::new"
                        | "String::from"
                        | "std::string::from"
                        | "Vec::new"
                        | "Vec::with_capacity"
                ) =>
            {
                let dst = self.new_local(Ty::Obj);
                match (path.as_str(), args) {
                    ("String::new", []) => self.heap(Some(dst), HeapOp::StrNew, None, None),
                    ("String::from" | "std::string::from", [a]) => {
                        let s = self.expr(a)?;
                        self.heap(Some(dst), HeapOp::StrClone, Some(s), None)
                    }
                    ("Vec::new", []) => self.heap(Some(dst), HeapOp::VecNew, None, None),
                    ("Vec::with_capacity", [n]) => {
                        // Capacity is a hint: evaluated (O1), not used.
                        self.expr(n)?;
                        self.heap(Some(dst), HeapOp::VecNew, None, None)
                    }
                    _ => return Err(LowerError::MissingFact(format!("arity of `{path}`"))),
                }
                Ok(dst)
            }
            // `Error::new(text)`: an `Error` holding a copy of the text.
            Some(CallTarget::BuiltinFn { path }) if path == "Error::new" && args.len() == 1 => {
                let t = self.type_of(e)?;
                let text = self.expr(&args[0])?;
                let msg = self.new_local(Ty::Obj);
                self.heap(Some(msg), HeapOp::StrClone, Some(text), None);
                let dst = self.alloc(&t)?;
                self.store_value(dst, 0, msg, &Type::String)?;
                Ok(dst)
            }
            // `drop(x)`: the run heap is freed when the run ends (IR v1.3),
            // so dropping is evaluating the argument (O1).
            Some(CallTarget::BuiltinFn { path }) if path.ends_with("drop") && args.len() == 1 => {
                self.expr(&args[0])?;
                Ok(self.unit())
            }
            Some(CallTarget::BuiltinFn { path })
                if path == "Result::Ok" || path == "Result::Err" =>
            {
                let v = usize::from(path == "Result::Err");
                let vals = self.eval_args(args)?;
                let t = self.type_of(e)?;
                self.construct(&t, v, vals)
            }
            Some(CallTarget::BuiltinFn { path }) if path == "Option::Some" => {
                let vals = self.eval_args(args)?;
                let t = self.type_of(e)?;
                self.construct(&t, 1, vals)
            }
            Some(CallTarget::BuiltinFn { path }) => unsupported(format!("builtin `{path}`")),
            Some(other) => unsupported(format!("call target {other:?}")),
            None => Err(LowerError::MissingFact("call target".into())),
        }
    }
}

/// An integer, a float or a `bool`: a value with an address of its own.
fn plain_scalar(t: Ty) -> bool {
    t.is_int() || t.is_float() || t == Ty::Bool
}

/// The `let` variables of `body` whose address is taken by `&x`/`&mut x`
/// (declaration span, name). A form the walk does not visit only leaves its
/// borrow refused later (fail-closed), never mis-lowered.
fn borrowed_locals(
    checked: &CheckedProgram,
    body: &AstBlock,
) -> std::collections::BTreeSet<(Span, String)> {
    fn block(
        c: &CheckedProgram,
        b: &AstBlock,
        out: &mut std::collections::BTreeSet<(Span, String)>,
    ) {
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Let(l) => {
                    if let Some(i) = &l.init {
                        expr(c, i, out);
                    }
                }
                StmtKind::Expr { expr: e, .. } => expr(c, e, out),
            }
        }
        if let Some(t) = &b.tail {
            expr(c, t, out);
        }
    }
    fn expr(c: &CheckedProgram, e: &Expr, out: &mut std::collections::BTreeSet<(Span, String)>) {
        match &e.kind {
            ExprKind::Unary { op, expr: inner } => {
                if matches!(op, ast::UnOp::Ref | ast::UnOp::RefMut) {
                    if let ExprKind::Path(p) = &inner.kind {
                        if let Some(Resolution::Local { name, decl, .. }) =
                            c.facts.resolutions.get(&p.id)
                        {
                            out.insert((*decl, name.clone()));
                        }
                    }
                }
                expr(c, inner, out);
            }
            ExprKind::Await(x) | ExprKind::Try(x) => expr(c, x, out),
            ExprKind::Return(x) => {
                if let Some(x) = x {
                    expr(c, x, out);
                }
            }
            ExprKind::Call { callee, args } => {
                expr(c, callee, out);
                args.iter().for_each(|a| expr(c, a, out));
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                expr(c, receiver, out);
                args.iter().for_each(|a| expr(c, a, out));
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => expr(c, base, out),
            ExprKind::Index { base, index } => {
                expr(c, base, out);
                expr(c, index, out);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                expr(c, lhs, out);
                expr(c, rhs, out);
            }
            ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
                expr(c, target, out);
                expr(c, value, out);
            }
            ExprKind::Cast { expr: x, .. } => expr(c, x, out),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                expr(c, cond, out);
                block(c, then_block, out);
                if let Some(x) = else_branch {
                    expr(c, x, out);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                expr(c, scrutinee, out);
                arms.iter().for_each(|a| expr(c, &a.body, out));
            }
            ExprKind::While { cond, body } => {
                expr(c, cond, out);
                block(c, body, out);
            }
            ExprKind::Loop { body } | ExprKind::Block(body) | ExprKind::Unsafe(body) => {
                block(c, body, out)
            }
            ExprKind::For { iter, body, .. } => {
                expr(c, iter, out);
                block(c, body, out);
            }
            ExprKind::Tuple(xs) | ExprKind::Array(xs) => xs.iter().for_each(|x| expr(c, x, out)),
            ExprKind::ArrayRepeat { value, len } => {
                expr(c, value, out);
                expr(c, len, out);
            }
            ExprKind::StructLit { fields, .. } => {
                fields.iter().for_each(|f| expr(c, &f.value, out))
            }
            ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    expr(c, x, out);
                }
            }
            ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::SelfValue
            | ExprKind::Path(_)
            | ExprKind::Break
            | ExprKind::Continue => {}
        }
    }
    let mut out = std::collections::BTreeSet::new();
    block(checked, body, &mut out);
    out
}

fn is_error(t: &Type) -> bool {
    matches!(t, Type::Nominal { name, .. } if name == "Error")
}

/// The entry wrapper of a `main() -> Result<(), Error>` whose body is the
/// function `body`: call it with a result slot; on `Err` print
/// `error: <message>` and a newline on stderr, as the rustc path does.
fn main_wrapper(layouts: &Layouts, ret: &Type, body: FuncId) -> R<Function> {
    let l = layouts.layout(ret)?;
    let (err_off, _) = layouts
        .variant_fields(ret, 1)?
        .first()
        .cloned()
        .ok_or_else(|| LowerError::MissingFact("`Err` payload".into()))?;
    // Locals: 0 slot address, 1 tag, 2 const 1, 3 is_err, 4 message.
    let locals = vec![Ty::Ptr, Ty::U32, Ty::U32, Ty::Bool, Ty::Obj];
    let entry = Block {
        insts: vec![
            Inst::StackAddr { dst: 0, slot: 0 },
            Inst::Call {
                dst: None,
                func: body,
                args: vec![0],
            },
            Inst::Load {
                dst: 1,
                ty: Ty::U32,
                addr: 0,
                offset: 0,
            },
            Inst::Const {
                dst: 2,
                value: Const::Int {
                    ty: Ty::U32,
                    value: 1,
                },
            },
            Inst::Bin {
                dst: 3,
                op: BinOp::Eq,
                ty: Ty::U32,
                lhs: 1,
                rhs: 2,
            },
        ],
        term: Term::Branch {
            cond: 3,
            then_to: 1,
            else_to: 2,
        },
    };
    let report = Block {
        insts: vec![
            Inst::Load {
                dst: 4,
                ty: Ty::Obj,
                addr: 0,
                offset: err_off,
            },
            Inst::Print {
                stream: Stream::Stderr,
                arg: PrintArg::Text("error: ".into()),
            },
            Inst::Print {
                stream: Stream::Stderr,
                arg: PrintArg::Obj(4),
            },
            Inst::Print {
                stream: Stream::Stderr,
                arg: PrintArg::Text("\n".into()),
            },
        ],
        term: Term::Return(None),
    };
    let done = Block {
        insts: Vec::new(),
        term: Term::Return(None),
    };
    Ok(Function {
        name: "main".into(),
        params: 0,
        ret: Ty::Unit,
        locals,
        blocks: vec![entry, report, done],
        stack: vec![StackSlot {
            size: l.size.max(1),
            align: l.align.max(1),
        }],
    })
}

/// Empty every block no path from the entry reaches, ending it with
/// `Unreachable`. Such a block is never executed, but it may hold a
/// placeholder of the wrong type (the join after an `if` whose branches all
/// `return`), which a native backend's verifier rejects.
fn prune_unreachable(mut blocks: Vec<Block>) -> Vec<Block> {
    let mut seen = vec![false; blocks.len()];
    let mut stack = vec![0usize];
    while let Some(b) = stack.pop() {
        if b >= blocks.len() || seen[b] {
            continue;
        }
        seen[b] = true;
        match &blocks[b].term {
            Term::Jump(t) => stack.push(*t as usize),
            Term::Branch {
                then_to, else_to, ..
            } => {
                stack.push(*then_to as usize);
                stack.push(*else_to as usize);
            }
            Term::Return(_) | Term::Trap(_) | Term::TrapBounds { .. } | Term::Unreachable => {}
        }
    }
    for (b, reached) in blocks.iter_mut().zip(seen) {
        if !reached {
            b.insts.clear();
            b.term = Term::Unreachable;
        }
    }
    blocks
}
