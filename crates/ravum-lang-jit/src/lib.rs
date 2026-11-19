//! Native development backend of Ravum (gate T15, ADR-018): compiles IR v1
//! (`ravum-lang-ir`) to machine code **in process** with Cranelift, without
//! `rustc` or a linker.
//!
//! # Semantics
//!
//! The backend implements exactly the IR v1 semantics, including every trap
//! condition and message (EXPR-3.1 to 3.5). Overflow checks are emitted
//! explicitly (no reliance on target flags). Traps do not unwind: every
//! compiled function receives a hidden pointer to a [`Ctx`]; a trap stores its
//! code there and returns, and every call site checks the code and returns
//! too. A call-depth counter and a back-edge fuel counter make stack overflow
//! and endless loops reported outcomes instead of crashes or hangs.
//!
//! # `unsafe`
//!
//! The crate is `#![deny(unsafe_code)]`. The only exception is the
//! [`exec`] module, which turns a finalized code pointer into a callable
//! function and hosts the `io::print*` callback: see its safety arguments.

#![deny(unsafe_code)]

pub mod hot;
pub mod replay;

use std::collections::BTreeMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlags, Type, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module as _};
use ravum_lang_ir::interp::{range, RunOutcome, Val};
use ravum_lang_ir::ir::{BinOp, Const, Inst, Module, PrintArg, Stream, Term, Ty, UnOp};

/// Runtime context shared by all compiled functions of one run.
#[repr(C)]
#[derive(Debug)]
pub struct Ctx {
    /// 0 = running; 1 = call depth limit; 2 = fuel exhausted; >= 3 = trap
    /// message index + 3.
    pub code: u32,
    pub depth: u32,
    /// Remaining loop back-edges.
    pub fuel: u64,
    /// Address of the call table (`[usize]`, one code address per slot).
    /// Every call goes through it: this indirection is what lets the hot-swap
    /// engine replace a single function (T16).
    pub table: usize,
    /// Address of the run's [`exec::OutputSink`] (`io::print*`, IR v1.2), or
    /// 0 when the entry may not print (real-time entries: refused statically
    /// by the hot-swap engine; a print reaching a 0 sink writes nothing).
    pub out: usize,
}

pub const MAX_DEPTH: u32 = 512;
pub const DEFAULT_FUEL: u64 = 1_000_000_000;

const CODE_DEPTH: u32 = 1;
const CODE_FUEL: u32 = 2;
pub(crate) const CODE_TRAP0: u32 = 3;

/// A compiled IR module, ready to run.
pub struct Compiled {
    // Owns the machine code the table points to.
    _module: JITModule,
    table: Box<[usize]>,
    ir: Module,
    messages: Vec<String>,
}

/// Why compilation failed.
#[derive(Debug)]
pub enum JitError {
    Invalid(String),
    Codegen(String),
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::Invalid(s) => write!(f, "invalid IR: {s}"),
            JitError::Codegen(s) => write!(f, "code generation failed: {s}"),
        }
    }
}

fn cl_ty(t: Ty) -> Type {
    match t {
        Ty::I8 | Ty::U8 | Ty::Bool | Ty::Unit => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize | Ty::Ptr => types::I64,
        Ty::F32 => types::F32,
        Ty::F64 => types::F64,
    }
}

extern "C" fn rv_fmod(a: f64, b: f64) -> f64 {
    a % b
}

extern "C" fn rv_fmodf(a: f32, b: f32) -> f32 {
    a % b
}

/// A fresh JIT module with the development-backend settings.
pub(crate) fn new_jit_module() -> Result<JITModule, JitError> {
    // Development backend: compile fast; the release path stays on rustc/LLVM.
    // The IR is validated above (`Module::validate`) and the differential
    // tests run debug builds with Cranelift's verifier on, so release builds
    // skip the per-function verifier.
    let mut flags = settings::builder();
    let verifier = if cfg!(debug_assertions) {
        "true"
    } else {
        "false"
    };
    for (k, v) in [
        ("opt_level", "none"),
        ("regalloc_algorithm", "single_pass"),
        ("enable_verifier", verifier),
    ] {
        flags
            .set(k, v)
            .map_err(|e| JitError::Codegen(format!("{k}={v}: {e}")))?;
    }
    let isa = cranelift_native::builder()
        .map_err(|e| JitError::Codegen(e.to_string()))?
        .finish(settings::Flags::new(flags))
        .map_err(|e| JitError::Codegen(e.to_string()))?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    jb.symbol("rv_fmod", rv_fmod as *const u8);
    jb.symbol("rv_fmodf", rv_fmodf as *const u8);
    jb.symbol("rv_print", exec::rv_print as *const u8);
    Ok(JITModule::new(jb))
}

/// Compile every function of `ir`.
pub fn compile(ir: &Module) -> Result<Compiled, JitError> {
    ir.validate().map_err(JitError::Invalid)?;
    let mut module = new_jit_module()?;
    let all: Vec<usize> = (0..ir.functions.len()).collect();
    let slots: Vec<u32> = (0..ir.functions.len() as u32).collect();
    let mut messages = Vec::new();
    let mut msg_ids = BTreeMap::new();
    let ids = compile_funcs(&mut module, ir, &all, &slots, &mut messages, &mut msg_ids)?;
    let table: Box<[usize]> = ids
        .iter()
        .map(|(_, id)| module.get_finalized_function(*id) as usize)
        .collect();
    Ok(Compiled {
        _module: module,
        table,
        ir: ir.clone(),
        messages,
    })
}

