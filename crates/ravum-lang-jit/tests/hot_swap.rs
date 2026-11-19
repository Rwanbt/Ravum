//! T16 — per-function hot swap on the native backend.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::{HotEngine, Refusal};

// ---- allocation counter for the real-time thread --------------------------

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

// ---- programs ---------------------------------------------------------------

/// `helpers` pure helpers, a `gain` function and the entry `process`.
fn program(gain: &str, helpers: usize, extra: &str) -> String {
    let mut s = String::new();
    for i in 0..helpers {
        s.push_str(&format!(
            "fn h{i}(x: f64) -> f64 effects() {{\n    let mut y = x;\n    if y > 1.0 {{\n        y = 1.0;\n    }}\n    if y < -1.0 {{\n        y = -1.0;\n    }}\n    return y * 0.999 + 0.0;\n}}\n\n"
        ));
    }
    s.push_str(&format!(
        "fn gain() -> f64 effects() {{\n    {extra}return {gain};\n}}\n\n"
    ));
    s.push_str("fn process(x: f64) -> f64 effects() {\n    let mut y = x;\n");
    for i in 0..helpers {
        s.push_str(&format!("    y = h{i}(y);\n"));
    }
    s.push_str("    return y * gain();\n}\n");
    s
}

fn expected(x: f64, gain: f64, helpers: usize) -> f64 {
    let mut y = x;
    for _ in 0..helpers {
        y = y.clamp(-1.0, 1.0) * 0.999 + 0.0;
    }
    y * gain
}

// ---- tests ------------------------------------------------------------------

#[test]
fn only_the_edited_function_is_recompiled_and_swapped_at_the_safepoint() {
    let v1 = compile_source(&program("0.5", 20, "")).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&v1, "process").unwrap();
    assert_eq!(rt.process_f64(0.3), expected(0.3, 0.5, 20));

    let v2 = compile_source(&program("0.25", 20, "")).unwrap();
    let report = engine.update(&v2).unwrap();
    assert_eq!(report.recompiled, vec!["gain".to_string()], "{report:?}");
    assert_eq!(report.reused, 21);
    // Not yet at a safepoint: the old version still runs.
    assert_eq!(rt.process_f64(0.3), expected(0.3, 0.5, 20));
    assert!(rt.poll());
    assert_eq!(rt.process_f64(0.3), expected(0.3, 0.25, 20));
    assert_eq!(rt.generation(), report.generation);

    // Whitespace-only edit: nothing to compile.
    let v3 =
        compile_source(&program("0.25", 20, "").replace("y * gain()", "y  *  gain()")).unwrap();
    assert!(engine.update(&v3).unwrap().recompiled.is_empty());
}

#[test]
fn an_incompatible_entry_is_refused_and_the_running_version_continues() {
    let v1 = compile_source(&program("0.5", 2, "")).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&v1, "process").unwrap();
    let bad = compile_source(
        &program("0.5", 2, "")
            .replace("fn process(x: f64) -> f64", "fn process(x: f32) -> f32")
            .replace(
                "let mut y = x;\n    y = h0",
                "let mut y = x as f64;\n    y = h0",
            )
            .replace("return y * gain();\n}", "return (y * gain()) as f32;\n}"),
    );
    let bad = bad.expect("negative program must still compile");
    assert!(matches!(
        engine.update(&bad),
        Err(Refusal::EntrySignature(_))
    ));
    let missing =
        compile_source("fn other(x: f64) -> f64 effects() {\n    return x;\n}\n").unwrap();
    assert!(matches!(
        engine.update(&missing),
        Err(Refusal::EntryMissing(_))
    ));
    assert!(!rt.poll(), "nothing may be published after a refusal");
    assert_eq!(rt.process_f64(0.3), expected(0.3, 0.5, 2));
}

