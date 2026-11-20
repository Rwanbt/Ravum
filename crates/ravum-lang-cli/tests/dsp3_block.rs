//! RFC-DSP-ABI-2 (ABI DSP 3), host side: a `.rv` block with its own state is
//! built as a `cdylib`, loaded by `ravum_patch_host::Block3Host`, and
//! - computes exactly the Rust reference (DSP3-T1);
//! - keeps its state across a same-schema replacement (DSP3-T2/T4);
//! - is refused when the schema changes, without touching the running block;
//! - never loads as an ABI 2 block, and an ABI 2 block never loads as ABI 3
//!   (DSP3-7, DSP3-T5); ABI 2 itself is unchanged (`tests/dsp_block.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use ravum_patch_host::{prepare_block, prepare_block3, Block3Host, ExpectedAbi, StateLayout};
use ravum_protocol::RejectReason;

/// Counts allocations and frees made while `COUNTING` is set, per thread.
struct Counting;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static EVENTS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.with(|c| c.get()) {
            EVENTS.with(|e| e.set(e.get() + 1));
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if COUNTING.with(|c| c.get()) {
            EVENTS.with(|e| e.set(e.get() + 1));
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

/// `host.process`, counting allocations and frees inside it only.
fn counted(host: &mut Block3Host, i: &[f32], o: &mut [f32]) {
    COUNTING.with(|c| c.set(true));
    host.process(i, o);
    COUNTING.with(|c| c.set(false));
}

/// Gain then first-order IIR, state `{ y }` (the `examples/dsp-gain-iir` block).
fn gain_iir(gain: &str) -> String {
    format!(
        "struct State {{\n    y: f32,\n}}\n\n\
fn init() -> State effects() {{\n    return State {{ y: 0.0 }};\n}}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {{\n    let a: f32 = 0.8;\n    let b: f32 = 0.2;\n    state.y = a * state.y + b * (input * {gain});\n    return state.y;\n}}\n\n\
fn main() -> () effects() {{\n}}\n"
    )
}

/// A wider state: a 4-tap delay line in a nested struct plus a counter.
const DELAY: &str = "struct Line {\n    z: [f32; 4],\n}\n\nstruct State {\n    line: Line,\n    n: u32,\n}\n\n\
fn init() -> State effects() {\n    return State { line: Line { z: [0.0; 4] }, n: 0 };\n}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let y: f32 = state.line.z[3];\n    state.line.z[3] = state.line.z[2];\n    state.line.z[2] = state.line.z[1];\n    state.line.z[1] = state.line.z[0];\n    state.line.z[0] = input;\n    state.n = state.n.wrapping_add(1);\n    return y;\n}\n\nfn main() -> () effects() {\n}\n";

const ABI2: &str = "struct State {\n    z1: f32,\n    z2: f32,\n}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    return input;\n}\n\nfn main() -> () effects() {\n}\n";

fn build(dir: &Path, name: &str, source: &str) -> PathBuf {
    let package = dir.join(name);
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(
        package.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    fs::write(package.join("src/main.rv"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["build", "--emit", "dsp-block", "."])
        .current_dir(&package)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "build failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    PathBuf::from(
        stdout
            .lines()
            .find_map(|l| l.strip_prefix("built: "))
            .expect("built: line")
            .trim(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ravum-dsp3-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn reference(input: &[f32], gain_at: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut y = 0.0f32;
    input
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            y = 0.8 * y + 0.2 * (x * gain_at(n));
            y
        })
        .collect()
}

fn signal(n: usize) -> Vec<f32> {
    (0..n).map(|i| ((i as f32) * 0.037).sin() * 0.5).collect()
}

#[test]
fn an_abi3_block_runs_and_keeps_its_state_across_a_same_schema_swap() {
    let dir = scratch("swap");
    let first = prepare_block3(&build(&dir, "gain2", &gain_iir("2.0"))).expect("ABI 3 block");
    assert_eq!(first.layout, StateLayout { size: 4, align: 4 });
    let mut host = Block3Host::new(first).expect("state allocated");
    let input = signal(4096);
    let mut out = vec![0.0f32; input.len()];
    let switch = 2048;
    for (k, (i, o)) in input.chunks(64).zip(out.chunks_mut(64)).enumerate() {
        if k * 64 == switch {
            let edited = prepare_block3(&build(&dir, "gain1", &gain_iir("1.0"))).unwrap();
            let old = host.replace(edited).expect("same schema: accepted");
            drop(old); // off the audio path, by the caller
        }
        assert_eq!(host.process(i, o), i.len());
    }
    let expected = reference(&input, |n| if n < switch { 2.0 } else { 1.0 });
    for (n, (o, e)) in out.iter().zip(&expected).enumerate() {
        assert_eq!(
            o.to_bits(),
            e.to_bits(),
            "sample {n}: {o} vs {e} (state kept at {switch})"
        );
    }
}

#[test]
fn a_wider_state_with_a_nested_struct_and_an_array_is_laid_out_and_used() {
    let dir = scratch("delay");
    let module = prepare_block3(&build(&dir, "delay", DELAY)).expect("ABI 3 block");
    assert_eq!(
        module.layout,
        StateLayout { size: 20, align: 4 },
        "[f32; 4] + u32, repr(C)"
    );
    let mut host = Block3Host::new(module).unwrap();
    let input: Vec<f32> = (1..=10).map(|i| i as f32).collect();
    let mut out = vec![0.0f32; 10];
    host.process(&input, &mut out);
    assert_eq!(out, [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn schema_changes_and_abi_mixes_are_refused() {
    let dir = scratch("refuse");
    let gain = prepare_block3(&build(&dir, "g", &gain_iir("2.0"))).unwrap();
    let delay = prepare_block3(&build(&dir, "d", DELAY)).unwrap();
    let mut host = Block3Host::new(gain.clone()).unwrap();
    match host.replace(delay) {
        Err(RejectReason::StateSchemaChanged { .. }) => {}
        other => panic!("a new schema must be refused: {:?}", other.map(|_| ())),
    }
    assert!(
        std::sync::Arc::ptr_eq(host.active(), &gain),
        "the running block is kept"
    );
    match host.replace(gain.clone()) {
        Err(RejectReason::AlreadyActive { .. }) => {}
        other => panic!("{:?}", other.map(|_| ())),
    }

    // DSP3-7: ABI 2 and ABI 3 never load as each other.
    let abi2 = build(&dir, "abi2", ABI2);
    match prepare_block3(&abi2) {
        Err(RejectReason::AbiMismatch {
            expected: 3,
            found: 2,
        }) => {}
        other => panic!("ABI 2 loaded as ABI 3: {:?}", other.map(|_| ())),
    }
    let abi3 = build(&dir, "abi3", &gain_iir("2.0"));
    match prepare_block(&abi3, &ExpectedAbi::dsp_with_state_schema()) {
        Err(RejectReason::AbiMismatch {
            expected: 2,
            found: 3,
        }) => {}
        other => panic!("ABI 3 loaded as ABI 2: {:?}", other.map(|m| m.abi_version)),
    }
}

#[test]
fn a_crossfaded_swap_mixes_both_versions_and_allocates_nothing_in_process() {
    let dir = scratch("fade");
    let g2 = prepare_block3(&build(&dir, "f2", &gain_iir("2.0"))).unwrap();
    let g1 = prepare_block3(&build(&dir, "f1", &gain_iir("1.0"))).unwrap();
    let input = signal(4096);
    let switch = 1024usize;
    let fade = 256u32;
    let run = |fade_frames: u32| {
        let mut host = Block3Host::new(g2.clone()).unwrap();
        let mut out = vec![0.0f32; input.len()];
        let before = EVENTS.with(|e| e.get());
        for (k, (i, o)) in input.chunks(64).zip(out.chunks_mut(64)).enumerate() {
            if k * 64 == switch {
                host.replace_with_fade(g1.clone(), fade_frames).unwrap();
            }
            counted(&mut host, i, o);
            if host.fade_remaining() == 0 {
                host.collect(); // caller's thread
            }
        }
        (out, EVENTS.with(|e| e.get()) - before)
    };
    let (faded, events) = run(fade);
    assert_eq!(events, 0, "allocations or frees inside process()");

    // Reference: the old version continues on its own copy of the state,
    // the new one on the host state; out = old * (1 - w) + new * w.
    let old = reference(&input, |_| 2.0);
    let new = reference(&input, |n| if n < switch { 2.0 } else { 1.0 });
    for n in 0..input.len() {
        let expected = if n < switch {
            old[n]
        } else if n < switch + fade as usize {
            let w = (n - switch + 1) as f32 / fade as f32;
            old[n] * (1.0 - w) + new[n] * w
        } else {
            new[n]
        };
        assert_eq!(faded[n].to_bits(), expected.to_bits(), "sample {n}");
    }

    // Fade measurement on the native (cdylib) path, for this signal and edit.
    let (immediate, _) = run(0);
    // Fade measurement (reported, not asserted): with this IIR the gain edit
    // makes no click, so a crossfade cannot be shown to remove one here. The
    // first difference at the swap mostly measures the signal's slope; the
    // second difference is the better click indicator.
    let d1 = |v: &[f32]| (v[switch] - v[switch - 1]).abs();
    let d2 = |v: &[f32]| (v[switch + 1] - 2.0 * v[switch] + v[switch - 1]).abs();
    let steady_d2 = immediate[64..switch]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0f32, f32::max);
    println!(
        "fade: {fade} frames; at the swap |Δy| = {:.6} (immediate {:.6}), \
         |Δ²y| = {:.6} (immediate {:.6}); max steady |Δ²y| = {:.6}",
        d1(&faded),
        d1(&immediate),
        d2(&faded),
        d2(&immediate),
        steady_d2
    );
}