fn signature_of(
    module: &JITModule,
    f: &ravum_lang_ir::ir::Function,
) -> cranelift_codegen::ir::Signature {
    let ptr = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    for p in &f.locals[..f.params as usize] {
        sig.params.push(AbiParam::new(cl_ty(*p)));
    }
    sig.params.push(AbiParam::new(ptr));
    sig.returns.push(AbiParam::new(cl_ty(f.ret)));
    sig
}

/// Compile the functions `which` (IR indices) of `ir` into `module` and
/// finalize it. A call to IR function `k` goes through table slot
/// `slot_of[k]`. Trap messages are interned in `messages` (shared across
/// modules so codes stay stable across hot swaps).
pub(crate) fn compile_funcs(
    module: &mut JITModule,
    ir: &Module,
    which: &[usize],
    slot_of: &[u32],
    messages: &mut Vec<String>,
    msg_ids: &mut BTreeMap<String, u32>,
) -> Result<Vec<(usize, FuncId)>, JitError> {
    let mut funcs = Vec::new();
    for &i in which {
        let f = &ir.functions[i];
        let sig = signature_of(module, f);
        let id = module
            .declare_function(&format!("rv_f{i}"), Linkage::Local, &sig)
            .map_err(|e| JitError::Codegen(e.to_string()))?;
        funcs.push((i, id));
    }
    let mut fmod_sig = module.make_signature();
    fmod_sig.params.push(AbiParam::new(types::F64));
    fmod_sig.params.push(AbiParam::new(types::F64));
    fmod_sig.returns.push(AbiParam::new(types::F64));
    let fmod = module
        .declare_function("rv_fmod", Linkage::Import, &fmod_sig)
        .map_err(|e| JitError::Codegen(e.to_string()))?;
    let mut fmodf_sig = module.make_signature();
    fmodf_sig.params.push(AbiParam::new(types::F32));
    fmodf_sig.params.push(AbiParam::new(types::F32));
    fmodf_sig.returns.push(AbiParam::new(types::F32));
    let fmodf = module
        .declare_function("rv_fmodf", Linkage::Import, &fmodf_sig)
        .map_err(|e| JitError::Codegen(e.to_string()))?;

    let ptr_ty = module.target_config().pointer_type();
    let mut print_sig = module.make_signature();
    for t in [ptr_ty, types::I32, types::I32, types::I64] {
        print_sig.params.push(AbiParam::new(t));
    }
    let print = module
        .declare_function("rv_print", Linkage::Import, &print_sig)
        .map_err(|e| JitError::Codegen(e.to_string()))?;

    let mut fctx = FunctionBuilderContext::new();
    let mut ctx: Context = module.make_context();
    for &(i, fid) in &funcs {
        let f = &ir.functions[i];
        ctx.func.signature = module
            .declarations()
            .get_function_decl(fid)
            .signature
            .clone();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            // Import only the callees this function uses: importing every
            // function into every function is quadratic in the module size.
            let mut callee_refs: BTreeMap<u32, (cranelift_codegen::ir::SigRef, u32)> =
                BTreeMap::new();
            for blk in &f.blocks {
                for inst in &blk.insts {
                    if let Inst::Call { func, .. } = inst {
                        if !callee_refs.contains_key(func) {
                            let sig = signature_of(module, &ir.functions[*func as usize]);
                            let sref = b.import_signature(sig);
                            callee_refs.insert(*func, (sref, slot_of[*func as usize]));
                        }
                    }
                }
            }
            let fmod_ref = module.declare_func_in_func(fmod, b.func);
            let fmodf_ref = module.declare_func_in_func(fmodf, b.func);
            let print_ref = module.declare_func_in_func(print, b.func);
            let blocks: Vec<_> = f.blocks.iter().map(|_| b.create_block()).collect();
            let vars: Vec<Variable> = f
                .locals
                .iter()
                .enumerate()
                .map(|(k, t)| {
                    let v = Variable::from_u32(k as u32);
                    b.declare_var(v, cl_ty(*t));
                    v
                })
                .collect();
            let stack_slots: Vec<_> = f
                .stack
                .iter()
                .map(|s| {
                    let shift = s.align.max(1).trailing_zeros() as u8;
                    b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                        s.size,
                        shift,
                    ))
                })
                .collect();
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let params = b.block_params(entry).to_vec();
            let ctxp = *params.last().unwrap();
            for (k, t) in f.locals.iter().enumerate() {
                let init = if k < f.params as usize {
                    params[k]
                } else {
                    zero(&mut b, *t)
                };
                b.def_var(vars[k], init);
            }
            let mut g = Gen {
                b,
                vars,
                ctxp,
                ret: f.ret,
                messages: &mut *messages,
                msg_ids: &mut *msg_ids,
                stop: None,
                stack_slots,
            };
            // Depth check on entry.
            let depth = g.b.ins().load(types::I32, MemFlags::trusted(), ctxp, 4);
            let over =
                g.b.ins()
                    .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, depth, MAX_DEPTH as i64);
            g.trap_if(over, CODE_DEPTH);
            let d1 = g.b.ins().iadd_imm(depth, 1);
            g.b.ins().store(MemFlags::trusted(), d1, ctxp, 4);
            g.b.ins().jump(blocks[0], &[]);

            for (bi, blk) in f.blocks.iter().enumerate() {
                g.b.switch_to_block(blocks[bi]);
                for inst in &blk.insts {
                    g.inst(
                        inst,
                        &callee_refs,
                        [fmod_ref, fmodf_ref, print_ref],
                        &ir.functions,
                    );
                }
                match &blk.term {
                    Term::Jump(t) => {
                        if *t as usize <= bi {
                            g.burn_fuel();
                        }
                        g.b.ins().jump(blocks[*t as usize], &[]);
                    }
                    Term::Branch {
                        cond,
                        then_to,
                        else_to,
                    } => {
                        if (*then_to as usize) <= bi || (*else_to as usize) <= bi {
                            g.burn_fuel();
                        }
                        let c = g.b.use_var(g.vars[*cond as usize]);
                        g.b.ins().brif(
                            c,
                            blocks[*then_to as usize],
                            &[],
                            blocks[*else_to as usize],
                            &[],
                        );
                    }
                    Term::Return(v) => {
                        let val = match v {
                            Some(x) => g.b.use_var(g.vars[*x as usize]),
                            None => g.b.ins().iconst(types::I8, 0),
                        };
                        g.leave();
                        g.b.ins().return_(&[val]);
                    }
                    Term::Trap(m) => {
                        let code = g.msg(m);
                        g.set_code_and_return(code);
                    }
                    Term::Unreachable => {
                        // Dead blocks the lowering left behind; behave like a
                        // fault rather than executing anything.
                        g.set_code_and_return(CODE_DEPTH);
                    }
                }
            }
            g.finish_stop();
            g.b.seal_all_blocks();
            g.b.finalize();
        }
        module
            .define_function(fid, &mut ctx)
            .map_err(|e| JitError::Codegen(format!("{}: {e:?}", f.name)))?;
        module.clear_context(&mut ctx);
    }
    module
        .finalize_definitions()
        .map_err(|e| JitError::Codegen(e.to_string()))?;
    Ok(funcs)
}

