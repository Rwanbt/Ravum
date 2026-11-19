//! Per-function hot swap on the native backend (gate T16, ADR-021/022/024).
//!
//! # Model
//!
//! * **Slots.** Every function name gets a stable slot in a call table; all
//!   calls go through `Ctx::table` (see the crate docs). A *version* is one
//!   immutable table of code addresses.
//! * **Editor side ([`HotEngine`]).** `update` compiles **only the functions
//!   whose canonical form changed** (plus the callers of a function whose
//!   signature changed) into a new JIT module, copies the other addresses from
//!   the current table, and publishes the new version in a one-slot mailbox.
//!   Anything incompatible is refused and nothing is published (fail-closed).
//! * **Real-time side ([`RtRunner`]).** `poll` is called at a safepoint (a
//!   block boundary): it takes the pending version, if any, with one atomic
//!   swap. No compilation, no allocation, no deallocation, no lock, no
//!   blocking happens on that thread: the version it leaves goes to a
//!   `retired` slot that the editor side frees (`HotEngine::collect`). If the
//!   retired slot is still full, the swap simply waits for the next boundary.
//! * **Rollback (HR-1.5).** The runner keeps the previous version. If the new
//!   code traps, the runner returns to the previous version and re-runs the
//!   same sample with it, so a bad edit never produces a trap on the output.
//!
//! # Limits (step 1)
//!
//! The entry is `fn(f32) -> f32` or `fn(f64) -> f64` (a stateless sample
//! processor). Host-owned state (`process_sample(&mut State, f32)`,
//! RFC-E11-DSP-BLOCK) needs structs and references in IR v1, which are not
//! there yet.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cranelift_jit::JITModule;
use ravum_lang_ir::ir::{Function, Inst, Module, Ty};

use crate::{compile_funcs, exec, new_jit_module, Ctx, JitError, CODE_TRAP0, DEFAULT_FUEL};

/// Signature of the host entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    F32ToF32,
    F64ToF64,
}

impl EntryKind {
    fn of(f: &Function) -> Option<EntryKind> {
        match (f.params, f.locals.first(), f.ret) {
            (1, Some(Ty::F32), Ty::F32) => Some(EntryKind::F32ToF32),
            (1, Some(Ty::F64), Ty::F64) => Some(EntryKind::F64ToF64),
            _ => None,
        }
    }
}

/// Why an update was refused. The running version is untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    EntryMissing(String),
    /// The entry signature differs from the one the host was started with.
    EntrySignature(String),
    Invalid(String),
    Codegen(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::EntryMissing(e) => write!(f, "entry `{e}` is missing"),
            Refusal::EntrySignature(e) => write!(f, "entry `{e}` changed signature"),
            Refusal::Invalid(e) => write!(f, "invalid IR: {e}"),
            Refusal::Codegen(e) => write!(f, "code generation failed: {e}"),
        }
    }
}

impl From<JitError> for Refusal {
    fn from(e: JitError) -> Self {
        match e {
            JitError::Invalid(s) => Refusal::Invalid(s),
            JitError::Codegen(s) => Refusal::Codegen(s),
        }
    }
}

/// One immutable version of the call table.
#[derive(Debug)]
pub struct Version {
    pub generation: u64,
    table: Box<[usize]>,
    entry_slot: u32,
}

/// The single exchange point between the two threads.
#[derive(Debug, Default)]
pub struct Mailbox {
    pending: AtomicPtr<Version>,
    retired: AtomicPtr<Version>,
    /// Generation the runner executes, and the one it keeps for rollback.
    pub active_generation: AtomicU64,
    pub previous_generation: AtomicU64,
    /// Last generation abandoned because it trapped (0 = none).
    pub rolled_back_generation: AtomicU64,
    /// Last trap code seen on the real-time side (0 = none).
    pub last_trap: AtomicU64,
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        raw::reclaim(self.pending.swap(std::ptr::null_mut(), SeqCst));
        raw::reclaim(self.retired.swap(std::ptr::null_mut(), SeqCst));
    }
}

/// What an accepted update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    pub generation: u64,
    /// Functions compiled for this version.
    pub recompiled: Vec<String>,
    /// Functions whose previous machine code is reused as is.
    pub reused: usize,
    pub compile_time: Duration,
}

/// Editor-side owner of all compiled code.
pub struct HotEngine {
    modules: BTreeMap<u64, JITModule>,
    slots: BTreeMap<String, u32>,
    slot_sig: Vec<(Vec<Ty>, Ty)>,
    canon: Vec<Option<String>>,
    table: Vec<usize>,
    owner: Vec<u64>,
    /// Version generation → module generations its table points into.
    live: BTreeMap<u64, BTreeSet<u64>>,
    generation: u64,
    messages: Vec<String>,
    msg_ids: BTreeMap<String, u32>,
    mailbox: Arc<Mailbox>,
    entry: String,
    kind: EntryKind,
}

