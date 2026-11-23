//! HR-1.7 on the native backend (decision of 2026-11-19: migration on the
//! editor side). A changed `State` with a `migrate(old: T) -> State` whose
//! `T` has the current schema is accepted; the runner only copies its state
//! into preallocated atomics and later swaps buffers (no allocation).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::Duration;

use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::{HotEngine, Refusal};

struct Counting;
static RT_ALLOCS: AtomicU64 = AtomicU64::new(0);
static RT_FREES: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static IN_RT: Cell<bool> = const { Cell::new(false) };
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if IN_RT.with(|f| f.get()) {
            RT_ALLOCS.fetch_add(1, SeqCst);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if IN_RT.with(|f| f.get()) {
            RT_FREES.fetch_add(1, SeqCst);
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

const V1: &str = "struct State {\n    z1: f32,\n    z2: f32,\n}\n\n\
fn init() -> State effects() {\n    return State { z1: 0.0, z2: 0.0 };\n}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let y = 0.2 * input + state.z1;\n    state.z1 = 0.4 * input + 0.5 * y + state.z2;\n    state.z2 = 0.2 * input - 0.25 * y;\n    return y;\n}\n";

/// V2 adds a gain to the state; `migrate` takes V1's schema (`Before` has
/// the same shape as V1's `State`: schemas name types by rank, HR-1.1).
fn v2(migrate_body: &str, extra: &str) -> String {
    format!(
        "struct Before {{\n    z1: f32,\n    z2: f32,\n}}\n\nstruct State {{\n    z1: f32,\n    z2: f32,\n    gain: f32,\n}}\n\n\
fn init() -> State effects() {{\n    return State {{ z1: 0.0, z2: 0.0, gain: 1.0 }};\n}}\n\n\
fn migrate(old: Before) -> State effects() {{\n    {migrate_body}\n}}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {{\n    {extra}let y = 0.2 * input + state.z1;\n    state.z1 = 0.4 * input + 0.5 * y + state.z2;\n    state.z2 = 0.2 * input - 0.25 * y;\n    return y * state.gain;\n}}\n"
    )
}

const MIGRATE: &str = "return State { z1: old.z1, z2: old.z2, gain: 3.0 };";

fn signal(n: usize) -> Vec<f32> {
    let mut s: u32 = 0x0bad_5eed;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s % 2000) as f32 / 1000.0 - 1.0
        })
        .collect()
}

/// The filter in Rust; from sample `at` on, the output is multiplied by 3.
fn reference(x: &[f32], at: usize) -> Vec<f32> {
    let (mut z1, mut z2) = (0.0f32, 0.0f32);
    x.iter()
        .enumerate()
        .map(|(n, v)| {
            let y = 0.2 * v + z1;
            z1 = 0.4 * v + 0.5 * y + z2;
            z2 = 0.2 * v - 0.25 * y;
            if n >= at {
                y * 3.0
            } else {
                y
            }
        })
        .collect()
}

#[test]
fn a_migrated_state_continues_exactly_when_no_sample_runs_in_between() {
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(V1).unwrap(), "process_sample").unwrap();
    let x = signal(3000);
    let mut out = Vec::new();
    for (n, v) in x.iter().enumerate() {
        if n == 1000 {
            let r = engine
                .update(&compile_source(&v2(MIGRATE, "")).unwrap())
                .unwrap();
            assert!(r.migration_pending);
            let ev = rt.poll_event();
            assert!(ev.snapshot && !ev.adopted, "{ev:?}");
            engine.collect_migration().unwrap();
            assert!(rt.poll());
        }
        out.push(rt.process_sample(*v));
    }
    let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&out), bits(&reference(&x, 1000)));
    // The state is now V2's: 3 words of f32 fit in 2 u64 words.
    assert_eq!(rt.state_words().len(), 2);
}

#[test]
fn a_migrate_from_another_schema_is_refused() {
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(V1).unwrap(), "process_sample").unwrap();
    // `Before` gains a field: its schema is no longer the current one.
    let src = v2(MIGRATE, "").replacen(
        "struct Before {\n    z1: f32,\n    z2: f32,\n}",
        "struct Before {\n    z1: f32,\n    z2: f32,\n    z3: f32,\n}",
        1,
    );
    match engine.update(&compile_source(&src).unwrap()) {
        Err(Refusal::StateSchemaChanged { .. }) => {}
        other => panic!("expected StateSchemaChanged, got {other:?}"),
    }
    assert!(!rt.poll_event().snapshot);
    assert!(!engine.migration_pending());
}