fn zero(b: &mut FunctionBuilder, t: Ty) -> Value {
    match t {
        Ty::F32 => b.ins().f32const(0.0),
        Ty::F64 => b.ins().f64const(0.0),
        t => b.ins().iconst(cl_ty(t), 0),
    }
}

struct Gen<'a, 'f> {
    b: FunctionBuilder<'f>,
    vars: Vec<Variable>,
    ctxp: Value,
    ret: Ty,
    messages: &'a mut Vec<String>,
    msg_ids: &'a mut BTreeMap<String, u32>,
    /// Shared exit block of the function, taking the stop code as parameter:
    /// one block per function instead of one per checked operation.
    stop: Option<cranelift_codegen::ir::Block>,
    stack_slots: Vec<cranelift_codegen::ir::StackSlot>,
}

impl Gen<'_, '_> {
    fn msg(&mut self, m: &str) -> u32 {
        if let Some(id) = self.msg_ids.get(m) {
            return *id;
        }
        let id = CODE_TRAP0 + self.messages.len() as u32;
        self.messages.push(m.to_string());
        self.msg_ids.insert(m.to_string(), id);
        id
    }

    fn get(&mut self, l: u32) -> Value {
        self.b.use_var(self.vars[l as usize])
    }

    fn set(&mut self, l: u32, v: Value) {
        self.b.def_var(self.vars[l as usize], v);
    }

    /// Decrement the depth counter (function exit).
    fn leave(&mut self) {
        let d = self
            .b
            .ins()
            .load(types::I32, MemFlags::trusted(), self.ctxp, 4);
        let d1 = self.b.ins().iadd_imm(d, -1);
        self.b.ins().store(MemFlags::trusted(), d1, self.ctxp, 4);
    }

    /// Store `code` and return a dummy value of the function's type.
    fn set_code_and_return(&mut self, code: u32) {
        let c = self.b.ins().iconst(types::I32, code as i64);
        self.b.ins().store(MemFlags::trusted(), c, self.ctxp, 0);
        self.leave();
        let z = zero(&mut self.b, self.ret);
        self.b.ins().return_(&[z]);
    }

    /// The shared exit block; created on first use, filled by
    /// [`Self::finish_stop`] once every other block is terminated.
    fn stop_block(&mut self) -> cranelift_codegen::ir::Block {
        if let Some(b) = self.stop {
            return b;
        }
        let blk = self.b.create_block();
        self.b.append_block_param(blk, types::I32);
        self.stop = Some(blk);
        blk
    }

    fn finish_stop(&mut self) {
        if let Some(blk) = self.stop {
            self.b.switch_to_block(blk);
            let code = self.b.block_params(blk)[0];
            self.b.ins().store(MemFlags::trusted(), code, self.ctxp, 0);
            self.leave();
            let z = zero(&mut self.b, self.ret);
            self.b.ins().return_(&[z]);
        }
    }

