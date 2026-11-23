//! T16 real-time budget: one real-time call is bounded by `RT_FUEL` (loop
//! back-edges and function entries), so a spinning or exploding edit costs
//! less than one audio block before the rollback; and the per-block
//! processing time, swaps included, is measured against its deadline.
//!
//! What this establishes (`TESTED`, this host): a **bounded cost per call**
//! (fuel) and the work of every block, in CPU time, far below its deadline.
//! What it cannot establish: that the OS runs the audio thread in time — a
//! wall-clock miss while the thread was preempted is counted and reported,
//! never hidden; it needs an RT scheduler (e.g. `SCHED_FIFO`/PREEMPT_RT),
//! which is the host's responsibility.

use std::time::{Duration, Instant};

use ravum_lang_ir::compile_source;
use ravum_lang_jit::hot::{HotEngine, RT_FUEL};

const BLOCK: usize = 64;
/// 64 frames at 48 kHz.
const DEADLINE: Duration = Duration::from_micros(1333);
/// CPU time consumed by the calling thread (Unix: `CLOCK_THREAD_CPUTIME_ID`).
#[cfg(unix)]
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out-pointer for the call's duration.
    let r = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(r, 0);
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// CPU time consumed by the calling thread (Windows). `GetThreadTimes`
/// advances by scheduler quanta (~15.6 ms), far coarser than one 1.33 ms
/// block, so a block would read 0 or 15.6 ms. `QueryThreadCycleTime` counts
/// the thread's own cycles exactly; they are converted to time with a rate
/// calibrated once against the monotonic clock while this thread spins (no
/// preemption expected over 30 ms on a busy core; a preempted calibration
/// only makes the rate lower, i.e. the measured CPU time longer: stricter).
#[cfg(windows)]
fn thread_cpu() -> Duration {
    extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn QueryThreadCycleTime(thread: *mut core::ffi::c_void, cycles: *mut u64) -> i32;
    }
    fn cycles() -> u64 {
        let mut c = 0u64;
        // SAFETY: the pseudo-handle of the current thread is always valid,
        // and `c` is a valid out-pointer for the call's duration.
        let ok = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
        assert_ne!(ok, 0, "QueryThreadCycleTime failed");
        c
    }
    static RATE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let rate = *RATE.get_or_init(|| {
        let (t0, c0) = (Instant::now(), cycles());
        while t0.elapsed() < Duration::from_millis(30) {
            std::hint::spin_loop();
        }
        let (dt, dc) = (t0.elapsed(), cycles() - c0);
        dc as f64 / dt.as_nanos() as f64
    });
    Duration::from_nanos((cycles() as f64 / rate) as u64)
}

