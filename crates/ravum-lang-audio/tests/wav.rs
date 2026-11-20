//! V1-A1: the WAV host drives a hot-swapped JIT entry block by block; the
//! output equals per-sample calls; a version published between blocks is
//! adopted at the next block; `process` allocates nothing (INV-05).

use std::alloc::{GlobalAlloc, Layout, System};

use ravum_lang_audio::{wav_bytes, Processor, StreamConfig, WavHost};
use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::HotEngine;

/// Counts allocations made while `COUNTING` is set (this test binary only).
struct Counting;
thread_local! {
    // Per thread: the other tests of this binary run in parallel.
    static COUNTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALLOCS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn allocs() -> usize {
    ALLOCS.with(|a| a.get())
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.with(|c| c.get()) {
            ALLOCS.with(|a| a.set(a.get() + 1));
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

/// Wraps a processor so that only `process` itself is counted.
struct Counted<'a>(&'a mut dyn Processor);
impl Processor for Counted<'_> {
    fn process(&mut self, i: &[f32], o: &mut [f32]) {
        COUNTING.with(|c| c.set(true));
        self.0.process(i, o);
        COUNTING.with(|c| c.set(false));
    }
}
// SAFETY-free: `Counted` holds a `&mut dyn Processor` that is `Send`.
unsafe impl Send for Counted<'_> {}

const GAIN: &str =
    "fn gain(x: f32) -> f32 effects() {\n    let k: f32 = 0.5;\n    return x * k;\n}\n";

fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32 / n as f32) - 0.5).collect()
}

#[test]
fn blocks_equal_per_sample_calls_and_swap_between_blocks() {
    let ir = compile_source(GAIN).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&ir, "gain").unwrap();
    let input = ramp(1000);
    let before = allocs();
    let mut host = WavHost::new(StreamConfig {
        sample_rate: 48_000,
        block: 64,
    });
    let v2 = compile_source(&GAIN.replace("0.5", "0.25")).unwrap();
    let mut swapped_after = None;
    let out = host
        .render(&mut Counted(&mut rt), &input, |k| {
            // Editor side, between blocks: publish v2 after block 3.
            if k == 3 {
                engine.update(&v2).unwrap();
                swapped_after = Some(k);
            }
        })
        .to_vec();
    assert_eq!(swapped_after, Some(3));
    assert_eq!(out.len(), 1024, "padded to whole blocks");
    for (i, (&y, &x)) in out.iter().zip(&input).enumerate() {
        // Blocks 0..=3 run v1; v2 is adopted at the start of block 4.
        let k = if i < 4 * 64 { 0.5 } else { 0.25 };
        assert_eq!(y, x * k, "sample {i}");
    }
    assert!(out[1000..].iter().all(|&y| y == 0.0), "silence padding");
    assert_eq!(
        allocs() - before,
        0,
        "INV-05: process allocated on the audio thread"
    );
}

#[test]
fn a_dsp_block_with_state_and_a_trapping_update_keeps_running() {
    let src = "struct State {\n    acc: f32,\n}\n\nfn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    state.acc = state.acc * 0.5 + input;\n    return state.acc;\n}\n";
    let ir = compile_source(src).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&ir, "process_sample").unwrap();
    // v2 traps on every sample (division by zero): the runner rolls back.
    let bad = src.replace(
        "    return state.acc;",
        "    let z: i64 = 0;\n    let q: i64 = 1 / z;\n    return state.acc + (q as f32);",
    );
    let mut host = WavHost::new(StreamConfig::default());
    let input = vec![1.0f32; 256];
    let out = host
        .render(&mut rt, &input, |k| {
            if k == 1 {
                engine.update(&compile_source(&bad).unwrap()).unwrap();
            }
        })
        .to_vec();
    // The reference: the v1 filter throughout (the trapping v2 is rolled
    // back, its partial state writes undone).
    let mut acc = 0.0f32;
    for (i, &y) in out.iter().enumerate() {
        acc = acc * 0.5 + 1.0;
        assert_eq!(y, acc, "sample {i}");
    }
}

#[test]
fn the_wav_file_is_well_formed() {
    let bytes = wav_bytes(&[0.0, 1.0, -1.0, 2.0], 44_100);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 8);
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        44_100
    );
    assert_eq!(&bytes[36..40], b"data");
    let s: Vec<i16> = bytes[44..]
        .chunks(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(s, [0, 32767, -32767, 32767], "clamped");
    let dir = std::env::temp_dir().join(format!("rv-wav-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ir = compile_source(GAIN).unwrap();
    let (_e, mut rt) = HotEngine::start(&ir, "gain").unwrap();
    let mut host = WavHost::new(StreamConfig::default());
    host.render(&mut rt, &ramp(100), |_| {});
    let path = dir.join("out.wav");
    host.write_wav(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 44 + 128 * 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_allocation_counter_detects_an_allocating_processor() {
    // Negative control: without it, `0 allocations` could be vacuous.
    struct Alloc;
    impl Processor for Alloc {
        fn process(&mut self, i: &[f32], o: &mut [f32]) {
            let v: Vec<f32> = i.to_vec();
            o.copy_from_slice(&v);
        }
    }
    let before = allocs();
    let mut p = Alloc;
    let mut host = WavHost::new(StreamConfig::default());
    host.render(&mut Counted(&mut p), &[0.5; 64], |_| {});
    assert!(allocs() > before);
}
