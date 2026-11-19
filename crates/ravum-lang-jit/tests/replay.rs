//! T17 (first slice): a live hot-swap session, whose swap instants depend on
//! thread timing, is reproduced bit for bit by replaying its log; 100 replays
//! give the same digest (ADR-016 proof shape).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::HotEngine;
use ravum_lang_jit::replay::{digest, replay, ReplayError, SessionLog, Swap};

const BLOCK: usize = 64;
const BLOCKS: usize = 400;

fn biquad(b0: &str, extra: &str) -> String {
    format!(
        "struct State {{\n    z1: f32,\n    z2: f32,\n}}\n\n\
fn init() -> State effects() {{\n    return State {{ z1: 0.0, z2: 0.0 }};\n}}\n\n\
fn b0() -> f32 effects() {{\n    return {b0};\n}}\n\n\
fn process_sample(state: &mut State, input: f32) -> f32 effects() {{\n    {extra}let y = b0() * input + state.z1;\n    state.z1 = 0.4 * input + 0.5 * y + state.z2;\n    state.z2 = 0.2 * input - 0.25 * y;\n    return y;\n}}\n"
    )
}

fn signal(n: usize) -> Vec<f32> {
    let mut s: u32 = 0x9e37_79b9;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s % 2000) as f32 / 1000.0 - 1.0
        })
        .collect()
}

/// Run a live session and return its log and its real output.
fn live_session() -> (SessionLog, Vec<f32>) {
    let start = compile_source(&biquad("0.1", "")).unwrap();
    let (mut engine, mut rt) = HotEngine::start(&start, "process_sample").unwrap();
    let mailbox = engine.mailbox().clone();
    let inputs = signal(BLOCK * BLOCKS);
    let rt_inputs = inputs.clone();
    let done = Arc::new(Mutex::new(false));
    let done_rt = done.clone();
    // Real-time side: preallocated buffers, adoption events recorded as
    // (first sample index, generation).
    let audio = std::thread::spawn(move || {
        let mut out = Vec::with_capacity(rt_inputs.len());
        let mut events: Vec<(u64, u64)> = Vec::with_capacity(256);
        for b in 0..BLOCKS {
            if rt.poll() {
                events.push(((b * BLOCK) as u64, mailbox.active_generation.load(SeqCst)));
            }
            for x in &rt_inputs[b * BLOCK..(b + 1) * BLOCK] {
                out.push(rt.process_sample(*x));
            }
            std::thread::sleep(Duration::from_micros(1333));
        }
        *done_rt.lock().unwrap() = true;
        (out, events)
    });
    // Editor side: 20 edits, one of which traps (rolled back by the runner).
    let mut by_gen: BTreeMap<u64, ravum_lang_ir::Module> = BTreeMap::new();
    for edit in 1..=20 {
        let src = if edit == 11 {
            biquad("0.5", "let d: i32 = 0;\n    let k: i32 = 1 / d;\n    ")
        } else {
            biquad(&format!("0.{}", (edit % 9) + 1), "")
        };
        let m = compile_source(&src).unwrap();
        let r = engine.update(&m).unwrap();
        by_gen.insert(r.generation, m);
        std::thread::sleep(Duration::from_millis(17));
        engine.collect();
        if *done.lock().unwrap() {
            break;
        }
    }
    let (out, events) = audio.join().unwrap();
    let mut log = SessionLog::new("process_sample", start);
    log.inputs = inputs;
    for (at, g) in events {
        log.swaps.push(Swap {
            at_sample: at,
            module: by_gen[&g].clone(),
        });
    }
    (log, out)
}

#[test]
fn a_live_session_is_reproduced_bit_for_bit_by_its_log() {
    let (log, live) = live_session();
    assert!(
        log.swaps.len() >= 10,
        "session too short to mean anything: {} swaps",
        log.swaps.len()
    );
    // Round trip through the serialized form an agent would store.
    let log = SessionLog::from_json(&log.to_json()).unwrap();
    let replayed = replay(&log).unwrap();
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(
        bits(&replayed),
        bits(&live),
        "replay differs from the live run"
    );

    let d0 = digest(&live);
    for _ in 0..100 {
        assert_eq!(digest(&replay(&log).unwrap()), d0);
    }
    eprintln!(
        "T17 replay: {} samples, {} swaps, 101/101 identical, digest {d0}",
        live.len(),
        log.swaps.len()
    );

    // Negative (power): the swap instants matter — moving one by a block
    // changes the output, so the equality above is not vacuous.
    let mut moved = log.clone();
    // Swap 3 is an ordinary coefficient edit (the trapping edit is rolled
    // back on its first sample, so moving it would change nothing).
    let k = 3;
    moved.swaps[k].at_sample += BLOCK as u64;
    if k + 1 < moved.swaps.len() && moved.swaps[k + 1].at_sample <= moved.swaps[k].at_sample {
        moved.swaps.remove(k + 1);
    }
    assert_ne!(digest(&replay(&moved).unwrap()), d0);
}

#[test]
fn inconsistent_or_unknown_logs_are_refused() {
    let start = compile_source(&biquad("0.1", "")).unwrap();
    let mut log = SessionLog::new("process_sample", start.clone());
    log.inputs = signal(256);
    let m = compile_source(&biquad("0.2", "")).unwrap();
    log.swaps = vec![
        Swap {
            at_sample: 100,
            module: m.clone(),
        },
        Swap {
            at_sample: 50,
            module: m.clone(),
        },
    ];
    assert!(matches!(replay(&log), Err(ReplayError::BadLog(_))));
    log.swaps = vec![Swap {
        at_sample: 10_000,
        module: m,
    }];
    assert!(matches!(replay(&log), Err(ReplayError::BadLog(_))));

    // A version refused live is refused by the replay, the same way.
    let wider = compile_source(
        &biquad("0.2", "")
            .replace("    z2: f32,\n}", "    z2: f32,\n    z3: f32,\n}")
            .replace(
                "State { z1: 0.0, z2: 0.0 }",
                "State { z1: 0.0, z2: 0.0, z3: 0.0 }",
            ),
    )
    .unwrap();
    log.swaps = vec![Swap {
        at_sample: 10,
        module: wider,
    }];
    match replay(&log) {
        Err(ReplayError::Refused { swap: 1, refusal }) => {
            assert!(refusal.contains("schema"), "{refusal}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    let json = SessionLog::new("process_sample", start)
        .to_json()
        .replace("ravum-hot-session-1", "ravum-hot-session-9");
    assert!(SessionLog::from_json(&json).is_err());
}
