//! IR v1.1 + T16: a stateful DSP block (`struct State`, `init`,
//! `process_sample(&mut State, f32) -> f32`, the RFC-E11-DSP-BLOCK surface)
//! hot-swapped on the native backend with its host-owned state preserved.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_dsp, DEFAULT_STEPS};
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

/// A biquad (transposed direct form II) with the host state schema of
/// RFC-E11-DSP-BLOCK, `struct State { z1: f32, z2: f32 }`.
fn biquad(b0: &str, extra: &str) -> String {
    format!(
        "struct State {{\n    z1: f32,\n    z2: f32,\n}}\n\n\
fn init() -> State effects() {{\n    return State {{ z1: 0.0, z2: 0.0 }};\n}}\n\n\
fn b0() -> f32 effects() {{\n    return {b0};\n}}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {{\n    {extra}let b1: f32 = 0.4;\n    let b2: f32 = 0.2;\n    let a1: f32 = -0.5;\n    let a2: f32 = 0.25;\n    let y = b0() * input + state.z1;\n    state.z1 = b1 * input - a1 * y + state.z2;\n    state.z2 = b2 * input - a2 * y;\n    return y;\n}}\n"
    )
}

/// The same filter written directly in Rust (same f32 operation order).
fn reference(inputs: &[f32], b0_at: impl Fn(usize) -> f32) -> Vec<f32> {
    let (mut z1, mut z2) = (0.0f32, 0.0f32);
    let (b1, b2, a1, a2) = (0.4f32, 0.2f32, -0.5f32, 0.25f32);
    inputs
        .iter()
        .enumerate()
        .map(|(n, x)| {
            let y = b0_at(n) * x + z1;
            z1 = b1 * x - a1 * y + z2;
            z2 = b2 * x - a2 * y;
            y
        })
        .collect()
}

fn signal(n: usize) -> Vec<f32> {
    let mut s: u32 = 0x1234_5678;
    (0..n)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            ((s % 2000) as f32 / 1000.0 - 1.0) * if i % 97 == 0 { 4.0 } else { 1.0 }
        })
        .collect()
}

#[test]
fn native_dsp_block_is_bit_identical_to_interpreter_and_rust_reference() {
    let ir = compile_source(&biquad("0.2", "")).unwrap();
    let st = ir.state.as_ref().expect("State schema");
    assert_eq!(
        st.text,
        ravum_lang_sema::reload::DSP_STATE_SCHEMA_V1_TEXT,
        "same schema as the rustc path"
    );
    let x = signal(10_000);
    let interp = run_dsp(&ir, Some("init"), "process_sample", &x, DEFAULT_STEPS * 100).unwrap();
    let (_engine, mut rt) = HotEngine::start(&ir, "process_sample").unwrap();
    let native: Vec<f32> = x.iter().map(|v| rt.process_sample(*v)).collect();
    let rust = reference(&x, |_| 0.2);
    let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&native), bits(&interp), "native vs interpreter");
    assert_eq!(bits(&native), bits(&rust), "native vs Rust reference");
}

#[test]
fn swapping_a_coefficient_keeps_the_filter_state() {
    let (mut engine, mut rt) = HotEngine::start(
        &compile_source(&biquad("0.2", "")).unwrap(),
        "process_sample",
    )
    .unwrap();
    let x = signal(4_000);
    let mut out = Vec::new();
    for (n, v) in x.iter().enumerate() {
        if n == 1_000 {
            let r = engine
                .update(&compile_source(&biquad("0.3", "")).unwrap())
                .unwrap();
            assert_eq!(r.recompiled, vec!["b0".to_string()]);
            assert!(rt.poll());
        }
        out.push(rt.process_sample(*v));
    }
    let expected = reference(&x, |n| if n < 1_000 { 0.2 } else { 0.3 });
    assert_eq!(
        out.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
        "state must continue across the swap (no reset, no glitch from a lost z1/z2)"
    );
}