    /// If `cond` is non-zero, stop with `code`; continue otherwise.
    fn trap_if(&mut self, cond: Value, code: u32) {
        let stop = self.stop_block();
        let cont = self.b.create_block();
        let c = self.b.ins().iconst(types::I32, code as i64);
        self.b.ins().brif(cond, stop, &[c], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn trap_msg_if(&mut self, cond: Value, msg: &str) {
        let code = self.msg(msg);
        self.trap_if(cond, code);
    }

    fn burn_fuel(&mut self) {
        let f = self
            .b
            .ins()
            .load(types::I64, MemFlags::trusted(), self.ctxp, 8);
        let empty = self.b.ins().icmp_imm(IntCC::Equal, f, 0);
        self.trap_if(empty, CODE_FUEL);
        let f1 = self.b.ins().iadd_imm(f, -1);
        self.b.ins().store(MemFlags::trusted(), f1, self.ctxp, 8);
    }

    fn inst(
        &mut self,
        inst: &Inst,
        callees: &BTreeMap<u32, (cranelift_codegen::ir::SigRef, u32)>,
        hosts: [cranelift_codegen::ir::FuncRef; 3],
        fns: &[ravum_lang_ir::ir::Function],
    ) {
        let [fmod, fmodf, print] = hosts;
        match inst {
            Inst::Print { stream, arg } => {
                let (kind, bits) = match arg {
                    PrintArg::Text(t) => {
                        let id = self.msg(t) - CODE_TRAP0;
                        (0, self.b.ins().iconst(types::I64, id as i64))
                    }
                    PrintArg::Value { ty, src } => {
                        let x = self.get(*src);
                        match *ty {
                            Ty::F64 => (3, self.b.ins().bitcast(types::I64, MemFlags::new(), x)),
                            Ty::F32 => {
                                let w = self.b.ins().bitcast(types::I32, MemFlags::new(), x);
                                (4, self.b.ins().uextend(types::I64, w))
                            }
                            Ty::Bool => (5, self.b.ins().uextend(types::I64, x)),
                            t if cl_ty(t) == types::I64 => (if t.is_signed() { 1 } else { 2 }, x),
                            t if t.is_signed() => (1, self.b.ins().sextend(types::I64, x)),
                            _ => (2, self.b.ins().uextend(types::I64, x)),
                        }
                    }
                };
                let k = self.b.ins().iconst(types::I32, kind);
                let st = self.b.ins().iconst(
                    types::I32,
                    match stream {
                        Stream::Stdout => 0,
                        Stream::Stderr => 1,
                    },
                );
                let ctxp = self.ctxp;
                self.b.ins().call(print, &[ctxp, k, st, bits]);
            }
            Inst::Const { dst, value } => {
                let v = match *value {
                    Const::Int { ty, value } => self.b.ins().iconst(cl_ty(ty), value as i64),
                    Const::F32Bits(bits) => self.b.ins().f32const(f32::from_bits(bits)),
                    Const::F64Bits(bits) => self.b.ins().f64const(f64::from_bits(bits)),
                    Const::Bool(x) => self.b.ins().iconst(types::I8, x as i64),
                    Const::Unit => self.b.ins().iconst(types::I8, 0),
                };
                self.set(*dst, v);
            }
            Inst::Copy { dst, src } => {
                let v = self.get(*src);
                self.set(*dst, v);
            }
            Inst::Un { dst, op, ty, src } => {
                let x = self.get(*src);
                let v = match (op, *ty) {
                    (UnOp::Neg, t) if t.is_float() => self.b.ins().fneg(x),
                    (UnOp::Neg, t) => {
                        let bad = if t.is_signed() {
                            self.b.ins().icmp_imm(IntCC::Equal, x, range(t).0 as i64)
                        } else {
                            self.b.ins().icmp_imm(IntCC::NotEqual, x, 0)
                        };
                        self.trap_msg_if(bad, "attempt to negate with overflow");
                        self.b.ins().ineg(x)
                    }
                    (UnOp::Not, Ty::Bool) => self.b.ins().bxor_imm(x, 1),
                    (UnOp::Not, _) => self.b.ins().bnot(x),
                    (UnOp::Abs, Ty::F64) => self.b.ins().fabs(x),
                    (UnOp::Abs, t) => {
                        let bad = self.b.ins().icmp_imm(IntCC::Equal, x, range(t).0 as i64);
                        self.trap_msg_if(bad, "attempt to negate with overflow");
                        self.b.ins().iabs(x)
                    }
                    (UnOp::WrappingNeg, _) => self.b.ins().ineg(x),
                    (UnOp::SaturatingNeg, t) => {
                        let (lo, hi) = range(t);
                        let min = self.b.ins().icmp_imm(IntCC::Equal, x, lo as i64);
                        let max = self.b.ins().iconst(cl_ty(t), hi as i64);
                        let n = self.b.ins().ineg(x);
                        self.b.ins().select(min, max, n)
                    }
                    (UnOp::Sqrt, _) => self.b.ins().sqrt(x),
                    (UnOp::Floor, _) => self.b.ins().floor(x),
                    (UnOp::Ceil, _) => self.b.ins().ceil(x),
                };
                self.set(*dst, v);
            }
            Inst::Bin {
                dst,
                op,
                ty,
                lhs,
                rhs,
            } => {
                let (x, y) = (self.get(*lhs), self.get(*rhs));
                let v = self.bin(*op, *ty, x, y, fmod, fmodf);
                self.set(*dst, v);
            }
            Inst::Cast { dst, from, to, src } => {
                let x = self.get(*src);
                let v = self.cast(*from, *to, x);
                self.set(*dst, v);
            }
            Inst::StackAddr { dst, slot } => {
                let v = self
                    .b
                    .ins()
                    .stack_addr(types::I64, self.stack_slots[*slot as usize], 0);
                self.set(*dst, v);
            }
            Inst::Load {
                dst,
                ty,
                addr,
                offset,
            } => {
                let a = self.get(*addr);
                let v = self
                    .b
                    .ins()
                    .load(cl_ty(*ty), MemFlags::trusted(), a, *offset as i32);
                self.set(*dst, v);
            }
            Inst::Store {
                addr, offset, src, ..
            } => {
                let a = self.get(*addr);
                let v = self.get(*src);
                self.b
                    .ins()
                    .store(MemFlags::trusted(), v, a, *offset as i32);
            }
            Inst::PtrAdd {
                dst,
                base,
                index,
                scale,
            } => {
                let b0 = self.get(*base);
                let i = self.get(*index);
                let scaled = self.b.ins().imul_imm(i, *scale as i64);
                let v = self.b.ins().iadd(b0, scaled);
                self.set(*dst, v);
            }
            Inst::Call { dst, func, args } => {
                let mut vals: Vec<Value> = args.iter().map(|a| self.get(*a)).collect();
                vals.push(self.ctxp);
                // Indirect call through the table in `Ctx` (offset 16).
                let (sref, slot) = callees[func];
                let table = self
                    .b
                    .ins()
                    .load(types::I64, MemFlags::trusted(), self.ctxp, 16);
                let addr =
                    self.b
                        .ins()
                        .load(types::I64, MemFlags::trusted(), table, (slot as i32) * 8);
                let call = self.b.ins().call_indirect(sref, addr, &vals);
                let r = self.b.inst_results(call)[0];
                let code = self
                    .b
                    .ins()
                    .load(types::I32, MemFlags::trusted(), self.ctxp, 0);
                // Propagate a stop from the callee (re-storing the same code).
                let stop = self.stop_block();
                let cont = self.b.create_block();
                self.b.ins().brif(code, stop, &[code], cont, &[]);
                self.b.switch_to_block(cont);
                if let Some(d) = dst {
                    let _ = fns;
                    self.set(*d, r);
                }
            }
        }
    }

    /// Wrapped result and overflow flag of a checked `+`, `-`, `*`.
    fn overflowing(&mut self, op: BinOp, ty: Ty, x: Value, y: Value) -> (Value, Value) {
        let signed = ty.is_signed();
        let bits = ty.int_bits().unwrap_or(8);
        match op {
            BinOp::Add => {
                let r = self.b.ins().iadd(x, y);
                let bad = if signed {
                    let a = self.b.ins().bxor(x, r);
                    let c = self.b.ins().bxor(y, r);
                    let m = self.b.ins().band(a, c);
                    self.b.ins().icmp_imm(IntCC::SignedLessThan, m, 0)
                } else {
                    self.b.ins().icmp(IntCC::UnsignedLessThan, r, x)
                };
                (r, bad)
            }
            BinOp::Sub => {
                let r = self.b.ins().isub(x, y);
                let bad = if signed {
                    let a = self.b.ins().bxor(x, y);
                    let c = self.b.ins().bxor(x, r);
                    let m = self.b.ins().band(a, c);
                    self.b.ins().icmp_imm(IntCC::SignedLessThan, m, 0)
                } else {
                    self.b.ins().icmp(IntCC::UnsignedLessThan, x, y)
                };
                (r, bad)
            }
            _ => {
                let r = self.b.ins().imul(x, y);
                let bad = if bits == 64 {
                    if signed {
                        let hi = self.b.ins().smulhi(x, y);
                        let sign = self.b.ins().sshr_imm(r, 63);
                        self.b.ins().icmp(IntCC::NotEqual, hi, sign)
                    } else {
                        let hi = self.b.ins().umulhi(x, y);
                        self.b.ins().icmp_imm(IntCC::NotEqual, hi, 0)
                    }
                } else {
                    // Exact product in 64 bits, then range check.
                    let (wx, wy) = if signed {
                        (
                            self.b.ins().sextend(types::I64, x),
                            self.b.ins().sextend(types::I64, y),
                        )
                    } else {
                        (
                            self.b.ins().uextend(types::I64, x),
                            self.b.ins().uextend(types::I64, y),
                        )
                    };
                    let p = self.b.ins().imul(wx, wy);
                    let (lo, hi) = range(ty);
                    let a = self.b.ins().icmp_imm(IntCC::SignedLessThan, p, lo as i64);
                    let c = self
                        .b
                        .ins()
                        .icmp_imm(IntCC::SignedGreaterThan, p, hi as i64);
                    self.b.ins().bor(a, c)
                };
                (r, bad)
            }
        }
    }

    fn bin(
        &mut self,
        op: BinOp,
        ty: Ty,
        x: Value,
        y: Value,
        fmod: cranelift_codegen::ir::FuncRef,
        fmodf: cranelift_codegen::ir::FuncRef,
    ) -> Value {
        use BinOp::*;
        if ty.is_float() {
            return match op {
                Add => self.b.ins().fadd(x, y),
                Sub => self.b.ins().fsub(x, y),
                Mul => self.b.ins().fmul(x, y),
                Div => self.b.ins().fdiv(x, y),
                Rem => {
                    let f = if ty == Ty::F64 { fmod } else { fmodf };
                    let c = self.b.ins().call(f, &[x, y]);
                    self.b.inst_results(c)[0]
                }
                Eq => self.b.ins().fcmp(FloatCC::Equal, x, y),
                Ne => self.b.ins().fcmp(FloatCC::NotEqual, x, y),
                Lt => self.b.ins().fcmp(FloatCC::LessThan, x, y),
                Le => self.b.ins().fcmp(FloatCC::LessThanOrEqual, x, y),
                Gt => self.b.ins().fcmp(FloatCC::GreaterThan, x, y),
                Ge => self.b.ins().fcmp(FloatCC::GreaterThanOrEqual, x, y),
                BitAnd | BitOr | BitXor | Shl | Shr | WrappingAdd | WrappingSub | WrappingMul
                | SaturatingAdd | SaturatingSub | SaturatingMul | OverflowsAdd | OverflowsSub
                | OverflowsMul => {
                    unreachable!("rejected by the checker / `validate`")
                }
            };
        }
        let signed = ty.is_signed();
        let bits = ty.int_bits().unwrap_or(8);
        match op {
            Add | Sub | Mul => {
                let (r, bad) = self.overflowing(op, ty, x, y);
                self.trap_msg_if(
                    bad,
                    match op {
                        Add => "attempt to add with overflow",
                        Sub => "attempt to subtract with overflow",
                        _ => "attempt to multiply with overflow",
                    },
                );
                r
            }
            OverflowsAdd | OverflowsSub | OverflowsMul => {
                let base = match op {
                    OverflowsAdd => Add,
                    OverflowsSub => Sub,
                    _ => Mul,
                };
                self.overflowing(base, ty, x, y).1
            }
            WrappingAdd => self.b.ins().iadd(x, y),
            WrappingSub => self.b.ins().isub(x, y),
            WrappingMul => self.b.ins().imul(x, y),
            SaturatingAdd | SaturatingSub | SaturatingMul => {
                let base = match op {
                    SaturatingAdd => Add,
                    SaturatingSub => Sub,
                    _ => Mul,
                };
                let (r, bad) = self.overflowing(base, ty, x, y);
                let t = cl_ty(ty);
                let (lo, hi) = range(ty);
                let lo = self.b.ins().iconst(t, lo as i64);
                let hi = self.b.ins().iconst(t, hi as i64);
                // Direction of the overflow: signed add/sub follow the sign
                // of `x` (for sub, `y` has the opposite sign); signed mul the
                // sign of `x * y`; unsigned sub goes low, add/mul high.
                let sat = if signed {
                    let neg = match base {
                        Mul => {
                            let s = self.b.ins().bxor(x, y);
                            self.b.ins().icmp_imm(IntCC::SignedLessThan, s, 0)
                        }
                        _ => self.b.ins().icmp_imm(IntCC::SignedLessThan, x, 0),
                    };
                    self.b.ins().select(neg, lo, hi)
                } else if base == Sub {
                    lo
                } else {
                    hi
                };
                self.b.ins().select(bad, sat, r)
            }
            Div | Rem => {
                let zero = self.b.ins().icmp_imm(IntCC::Equal, y, 0);
                self.trap_msg_if(
                    zero,
                    if op == Div {
                        "division by zero"
                    } else {
                        "remainder by zero"
                    },
                );
                if signed {
                    let a = self.b.ins().icmp_imm(IntCC::Equal, x, range(ty).0 as i64);
                    let c = self.b.ins().icmp_imm(IntCC::Equal, y, -1);
                    let both = self.b.ins().band(a, c);
                    self.trap_msg_if(
                        both,
                        if op == Div {
                            "attempt to divide with overflow"
                        } else {
                            "attempt to calculate the remainder with overflow"
                        },
                    );
                }
                match (op, signed) {
                    (Div, true) => self.b.ins().sdiv(x, y),
                    (Div, false) => self.b.ins().udiv(x, y),
                    (_, true) => self.b.ins().srem(x, y),
                    (_, false) => self.b.ins().urem(x, y),
                }
            }
            BitAnd => self.b.ins().band(x, y),
            BitOr => self.b.ins().bor(x, y),
            BitXor => self.b.ins().bxor(x, y),
            Shl | Shr => {
                // `y` is a u64 amount (IR v1).
                let bad = self
                    .b
                    .ins()
                    .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, y, bits as i64);
                self.trap_msg_if(bad, "shift out of range");
                match (op, signed) {
                    (Shl, _) => self.b.ins().ishl(x, y),
                    (_, true) => self.b.ins().sshr(x, y),
                    (_, false) => self.b.ins().ushr(x, y),
                }
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                let cc = match (op, signed) {
                    (Eq, _) => IntCC::Equal,
                    (Ne, _) => IntCC::NotEqual,
                    (Lt, true) => IntCC::SignedLessThan,
                    (Lt, false) => IntCC::UnsignedLessThan,
                    (Le, true) => IntCC::SignedLessThanOrEqual,
                    (Le, false) => IntCC::UnsignedLessThanOrEqual,
                    (Gt, true) => IntCC::SignedGreaterThan,
                    (Gt, false) => IntCC::UnsignedGreaterThan,
                    (_, true) => IntCC::SignedGreaterThanOrEqual,
                    (_, false) => IntCC::UnsignedGreaterThanOrEqual,
                };
                self.b.ins().icmp(cc, x, y)
            }
        }
    }

    fn cast(&mut self, from: Ty, to: Ty, x: Value) -> Value {
        let (ft, tt) = (cl_ty(from), cl_ty(to));
        if from.is_float() && to.is_float() {
            return match (from, to) {
                (Ty::F64, Ty::F32) => self.b.ins().fdemote(types::F32, x),
                (Ty::F32, Ty::F64) => self.b.ins().fpromote(types::F64, x),
                _ => x,
            };
        }
        if from.is_float() {
            // Saturating (NaN → 0) through 64 bits, then clamp to `to`.
            let wide = if to == Ty::U64 || to == Ty::Usize {
                return self.b.ins().fcvt_to_uint_sat(types::I64, x);
            } else {
                self.b.ins().fcvt_to_sint_sat(types::I64, x)
            };
            if tt == types::I64 {
                return wide;
            }
            let (lo, hi) = range(to);
            let lo_v = self.b.ins().iconst(types::I64, lo as i64);
            let hi_v = self.b.ins().iconst(types::I64, hi as i64);
            let c1 = self.b.ins().smax(wide, lo_v);
            let c2 = self.b.ins().smin(c1, hi_v);
            return self.b.ins().ireduce(tt, c2);
        }
        if to.is_float() {
            let signed = from.is_signed();
            return if signed {
                self.b.ins().fcvt_from_sint(tt, x)
            } else {
                self.b.ins().fcvt_from_uint(tt, x)
            };
        }
        if to == Ty::Bool {
            return x;
        }
        // int/bool → int.
        let (fb, tb) = (ft.bits(), tt.bits());
        if fb == tb {
            x
        } else if fb > tb {
            self.b.ins().ireduce(tt, x)
        } else if from.is_signed() {
            self.b.ins().sextend(tt, x)
        } else {
            self.b.ins().uextend(tt, x)
        }
    }
}

pub(crate) mod exec {
    //! The `unsafe` sites of the crate (#7, #7b, #7c, #7d of
    //! `docs/ffi/UNSAFE-INVENTORY.md`).
    //!
    //! SAFETY argument: `code` comes from `JITModule::get_finalized_function`
    //! for a function this crate declared with the signature
    //! `(ctx: *mut Ctx) -> R` (entry functions take no IR parameter), and the
    //! module has been finalized and is kept alive by `&Compiled` for the
    //! duration of the call. `R` is chosen from the declared return type, so
    //! the transmuted type matches the ABI signature exactly. `ctx` is a valid,
    //! exclusive `&mut Ctx`; compiled code only reads/writes its three fields
    //! at the offsets of the `#[repr(C)]` layout (0, 4, 8).
    #![allow(unsafe_code)]

