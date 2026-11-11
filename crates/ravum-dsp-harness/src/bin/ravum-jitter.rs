//! OS scheduling-jitter probe (MVP-0b / L4-C).
//!
//! # What this measures, exactly
//!
//! A periodic thread with a fixed deadline (default 5 333 µs = 256 frames at 48 kHz) is
//! woken with `timeBeginPeriod(1)` set. For each period the probe records:
//!
//! * `lateness_us` — how late the wake-up was relative to a **fixed deadline grid**
//!   (`start + i*period`), not a drifting sleep loop;
//! * `interval_us` — the distance between two consecutive wake-ups, which is what an
//!   audio host sees as the callback period.
//!
//! This characterises the **operating system scheduler** under the current load.
//!
//! It does **not** measure an audio device: there is no driver, no buffer, no XRUN, no
//! WASAPI/ASIO callback, no real-time priority. A low lateness here does not license a
//! hard-RT claim; a high lateness is a concrete reason **not** to make one.
//!
//! Output: JSON on stdout.

use serde_json::json;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}

fn arg_u64(args: &[String], name: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn stats(mut values: Vec<u64>) -> serde_json::Value {
    values.sort_unstable();
    json!({
        "count": values.len(),
        "min": values.first().copied().unwrap_or(0),
        "p50": percentile(&values, 50.0),
        "p95": percentile(&values, 95.0),
        "p99": percentile(&values, 99.0),
        "max": values.last().copied().unwrap_or(0),
    })
}

struct TimerGuard {
    active: bool,
    period_ms: u32,
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.active {
            // SAFETY: matches the timeBeginPeriod call below.
            unsafe {
                timeEndPeriod(self.period_ms);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iterations = arg_u64(&args, "--iterations", 2000).max(2);
    let period_us = arg_u64(&args, "--period-us", 5_333).max(100);
    let timer_period_ms = arg_u64(&args, "--timer-period-ms", 1).clamp(1, 16) as u32;

    #[cfg(windows)]
    let timer = {
        // SAFETY: documented Win32 multimedia timer call, released by TimerGuard.
        let rc = unsafe { timeBeginPeriod(timer_period_ms) };
        TimerGuard {
            active: rc == 0,
            period_ms: timer_period_ms,
        }
    };
    #[cfg(not(windows))]
    let timer = TimerGuard {
        active: false,
        period_ms: timer_period_ms,
    };

    // Warm up the scheduler path before measuring.
    std::thread::sleep(Duration::from_millis(5));

    let period = Duration::from_micros(period_us);
    let start = Instant::now();
    let mut lateness_us: Vec<u64> = Vec::with_capacity(iterations as usize);
    let mut intervals_us: Vec<u64> = Vec::with_capacity(iterations as usize);
    let mut previous_wake: Option<Instant> = None;
    let mut deadline = start + period;

    for _ in 0..iterations {
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
        let woke = Instant::now();
        lateness_us.push(woke.saturating_duration_since(deadline).as_micros() as u64);
        if let Some(previous) = previous_wake {
            intervals_us.push(woke.saturating_duration_since(previous).as_micros() as u64);
        }
        previous_wake = Some(woke);
        deadline += period;
    }
    let elapsed = start.elapsed();

    let missed = lateness_us.iter().filter(|late| **late > period_us).count() as u64;
    let payload = json!({
        "schema_version": 1,
        "kind": "os-scheduling-jitter",
        "tool": "ravum-jitter 0.1.0",
        "iterations": iterations,
        "period_us": period_us,
        "timer_period_ms": timer_period_ms,
        "lateness_us": stats(lateness_us),
        "interval_us": stats(intervals_us),
        "missed_deadlines_gt_one_period": missed,
        "wall_ms": elapsed.as_millis() as u64,
        "cpu_load_percent_at_start": std::env::var("RAVUM_JITTER_CPU_PERCENT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok()),
        "memory_available_bytes_at_start": std::env::var("RAVUM_JITTER_FREE_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok()),
        "limits": [
            "operating-system scheduler only",
            "no audio device, driver, buffer, XRUN or WASAPI/ASIO callback",
            "no real-time thread priority was requested",
            "this is not a hard real-time qualification"
        ],
    });
    let _ = timer;
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );
}