#[test]
fn a_trapping_migrate_refuses_the_module_and_the_block_continues() {
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(V1).unwrap(), "process_sample").unwrap();
    let x = signal(600);
    let mut out = Vec::new();
    for (n, v) in x.iter().enumerate() {
        if n == 200 {
            let bad = v2("let d: i32 = 0;\n    let k: i32 = 1 / d;\n    return State { z1: old.z1, z2: old.z2, gain: 3.0 };", "");
            engine.update(&compile_source(&bad).unwrap()).unwrap();
            assert!(rt.poll_event().snapshot);
            match engine.collect_migration() {
                Err(Refusal::Invalid(m)) => assert!(m.contains("division by zero"), "{m}"),
                other => panic!("expected a refusal, got {other:?}"),
            }
            assert!(!rt.poll(), "nothing may be published");
        }
        out.push(rt.process_sample(*v));
    }
    assert_eq!(out, reference(&x, usize::MAX));
    // The engine is usable again: a correct migration then goes through.
    engine
        .update(&compile_source(&v2(MIGRATE, "")).unwrap())
        .unwrap();
    assert!(rt.poll_event().snapshot);
    engine.collect_migration().unwrap();
    assert!(rt.poll());
}

#[test]
fn a_trap_after_migration_rolls_back_to_the_old_schema_and_state() {
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(V1).unwrap(), "process_sample").unwrap();
    let x = signal(800);
    let mut out = Vec::new();
    for (n, v) in x.iter().enumerate() {
        if n == 400 {
            // V2's process_sample divides by zero: the runner must return to
            // V1 *and* to V1's state, as it was at adoption.
            let bad = v2(MIGRATE, "let d: i32 = 0;\n    let k: i32 = 1 / d;\n    ");
            engine.update(&compile_source(&bad).unwrap()).unwrap();
            assert!(rt.poll_event().snapshot);
            engine.collect_migration().unwrap();
            assert!(rt.poll());
        }
        out.push(rt.process_sample(*v));
    }
    assert_eq!(out, reference(&x, usize::MAX));
    assert_eq!(rt.state_words().len(), 1, "back on V1's buffers");
}

#[test]
fn migration_on_a_real_time_thread_allocates_nothing() {
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(V1).unwrap(), "process_sample").unwrap();
    let mailbox = engine.mailbox().clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rt = stop.clone();
    let audio = std::thread::spawn(move || {
        let input = signal(64);
        let mut buf = [0.0f32; 64];
        let (mut snaps, mut adopted) = (0u32, 0u32);
        IN_RT.with(|f| f.set(true));
        while !stop_rt.load(SeqCst) {
            let ev = rt.poll_event();
            snaps += ev.snapshot as u32;
            adopted += ev.adopted as u32;
            for (o, i) in buf.iter_mut().zip(&input) {
                *o = rt.process_sample(*i);
            }
            std::thread::sleep(Duration::from_micros(1333));
        }
        IN_RT.with(|f| f.set(false));
        (rt, snaps, adopted)
    });
    let r = engine
        .update(&compile_source(&v2(MIGRATE, "")).unwrap())
        .unwrap();
    while engine.migration_pending() {
        engine.collect_migration().unwrap();
        std::thread::yield_now();
    }
    while mailbox.active_generation.load(SeqCst) != r.generation {
        std::thread::yield_now();
    }
    std::thread::sleep(Duration::from_millis(20));
    engine.collect();
    stop.store(true, SeqCst);
    let (rt, snaps, adopted) = audio.join().unwrap();
    assert_eq!((snaps, adopted), (1, 1));
    assert_eq!(rt.state_words().len(), 2);
    assert_eq!(
        RT_ALLOCS.load(SeqCst),
        0,
        "allocation on the real-time thread"
    );
    assert_eq!(RT_FREES.load(SeqCst), 0, "free on the real-time thread");
}