    use super::Ctx;

    pub fn call<R>(code: *const u8, ctx: &mut Ctx) -> R {
        let f: extern "C" fn(*mut Ctx) -> R = unsafe { std::mem::transmute(code) };
        f(ctx as *mut Ctx)
    }

    /// Same argument, for a one-parameter entry `(A, *mut Ctx) -> R` (the
    /// hot-swap entry kinds check the IR signature before calling).
    pub fn call1<A, R>(code: *const u8, a: A, ctx: &mut Ctx) -> R {
        let f: extern "C" fn(A, *mut Ctx) -> R = unsafe { std::mem::transmute(code) };
        f(a, ctx as *mut Ctx)
    }

    /// Same argument, for the DSP entry `(state: usize, x: B, *mut Ctx) -> R`:
    /// `state` is the address of a live, exclusively borrowed, 8-byte aligned
    /// buffer at least as large as `State` (`StateInfo::size`); compiled code
    /// only accesses it at offsets inside that layout (bounds-checked
    /// indexing, fixed field offsets).
    pub fn call2<A, B, R>(code: *const u8, a: A, b: B, ctx: &mut Ctx) -> R {
        let f: extern "C" fn(A, B, *mut Ctx) -> R = unsafe { std::mem::transmute(code) };
        f(a, b, ctx as *mut Ctx)
    }