#[test]
fn a_state_schema_change_is_refused_and_the_block_continues() {
    let (mut engine, mut rt) = HotEngine::start(
        &compile_source(&biquad("0.2", "")).unwrap(),
        "process_sample",
    )
    .unwrap();
    let wider = biquad("0.2", "")
        .replace("    z2: f32,\n}", "    z2: f32,\n    z3: f32,\n}")
        .replace(
            "State { z1: 0.0, z2: 0.0 }",
            "State { z1: 0.0, z2: 0.0, z3: 0.0 }",
        );
    match engine.update(&compile_source(&wider).unwrap()) {
        Err(Refusal::StateSchemaChanged { old, new }) => assert_ne!(old, new),
        other => panic!("expected StateSchemaChanged, got {other:?}"),
    }
    assert!(!rt.poll());
    let x = signal(100);
    let out: Vec<f32> = x.iter().map(|v| rt.process_sample(*v)).collect();
    assert_eq!(out, reference(&x, |_| 0.2));
}

#[test]
fn a_trap_after_a_partial_state_write_restores_the_state_and_rolls_back() {
    let (mut engine, mut rt) = HotEngine::start(
        &compile_source(&biquad("0.2", "")).unwrap(),
        "process_sample",
    )
    .unwrap();
    let x = signal(600);
    let mut out = Vec::new();
    for (n, v) in x.iter().enumerate() {
        if n == 300 {
            // Writes the state, then divides by zero: the write must be undone.
            let bad = biquad(
                "0.2",
                "state.z1 = 1000.0;\n    let d: i32 = 0;\n    let k: i32 = 1 / d;\n    ",
            );
            engine.update(&compile_source(&bad).unwrap()).unwrap();
            assert!(rt.poll());
        }
        out.push(rt.process_sample(*v));
    }
    assert_eq!(
        out,
        reference(&x, |_| 0.2),
        "output must be the old filter, state intact"
    );
    assert_eq!(
        engine.trap_message(engine.mailbox().last_trap.load(SeqCst)),
        Some("division by zero")
    );
}

#[test]
fn dsp_swaps_on_a_real_time_thread_without_allocating() {
    let (mut engine, mut rt) = HotEngine::start(
        &compile_source(&biquad("0.1", "")).unwrap(),
        "process_sample",
    )
    .unwrap();
    let mailbox = engine.mailbox().clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rt = stop.clone();
    let blocks = Arc::new(AtomicU64::new(0));
    let blocks_rt = blocks.clone();
    let audio = std::thread::spawn(move || {
        let input = signal(64);
        let mut buf = [0.0f32; 64];
        for (o, i) in buf.iter_mut().zip(&input) {
            *o = rt.process_sample(*i);
        }
        IN_RT.with(|f| f.set(true));
        while !stop_rt.load(SeqCst) {
            rt.poll();
            for (o, i) in buf.iter_mut().zip(&input) {
                *o = rt.process_sample(*i);
            }
            blocks_rt.fetch_add(1, SeqCst);
            std::thread::sleep(Duration::from_micros(1333));
        }
        IN_RT.with(|f| f.set(false));
        rt
    });
    let mut lat = Vec::new();
    for edit in 1..=20 {
        let t0 = Instant::now();
        let r = engine
            .update(&compile_source(&biquad(&format!("0.{}", (edit % 9) + 1), "")).unwrap())
            .unwrap();
        while mailbox.active_generation.load(SeqCst) != r.generation {
            std::thread::yield_now();
        }
        lat.push(t0.elapsed());
        let b = blocks.load(SeqCst);
        while blocks.load(SeqCst) < b + 2 {
            std::thread::yield_now();
        }
        engine.collect();
    }
    stop.store(true, SeqCst);
    let rt = audio.join().unwrap();
    lat.sort();
    eprintln!(
        "DSP hot swap: blocks={} rt_allocs={} rt_frees={} edit->activation p50={:?} p95={:?} live_modules={}",
        blocks.load(SeqCst),
        RT_ALLOCS.load(SeqCst),
        RT_FREES.load(SeqCst),
        lat[lat.len() / 2],
        lat[(lat.len() * 95) / 100],
        engine.live_modules()
    );
    assert_eq!(RT_ALLOCS.load(SeqCst), 0);
    assert_eq!(RT_FREES.load(SeqCst), 0);
    assert!(engine.live_modules() <= 3);
    drop(rt);
}