#[test]
fn a_trapping_edit_rolls_back_to_the_previous_version() {
    let v1 = compile_source(&program("0.5", 2, "")).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&v1, "process").unwrap();
    let v2 = compile_source(&program(
        "0.5",
        2,
        "let d: i64 = 0;\n    let k: i64 = 10 / d;\n    ",
    ))
    .unwrap();
    let r = engine.update(&v2).unwrap();
    assert!(rt.poll());
    // The new `gain` traps: output comes from the previous version.
    assert_eq!(rt.process_f64(0.3), expected(0.3, 0.5, 2));
    let mb = engine.mailbox();
    assert_eq!(mb.rolled_back_generation.load(SeqCst), r.generation);
    assert_eq!(
        engine.trap_message(mb.last_trap.load(SeqCst)),
        Some("division by zero")
    );
    assert_ne!(rt.generation(), r.generation);
}

#[test]
fn real_time_thread_swaps_20_edits_without_allocating() {
    const HELPERS: usize = 50;
    const BLOCK: usize = 64;
    let v1 = compile_source(&program("1.0", HELPERS, "")).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&v1, "process").unwrap();
    let mailbox = engine.mailbox().clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rt = stop.clone();
    let blocks = Arc::new(AtomicU64::new(0));
    let blocks_rt = blocks.clone();
    let last = Arc::new(AtomicU64::new(0));
    let last_rt = last.clone();

    let audio = std::thread::spawn(move || {
        let mut buf = [0.0f64; BLOCK];
        // Warm-up outside the measured region.
        for s in buf.iter_mut() {
            *s = rt.process_f64(0.3);
        }
        IN_RT.with(|f| f.set(true));
        while !stop_rt.load(SeqCst) {
            rt.poll(); // safepoint: block boundary
            for s in buf.iter_mut() {
                *s = rt.process_f64(0.3);
            }
            last_rt.store(buf[BLOCK - 1].to_bits(), SeqCst);
            blocks_rt.fetch_add(1, SeqCst);
            std::thread::sleep(Duration::from_micros(1333)); // 64 frames @ 48 kHz
        }
        IN_RT.with(|f| f.set(false));
        rt
    });

    let mut latencies = Vec::new();
    let mut compile_times = Vec::new();
    for edit in 1..=20 {
        let g = format!("{}.0", edit + 1);
        let t0 = Instant::now(); // the edit is saved
        let ir = compile_source(&program(&g, HELPERS, "")).unwrap();
        let report = engine.update(&ir).unwrap();
        assert_eq!(report.recompiled, vec!["gain".to_string()]);
        compile_times.push(report.compile_time);
        while mailbox.active_generation.load(SeqCst) != report.generation {
            std::thread::yield_now();
        }
        latencies.push(t0.elapsed());
        // Let a few blocks run on the new version, then collect retired code.
        let b = blocks.load(SeqCst);
        while blocks.load(SeqCst) < b + 3 {
            std::thread::yield_now();
        }
        assert_eq!(
            f64::from_bits(last.load(SeqCst)),
            expected(0.3, (edit + 1) as f64, HELPERS)
        );
        engine.collect();
    }
    stop.store(true, SeqCst);
    let rt = audio.join().unwrap();
    engine.collect();

    latencies.sort();
    compile_times.sort();
    let p = |v: &Vec<Duration>, q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
    eprintln!(
        "T16: blocks={} rt_allocs={} rt_frees={} edit->activation p50={:?} p95={:?} max={:?} update(compile) p50={:?} live_modules={}",
        blocks.load(SeqCst),
        RT_ALLOCS.load(SeqCst),
        RT_FREES.load(SeqCst),
        p(&latencies, 0.5),
        p(&latencies, 0.95),
        latencies.last().unwrap(),
        p(&compile_times, 0.5),
        engine.live_modules()
    );
    assert_eq!(
        RT_ALLOCS.load(SeqCst),
        0,
        "allocation on the real-time thread"
    );
    assert_eq!(
        RT_FREES.load(SeqCst),
        0,
        "deallocation on the real-time thread"
    );
    assert!(
        engine.live_modules() <= 3,
        "old machine code is not reclaimed: {}",
        engine.live_modules()
    );
    assert!(
        p(&latencies, 0.95) < Duration::from_millis(200),
        "p95 edit->activation too slow"
    );
    drop(rt);
}