/// Real-time side: calls the entry and switches versions at safepoints.
pub struct RtRunner {
    mailbox: Arc<Mailbox>,
    current: Box<Version>,
    previous: Option<Box<Version>>,
    ctx: Ctx,
    kind: EntryKind,
    fuel: u64,
}

fn signature(f: &Function) -> (Vec<Ty>, Ty) {
    (f.locals[..f.params as usize].to_vec(), f.ret)
}

impl HotEngine {
    /// Compile `ir` (generation 1) and return the engine and its runner.
    pub fn start(ir: &Module, entry: &str) -> Result<(HotEngine, RtRunner), Refusal> {
        let id = ir
            .func_id(entry)
            .ok_or_else(|| Refusal::EntryMissing(entry.into()))?;
        let kind = EntryKind::of(&ir.functions[id as usize]).ok_or_else(|| {
            Refusal::EntrySignature(format!(
                "{entry}: expected fn(f32) -> f32 or fn(f64) -> f64"
            ))
        })?;
        let mut engine = HotEngine {
            modules: BTreeMap::new(),
            slots: BTreeMap::new(),
            slot_sig: Vec::new(),
            canon: Vec::new(),
            table: Vec::new(),
            owner: Vec::new(),
            live: BTreeMap::new(),
            generation: 0,
            messages: Vec::new(),
            msg_ids: BTreeMap::new(),
            mailbox: Arc::new(Mailbox::default()),
            entry: entry.to_string(),
            kind,
        };
        let version = engine.build(ir)?.0;
        engine
            .mailbox
            .active_generation
            .store(version.generation, SeqCst);
        let runner = RtRunner {
            mailbox: engine.mailbox.clone(),
            current: version,
            previous: None,
            ctx: Ctx {
                code: 0,
                depth: 0,
                fuel: DEFAULT_FUEL,
                table: 0,
            },
            kind,
            fuel: DEFAULT_FUEL,
        };
        Ok((engine, runner))
    }

    pub fn mailbox(&self) -> &Arc<Mailbox> {
        &self.mailbox
    }