    /// Output buffer of one run (`Ctx::out`), with the interned texts.
    pub struct OutputSink {
        pub out: ravum_lang_ir::interp::Output,
        texts: *const String,
        n: usize,
    }

    impl OutputSink {
        pub fn new(texts: &[String]) -> Self {
            OutputSink {
                out: Default::default(),
                texts: texts.as_ptr(),
                n: texts.len(),
            }
        }
    }

    /// Host function called by compiled `Inst::Print` (site #7d).
    ///
    /// SAFETY argument: compiled code passes its own `ctx`, the exclusive
    /// `&mut Ctx` of the running call (see `call*`). `ctx.out` is 0 or the
    /// address of an `OutputSink` that lives on the caller's stack for the
    /// whole call (`run_with_output`, `RtRunner::run_nullary_with_output`)
    /// and that nothing else touches meanwhile; its `texts` slice is the
    /// message table borrowed for the same duration, and a text index is
    /// checked against its length (an out-of-range index prints nothing).
    pub extern "C" fn rv_print(ctx: *mut Ctx, kind: u32, stream: u32, bits: u64) {
        let ctx = unsafe { &mut *ctx };
        if ctx.out == 0 {
            return;
        }
        let sink = unsafe { &mut *(ctx.out as *mut OutputSink) };
        let text = match kind {
            0 => {
                if bits as usize >= sink.n {
                    return;
                }
                unsafe { (*sink.texts.add(bits as usize)).clone() }
            }
            1 => (bits as i64).to_string(),
            2 => bits.to_string(),
            3 => f64::from_bits(bits).to_string(),
            4 => f32::from_bits(bits as u32).to_string(),
            _ => (bits != 0).to_string(),
        };
        if stream == 0 {
            sink.out.stdout.push_str(&text);
        } else {
            sink.out.stderr.push_str(&text);
        }
    }
}

impl Compiled {
    /// Run `entry` (no parameters) natively.
    pub fn run(&self, entry: &str, fuel: u64) -> RunOutcome {
        let Some(id) = self.ir.func_id(entry) else {
            return RunOutcome::Fault(format!("no function `{entry}`"));
        };
        let f = &self.ir.functions[id as usize];
        if f.params != 0 {
            return RunOutcome::Fault(format!("`{entry}` takes arguments"));
        }
        let code = self.table[id as usize] as *const u8;
        let mut ctx = Ctx {
            code: 0,
            depth: 0,
            fuel,
            table: self.table.as_ptr() as usize,
            out: 0,
        };
        call_nullary(code, &mut ctx, f.ret, &self.messages)
    }

