//! V2-L4 (T8/T9): the `examples/dsp-gain-iir` DSP block runs on the audio
//! host through the JIT: output = Rust reference = IR interpreter, bit for
//! bit; a hot edit (gain 2.0 -> 1.0) is adopted at a block boundary with the
//! filter state kept; `process` allocates nothing; the swap discontinuity is
//! measured (the JIT host has no crossfade: fade length 0 frames).

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;

use ravum_lang_audio::{Processor, StreamConfig, WavHost};
use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_dsp, DEFAULT_STEPS};
use ravum_lang_jit::hot::HotEngine;

struct Counting;
thread_local! {
    static COUNTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALLOCS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.with(|c| c.get()) {
            ALLOCS.with(|a| a.set(a.get() + 1));
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if COUNTING.with(|c| c.get()) {
            ALLOCS.with(|a| a.set(a.get() + 1));
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

/// Counts allocations and frees made inside `process` only.
struct Counted<'a>(&'a mut dyn Processor);
impl Processor for Counted<'_> {
    fn process(&mut self, i: &[f32], o: &mut [f32]) {
        COUNTING.with(|c| c.set(true));
        self.0.process(i, o);
        COUNTING.with(|c| c.set(false));
    }
}

fn block_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/dsp-gain-iir/block/process.rv");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn with_gain(src: &str, gain: &str) -> String {
    let edited = src.replace("    return 2.0;\n", &format!("    return {gain};\n"));
    assert_ne!(edited, src, "the gain line of the example changed");
    edited
}

/// Rust reference, gain switching from `g0` to `g1` at sample `switch`.
fn reference(input: &[f32], g0: f32, g1: f32, switch: usize) -> Vec<f32> {
    let (a, b) = (0.8f32, 0.2f32);
    let mut y = 0.0f32;
    input
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            let boosted = x * if n < switch { g0 } else { g1 };
            y = a * y + b * boosted;
            y
        })
        .collect()
}

fn sine(n: usize, freq: f32, rate: f32) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
        .collect()
}

const CONFIG: StreamConfig = StreamConfig {
    sample_rate: 48_000,
    block: 64,
};

#[test]
fn the_example_block_equals_the_reference_and_the_interpreter() {
    let src = block_source();
    let ir = compile_source(&src).unwrap();
    let input = sine(4800, 440.0, 48_000.0);
    let (_engine, mut rt) = HotEngine::start(&ir, "process_sample").unwrap();
    let mut host = WavHost::new(CONFIG);
    let out = host.render(&mut rt, &input, |_| {}).to_vec();
    let expected = reference(&input, 2.0, 2.0, usize::MAX);
    assert_eq!(
        out.len(),
        expected.len(),
        "4800 is a whole number of blocks"
    );
    for (n, (o, e)) in out.iter().zip(&expected).enumerate() {
        assert_eq!(
            o.to_bits(),
            e.to_bits(),
            "sample {n}: jit {o} vs reference {e}"
        );
    }
    let interp = run_dsp(
        &ir,
        Some("init"),
        "process_sample",
        &input,
        DEFAULT_STEPS * 100,
    )
    .unwrap();
    for (n, (o, i)) in out.iter().zip(&interp).enumerate() {
        assert_eq!(
            o.to_bits(),
            i.to_bits(),
            "sample {n}: jit {o} vs interpreter {i}"
        );
    }
}

#[test]
fn a_hot_gain_edit_is_adopted_at_a_block_boundary_without_allocation() {
    let src = block_source();
    let input = sine(48_000, 440.0, 48_000.0);
    let (mut engine, mut rt) =
        HotEngine::start(&compile_source(&src).unwrap(), "process_sample").unwrap();
    let edited = compile_source(&with_gain(&src, "1.0")).unwrap();
    let publish_after = 300usize; // block index; adopted at the next block
    let mut host = WavHost::new(CONFIG);
    let allocs_before = ALLOCS.with(|a| a.get());
    let mut counted = Counted(&mut rt);
    let out = host
        .render(&mut counted, &input, |k| {
            if k == publish_after {
                engine.update(&edited).expect("same state schema: accepted");
            }
        })
        .to_vec();
    let allocs = ALLOCS.with(|a| a.get()) - allocs_before;
    assert_eq!(allocs, 0, "allocations or frees inside process()");

    let switch = (publish_after + 1) * CONFIG.block;
    let expected = reference(&input, 2.0, 1.0, switch);
    for (n, (o, e)) in out.iter().zip(&expected).enumerate() {
        assert_eq!(
            o.to_bits(),
            e.to_bits(),
            "sample {n}: jit {o} vs reference {e} (switch at {switch}, state kept)"
        );
    }
    // Negative control: a reference that reset the filter state at the switch
    // differs, so the equality above really shows the state was kept.
    let mut reset = expected.clone();
    let mut y = 0.0f32;
    for n in switch..input.len() {
        y = 0.8 * y + 0.2 * input[n];
        reset[n] = y;
    }
    assert_ne!(
        out[switch], reset[switch],
        "state reset would be indistinguishable"
    );

    // Fade measurement (RFC-E11-DSP-BLOCK §0 « fondu non mesuré ») : the JIT
    // host switches at a block boundary, no crossfade. Report the step at the
    // switch against the largest step of the steady signal before it.
    let step_at_switch = (out[switch] - out[switch - 1]).abs();
    let steady_max = out[CONFIG.block..switch]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    println!(
        "fade: 0 frames (block-boundary switch at sample {switch}); \
         |Δy| at switch = {step_at_switch:.6}, max steady |Δy| = {steady_max:.6}"
    );
    assert!(
        step_at_switch <= steady_max,
        "with the IIR smoothing, the gain edit must not produce a step larger than the signal's own"
    );
}