    /// Message of a trap code reported by the runner.
    pub fn trap_message(&self, code: u64) -> Option<&str> {
        let c = code as u32;
        if c >= CODE_TRAP0 {
            self.messages
                .get((c - CODE_TRAP0) as usize)
                .map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Compile what changed in `ir` and publish it for the next safepoint.
    pub fn update(&mut self, ir: &Module) -> Result<UpdateReport, Refusal> {
        let id = ir
            .func_id(&self.entry)
            .ok_or_else(|| Refusal::EntryMissing(self.entry.clone()))?;
        if EntryKind::of(&ir.functions[id as usize]) != Some(self.kind) {
            return Err(Refusal::EntrySignature(self.entry.clone()));
        }
        let (version, report) = self.build(ir)?;
        let fresh = Box::into_raw(version);
        let stale = self.mailbox.pending.swap(fresh, SeqCst);
        if !stale.is_null() {
            // Never seen by the runner: forget it.
            let g = raw::reclaim(stale);
            if let Some(g) = g {
                self.live.remove(&g);
            }
        }
        self.collect();
        Ok(report)
    }

    /// Compile the changed functions and build the next table (not published).
    fn build(&mut self, ir: &Module) -> Result<(Box<Version>, UpdateReport), Refusal> {
        let started = Instant::now();
        ir.validate().map_err(Refusal::Invalid)?;
        // Stage slot assignments; committed only once compilation succeeded.
        let mut slots = self.slots.clone();
        let mut slot_of = Vec::with_capacity(ir.functions.len());
        for f in &ir.functions {
            let next = slots.len() as u32;
            slot_of.push(*slots.entry(f.name.clone()).or_insert(next));
        }
        let sig_changed: BTreeSet<u32> = ir
            .functions
            .iter()
            .zip(&slot_of)
            .filter(|(f, s)| {
                self.slot_sig
                    .get(**s as usize)
                    .is_some_and(|old| *old != signature(f))
            })
            .map(|(_, s)| *s)
            .collect();
        let mut which = Vec::new();
        let mut canons = Vec::new();
        for (i, f) in ir.functions.iter().enumerate() {
            let c = canonical(f, &slot_of);
            let slot = slot_of[i] as usize;
            let calls_changed_sig = f.blocks.iter().flat_map(|b| &b.insts).any(|inst| {
                matches!(inst, Inst::Call { func, .. } if sig_changed.contains(&slot_of[*func as usize]))
            });
            let same = self.canon.get(slot).and_then(|c| c.as_ref()) == Some(&c);
            if !same || calls_changed_sig {
                which.push(i);
            }
            canons.push(c);
        }
        let generation = self.generation + 1;
        let mut table = self.table.clone();
        let mut owner = self.owner.clone();
        table.resize(slots.len(), 0);
        owner.resize(slots.len(), 0);
        if !which.is_empty() {
            let mut module = new_jit_module()?;
            let ids = compile_funcs(
                &mut module,
                ir,
                &which,
                &slot_of,
                &mut self.messages,
                &mut self.msg_ids,
            )?;
            for (i, fid) in ids {
                let slot = slot_of[i] as usize;
                table[slot] = module.get_finalized_function(fid) as usize;
                owner[slot] = generation;
            }
            self.modules.insert(generation, module);
        }
        // Commit.
        self.generation = generation;
        self.slots = slots;
        self.slot_sig
            .resize(self.slots.len(), (Vec::new(), Ty::Unit));
        self.canon.resize(self.slots.len(), None);
        for (i, f) in ir.functions.iter().enumerate() {
            let slot = slot_of[i] as usize;
            self.slot_sig[slot] = signature(f);
            self.canon[slot] = Some(canons[i].clone());
        }
        self.table = table.clone();
        self.owner = owner.clone();
        // A slot whose function was removed keeps its last address: no caller
        // in this version can reach it (the checker rejected such calls).
        let used: BTreeSet<u64> = owner.iter().copied().filter(|g| *g != 0).collect();
        self.live.insert(generation, used);
        let entry_slot = self.slots[&self.entry];
        let recompiled = which
            .iter()
            .map(|i| ir.functions[*i].name.clone())
            .collect();
        let report = UpdateReport {
            generation,
            recompiled,
            reused: ir.functions.len() - which.len(),
            compile_time: started.elapsed(),
        };
        let version = Box::new(Version {
            generation,
            table: table.into_boxed_slice(),
            entry_slot,
        });
        Ok((version, report))
    }

    /// Free what the runner has retired and the machine code no live version
    /// uses any more. Editor side only.
    pub fn collect(&mut self) {
        if let Some(g) = raw::reclaim(self.mailbox.retired.swap(std::ptr::null_mut(), SeqCst)) {
            let _ = g;
        }
        let active = self.mailbox.active_generation.load(SeqCst);
        let previous = self.mailbox.previous_generation.load(SeqCst);
        let oldest = if previous == 0 {
            active
        } else {
            active.min(previous)
        };
        // Versions older than what the runner holds can never run again.
        self.live.retain(|g, _| *g >= oldest);
        let keep: BTreeSet<u64> = self.live.values().flatten().copied().collect();
        let dead: Vec<u64> = self
            .modules
            .keys()
            .copied()
            .filter(|g| !keep.contains(g))
            .collect();
        for g in dead {
            if let Some(m) = self.modules.remove(&g) {
                raw::free_module(m);
            }
        }
    }

    /// Number of JIT modules still holding machine code.
    pub fn live_modules(&self) -> usize {
        self.modules.len()
    }
}

impl Drop for HotEngine {
    fn drop(&mut self) {
        // Free machine code only when the runner is gone; otherwise leak it
        // (safe: code that may still run is never unmapped).
        if Arc::strong_count(&self.mailbox) == 1 {
            for (_, m) in std::mem::take(&mut self.modules) {
                raw::free_module(m);
            }
        }
    }
}

impl RtRunner {
    /// Safepoint: adopt the pending version if there is one. Wait-free, no
    /// allocation. Returns whether the version changed.
    pub fn poll(&mut self) -> bool {
        if self.previous.is_some() && !self.mailbox.retired.load(SeqCst).is_null() {
            return false; // the editor has not collected yet; retry later
        }
        let Some(next) = raw::take(&self.mailbox.pending) else {
            return false;
        };
        let old = std::mem::replace(&mut self.current, next);
        if let Some(prev) = self.previous.replace(old) {
            self.mailbox.retired.store(Box::into_raw(prev), SeqCst);
        }
        let pg = self.previous.as_ref().map(|v| v.generation).unwrap_or(0);
        self.mailbox.previous_generation.store(pg, SeqCst);
        self.mailbox
            .active_generation
            .store(self.current.generation, SeqCst);
        true
    }

    pub fn generation(&self) -> u64 {
        self.current.generation
    }

    /// Roll back to the previous version after a trap (HR-1.5).
    fn roll_back(&mut self, code: u32) -> bool {
        self.mailbox.last_trap.store(code as u64, SeqCst);
        match self.previous.take() {
            Some(prev) => {
                let bad = std::mem::replace(&mut self.current, prev);
                self.mailbox
                    .rolled_back_generation
                    .store(bad.generation, SeqCst);
                self.previous = Some(bad);
                self.mailbox
                    .active_generation
                    .store(self.current.generation, SeqCst);
                self.mailbox
                    .previous_generation
                    .store(self.previous.as_ref().unwrap().generation, SeqCst);
                true
            }
            None => false,
        }
    }

    /// Process one sample (entry `fn(f32) -> f32`). Never traps: a trap rolls
    /// back and re-runs the sample; with nothing to roll back to, outputs 0.
    pub fn process_f32(&mut self, x: f32) -> f32 {
        assert_eq!(self.kind, EntryKind::F32ToF32);
        let (y, code) = call_f32(&mut self.ctx, self.fuel, &self.current, x);
        if code == 0 {
            return y;
        }
        if self.roll_back(code) {
            let (y, code) = call_f32(&mut self.ctx, self.fuel, &self.current, x);
            if code == 0 {
                return y;
            }
        }
        0.0
    }

    /// Process one sample (entry `fn(f64) -> f64`). Same contract.
    pub fn process_f64(&mut self, x: f64) -> f64 {
        assert_eq!(self.kind, EntryKind::F64ToF64);
        let (y, code) = call_f64(&mut self.ctx, self.fuel, &self.current, x);
        if code == 0 {
            return y;
        }
        if self.roll_back(code) {
            let (y, code) = call_f64(&mut self.ctx, self.fuel, &self.current, x);
            if code == 0 {
                return y;
            }
        }
        0.0
    }
}

fn call_f32(ctx: &mut Ctx, fuel: u64, v: &Version, x: f32) -> (f32, u32) {
    ctx.code = 0;
    ctx.depth = 0;
    ctx.fuel = fuel;
    ctx.table = v.table.as_ptr() as usize;
    let code = v.table[v.entry_slot as usize] as *const u8;
    let y = exec::call1::<f32, f32>(code, x, ctx);
    (y, ctx.code)
}

fn call_f64(ctx: &mut Ctx, fuel: u64, v: &Version, x: f64) -> (f64, u32) {
    ctx.code = 0;
    ctx.depth = 0;
    ctx.fuel = fuel;
    ctx.table = v.table.as_ptr() as usize;
    let code = v.table[v.entry_slot as usize] as *const u8;
    let y = exec::call1::<f64, f64>(code, x, ctx);
    (y, ctx.code)
}

/// Canonical text of a function: its IR with calls renamed to stable slots.
fn canonical(f: &Function, slot_of: &[u32]) -> String {
    let mut g = f.clone();
    for b in &mut g.blocks {
        for inst in &mut b.insts {
            if let Inst::Call { func, .. } = inst {
                *func = slot_of[*func as usize];
            }
        }
    }
    serde_json_like(&g)
}

fn serde_json_like(f: &Function) -> String {
    // `Module::to_json` gives the canonical serialization of the IR.
    Module {
        version: ravum_lang_ir::IR_VERSION.to_string(),
        functions: vec![f.clone()],
        state: None,
    }
    .to_json()
}

mod raw {
    //! The `unsafe` sites of the hot-swap engine (inventory #8 and #9).
    //!
    //! SAFETY argument:
    //! * `take`/`reclaim`: every non-null pointer stored in a mailbox slot was
    //!   produced by `Box::into_raw` on a `Box<Version>` and is moved out with
    //!   an atomic `swap`, so exactly one side ever turns it back into a `Box`.
    //! * `free_module`: called only for a module no live version points into.
    //!   A version stops being live only once the runner reports (through
    //!   `active_generation`/`previous_generation`) that it holds newer ones;
    //!   the runner executes compiled code only inside `process_*`, on its own
    //!   thread, through the version it holds. Code of a freed module can
    //!   therefore not be running nor be called again.
    #![allow(unsafe_code)]

    use super::Version;
    use cranelift_jit::JITModule;
    use std::sync::atomic::{AtomicPtr, Ordering::SeqCst};

    pub fn take(slot: &AtomicPtr<Version>) -> Option<Box<Version>> {
        let p = slot.swap(std::ptr::null_mut(), SeqCst);
        if p.is_null() {
            None
        } else {
            Some(unsafe { Box::from_raw(p) })
        }
    }

    /// Drop a version taken out of a mailbox slot; returns its generation.
    pub fn reclaim(p: *mut Version) -> Option<u64> {
        if p.is_null() {
            return None;
        }
        let v = unsafe { Box::from_raw(p) };
        Some(v.generation)
    }

    pub fn free_module(m: JITModule) {
        unsafe { m.free_memory() }
    }
}