    /// [`Compiled::run`], also returning what the program printed.
    pub fn run_with_output(
        &self,
        entry: &str,
        fuel: u64,
    ) -> (RunOutcome, ravum_lang_ir::interp::Output) {
        let Some(id) = self.ir.func_id(entry) else {
            return (
                RunOutcome::Fault(format!("no function `{entry}`")),
                Default::default(),
            );
        };
        let f = &self.ir.functions[id as usize];
        if f.params != 0 {
            return (
                RunOutcome::Fault(format!("`{entry}` takes arguments")),
                Default::default(),
            );
        }
        let code = self.table[id as usize] as *const u8;
        let mut sink = exec::OutputSink::new(&self.messages);
        let mut ctx = Ctx {
            code: 0,
            depth: 0,
            fuel,
            table: self.table.as_ptr() as usize,
            out: &mut sink as *mut exec::OutputSink as usize,
        };
        let r = call_nullary(code, &mut ctx, f.ret, &self.messages);
        (r, sink.out)
    }
}

/// Call a no-argument entry returning `ret` and decode its outcome.
pub(crate) fn call_nullary(
    code: *const u8,
    ctx: &mut Ctx,
    ret: Ty,
    messages: &[String],
) -> RunOutcome {
    let val = match ret {
        Ty::I8 => Val::Int(Ty::I8, exec::call::<i8>(code, ctx) as i128),
        Ty::U8 => Val::Int(Ty::U8, exec::call::<u8>(code, ctx) as i128),
        Ty::I16 => Val::Int(Ty::I16, exec::call::<i16>(code, ctx) as i128),
        Ty::U16 => Val::Int(Ty::U16, exec::call::<u16>(code, ctx) as i128),
        Ty::I32 => Val::Int(Ty::I32, exec::call::<i32>(code, ctx) as i128),
        Ty::U32 => Val::Int(Ty::U32, exec::call::<u32>(code, ctx) as i128),
        t @ (Ty::I64 | Ty::Isize) => Val::Int(t, exec::call::<i64>(code, ctx) as i128),
        t @ (Ty::U64 | Ty::Usize) => Val::Int(t, exec::call::<u64>(code, ctx) as i128),
        Ty::F32 => Val::F32(exec::call::<f32>(code, ctx)),
        Ty::F64 => Val::F64(exec::call::<f64>(code, ctx)),
        Ty::Bool => Val::Bool(exec::call::<u8>(code, ctx) != 0),
        Ty::Unit => {
            exec::call::<u8>(code, ctx);
            Val::Unit
        }
        Ty::Ptr => return RunOutcome::Fault("an entry cannot return an address".into()),
    };
    match ctx.code {
        0 => RunOutcome::Value(val),
        CODE_DEPTH => RunOutcome::Fault("call depth limit".into()),
        CODE_FUEL => RunOutcome::Diverge,
        c => RunOutcome::Panic(messages[(c - CODE_TRAP0) as usize].clone()),
    }
}

/// Compile and run in one step.
pub fn compile_and_run(ir: &Module, entry: &str) -> Result<RunOutcome, JitError> {
    Ok(compile(ir)?.run(entry, DEFAULT_FUEL))
}