/// Diagnostic only (no assertion depends on it): whether the host is a
/// virtual machine and its load. In a VM, time the hypervisor takes the vCPU
/// away while this thread runs is charged to the thread's CPU clock, so a
/// block can read as "working" past its deadline without doing more work.
fn host_note() -> String {
    let mut note = String::new();
    #[cfg(target_os = "macos")]
    {
        let mut v: i32 = -1;
        let mut len = std::mem::size_of::<i32>();
        // SAFETY: valid NUL-terminated name, out-pointer and length for an
        // `int` sysctl; no new value is written.
        let r = unsafe {
            libc::sysctlbyname(
                c"kern.hv_vmm_present".as_ptr(),
                (&mut v as *mut i32).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        note.push_str(&format!(
            " vm={}",
            if r == 0 { v.to_string() } else { "?".into() }
        ));
    }
    #[cfg(unix)]
    {
        let mut l = [0f64; 3];
        // SAFETY: `l` has room for the 3 averages requested.
        if unsafe { libc::getloadavg(l.as_mut_ptr(), 3) } == 3 {
            note.push_str(&format!(" load={:.2}/{:.2}/{:.2}", l[0], l[1], l[2]));
        }
    }
    note
}

const OK: &str = "fn gain(x: f32) -> f32 effects() {\n    return x * 0.5;\n}\n";

fn worst_sample(bad: &str) -> (Duration, f32) {
    let (mut e, mut rt) = HotEngine::start(&compile_source(OK).unwrap(), "gain").unwrap();
    assert_eq!(rt.fuel_budget(), RT_FUEL);
    e.update(&compile_source(bad).unwrap()).unwrap();
    rt.poll();
    // CPU time of the thread: the call's own cost, whatever the scheduler.
    let c = thread_cpu();
    let y = rt.process_f32(1.0);
    (thread_cpu() - c, y)
}

#[test]
fn a_spinning_edit_costs_less_than_one_block_then_rolls_back() {
    let spin = "fn gain(x: f32) -> f32 effects() {\n    let mut i: i64 = 0;\n    while i >= 0 {\n        i = i + 0;\n    }\n    return x;\n}\n";
    let (t, y) = worst_sample(spin);
    eprintln!("spin: {t:?}");
    assert_eq!(y, 0.5, "rolled back to the previous version");
    assert!(t < DEADLINE, "one call took {t:?}");
}

#[test]
fn an_exploding_recursion_is_bounded_by_the_fuel_not_only_the_depth() {
    // 2^depth calls without a single loop: bounded only since function
    // entries burn fuel.
    let bomb = "fn f(n: i64) -> i64 effects() {\n    if n > 1000000 {\n        return 0;\n    }\n    return f(n + 1) + f(n + 1);\n}\n\nfn gain(x: f32) -> f32 effects() {\n    let k = f(0);\n    return x + (k as f32);\n}\n";
    let (t, y) = worst_sample(bomb);
    eprintln!("bomb: {t:?}");
    assert_eq!(y, 0.5);
    assert!(t < DEADLINE, "one call took {t:?}");
}

#[test]
fn legitimate_dsp_loops_fit_the_budget() {
    // A 256-iteration inner loop per sample: far below RT_FUEL, no trap.
    let src = "fn gain(x: f32) -> f32 effects() {\n    let mut acc: f32 = 0.0;\n    let mut i: i64 = 0;\n    while i < 256 {\n        acc = acc + x * 0.001;\n        i = i + 1;\n    }\n    return acc;\n}\n";
    let (_e, mut rt) = HotEngine::start(&compile_source(src).unwrap(), "gain").unwrap();
    let y = rt.process_f32(1.0);
    assert!((y - 0.256).abs() < 1e-4, "{y}");
}

#[test]
fn block_processing_meets_its_deadline_across_hot_swaps() {
    let state = "struct State {\n    z: f32,\n}\n\nfn process_sample(state: &mut State, input: f32) -> f32 effects() {\n    let mut acc: f32 = 0.0;\n    let mut i: i64 = 0;\n    while i < 32 {\n        acc = acc + input * K;\n        i = i + 1;\n    }\n    state.z = state.z * 0.5 + acc;\n    return state.z;\n}\n";
    let v = |k: u32| compile_source(&state.replace("K", &format!("0.0{k}"))).unwrap();
    let (mut e, mut rt) = HotEngine::start(&v(1), "process_sample").unwrap();
    const BLOCKS: usize = 20_000;
    let mut times = Vec::with_capacity(BLOCKS);
    let mut cpu_times = Vec::with_capacity(BLOCKS);
    let mut preempted = 0usize;
    let input = [0.25f32; BLOCK];
    let mut out = [0.0f32; BLOCK];
    for b in 0..BLOCKS {
        if b % 400 == 399 {
            // Editor side, outside the measured region.
            e.update(&v(1 + (b as u32 / 400) % 9)).unwrap();
            e.collect();
        }
        let t = Instant::now();
        let c = thread_cpu();
        rt.poll();
        for (o, &x) in out.iter_mut().zip(&input) {
            *o = rt.process_sample(x);
        }
        let cpu = thread_cpu() - c;
        let d = t.elapsed();
        if d > DEADLINE {
            // Wall time over the deadline: either our work, or the OS ran
            // something else (preemption: CPU time stays low).
            eprintln!("wall miss at block {b}: wall {d:?}, cpu {cpu:?}");
            if cpu < DEADLINE {
                preempted += 1;
            }
        }
        times.push(d);
        cpu_times.push(cpu);
    }
    std::hint::black_box(out);
    times.sort();
    cpu_times.sort();
    let q = |v: &Vec<Duration>, p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let wall_misses = times.iter().filter(|t| **t > DEADLINE).count();
    let cpu_misses = cpu_times.iter().filter(|t| **t > DEADLINE).count();
    eprintln!(
        "T16 deadline: blocks={BLOCKS} swaps={} cpu p50={:?} p99.9={:?} max={:?} | wall p50={:?} p99.9={:?} max={:?} | deadline={DEADLINE:?} cpu_misses={cpu_misses} wall_misses={wall_misses} (preempted {preempted}) |{}",
        BLOCKS / 400,
        q(&cpu_times, 0.5),
        q(&cpu_times, 0.999),
        cpu_times.last().unwrap(),
        q(&times, 0.5),
        q(&times, 0.999),
        times.last().unwrap(),
        host_note()
    );
    // The work itself: no block, swaps included, needs more CPU than its
    // deadline (with a 10x margin at p99.9).
    assert_eq!(cpu_misses, 0, "a block's own work exceeded its deadline");
    assert!(
        q(&cpu_times, 0.999) < DEADLINE / 10,
        "cpu p99.9 {:?}",
        q(&cpu_times, 0.999)
    );
    // A wall-clock miss is only acceptable when the thread was preempted
    // (non-RT scheduler): never when it was working.
    assert_eq!(wall_misses, preempted, "a wall miss without preemption");
}

/// Stack margins. The reference interpreter recurses once per IR call: at
/// its depth limit it must fit a 1 MiB thread (half the Rust default), so a
/// grown `exec` frame is caught here rather than as a stack overflow abort
/// in another test. Compiled code with large frames recursing deeply must
/// trap (stack guard), never overflow the native stack.
#[test]
fn deep_recursion_fits_the_stack_or_traps() {
    let deep = "fn d(n: i64) -> i64 effects() {\n    if n <= 0 {\n        return 0;\n    }\n    let a: [i64; 8] = [n, n, n, n, n, n, n, n];\n    return d(n - 1) + a[3] - n + 1;\n}\n\nfn check() -> i64 effects() {\n    return d(600);\n}\n";
    let ir = compile_source(deep).unwrap();
    let ir2 = ir.clone();
    let interp = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            ravum_lang_ir::interp::run_with_output(
                &ir2,
                "check",
                ravum_lang_ir::interp::DEFAULT_STEPS,
            )
            .0
            .render()
        })
        .unwrap()
        .join()
        .expect("the interpreter overflowed a 1 MiB stack at its depth limit");
    // 600 > MAX_DEPTH (512): a fault, reported, not a crash.
    assert!(interp.starts_with("fault"), "{interp}");

    // Large frames: 32 KiB of locals per call, 500 calls would need 16 MiB.
    let big = "fn b(n: i64) -> i64 effects() {\n    if n <= 0 {\n        return 0;\n    }\n    let a: [i64; 4096] = [n; 4096];\n    return b(n - 1) + a[100] - n;\n}\n\nfn check() -> i64 effects() {\n    return b(500);\n}\n";
    let ir = compile_source(big).unwrap();
    let native = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            ravum_lang_jit::compile(&ir)
                .unwrap()
                .run("check", ravum_lang_jit::DEFAULT_FUEL)
                .render()
        })
        .unwrap()
        .join()
        .expect("compiled code overflowed the native stack");
    assert!(
        native.starts_with("fault"),
        "the stack guard trapped: {native}"
    );
}

