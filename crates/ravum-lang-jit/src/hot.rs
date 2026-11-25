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
    /// `fn process_sample(state: &mut State, input: f32) -> f32` with a
    /// host-owned `struct State` (RFC-E11-DSP-BLOCK surface).
    DspF32,
    /// A no-argument entry (`main`, `check`) run on demand from the editor
    /// side (`ravum-lang watch --backend jit`); not a real-time entry.
    Nullary(Ty),
}

impl EntryKind {
    fn of(f: &Function) -> Option<EntryKind> {
        match (f.params, f.locals.first(), f.ret) {
            (1, Some(Ty::F32), Ty::F32) => Some(EntryKind::F32ToF32),
            (1, Some(Ty::F64), Ty::F64) => Some(EntryKind::F64ToF64),
            (2, Some(Ty::Ptr), Ty::F32) if f.locals.get(1) == Some(&Ty::F32) => {
                Some(EntryKind::DspF32)
            }
            (0, _, r) if r != Ty::Ptr => Some(EntryKind::Nullary(r)),
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
    /// The schema of `struct State` changed (HR-1.3) and the module has no
    /// `migrate` taking the current schema (HR-1.7): the running version and
    /// its state continue.
    StateSchemaChanged {
        old: String,
        new: String,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::EntryMissing(e) => write!(f, "entry `{e}` is missing"),
            Refusal::EntrySignature(e) => write!(f, "entry `{e}` changed signature"),
            Refusal::Invalid(e) => write!(f, "invalid IR: {e}"),
            Refusal::Codegen(e) => write!(f, "code generation failed: {e}"),
            Refusal::StateSchemaChanged { old, new } => {
                write!(f, "state schema changed ({old} -> {new})")
            }
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

/// The state buffers (state, pre-call backup) of one state schema.
#[derive(Debug)]
pub struct StateBufs {
    state: Box<[u64]>,
    backup: Box<[u64]>,
}

/// One immutable version of the call table.
#[derive(Debug)]
pub struct Version {
    pub generation: u64,
    table: Box<[usize]>,
    entry_slot: u32,
    /// Buffers this version runs on when it becomes current, if they differ
    /// from the runner's (a migrated state, or after adoption the state the
    /// version left behind — so a rollback finds its own schema again).
    /// Allocated and freed on the editor side only.
    bufs: Option<StateBufs>,
}

/// Largest state (in 8-byte words) a migration snapshot can carry.
pub const MAX_SNAPSHOT_WORDS: usize = 4096;

/// The single exchange point between the two threads.
#[derive(Debug, Default)]
pub struct Mailbox {
    /// Migration (HR-1.7, editor side): the editor sets `snapshot_request`;
    /// at its next safepoint the runner copies its state words into
    /// `snapshot` (preallocated atomics, no allocation), stores their count
    /// and bumps `snapshot_seq`.
    snapshot_request: std::sync::atomic::AtomicBool,
    pub snapshot_seq: AtomicU64,
    snapshot_len: AtomicU64,
    snapshot: Box<[AtomicU64]>,
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

/// What one safepoint did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollEvent {
    /// The state was copied for a pending migration.
    pub snapshot: bool,
    /// A new version became current.
    pub adopted: bool,
}

/// What an accepted update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateReport {
    pub generation: u64,
    /// The state schema changes through `migrate` (HR-1.7): the version is
    /// published by [`HotEngine::collect`] once the runner has sent its
    /// state snapshot.
    pub migration_pending: bool,
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
    /// HR-1.1 digest of `struct State` (DSP entry only).
    state_digest: Option<String>,
    /// A built version waiting for the runner's state snapshot (HR-1.7):
    /// (version, its module, the snapshot sequence number to wait past).
    migrating: Option<(Box<Version>, Module, u64)>,
}

/// Fuel of one real-time **attempt** (V2-G10): each loop back-edge and each
/// function entry costs 1. An edit that loops or recurses without end traps
/// after this budget and the runner rolls back to the previous version.
///
/// Budgets, made explicit:
/// - **per attempt**: `RT_FUEL` (or the host's [`RtRunner::set_fuel_budget`]);
/// - **rollback re-run**: the previous version runs the same call once more,
///   with its own budget of the same size;
/// - **total per call**: therefore at most twice the budget,
///   [`RT_FUEL_MAX_PER_CALL`] by default (a call is never retried more
///   than once);
/// - **per audio block**: the block length times the total per call.
///
/// Fuel is a work bound in the fuel model, **not** a time bound: the time a
/// unit costs depends on the code and the machine, and is qualified
/// separately on physical hardware (`tools/qualification/`). At the default,
/// one attempt of a minimal loop is about 0.25 ms on the reference machine.
/// Legitimate DSP code uses far less (a 64-tap filter: 64).
pub const RT_FUEL: u64 = 100_000;

/// V2-G10: bound on the fuel of one call, all attempts included (the
/// trapping attempt plus the single re-run after rollback), at the default
/// budget.
pub const RT_FUEL_MAX_PER_CALL: u64 = 2 * RT_FUEL;

/// Real-time side: calls the entry and switches versions at safepoints.
pub struct RtRunner {
    mailbox: Arc<Mailbox>,
    current: Box<Version>,
    previous: Option<Box<Version>>,
    ctx: Ctx,
    kind: EntryKind,
    fuel: u64,
    /// V2-G10: fuel of the last **call**, all attempts included (the trapping
    /// one and its re-run after rollback), not only the last attempt.
    call_fuel: u64,
    /// Host-owned state (DSP entry), 8-byte aligned, and the copy taken
    /// before each call so a trapping version cannot leave it half-written.
    state: Box<[u64]>,
    backup: Box<[u64]>,
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
                "{entry}: expected fn() -> T, fn(f32) -> f32, fn(f64) -> f64 or process_sample(&mut State, f32) -> f32"
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
            mailbox: Arc::new({
                let mut mb = Mailbox::default();
                if kind == EntryKind::DspF32 {
                    // Preallocated here, on the editor side.
                    mb.snapshot = (0..MAX_SNAPSHOT_WORDS).map(|_| AtomicU64::new(0)).collect();
                }
                mb
            }),
            migrating: None,
            entry: entry.to_string(),
            kind,
            state_digest: None,
        };
        let words = if kind == EntryKind::DspF32 {
            let st = ir.state.as_ref().ok_or_else(|| {
                Refusal::EntrySignature(format!("{entry}: a DSP block declares `struct State`"))
            })?;
            engine.state_digest = Some(st.digest.clone());
            (st.size as usize).div_ceil(8).max(1)
        } else {
            1
        };
        let version = engine.build(ir)?.0;
        // Allocated here, on the editor side, never on the real-time thread.
        let mut state = vec![0u64; words].into_boxed_slice();
        let backup = vec![0u64; words].into_boxed_slice();
        if kind == EntryKind::DspF32 {
            engine.run_init(ir, &version, &mut state)?;
        }
        engine
            .mailbox
            .active_generation
            .store(version.generation, SeqCst);
        let runner = RtRunner {
            call_fuel: 0,
            mailbox: engine.mailbox.clone(),
            current: version,
            previous: None,
            ctx: Ctx {
                code: 0,
                depth: 0,
                fuel: DEFAULT_FUEL,
                table: 0,
                out: 0,
                scratch: 0,
                scratch2: 0,
                stack_floor: 0,
                stack_budget: 0,
            },
            kind,
            // A real-time entry gets a bounded budget per call; an editor-side
            // run (`Nullary`) keeps the large default.
            fuel: if matches!(kind, EntryKind::Nullary(_)) {
                DEFAULT_FUEL
            } else {
                RT_FUEL
            },
            state,
            backup,
        };
        Ok((engine, runner))
    }

    /// Run `fn init() -> State` (lowered with its result address) once into
    /// the state buffer, on the editor side. Without `init`, the state is
    /// zero.
    fn run_init(&self, ir: &Module, v: &Version, state: &mut [u64]) -> Result<(), Refusal> {
        let Some(id) = ir.func_id("init") else {
            return Ok(());
        };
        let f = &ir.functions[id as usize];
        if f.params != 1 || f.locals[0] != Ty::Ptr || f.ret != Ty::Unit {
            return Err(Refusal::EntrySignature(
                "init: expected fn() -> State".into(),
            ));
        }
        let slot = self.slots["init"] as usize;
        let mut ctx = Ctx {
            code: 0,
            depth: 0,
            fuel: DEFAULT_FUEL,
            table: v.table.as_ptr() as usize,
            out: 0,
            scratch: 0,
            scratch2: 0,
            stack_floor: 0,
            stack_budget: 0,
        };
        exec::call1::<usize, u8>(
            v.table[slot] as *const u8,
            state.as_mut_ptr() as usize,
            &mut ctx,
        );
        if ctx.code != 0 {
            return Err(Refusal::Invalid(format!(
                "init stopped with code {}",
                ctx.code
            )));
        }
        Ok(())
    }

    pub fn mailbox(&self) -> &Arc<Mailbox> {
        &self.mailbox
    }

    /// Interned trap messages (index = code - 3).
    pub fn messages(&self) -> &[String] {
        &self.messages
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
        if self.kind == EntryKind::DspF32 {
            let new = ir
                .state
                .as_ref()
                .map(|s| s.digest.clone())
                .unwrap_or_default();
            let old = self.state_digest.clone().unwrap_or_default();
            if new != old {
                return self.begin_migration(ir, old, new);
            }
        }
        if self.migrating.is_some() {
            return Err(Refusal::Invalid(
                "a state migration is in progress; retry after it is published".into(),
            ));
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
        // INV-05: no I/O and no allocation on the real-time thread.
        // Conservative: any `Print` or heap instruction anywhere in the
        // module refuses a real-time entry.
        if !matches!(self.kind, EntryKind::Nullary(_)) {
            let mut kinds = ir
                .functions
                .iter()
                .flat_map(|f| f.blocks.iter().flat_map(|b| b.insts.iter()));
            if let Some(i) = kinds.find(|i| i.touches_heap_or_io()) {
                let what = if matches!(i, Inst::Print { .. }) {
                    "`io::print*`"
                } else {
                    "a heap allocation or access (`String`/`Vec`/`Map`)"
                };
                return Err(Refusal::Invalid(format!(
                    "{what} in a module with a real-time entry (INV-05)"
                )));
            }
        }
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
            migration_pending: false,
            recompiled,
            reused: ir.functions.len() - which.len(),
            compile_time: started.elapsed(),
        };
        let version = Box::new(Version {
            generation,
            table: table.into_boxed_slice(),
            entry_slot,
            bufs: None,
        });
        Ok((version, report))
    }

    /// HR-1.3/HR-1.7: a changed state schema is accepted only when the
    /// module's `migrate` takes exactly the current schema. The version is
    /// built now; it is published once the runner's snapshot arrives.
    fn begin_migration(
        &mut self,
        ir: &Module,
        old: String,
        new: String,
    ) -> Result<UpdateReport, Refusal> {
        let from = ir.migrate_from.as_ref().map(|m| m.digest.clone());
        if from.as_deref() != Some(old.as_str()) || ir.func_id("migrate").is_none() {
            return Err(Refusal::StateSchemaChanged { old, new });
        }
        if self.migrating.is_some() {
            return Err(Refusal::Invalid(
                "a state migration is already in progress".into(),
            ));
        }
        let st = ir.state.as_ref().ok_or_else(|| {
            Refusal::EntrySignature(format!(
                "{}: a DSP block declares `struct State`",
                self.entry
            ))
        })?;
        let words = (st.size as usize).div_ceil(8).max(1);
        let old_words = ir
            .migrate_from
            .as_ref()
            .map(|m| (m.size as usize).div_ceil(8).max(1))
            .unwrap_or(1);
        if words > MAX_SNAPSHOT_WORDS || old_words > MAX_SNAPSHOT_WORDS {
            return Err(Refusal::Invalid(format!(
                "state larger than {MAX_SNAPSHOT_WORDS} words cannot be migrated"
            )));
        }
        let (version, mut report) = self.build(ir)?;
        let seq = self.mailbox.snapshot_seq.load(SeqCst);
        self.mailbox.snapshot_request.store(true, SeqCst);
        self.migrating = Some((version, ir.clone(), seq));
        report.migration_pending = true;
        Ok(report)
    }

    /// Whether a migration is waiting for the runner's snapshot.
    pub fn migration_pending(&self) -> bool {
        self.migrating.is_some()
    }

    /// Run `migrate` on the snapshot (editor side) and publish the version
    /// with its new state. A trap in `migrate` refuses the module (HR-1.7).
    fn finish_migration(&mut self) -> Result<(), Refusal> {
        let ready = matches!(&self.migrating, Some((_, _, seq))
            if self.mailbox.snapshot_seq.load(SeqCst) > *seq);
        if !ready {
            return Ok(());
        }
        let (mut version, ir, _) = self.migrating.take().expect("checked above");
        let n = self.mailbox.snapshot_len.load(SeqCst) as usize;
        let mut old: Box<[u64]> = (0..n)
            .map(|i| self.mailbox.snapshot[i].load(SeqCst))
            .collect();
        let st = ir.state.as_ref().expect("checked in begin_migration");
        let words = (st.size as usize).div_ceil(8).max(1);
        let mut state = vec![0u64; words].into_boxed_slice();
        let id = ir.func_id("migrate").expect("checked in begin_migration");
        let slot = self.slots["migrate"] as usize;
        let f = &ir.functions[id as usize];
        if f.params != 2 || f.ret != Ty::Unit {
            return Err(Refusal::EntrySignature(
                "migrate: expected fn(old: T) -> State".into(),
            ));
        }
        let mut ctx = Ctx {
            code: 0,
            depth: 0,
            fuel: DEFAULT_FUEL,
            table: version.table.as_ptr() as usize,
            out: 0,
            scratch: 0,
            scratch2: 0,
            stack_floor: 0,
            stack_budget: 0,
        };
        exec::call2::<usize, usize, u8>(
            version.table[slot] as *const u8,
            state.as_mut_ptr() as usize,
            old.as_mut_ptr() as usize,
            &mut ctx,
        );
        if ctx.code != 0 {
            // Not published. Its `live` entry stays: the engine's table, and
            // so every later version, may reuse this version's machine code;
            // `collect` drops the entry once a newer generation is active.
            return Err(Refusal::Invalid(format!(
                "migrate trapped: {}",
                self.trap_message(ctx.code as u64).unwrap_or("fault")
            )));
        }
        version.bufs = Some(StateBufs {
            backup: vec![0u64; words].into_boxed_slice(),
            state,
        });
        self.state_digest = Some(st.digest.clone());
        let stale = self.mailbox.pending.swap(Box::into_raw(version), SeqCst);
        if let Some(g) = raw::reclaim(stale) {
            self.live.remove(&g);
        }
        Ok(())
    }

    /// Free what the runner has retired and the machine code no live version
    /// uses any more; finish a migration whose snapshot has arrived. Editor
    /// side only. A migration refused here (trap in `migrate`) is returned.
    pub fn collect_migration(&mut self) -> Result<(), Refusal> {
        let r = self.finish_migration();
        self.collect();
        r
    }

    /// Free what the runner has retired and the machine code no live version
    /// uses any more. Editor side only. Also finishes a pending migration
    /// (see [`HotEngine::collect_migration`] to observe its refusal).
    pub fn collect(&mut self) {
        let _ = self.finish_migration();
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
        self.poll_event().adopted
    }

    /// [`RtRunner::poll`], also reporting whether a migration snapshot was
    /// taken (a session log records both instants, T17).
    pub fn poll_event(&mut self) -> PollEvent {
        let mut ev = PollEvent::default();
        if self.mailbox.snapshot_request.swap(false, SeqCst) {
            // A copy of a few words into preallocated atomics: no
            // allocation, no lock, no migration work here (INV-05).
            let n = self.state.len().min(self.mailbox.snapshot.len());
            for i in 0..n {
                self.mailbox.snapshot[i].store(self.state[i], SeqCst);
            }
            self.mailbox.snapshot_len.store(n as u64, SeqCst);
            self.mailbox.snapshot_seq.fetch_add(1, SeqCst);
            ev.snapshot = true;
        }
        if self.previous.is_some() && !self.mailbox.retired.load(SeqCst).is_null() {
            return ev; // the editor has not collected yet; retry later
        }
        let Some(mut next) = raw::take(&self.mailbox.pending) else {
            return ev;
        };
        // aarch64: code finalized on the editor thread may sit stale in this
        // core's instruction pipeline; `isb` discards prefetched instructions
        // before the first call into the new version (no allocation, INV-05).
        raw::instruction_barrier();
        // A migrated version brings its own buffers: swap them in (moves of
        // `Box`es, no allocation) and leave ours with the version we quit.
        let left = self.swap_bufs(&mut next);
        let mut old = std::mem::replace(&mut self.current, next);
        if left.is_some() {
            old.bufs = left;
        }
        ev.adopted = true;
        if let Some(prev) = self.previous.replace(old) {
            self.mailbox.retired.store(Box::into_raw(prev), SeqCst);
        }
        let pg = self.previous.as_ref().map(|v| v.generation).unwrap_or(0);
        self.mailbox.previous_generation.store(pg, SeqCst);
        self.mailbox
            .active_generation
            .store(self.current.generation, SeqCst);
        ev
    }

    /// If `v` carries buffers, make them the runner's and return the
    /// runner's previous ones.
    fn swap_bufs(&mut self, v: &mut Version) -> Option<StateBufs> {
        let b = v.bufs.take()?;
        Some(StateBufs {
            state: std::mem::replace(&mut self.state, b.state),
            backup: std::mem::replace(&mut self.backup, b.backup),
        })
    }

    pub fn generation(&self) -> u64 {
        self.current.generation
    }

    /// Roll back to the previous version after a trap (HR-1.5).
    fn roll_back(&mut self, code: u32) -> bool {
        self.mailbox.last_trap.store(code as u64, SeqCst);
        match self.previous.take() {
            Some(mut prev) => {
                // Back to the previous version's own state if it ran on
                // another schema (it was left with it at adoption).
                let left = self.swap_bufs(&mut prev);
                let mut bad = std::mem::replace(&mut self.current, prev);
                if left.is_some() {
                    bad.bufs = left;
                }
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

    /// Per-**attempt** fuel budget (see [`RT_FUEL`]); a call costs at most
    /// twice this (one re-run after rollback). 0 is refused (no call could
    /// run), and so is a budget whose doubled total would not fit a `u64`:
    /// the total of a call is then always representable. Called by the host
    /// outside the audio callback.
    pub fn set_fuel_budget(&mut self, fuel: u64) {
        assert!(fuel > 0, "fuel budget");
        assert!(
            fuel <= u64::MAX / 2,
            "fuel budget: the total of two attempts must fit a u64"
        );
        self.fuel = fuel;
    }

    pub fn fuel_budget(&self) -> u64 {
        self.fuel
    }

    /// Fuel the last call consumed **in total** (V2-G10): a trapping attempt
    /// plus its re-run after rollback, when there was one. A deterministic
    /// measure of a call's work in the fuel model (loop back-edges and
    /// function entries), independent of the host's clock and load; it is
    /// not a bound on execution time. Reads one field; never allocates.
    pub fn last_fuel_used(&self) -> u64 {
        self.call_fuel
    }

    /// Native stack bytes compiled code may use below the host's call
    /// (default [`crate::STACK_BUDGET`], 1 MiB). A host whose audio thread
    /// has a small stack (an OS audio callback) sets less: deeper code then
    /// traps (rollback) instead of overflowing.
    pub fn set_stack_budget(&mut self, bytes: usize) {
        assert!(bytes >= 16 << 10, "stack budget below 16 KiB");
        self.ctx.stack_budget = bytes;
    }

    /// The entry's shape (an audio host picks the per-sample call).
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Process one sample (entry `fn(f32) -> f32`). Never traps: a trap rolls
    /// back and re-runs the sample; with nothing to roll back to, outputs 0.
    pub fn process_f32(&mut self, x: f32) -> f32 {
        assert_eq!(self.kind, EntryKind::F32ToF32);
        let (y, code) = call_f32(&mut self.ctx, self.fuel, &self.current, x);
        self.call_fuel = self.fuel.saturating_sub(self.ctx.fuel);
        if code == 0 {
            return y;
        }
        if self.roll_back(code) {
            let (y, code) = call_f32(&mut self.ctx, self.fuel, &self.current, x);
            self.call_fuel = self
                .call_fuel
                .saturating_add(self.fuel.saturating_sub(self.ctx.fuel));
            if code == 0 {
                return y;
            }
        }
        0.0
    }

    /// Process one sample of a DSP block (`process_sample(&mut State, f32)`).
    /// Same contract, plus HR-1.5: a trapping call's partial writes to the
    /// state are undone before the previous version re-runs the sample.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        assert_eq!(self.kind, EntryKind::DspF32);
        self.backup.copy_from_slice(&self.state);
        let (y, code) = call_dsp(&mut self.ctx, self.fuel, &self.current, &mut self.state, x);
        self.call_fuel = self.fuel.saturating_sub(self.ctx.fuel);
        if code == 0 {
            return y;
        }
        self.state.copy_from_slice(&self.backup);
        if self.roll_back(code) {
            let (y, code) = call_dsp(&mut self.ctx, self.fuel, &self.current, &mut self.state, x);
            self.call_fuel = self
                .call_fuel
                .saturating_add(self.fuel.saturating_sub(self.ctx.fuel));
            if code == 0 {
                return y;
            }
            self.state.copy_from_slice(&self.backup);
        }
        0.0
    }

    /// Run a no-argument entry once with the current version (not a
    /// real-time call: the outcome is decoded with allocation).
    /// [`RtRunner::run_nullary`], also returning what the entry printed.
    pub fn run_nullary_with_output(
        &mut self,
        messages: &[String],
    ) -> (
        ravum_lang_ir::interp::RunOutcome,
        ravum_lang_ir::interp::Output,
    ) {
        let mut sink = crate::exec::OutputSink::new(messages);
        self.ctx.out = &mut sink as *mut crate::exec::OutputSink as usize;
        let r = self.run_nullary_inner(messages);
        self.ctx.out = 0;
        (crate::with_note(r, &sink.out), sink.out)
    }

    pub fn run_nullary(&mut self, messages: &[String]) -> ravum_lang_ir::interp::RunOutcome {
        if self.ctx.out == 0 {
            // A run always has its environment (heap); output is dropped.
            return self.run_nullary_with_output(messages).0;
        }
        self.run_nullary_inner(messages)
    }

    fn run_nullary_inner(&mut self, messages: &[String]) -> ravum_lang_ir::interp::RunOutcome {
        let EntryKind::Nullary(ret) = self.kind else {
            panic!("run_nullary on a {:?} entry", self.kind);
        };
        self.ctx.code = 0;
        self.ctx.depth = 0;
        self.ctx.fuel = self.fuel;
        self.ctx.table = self.current.table.as_ptr() as usize;
        let code = self.current.table[self.current.entry_slot as usize] as *const u8;
        crate::call_nullary(code, &mut self.ctx, ret, messages)
    }

    /// Read-only view of the host-owned state (for tests and hosts).
    pub fn state_words(&self) -> &[u64] {
        &self.state
    }

    /// Process one sample (entry `fn(f64) -> f64`). Same contract.
    pub fn process_f64(&mut self, x: f64) -> f64 {
        assert_eq!(self.kind, EntryKind::F64ToF64);
        let (y, code) = call_f64(&mut self.ctx, self.fuel, &self.current, x);
        self.call_fuel = self.fuel.saturating_sub(self.ctx.fuel);
        if code == 0 {
            return y;
        }
        if self.roll_back(code) {
            let (y, code) = call_f64(&mut self.ctx, self.fuel, &self.current, x);
            self.call_fuel = self
                .call_fuel
                .saturating_add(self.fuel.saturating_sub(self.ctx.fuel));
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

fn call_dsp(ctx: &mut Ctx, fuel: u64, v: &Version, state: &mut [u64], x: f32) -> (f32, u32) {
    ctx.code = 0;
    ctx.depth = 0;
    ctx.fuel = fuel;
    ctx.table = v.table.as_ptr() as usize;
    let code = v.table[v.entry_slot as usize] as *const u8;
    let y = exec::call2::<usize, f32, f32>(code, state.as_mut_ptr() as usize, x, ctx);
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
        migrate_from: None,
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

    /// aarch64 `isb`: discard instructions prefetched before a new version
    /// becomes reachable. No-op elsewhere.
    #[inline]
    pub fn instruction_barrier() {
        #[cfg(target_arch = "aarch64")]
        // SAFETY: a context synchronization barrier; touches no memory,
        // stack or flags.
        unsafe {
            std::arch::asm!("isb", options(nostack, preserves_flags));
        }
    }

    pub fn free_module(m: JITModule) {
        unsafe { m.free_memory() }
    }
}
