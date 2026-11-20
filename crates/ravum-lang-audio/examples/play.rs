//! Manual device check. Listen for a pitch change halfway through the run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use ravum_lang_audio::{device::CpalHost, Processor};
use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::{HotEngine, RtRunner};

const TONE_DURATION: Duration = Duration::from_secs(2);

fn oscillator(phase_step: &str) -> String {
    format!(
        r#"
struct State {{
    phase: f32,
}}

fn process_sample(state: &mut State, input: f32) -> f32 effects() {{
    let phase = state.phase;
    state.phase = phase + {phase_step};
    if state.phase >= 1.0 {{
        state.phase = state.phase - 1.0;
    }}
    if phase < 0.5 {{
        return phase * 0.4 - 0.1;
    }}
    return 0.3 - phase * 0.4;
}}
"#
    )
}

thread_local! {
    static ALLOCATIONS: Cell<Option<u64>> = const { Cell::new(None) };
}

struct CountingAllocator;

// WHY: Rust's global allocator is the interception point for the processor's
// heap calls. The thread-local counter is armed only during DSP processing.
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// SAFETY: All requests are forwarded to System with the original arguments.
// Recording an allocation uses const-initialized thread-local storage.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get().map(|n| n.wrapping_add(1))));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[derive(Default)]
struct Playback {
    generation: AtomicU64,
    blocks: AtomicU64,
    allocations: AtomicU64,
}

struct MeasuredRunner {
    runner: RtRunner,
    playback: Arc<Playback>,
}

impl Processor for MeasuredRunner {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        ALLOCATIONS.with(|count| count.set(Some(0)));
        self.runner.process(input, output);
        let allocations = ALLOCATIONS.with(|count| count.take().unwrap_or_default());

        self.playback.allocations.fetch_add(allocations, Relaxed);
        self.playback.blocks.fetch_add(1, Relaxed);
        self.playback
            .generation
            .store(self.runner.generation(), Relaxed);
    }
}

fn main() -> Result<(), String> {
    // Phase advances by a fraction of a cycle per sample. Doubling the step
    // raises the pitch by one octave at any device sample rate.
    let initial = compile_source(&oscillator("0.01")).map_err(|err| format!("{err:?}"))?;
    let updated = oscillator("0.02");
    let (mut engine, mut runner) =
        HotEngine::start(&initial, "process_sample").map_err(|err| format!("{err:?}"))?;
    // The callback thread belongs to the audio backend: conservative budget.
    runner.set_stack_budget(CpalHost::CALLBACK_STACK_BUDGET);
    let initial_generation = runner.generation();
    let playback = Arc::new(Playback::default());
    let host = CpalHost::start(Box::new(MeasuredRunner {
        runner,
        playback: Arc::clone(&playback),
    }))?;

    println!(
        "sample_rate={} Hz, channels={}, format=f32",
        host.sample_rate, host.channels
    );
    sleep(TONE_DURATION);
    let first_blocks = playback.blocks.load(Relaxed);
    let first_generation = playback.generation.load(Relaxed);
    println!("Initial generation={initial_generation}, observed={first_generation}, blocks={first_blocks}");

    let edited = compile_source(&updated).map_err(|err| format!("{err:?}"))?;
    let update = engine.update(&edited).map_err(|err| format!("{err:?}"))?;
    sleep(TONE_DURATION);
    drop(host);

    let blocks = playback.blocks.load(Relaxed);
    let generation = playback.generation.load(Relaxed);
    let allocations = playback.allocations.load(Relaxed);
    println!("Published generation={}, observed={generation}, blocks={blocks}, Rust allocations in Processor::process={allocations}", update.generation);
    if first_blocks == 0
        || first_generation != initial_generation
        || generation != update.generation
    {
        return Err("audio processing did not observe both generations".to_string());
    }
    if allocations != 0 {
        return Err("audio processor allocated".to_string());
    }
    Ok(())
}