/// A host with a small callback stack sets a smaller budget: compiled code
/// that would need more traps (and rolls back) on a 512 KiB thread instead
/// of overflowing it.
#[test]
fn a_small_stack_budget_protects_a_small_callback_thread() {
    let ok = "fn gain(x: f32) -> f32 effects() {\n    return x * 0.5;\n}\n";
    let deep = "fn b(n: i64) -> i64 effects() {\n    if n <= 0 {\n        return 0;\n    }\n    let a: [i64; 1024] = [n; 1024];\n    return b(n - 1) + a[10] - n;\n}\n\nfn gain(x: f32) -> f32 effects() {\n    return x + (b(400) as f32);\n}\n";
    let (mut e, mut rt) = HotEngine::start(&compile_source(ok).unwrap(), "gain").unwrap();
    rt.set_stack_budget(256 << 10);
    e.update(&compile_source(deep).unwrap()).unwrap();
    // 400 frames of 8 KiB = 3.2 MiB: far beyond a 512 KiB thread.
    let y = std::thread::Builder::new()
        .stack_size(512 << 10)
        .spawn(move || {
            rt.poll();
            rt.process_f32(1.0)
        })
        .unwrap()
        .join()
        .expect("overflowed a 512 KiB callback thread");
    assert_eq!(y, 0.5, "trapped and rolled back");
}
