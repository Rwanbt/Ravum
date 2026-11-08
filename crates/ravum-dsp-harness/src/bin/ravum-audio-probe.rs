//! Windows audio-device probe (MVP-0b / L4-C).
//!
//! # What this measures, exactly
//!
//! The probe opens a real `waveOut` (winmm) device in shared PCM-16/stereo mode, keeps
//! `--buffers` buffers of `--frames` frames in rotation, and calls the real block DSP
//! path ([`ravum_patch_host::BlockHost::process_rt`]) from a dedicated render thread
//! each time a buffer completes (`WOM_DONE`).
//!
//! **The device always receives digital silence.** The DSP signal (a mono 440 Hz sine,
//! amplitude 0.5, fed block after block) is computed, measured, and then dropped: the
//! buffer submitted to the driver is zeroed right before `waveOutWrite`. Nothing is
//! audible; the hardware is only used as a real driver/scheduler clock.
//!
//! Per buffer the probe records, on the render thread:
//!
//! * `work_us` — duration of the DSP block call alone;
//! * `callback_total_us` — wake-up to re-submission (DSP + zeroing + `waveOutWrite`);
//! * `wake_lateness_us` — per wake with at least one completion: wake instant minus
//!   `anchor + k*period`, where `anchor` is the first measured completion after warm-up
//!   and `k` is the index of the oldest buffer in that wake's batch. It excludes the
//!   device start-up and first-event delivery bias; the start-up cost is reported
//!   separately as `first_completion_offset_us`. `waveOut` delivers `WOM_DONE` in
//!   batches, so this figure also contains the driver's event-delivery quantisation,
//!   not only OS scheduler jitter;
//! * `interval_us` — distance between two consecutive wakes that had a completion;
//! * `queue_margin_us` — derived: `(buffers-1)*period - lateness - callback_total`,
//!   the time left before the queue could drain. Negative values are **queue-drain
//!   risks**, not measured XRUNs;
//! * position drift from `waveOutGetPosition` (when the driver supports it);
//! * allocations/deallocations performed on this thread, counted by the counting global
//!   allocator of `ravum-dsp-harness`, after a warm-up of `2*buffers` completions.
//!
//! # Thread priority: MMCSS and raw Win32 priority
//!
//! With `--mmcss`, the **render thread** registers itself with the Windows Multimedia
//! Class Scheduler Service as `Pro Audio`
//! (`AvSetMmThreadCharacteristicsW(L"Pro Audio", ...)`) and asks for
//! `AVRT_PRIORITY_CRITICAL` (`AvSetMmThreadPriority`). It reverts with
//! `AvRevertMmThreadCharacteristics` before the thread ends. `--thread-priority
//! <normal|above|high|time-critical>` additionally calls `SetThreadPriority` on the same
//! thread; the applied order is MMCSS first, raw Win32 priority second, and both the
//! requested values and the base priorities observed with `GetThreadPriority` are
//! reported. A requested priority that cannot be applied is an **explicit failure**
//! (Win32/AVRT error code, exit 1), never a silent success.
//!
//! Reporting the effect is the point: **MMCSS `Pro Audio` + `AVRT_PRIORITY_CRITICAL` is
//! not a hard real-time guarantee.** It asks the scheduler for preferential treatment; it
//! does not promise a deadline, does not make `waveOut` a certified driver, and does not
//! turn the callback into a bounded-latency real-time path.
//!
//! # What this does **not** measure
//!
//! * `waveOut` is a legacy emulated mapper API, not a certified real-time driver path
//!   (no WASAPI exclusive, no ASIO). MMCSS changes the **scheduling class** of the
//!   probe's render thread; it does not change the driver or the API. This is **not** a
//!   hard real-time qualification.
//! * XRUNs are driver events that `waveOut` does not expose; the report talks about
//!   "buffers submitted late" and "queue-drain risk", never about measured XRUNs.
//! * One machine, one device, one format, a few seconds. Results vary with load.
//! * The allocation counters instrument Rust's global allocator per thread: a positive
//!   count is proof of a violation, a zero is not proof of hard-RT behaviour.
//!
//! The probe also samples the machine load around the run (`GetSystemTimes` delta and
//! `GlobalMemoryStatusEx`), so every JSON report states the measured CPU busy percentage
//! and available RAM instead of assuming a quiet machine.
//!
//! The DSP module is **prebuilt**: this binary takes `--module <path>` (a cdylib built
//! by `ravum build`) and hot-swap candidates as `--swap-build <json>` files (the JSON
//! emitted by `ravum build ... --json`). Building the module in-process would drag the
//! compiler adapter into the probe; `scripts/audio-probe.ps1` does the build step and
//! passes the paths, so the probe has no build machinery in its measurement process.

#![recursion_limit = "256"]

use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ravum_dsp_harness::{
    allocations_on_this_thread, deallocations_on_this_thread, reset_thread_counters,
};
use ravum_patch_host::{BlockHost, ExpectedAbi};
use ravum_protocol::{BuildOutput, DspState};

const TOOL: &str = "ravum-audio-probe";
const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Amplitude of the generated sine. The signal is never audible: it is computed and
/// discarded (only digital silence is submitted to the driver).
const SINE_AMPLITUDE: f32 = 0.5;
const SINE_FREQUENCY_HZ: f32 = 440.0;

const MIN_SECONDS: u64 = 1;
const MAX_SECONDS: u64 = 10;

/// `WaitForSingleObject` timeout once the first buffer has completed. Wakes up the
/// render thread often enough to observe a stop request, even if no buffer completes.
const WAIT_TIMEOUT_MS: u32 = 20;
/// Longer timeout for the very first completion: opening a device takes longer than one
/// period and a short timeout there would only invent "missed events".
const FIRST_WAIT_TIMEOUT_MS: u32 = 250;

// ---------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------

/// Raw Win32 thread priority requested with `--thread-priority`. Kept apart from MMCSS
/// on purpose: the caller can measure the scheduler effect of one, the other, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadPriorityChoice {
    Normal,
    AboveNormal,
    Highest,
    TimeCritical,
}

impl ThreadPriorityChoice {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "above" | "above-normal" => Some(Self::AboveNormal),
            "high" | "highest" => Some(Self::Highest),
            "time-critical" | "time_critical" => Some(Self::TimeCritical),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::AboveNormal => "above-normal",
            Self::Highest => "high",
            Self::TimeCritical => "time-critical",
        }
    }

    /// `SetThreadPriority` value (`THREAD_PRIORITY_*`).
    fn win32_value(&self) -> i32 {
        match self {
            Self::Normal => 0,         // THREAD_PRIORITY_NORMAL
            Self::AboveNormal => 1,    // THREAD_PRIORITY_ABOVE_NORMAL
            Self::Highest => 2,        // THREAD_PRIORITY_HIGHEST
            Self::TimeCritical => 15,  // THREAD_PRIORITY_TIME_CRITICAL
        }
    }

    fn win32_name(&self) -> &'static str {
        match self {
            Self::Normal => "THREAD_PRIORITY_NORMAL",
            Self::AboveNormal => "THREAD_PRIORITY_ABOVE_NORMAL",
            Self::Highest => "THREAD_PRIORITY_HIGHEST",
            Self::TimeCritical => "THREAD_PRIORITY_TIME_CRITICAL",
        }
    }
}

#[derive(Debug, Clone)]
struct Config {
    seconds: u64,
    buffers: usize,
    frames: usize,
    rate: u32,
    module: Option<String>,
    swap_builds: Vec<String>,
    swap_every_ms: u64,
    crossfade_frames: u32,
    device: Option<u32>,
    /// Request Windows MMCSS registration (`Pro Audio` + `AVRT_PRIORITY_CRITICAL`) on
    /// the render thread. Off by default: the baseline must be a normal thread.
    mmcss: bool,
    /// Raw Win32 thread priority for the render thread, applied after MMCSS.
    thread_priority: Option<ThreadPriorityChoice>,
    json: bool,
    out: Option<String>,
}

impl Config {
    fn period_us(&self) -> u64 {
        (self.frames as u64 * 1_000_000) / self.rate.max(1) as u64
    }

    fn parse(args: &[String]) -> Result<Self, String> {
        let mut config = Config {
            seconds: 3,
            buffers: 4,
            frames: 256,
            rate: 48_000,
            module: None,
            swap_builds: Vec::new(),
            swap_every_ms: 0,
            crossfade_frames: 0,
            device: None,
            mmcss: false,
            thread_priority: None,
            json: false,
            out: None,
        };
        let mut index = 0usize;
        while index < args.len() {
            let flag = args[index].as_str();
            match flag {
                "--seconds" => {
                    config.seconds = take_u64(args, &mut index, flag)?;
                }
                "--buffers" => {
                    config.buffers = take_u64(args, &mut index, flag)? as usize;
                }
                "--frames" => {
                    config.frames = take_u64(args, &mut index, flag)? as usize;
                }
                "--rate" => {
                    config.rate = take_u64(args, &mut index, flag)? as u32;
                }
                "--module" => {
                    config.module = Some(take_string(args, &mut index, flag)?);
                }
                "--swap-build" => {
                    config.swap_builds.push(take_string(args, &mut index, flag)?);
                }
                "--swap-every-ms" => {
                    config.swap_every_ms = take_u64(args, &mut index, flag)?;
                }
                "--crossfade-frames" => {
                    config.crossfade_frames = take_u64(args, &mut index, flag)? as u32;
                }
                "--device" => {
                    config.device = Some(take_u64(args, &mut index, flag)? as u32);
                }
                "--mmcss" => {
                    config.mmcss = true;
                    index += 1;
                }
                "--thread-priority" => {
                    let value = take_string(args, &mut index, flag)?;
                    config.thread_priority = Some(ThreadPriorityChoice::parse(&value).ok_or_else(
                        || {
                            format!(
                                "--thread-priority expects normal|above|high|time-critical, got {value:?}"
                            )
                        },
                    )?);
                }
                "--out" => {
                    config.out = Some(take_string(args, &mut index, flag)?);
                }
                "--json" => {
                    config.json = true;
                    index += 1;
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        if config.seconds < MIN_SECONDS {
            config.seconds = MIN_SECONDS;
        }
        if config.seconds > MAX_SECONDS {
            config.seconds = MAX_SECONDS;
        }
        if !(1..=16).contains(&config.buffers) {
            return Err(format!(
                "--buffers must be in 1..=16 (got {})",
                config.buffers
            ));
        }
        if !(32..=4096).contains(&config.frames) {
            return Err(format!(
                "--frames must be in 32..=4096 (got {})",
                config.frames
            ));
        }
        if !(8_000..=192_000).contains(&config.rate) {
            return Err(format!(
                "--rate must be in 8000..=192000 (got {})",
                config.rate
            ));
        }
        if config.swap_every_ms > 0 && config.swap_every_ms < 20 {
            return Err("--swap-every-ms must be 0 (disabled) or >= 20".to_owned());
        }
        if !config.swap_builds.is_empty() && config.swap_every_ms == 0 {
            return Err("--swap-build requires --swap-every-ms > 0".to_owned());
        }
        if !config.swap_builds.is_empty() && config.module.is_none() {
            return Err("--swap-build requires --module (the version to swap away from)".to_owned());
        }
        if config.crossfade_frames > 0 && config.module.is_none() {
            return Err("--crossfade-frames requires --module".to_owned());
        }
        if config.device == Some(u32::MAX) {
            // Explicitly asking for WAVE_MAPPER is valid; normalise to the default.
            config.device = None;
        }
        Ok(config)
    }
}

fn take_string(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let value = args
        .get(*index + 1)
        .ok_or_else(|| format!("{flag} expects a value"))?
        .clone();
    *index += 2;
    Ok(value)
}

fn take_u64(args: &[String], index: &mut usize, flag: &str) -> Result<u64, String> {
    let raw = take_string(args, index, flag)?;
    raw.parse::<u64>()
        .map_err(|_| format!("{flag} expects a non-negative integer, got {raw:?}"))
}

fn usage() -> String {
    format!(
        "{TOOL} {TOOL_VERSION} — Windows waveOut DSP probe (MVP-0b / L4-C)\n\
\n\
Usage: {TOOL} [OPTIONS]\n\
\n\
  --seconds N           probe duration in seconds, 1..=10 (default 3)\n\
  --buffers N           buffers kept in rotation, 1..=16 (default 4)\n\
  --frames N            frames per buffer, 32..=4096 (default 256)\n\
  --rate N              sample rate in Hz, 8000..=192000 (default 48000)\n\
  --module PATH         block module (.dll) built by `ravum build`. Without it the\n\
                        DSP path is an identity copy (no module exercised)\n\
  --swap-build PATH     JSON emitted by `ravum build ... --json`; repeatable. Each\n\
                        candidate is prepared + verified + hot-swapped while playing\n\
  --swap-every-ms N     delay between hot swaps, 0 disables swaps (default 0)\n\
  --crossfade-frames N  crossfade length armed for swaps (default 0 = immediate)\n\
  --device N            waveOut device index (default: WAVE_MAPPER, i.e. the OS default)\n\
  --mmcss               register the render thread with Windows MMCSS as \"Pro Audio\" and\n\
                        ask for AVRT_PRIORITY_CRITICAL (default off; explicit failure if the\n\
                        registration or the priority cannot be set)\n\
  --thread-priority P   also call SetThreadPriority on the render thread:\n\
                        normal | above | high | time-critical. Applied AFTER MMCSS when both\n\
                        are requested, and the observed base priorities are reported\n\
  --json                print the full JSON report on stdout\n\
  --out PATH            also write the JSON report to PATH\n\
  --help                show this help\n\
\n\
The audio device is only used as a clock: it always receives digital silence. The DSP\n\
output is computed, measured (work, lateness, sample-to-sample delta), and discarded.\n"
    )
}

// ---------------------------------------------------------------------------------------
// Statistics helpers (shared by every platform)
// ---------------------------------------------------------------------------------------

fn percentile_u64(sorted: &[u64], percentile: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn percentile_i64(sorted: &[i64], percentile: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Empty samples are reported as `null`, never as a measured `0`.
fn stats_u64(values: &[u64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0, "min": null, "p50": null, "p95": null, "p99": null, "max": null});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "p50": percentile_u64(&sorted, 50.0),
        "p95": percentile_u64(&sorted, 95.0),
        "p99": percentile_u64(&sorted, 99.0),
        "max": sorted[sorted.len() - 1],
    })
}

fn stats_i64(values: &[i64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0, "min": null, "p50": null, "p95": null, "p99": null, "max": null});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "p50": percentile_i64(&sorted, 50.0),
        "p95": percentile_i64(&sorted, 95.0),
        "p99": percentile_i64(&sorted, 99.0),
        "max": sorted[sorted.len() - 1],
    })
}

fn config_json(config: &Config) -> Value {
    json!({
        "seconds": config.seconds,
        "buffers": config.buffers,
        "frames": config.frames,
        "rate": config.rate,
        "period_us": config.period_us(),
        "requested_device": match config.device {
            Some(index) => json!({"kind": "index", "index": index}),
            None => json!({"kind": "wave_mapper"}),
        },
        "module": config.module,
        "swap_builds": config.swap_builds,
        "swap_every_ms": config.swap_every_ms,
        "crossfade_frames": config.crossfade_frames,
        "mmcss_requested": config.mmcss,
        "thread_priority_requested": config
            .thread_priority
            .map(|priority| priority.as_str()),
    })
}

fn limits_json(config: &Config) -> Value {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    json!([
        "waveOut (winmm) is a legacy emulated mapper API, not a certified real-time audio path: no WASAPI exclusive mode, no ASIO",
        "MMCSS registration (Pro Audio) and AVRT_PRIORITY_CRITICAL change the scheduling class the OS grants this thread; they are not a deadline guarantee and not a hard real-time qualification",
        "MMCSS changes the probe's render thread, never the waveOut driver path or the waveOut API semantics",
        "not a hard real-time qualification; the numbers describe this machine under this load for a few seconds, not a guarantee",
        format!("the probe binary runs in the {profile} Cargo profile; host-side overhead of process_rt reflects that profile"),
        "the device always receives digital silence; the DSP signal is computed, measured and discarded, so nothing is played audibly",
        "XRUNs are driver events that waveOut does not expose; the report gives buffers submitted late and a derived queue-drain risk, never measured XRUNs",
        "wake lateness is measured per wake, anchored on the first measured completion after warm-up and on the oldest completion of each wake's batch; it excludes the device start-up bias (reported as first_completion_offset_us)",
        "waveOut delivers WOM_DONE in batches at the audio engine's service cadence (observed: consecutive wakes ~10 ms apart, sometimes two buffers per wake); wake lateness therefore contains driver event-delivery quantisation, not only OS scheduler jitter",
        "a buffer shorter than the driver's service quantum can make the waveOut path consume the stream slower than real time; the report exposes delivered-vs-expected completion cadence for that reason",
        "queue_margin_us is derived from the anchored lateness and the callback duration; it is a risk indicator, not an observation of the driver's internal buffering",
        "allocation counters instrument Rust's global allocator per thread; a positive count proves a violation, a zero count does not prove hard real-time behaviour",
        "the measured CPU busy percentage comes from GetSystemTimes over the probe window and includes every process on the machine, not only this probe",
        "the controlled load used for some runs is generated by separate PowerShell busy-loop processes and is reported in load.probe_load_note",
        "one machine, one device, one format, one short window: results are not portable and were not repeated over minutes",
        if config.module.is_none() {
            "no DSP module was given: the measured work is an identity copy, not the real block ABI path"
        } else {
            "the DSP module is prebuilt by `ravum build` and loaded by the host; the probe does not compile anything"
        },
    ])
}

fn os_info() -> Value {
    #[cfg(windows)]
    let windows_version = crate::win::windows_version_string();
    #[cfg(not(windows))]
    let windows_version: Option<String> = None;
    json!({
        "name": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "windows_version": windows_version,
        "processor_identifier": std::env::var("PROCESSOR_IDENTIFIER").ok(),
        "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "process_id": std::process::id(),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
    })
}

// ---------------------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{}", usage());
        return;
    }
    let config = match Config::parse(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{TOOL}: {error}");
            eprint!("{}", usage());
            std::process::exit(2);
        }
    };
    let (report, mut exit_code) = execute(&config);
    let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_owned());
    if let Some(path) = &config.out {
        if let Err(error) = std::fs::write(path, format!("{text}\n")) {
            eprintln!("{TOOL}: cannot write report to {path}: {error}");
            if exit_code == 0 {
                exit_code = 1;
            }
        }
    }
    if config.json {
        println!("{text}");
    } else {
        print_human(&report);
    }
    std::process::exit(exit_code);
}

fn execute(config: &Config) -> (Value, i32) {
    let os = os_info();
    #[cfg(windows)]
    {
        crate::win::execute(config, os)
    }
    #[cfg(not(windows))]
    {
        let report = json!({
            "schema_version": 1,
            "kind": "audio-device-probe",
            "tool": TOOL,
            "tool_version": TOOL_VERSION,
            "status": "unsupported",
            "os": os,
            "config": config_json(config),
            "realtime": {
                "mmcss_requested": config.mmcss,
                "mmcss_acquired": false,
                "thread_priority_requested": config
                    .thread_priority
                    .map(|priority| priority.as_str()),
                "thread_priority_set": false,
                "notes": ["MMCSS (avrt.dll) is Windows-only; this platform reports Unsupported"],
            },
            "error": {
                "stage": "platform",
                "message": "the waveOut audio probe is Windows-only; this platform reports Unsupported instead of inventing a measurement",
            },
            "limits": limits_json(config),
        });
        (report, 3)
    }
}

fn print_human(report: &Value) {
    let status = report
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("{TOOL} {TOOL_VERSION} — statut: {status}");
    if let Some(error) = report.get("error") {
        println!("  erreur       {}", compact_error(error));
        return;
    }
    if status == "unsupported" {
        println!("  plateforme   non Windows: aucune mesure produite");
        return;
    }
    let device = report
        .get("device")
        .and_then(Value::as_object)
        .and_then(|device| device.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let device_id = report
        .get("device")
        .and_then(|device| device.get("resolved_device_id"))
        .cloned()
        .unwrap_or(Value::Null);
    println!("  périphérique {device} (waveOut id {device_id})");
    if let Some(format) = report.get("format") {
        println!(
            "  format       PCM 16 bits, {} canaux, {} Hz, {} frames/tampon ({} tampons)",
            format
                .get("channels")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            format
                .get("sample_rate")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            format
                .get("frames_per_buffer")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            format
                .get("buffers_in_rotation")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
    }
    if let Some(realtime) = report.get("realtime") {
        let requested = realtime
            .get("mmcss_requested")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let acquired = realtime
            .get("mmcss_acquired")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let raw_priority = realtime
            .get("thread_priority_requested")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let observed = realtime
            .get("thread_priority_observed")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_owned());
        println!(
            "  priorité     MMCSS demandé={requested} acquis={acquired} · SetThreadPriority={raw_priority} · priorité de base observée={observed}"
        );
        if let Some(notes) = realtime.get("notes").and_then(Value::as_array) {
            for note in notes {
                if let Some(text) = note.as_str() {
                    println!("               note: {text}");
                }
            }
        }
    }
    if let Some(load) = report.get("load") {
        let cpu = load
            .get("cpu_busy_percent_measured")
            .and_then(Value::as_f64)
            .map(|value| format!("{:.1}", (value * 10.0).round() / 10.0))
            .unwrap_or_else(|| "n/a".to_owned());
        let window = load
            .get("cpu_sample_window_ms")
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_owned());
        let ram_free = load
            .get("ram_available_bytes_start")
            .and_then(Value::as_u64)
            .map(|value| (value / (1024 * 1024)).to_string())
            .unwrap_or_else(|| "n/a".to_owned());
        let ram_total = load
            .get("ram_total_bytes")
            .and_then(Value::as_u64)
            .map(|value| (value / (1024 * 1024)).to_string())
            .unwrap_or_else(|| "n/a".to_owned());
        println!(
            "  charge       CPU occupé mesuré {cpu} % (fenêtre {window} ms) · RAM libre {ram_free} Mio / {ram_total} Mio"
        );
    }
    let measurements = report
        .get("measurements")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let lateness = measurements
        .get("wake_lateness_us")
        .cloned()
        .unwrap_or(Value::Null);
    let work = measurements.get("work_us").cloned().unwrap_or(Value::Null);
    println!(
        "  mesure       {} tampons, période {} µs",
        measurements
            .get("completions_measured")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        measurements
            .get("period_us")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    if let Some(cadence) = report.get("realtime_cadence") {
        println!(
            "  cadence      ratio {:.2} (temps réel tenu: {})",
            cadence.get("ratio").and_then(Value::as_f64).unwrap_or(0.0),
            cadence
                .get("held_real_time")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
    }
    println!(
        "  work         p50 {} µs, p95 {} µs, max {} µs",
        field_u64(&work, "p50"),
        field_u64(&work, "p95"),
        field_u64(&work, "max"),
    );
    println!(
        "  lateness     p50 {} µs, p95 {} µs, p99 {} µs, max {} µs",
        field_i64(&lateness, "p50"),
        field_i64(&lateness, "p95"),
        field_i64(&lateness, "p99"),
        field_i64(&lateness, "max"),
    );
    println!(
        "  tampons en retard > période: {}   risque de starvation: {}   réveils sans tampon: {}",
        measurements
            .get("buffers_late_gt_one_period")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        measurements
            .get("queue_drain_risk_events")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        measurements
            .get("wakes_without_completion")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    println!(
        "  allocations / désallocations sur le thread de rendu: {} / {}",
        measurements
            .get("allocations_on_render_thread")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        measurements
            .get("deallocations_on_render_thread")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    if let Some(swaps) = report.get("swaps") {
        println!(
            "  swaps        {} demandés, {} activés, {} refusés",
            swaps.get("requested").and_then(Value::as_u64).unwrap_or(0),
            swaps.get("activations").and_then(Value::as_u64).unwrap_or(0),
            swaps.get("rejections").and_then(Value::as_u64).unwrap_or(0),
        );
    }
    if let Some(dsp) = report.get("dsp") {
        println!(
            "  |delta| max  {} (sinusoïde de référence {})",
            field_f32(dsp, "max_sample_delta"),
            field_f32(dsp, "baseline_sample_delta"),
        );
    }
    println!(
        "  limites      waveOut legacy, pas de hard-RT, XRUN non observables ; voir `limits` du JSON"
    );
}

fn compact_error(error: &Value) -> String {
    let stage = error
        .get("stage")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    let code = error.get("code").cloned().unwrap_or(Value::Null);
    let name = error
        .get("code_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if code.is_null() {
        format!("{stage}: {message}")
    } else {
        format!("{stage}: {message} [{code} {name}]")
    }
}

fn field_u64(value: &Value, field: &str) -> String {
    match value.get(field).and_then(Value::as_u64) {
        Some(number) => number.to_string(),
        None => "n/a".to_owned(),
    }
}

fn field_i64(value: &Value, field: &str) -> String {
    match value.get(field).and_then(Value::as_i64) {
        Some(number) => number.to_string(),
        None => "n/a".to_owned(),
    }
}

fn field_f32(value: &Value, field: &str) -> String {
    match value.get(field).and_then(Value::as_f64) {
        Some(number) => format!("{number:.4}"),
        None => "n/a".to_owned(),
    }
}

// ---------------------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;

    type Handle = *mut c_void;
    type HwavEout = isize;
    type MmResult = u32;

    const WAVE_MAPPER: u32 = u32::MAX;
    const WAVE_FORMAT_PCM: u16 = 1;
    const CALLBACK_EVENT: u32 = 0x0005_0000;
    const WHDR_DONE: u32 = 0x0000_0001;
    const WHDR_PREPARED: u32 = 0x0000_0002;
    const TIME_SAMPLES: u32 = 0x0000_0002;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAVE_FORMAT_8S16: u32 = 0x0000_8000;
    const SAMPLE_CHANNELS: u16 = 2;
    const SAMPLE_BITS: u16 = 16;
    const SAMPLE_BYTES: usize = 2;

    /// `WAVEFORMATEX`. Windows declares it inside `pshpack1.h`, but the field offsets are
    /// identical to a naturally aligned C layout; `waveOutOpen` takes a pointer (no size
    /// argument), so the trailing padding of this `repr(C)` layout is never observed.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WaveFormatEx {
        w_format_tag: u16,
        n_channels: u16,
        n_samples_per_sec: u32,
        n_avg_bytes_per_sec: u32,
        n_block_align: u16,
        w_bits_per_sample: u16,
        cb_size: u16,
    }

    #[repr(C)]
    struct WaveHdr {
        lp_data: *mut u8,
        dw_buffer_length: u32,
        dw_bytes_recorded: u32,
        dw_user: usize,
        dw_flags: u32,
        dw_loops: u32,
        lp_next: *mut WaveHdr,
        reserved: usize,
    }

    impl WaveHdr {
        fn zeroed() -> Self {
            Self {
                lp_data: std::ptr::null_mut(),
                dw_buffer_length: 0,
                dw_bytes_recorded: 0,
                dw_user: 0,
                dw_flags: 0,
                dw_loops: 0,
                lp_next: std::ptr::null_mut(),
                reserved: 0,
            }
        }
    }

    #[repr(C)]
    struct WaveOutCapsW {
        w_mid: u16,
        w_pid: u16,
        v_driver_version: u32,
        sz_pname: [u16; 32],
        dw_formats: u32,
        w_channels: u16,
        w_reserved1: u16,
        dw_support: u32,
    }

    impl WaveOutCapsW {
        fn zeroed() -> Self {
            Self {
                w_mid: 0,
                w_pid: 0,
                v_driver_version: 0,
                sz_pname: [0u16; 32],
                dw_formats: 0,
                w_channels: 0,
                w_reserved1: 0,
                dw_support: 0,
            }
        }
    }

    /// `MMTIME` as declared by Windows: `wType` + an 8-byte union (SMPTE is 8 bytes).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MmTime {
        w_type: u32,
        samples: u32,
        extra: u32,
    }

    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd_version: [u16; 128],
    }

    /// `FILETIME`: 100 ns intervals since 1601, split in two `DWORD`s.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTimeRaw {
        low: u32,
        high: u32,
    }

    impl FileTimeRaw {
        fn zeroed() -> Self {
            Self { low: 0, high: 0 }
        }

        fn to_u64(&self) -> u64 {
            ((self.high as u64) << 32) | self.low as u64
        }
    }

    /// `MEMORYSTATUSEX` from kernel32/`sysinfoapi.h`.
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load_percent: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    impl MemoryStatusEx {
        fn zeroed() -> Self {
            Self {
                length: size_of::<MemoryStatusEx>() as u32,
                memory_load_percent: 0,
                total_phys: 0,
                avail_phys: 0,
                total_page_file: 0,
                avail_page_file: 0,
                total_virtual: 0,
                avail_virtual: 0,
                avail_extended_virtual: 0,
            }
        }
    }

    #[link(name = "winmm")]
    extern "system" {
        fn waveOutGetNumDevs() -> u32;
        fn waveOutGetDevCapsW(device_id: usize, caps: *mut WaveOutCapsW, size: u32) -> MmResult;
        fn waveOutOpen(
            handle: *mut HwavEout,
            device_id: u32,
            format: *const WaveFormatEx,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> MmResult;
        fn waveOutGetID(handle: HwavEout, device_id: *mut u32) -> MmResult;
        fn waveOutPrepareHeader(handle: HwavEout, header: *mut WaveHdr, size: u32) -> MmResult;
        fn waveOutWrite(handle: HwavEout, header: *mut WaveHdr, size: u32) -> MmResult;
        fn waveOutReset(handle: HwavEout) -> MmResult;
        fn waveOutUnprepareHeader(handle: HwavEout, header: *mut WaveHdr, size: u32) -> MmResult;
        fn waveOutClose(handle: HwavEout) -> MmResult;
        fn waveOutGetPosition(handle: HwavEout, time: *mut MmTime, size: u32) -> MmResult;
        fn timeBeginPeriod(period: u32) -> u32;
        fn timeEndPeriod(period: u32) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateEventW(
            attributes: *mut c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn GetCurrentThread() -> Handle;
        fn SetThreadPriority(thread: Handle, priority: i32) -> i32;
        fn GetThreadPriority(thread: Handle) -> i32;
        fn GetSystemTimes(
            idle: *mut FileTimeRaw,
            kernel: *mut FileTimeRaw,
            user: *mut FileTimeRaw,
        ) -> i32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    }

    // avrt.dll: Windows Multimedia Class Scheduler Service (MMCSS) registration.
    #[link(name = "avrt")]
    extern "system" {
        fn AvSetMmThreadCharacteristicsW(task_name: *const u16, task_index: *mut u32) -> Handle;
        fn AvSetMmThreadPriority(handle: Handle, priority: i32) -> i32;
        fn AvRevertMmThreadCharacteristics(handle: Handle) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }

    pub fn windows_version_string() -> Option<String> {
        let mut info = OsVersionInfoW {
            size: size_of::<OsVersionInfoW>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            csd_version: [0u16; 128],
        };
        // SAFETY: `info` is a valid, correctly initialised OSVERSIONINFOW.
        let status = unsafe { RtlGetVersion(&mut info) };
        if status != 0 {
            return None;
        }
        Some(format!("{}.{}.{}", info.major, info.minor, info.build))
    }

    fn mm_name(code: u32) -> &'static str {
        match code {
            0 => "MMSYSERR_NOERROR",
            1 => "MMSYSERR_ERROR",
            2 => "MMSYSERR_BADDEVICEID",
            3 => "MMSYSERR_NOTENABLED",
            4 => "MMSYSERR_ALLOCATED",
            5 => "MMSYSERR_INVALHANDLE",
            6 => "MMSYSERR_NODRIVER",
            7 => "MMSYSERR_NOMEM",
            8 => "MMSYSERR_NOTSUPPORTED",
            9 => "MMSYSERR_BADERRNUM",
            10 => "MMSYSERR_INVALFLAG",
            11 => "MMSYSERR_INVALPARAM",
            12 => "MMSYSERR_HANDLEBUSY",
            13 => "MMSYSERR_INVALIDALIAS",
            32 => "WAVERR_BADFORMAT",
            33 => "WAVERR_STILLPLAYING",
            34 => "WAVERR_UNPREPARED",
            35 => "WAVERR_SYNC",
            36 => "WAVERR_WRONGDEVICE",
            _ => "MMSYSERR_UNKNOWN",
        }
    }

    #[derive(Debug)]
    struct Failure {
        stage: &'static str,
        code: Option<u32>,
        message: String,
        code_system: &'static str,
    }

    impl Failure {
        fn new(stage: &'static str, code: Option<u32>, message: impl Into<String>) -> Self {
            Self {
                stage,
                code,
                message: message.into(),
                code_system: "mmresult",
            }
        }

        /// Failure carrying a Win32 `GetLastError` code (AVRT / kernel32 calls).
        fn win32(stage: &'static str, code: u32, message: impl Into<String>) -> Self {
            Self {
                stage,
                code: Some(code),
                message: message.into(),
                code_system: "win32",
            }
        }

        fn to_json(&self) -> Value {
            let code_name = match (self.code, self.code_system) {
                (None, _) => Value::Null,
                (Some(code), "win32") => Value::from(win32_name(code)),
                (Some(code), _) => Value::from(mm_name(code)),
            };
            json!({
                "stage": self.stage,
                "code": self.code,
                "code_name": code_name,
                "code_system": self.code_system,
                "message": self.message,
            })
        }
    }

    /// A small, explicit mapping for the Win32 error codes this probe can receive.
    fn win32_name(code: u32) -> &'static str {
        match code {
            0 => "ERROR_SUCCESS",
            5 => "ERROR_ACCESS_DENIED",
            6 => "ERROR_INVALID_HANDLE",
            8 => "ERROR_NOT_ENOUGH_MEMORY",
            13 => "ERROR_INVALID_DATA",
            87 => "ERROR_INVALID_PARAMETER",
            126 => "ERROR_MOD_NOT_FOUND",
            127 => "ERROR_PROC_NOT_FOUND",
            998 => "ERROR_NOACCESS",
            _ => "WIN32_ERROR",
        }
    }

    /// A formatted `MMRESULT` (positive) or Win32 `GetLastError` (negative context).
    fn mm_error(stage: &'static str, code: u32) -> Failure {
        Failure::new(
            stage,
            Some(code),
            format!("{} (MMRESULT / winmm)", mm_name(code)),
        )
    }

    // -----------------------------------------------------------------------------------
    // Thread priority: MMCSS (avrt.dll) and raw Win32 priority
    // -----------------------------------------------------------------------------------

    /// `L"Pro Audio"` as a NUL-terminated UTF-16 array (no allocation needed).
    const PRO_AUDIO_UTF16: [u16; 10] = [
        b'P' as u16,
        b'r' as u16,
        b'o' as u16,
        b' ' as u16,
        b'A' as u16,
        b'u' as u16,
        b'd' as u16,
        b'i' as u16,
        b'o' as u16,
        0,
    ];
    const AVRT_PRIORITY_CRITICAL: i32 = 2;
    /// `GetThreadPriority` returns this value on failure.
    const THREAD_PRIORITY_ERROR_RETURN: i32 = -2;

    struct RealtimeState {
        mmcss_requested: bool,
        mmcss_acquired: bool,
        mmcss_handle: Handle,
        mmcss_task_index: Option<u32>,
        mmcss_priority_set: bool,
        thread_priority_requested: Option<ThreadPriorityChoice>,
        thread_priority_set: bool,
        baseline_priority: Option<i32>,
        priority_after_mmcss: Option<i32>,
        thread_priority_observed: Option<i32>,
        applied_order: Vec<&'static str>,
        avrt_error: Option<u32>,
        win32_error: Option<u32>,
        notes: Vec<String>,
    }

    impl RealtimeState {
        /// Apply the requested scheduling characteristics to the **calling** thread.
        ///
        /// Order: MMCSS registration (`Pro Audio`) → `AVRT_PRIORITY_CRITICAL` → raw
        /// `SetThreadPriority`. Any failure is explicit: the returned `Failure` carries
        /// the Win32 error and the partial state is reported, so a request that was not
        /// fulfilled can never look like a success.
        fn apply(config: &Config) -> Result<Self, (Failure, Value)> {
            let mut state = RealtimeState {
                mmcss_requested: config.mmcss,
                mmcss_acquired: false,
                mmcss_handle: std::ptr::null_mut(),
                mmcss_task_index: None,
                mmcss_priority_set: false,
                thread_priority_requested: config.thread_priority,
                thread_priority_set: false,
                baseline_priority: None,
                priority_after_mmcss: None,
                thread_priority_observed: None,
                applied_order: Vec::new(),
                avrt_error: None,
                win32_error: None,
                notes: Vec::new(),
            };

            // SAFETY: GetCurrentThread returns a pseudo-handle usable for the calling thread.
            let baseline = unsafe { GetThreadPriority(GetCurrentThread()) };
            if baseline != THREAD_PRIORITY_ERROR_RETURN {
                state.baseline_priority = Some(baseline);
            } else {
                state.notes.push(
                    "GetThreadPriority returned THREAD_PRIORITY_ERROR_RETURN before setup"
                        .to_owned(),
                );
            }

            if config.mmcss {
                let mut task_index: u32 = 0;
                // SAFETY: PRO_AUDIO_UTF16 is a valid NUL-terminated UTF-16 string and
                // task_index is a writable u32.
                let handle = unsafe {
                    AvSetMmThreadCharacteristicsW(PRO_AUDIO_UTF16.as_ptr(), &mut task_index)
                };
                if handle.is_null() {
                    let error = code_of_last_error();
                    state.avrt_error = Some(error);
                    state.notes.push(format!(
                        "AvSetMmThreadCharacteristicsW(L\"Pro Audio\") failed with Win32 error {error}; the render thread is NOT MMCSS-registered"
                    ));
                    let failure = Failure::win32(
                        "AvSetMmThreadCharacteristicsW",
                        error,
                        "MMCSS 'Pro Audio' registration failed; refusing to run as if a real-time priority had been applied",
                    );
                    return Err((failure, state.to_json()));
                }
                state.mmcss_acquired = true;
                state.mmcss_handle = handle;
                state.mmcss_task_index = Some(task_index);
                state
                    .applied_order
                    .push("AvSetMmThreadCharacteristicsW(Pro Audio)");

                // SAFETY: `handle` comes from AvSetMmThreadCharacteristicsW; the priority
                // value is one of the documented AVRT_PRIORITY_* constants.
                let priority_ok = unsafe { AvSetMmThreadPriority(handle, AVRT_PRIORITY_CRITICAL) };
                if priority_ok == 0 {
                    let error = code_of_last_error();
                    state.avrt_error = Some(error);
                    state.notes.push(format!(
                        "AvSetMmThreadPriority(AVRT_PRIORITY_CRITICAL) failed with Win32 error {error}"
                    ));
                    // SAFETY: `handle` is still owned by this thread at this point.
                    let reverted = unsafe { AvRevertMmThreadCharacteristics(handle) } != 0;
                    state.mmcss_acquired = false;
                    state.mmcss_handle = std::ptr::null_mut();
                    state.notes.push(format!(
                        "MMCSS handle released after the priority failure: {reverted}"
                    ));
                    let failure = Failure::win32(
                        "AvSetMmThreadPriority",
                        error,
                        "AVRT_PRIORITY_CRITICAL could not be set; refusing to report a real-time priority that was not applied",
                    );
                    return Err((failure, state.to_json()));
                }
                state.mmcss_priority_set = true;
                state
                    .applied_order
                    .push("AvSetMmThreadPriority(AVRT_PRIORITY_CRITICAL)");
                // SAFETY: pseudo-handle of the calling thread.
                let observed = unsafe { GetThreadPriority(GetCurrentThread()) };
                if observed != THREAD_PRIORITY_ERROR_RETURN {
                    state.priority_after_mmcss = Some(observed);
                }
            }

            if let Some(choice) = config.thread_priority {
                // SAFETY: pseudo-handle; `win32_value` is a documented THREAD_PRIORITY_*.
                let ok = unsafe { SetThreadPriority(GetCurrentThread(), choice.win32_value()) };
                if ok == 0 {
                    let error = code_of_last_error();
                    state.win32_error = Some(error);
                    state.notes.push(format!(
                        "SetThreadPriority({}) failed with Win32 error {error}",
                        choice.win32_name()
                    ));
                    if state.mmcss_acquired {
                        // SAFETY: handle still owned by this thread.
                        let _ = unsafe { AvRevertMmThreadCharacteristics(state.mmcss_handle) };
                        state.mmcss_acquired = false;
                        state.mmcss_handle = std::ptr::null_mut();
                    }
                    let failure = Failure::win32(
                        "SetThreadPriority",
                        error,
                        format!(
                            "SetThreadPriority({}) failed; refusing to report a priority that was not applied",
                            choice.win32_name()
                        ),
                    );
                    return Err((failure, state.to_json()));
                }
                state.thread_priority_set = true;
                state.applied_order.push(match choice {
                    ThreadPriorityChoice::Normal => "SetThreadPriority(THREAD_PRIORITY_NORMAL)",
                    ThreadPriorityChoice::AboveNormal => {
                        "SetThreadPriority(THREAD_PRIORITY_ABOVE_NORMAL)"
                    }
                    ThreadPriorityChoice::Highest => "SetThreadPriority(THREAD_PRIORITY_HIGHEST)",
                    ThreadPriorityChoice::TimeCritical => {
                        "SetThreadPriority(THREAD_PRIORITY_TIME_CRITICAL)"
                    }
                });
                // SAFETY: pseudo-handle of the calling thread.
                let observed = unsafe { GetThreadPriority(GetCurrentThread()) };
                if observed != THREAD_PRIORITY_ERROR_RETURN {
                    state.thread_priority_observed = Some(observed);
                }
            } else {
                // No raw priority requested: report the base priority resulting from the
                // MMCSS setup (or the untouched baseline when MMCSS was not requested).
                state.thread_priority_observed =
                    state.priority_after_mmcss.or(state.baseline_priority);
            }

            if state.mmcss_acquired {
                state.notes.push(
                    "MMCSS 'Pro Audio' + AVRT_PRIORITY_CRITICAL is a scheduling hint: it is not a deadline guarantee and not a hard real-time qualification"
                        .to_owned(),
                );
            }
            if state.thread_priority_requested.is_some() {
                state.notes.push(
                    "SetThreadPriority was applied after MMCSS (when both were requested); thread_priority_observed is the base priority read back with GetThreadPriority"
                        .to_owned(),
                );
            }
            Ok(state)
        }

        /// Revert the MMCSS registration of the calling thread, reporting the result.
        fn revert(&mut self) -> Value {
            if !self.mmcss_acquired {
                return json!({
                    "called": false,
                    "ok": Value::Null,
                    "error": Value::Null,
                    "error_name": Value::Null,
                    "thread_priority_after_revert": Value::Null,
                    "note": "no MMCSS handle was held by this thread",
                });
            }
            // SAFETY: `mmcss_handle` was returned by AvSetMmThreadCharacteristicsW for
            // this thread and is still owned by it.
            let ok = unsafe { AvRevertMmThreadCharacteristics(self.mmcss_handle) };
            let error = if ok == 0 {
                Some(code_of_last_error())
            } else {
                None
            };
            // SAFETY: pseudo-handle of the calling thread.
            let observed = unsafe { GetThreadPriority(GetCurrentThread()) };
            self.mmcss_acquired = false;
            self.mmcss_handle = std::ptr::null_mut();
            json!({
                "called": true,
                "ok": ok != 0,
                "error": error,
                "error_name": error.map(win32_name),
                "thread_priority_after_revert": if observed != THREAD_PRIORITY_ERROR_RETURN { Value::from(observed) } else { Value::Null },
            })
        }

        fn to_json(&self) -> Value {
            json!({
                "mmcss_requested": self.mmcss_requested,
                "mmcss_acquired": self.mmcss_acquired,
                "mmcss_task_name": "Pro Audio",
                "mmcss_task_index": self.mmcss_task_index,
                "mmcss_priority": if self.mmcss_requested { json!("critical") } else { Value::Null },
                "mmcss_priority_set": self.mmcss_priority_set,
                "thread_priority_requested": self.thread_priority_requested.map(|choice| choice.as_str()),
                "thread_priority_win32_constant": self.thread_priority_requested.map(|choice| choice.win32_name()),
                "thread_priority_value": self.thread_priority_requested.map(|choice| choice.win32_value()),
                "thread_priority_set": self.thread_priority_set,
                "thread_priority_observed": self.thread_priority_observed,
                "baseline_thread_priority": self.baseline_priority,
                "priority_after_mmcss": self.priority_after_mmcss,
                "applied_order": self.applied_order,
                "avrt_error": self.avrt_error,
                "win32_error": self.win32_error,
                "notes": self.notes,
            })
        }
    }

    fn code_of_last_error() -> u32 {
        // SAFETY: documented kernel32 call; no arguments, no lifetime.
        unsafe { GetLastError() }
    }

    // -----------------------------------------------------------------------------------
    // Machine load sampling (CPU + RAM) around the measured window
    // -----------------------------------------------------------------------------------

    #[derive(Clone, Copy, Debug)]
    pub struct SystemSample {
        busy_100ns: u64,
        total_100ns: u64,
        ram_available: u64,
        ram_total: u64,
        ram_load_percent: u32,
    }

    pub fn system_sample() -> Option<SystemSample> {
        let mut idle = FileTimeRaw::zeroed();
        let mut kernel = FileTimeRaw::zeroed();
        let mut user = FileTimeRaw::zeroed();
        // SAFETY: three writable FILETIME buffers.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        let idle = idle.to_u64();
        let total = kernel.to_u64().saturating_add(user.to_u64());
        let mut memory = MemoryStatusEx::zeroed();
        // SAFETY: `memory` has `length` set to its own size, as the API requires.
        let memory_ok = unsafe { GlobalMemoryStatusEx(&mut memory) };
        let (ram_available, ram_total, ram_load_percent) = if memory_ok != 0 {
            (
                memory.avail_phys,
                memory.total_phys,
                memory.memory_load_percent,
            )
        } else {
            (0, 0, 0)
        };
        Some(SystemSample {
            busy_100ns: total.saturating_sub(idle),
            total_100ns: total,
            ram_available,
            ram_total,
            ram_load_percent,
        })
    }

    pub fn system_load_json(
        start: Option<SystemSample>,
        end: Option<SystemSample>,
        window_ms: u64,
        note: Option<String>,
    ) -> Value {
        let cpu_busy_percent = match (start, end) {
            (Some(start), Some(end)) if end.total_100ns > start.total_100ns => {
                let total = end.total_100ns - start.total_100ns;
                let busy = end.busy_100ns.saturating_sub(start.busy_100ns);
                Some(busy as f64 * 100.0 / total as f64)
            }
            _ => None,
        };
        let logical = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(0);
        json!({
            "cpu_busy_percent_measured": cpu_busy_percent,
            "cpu_sample_window_ms": window_ms,
            "cpu_logical": logical,
            "ram_available_bytes_start": start.map(|sample| sample.ram_available),
            "ram_available_bytes_end": end.map(|sample| sample.ram_available),
            "ram_total_bytes": end.or(start).map(|sample| sample.ram_total),
            "ram_load_percent_start": start.map(|sample| sample.ram_load_percent),
            "probe_load_note": note,
            "method": "GetSystemTimes delta (idle vs kernel+user, all CPUs) and GlobalMemoryStatusEx, sampled by this probe process for the measured window",
        })
    }

    #[derive(Debug, Clone)]
    struct Caps {
        name: String,
        mid: u16,
        pid: u16,
        driver_version: u32,
        channels: u16,
        formats: u32,
        formats_supported: Vec<String>,
        support: u32,
        support_flags: Vec<String>,
    }

    fn read_caps(device_id: u32) -> Result<Caps, Failure> {
        let mut raw = WaveOutCapsW::zeroed();
        // SAFETY: `raw` is a writable WAVEOUTCAPSW; WAVE_MAPPER is passed as UINT_PTR.
        let result = unsafe {
            waveOutGetDevCapsW(
                device_id as usize,
                &mut raw,
                size_of::<WaveOutCapsW>() as u32,
            )
        };
        if result != 0 {
            return Err(mm_error("waveOutGetDevCapsW", result));
        }
        let name = decode_wide(&raw.sz_pname);
        Ok(Caps {
            name,
            mid: raw.w_mid,
            pid: raw.w_pid,
            driver_version: raw.v_driver_version,
            channels: raw.w_channels,
            formats: raw.dw_formats,
            formats_supported: decode_formats(raw.dw_formats),
            support: raw.dw_support,
            support_flags: decode_support(raw.dw_support),
        })
    }

    fn decode_wide(buffer: &[u16]) -> String {
        let length = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..length])
    }

    fn decode_formats(mask: u32) -> Vec<String> {
        let groups = [
            (0u32, 11_025u32),
            (4u32, 22_050u32),
            (8u32, 44_100u32),
            (12u32, 48_000u32),
        ];
        let variants = [(0u32, 1u32, 8u32), (1, 1, 16), (2, 2, 8), (3, 2, 16)];
        let mut supported = Vec::new();
        for (offset, rate) in groups {
            for (variant, channels, bits) in variants {
                if mask & (1u32 << (offset + variant)) != 0 {
                    supported.push(format!("{rate}Hz:{channels}ch:{bits}bit"));
                }
            }
        }
        supported
    }

    fn decode_support(mask: u32) -> Vec<String> {
        let flags = [
            (0x0001u32, "pitch"),
            (0x0002, "playback-rate"),
            (0x0004, "volume"),
            (0x0008, "left-right-volume"),
            (0x0010, "sync"),
        ];
        flags
            .iter()
            .filter(|(bit, _)| mask & bit != 0)
            .map(|(_, name)| (*name).to_owned())
            .collect()
    }

    fn caps_json(device_id: u32, caps: &Caps) -> Value {
        json!({
            "resolved_device_id": device_id,
            "name": caps.name,
            "mid": caps.mid,
            "pid": caps.pid,
            "driver_version": caps.driver_version,
            "channels": caps.channels,
            "formats_mask": format!("0x{:08X}", caps.formats),
            "formats_supported": caps.formats_supported,
            "claims_48k_stereo_16bit": caps.formats & WAVE_FORMAT_8S16 != 0,
            "support_flags": caps.support_flags,
            "support_mask": format!("0x{:08X}", caps.support),
        })
    }

    fn create_event() -> Result<Handle, Failure> {
        // SAFETY: anonymous, auto-reset, initially unsignalled event; null attributes.
        let handle = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
        if handle.is_null() {
            let code = unsafe { GetLastError() };
            return Err(Failure::new(
                "CreateEventW",
                Some(code),
                "cannot create the waveOut completion event",
            ));
        }
        Ok(handle)
    }

    struct Session {
        handle: HwavEout,
        event: Handle,
        format: WaveFormatEx,
        format_bytes_per_buffer: u32,
        device: Caps,
        resolved_device_id: u32,
        requested_device: u32,
        timer_period_ms: u32,
        timer_active: bool,
        buffers: Vec<Box<[u8]>>,
        headers: Box<[WaveHdr]>,
        prepared: Vec<bool>,
        cleanup: Vec<Value>,
        closed: bool,
    }

    impl Session {
        fn open(config: &Config) -> Result<Self, Failure> {
            let requested_device = config.device.unwrap_or(WAVE_MAPPER);
            let timer_period_ms = 1u32;
            // SAFETY: documented winmm timer granularity call, balanced by close().
            let timer_result = unsafe { timeBeginPeriod(timer_period_ms) };
            let timer_active = timer_result == 0;

            let event = match create_event() {
                Ok(event) => event,
                Err(failure) => {
                    if timer_active {
                        unsafe { timeEndPeriod(timer_period_ms) };
                    }
                    return Err(failure);
                }
            };

            let frames = config.frames as u32;
            let rate = config.rate;
            let block_align = SAMPLE_CHANNELS as u32 * SAMPLE_BYTES as u32;
            let format = WaveFormatEx {
                w_format_tag: WAVE_FORMAT_PCM,
                n_channels: SAMPLE_CHANNELS,
                n_samples_per_sec: rate,
                n_avg_bytes_per_sec: rate * block_align,
                n_block_align: block_align as u16,
                w_bits_per_sample: SAMPLE_BITS,
                cb_size: 0,
            };
            let bytes_per_buffer = frames * block_align;

            let mut handle: HwavEout = 0;
            // SAFETY: `format` is a fully initialised PCM WAVEFORMATEX; `event` is a
            // valid HANDLE used as the CALLBACK_EVENT target.
            let result = unsafe {
                waveOutOpen(
                    &mut handle,
                    requested_device,
                    &format,
                    event as usize,
                    0,
                    CALLBACK_EVENT,
                )
            };
            if result != 0 {
                unsafe { CloseHandle(event) };
                if timer_active {
                    unsafe { timeEndPeriod(timer_period_ms) };
                }
                return Err(Failure::new(
                    "waveOutOpen",
                    Some(result),
                    format!(
                        "the device refused PCM 16-bit stereo {} Hz (requested device {})",
                        rate,
                        describe_device_id(requested_device)
                    ),
                ));
            }

            let mut actual_id = requested_device;
            // SAFETY: `handle` was just opened successfully.
            let id_result = unsafe { waveOutGetID(handle, &mut actual_id) };
            if id_result != 0 {
                actual_id = requested_device;
            }
            let resolved_device_id = actual_id;
            let device = match read_caps(actual_id) {
                Ok(caps) => caps,
                Err(_) => match read_caps(requested_device) {
                    Ok(caps) => caps,
                    Err(failure) => {
                        // Caps are metadata only: never abort a successfully opened
                        // device because its name could not be read.
                        let _ = failure;
                        Caps {
                            name: format!("waveOut device {actual_id} (caps unavailable)"),
                            mid: 0,
                            pid: 0,
                            driver_version: 0,
                            channels: SAMPLE_CHANNELS,
                            formats: 0,
                            formats_supported: Vec::new(),
                            support: 0,
                            support_flags: Vec::new(),
                        }
                    }
                },
            };

            let buffers = (0..config.buffers)
                .map(|_| vec![0u8; bytes_per_buffer as usize].into_boxed_slice())
                .collect::<Vec<Box<[u8]>>>();
            let headers = (0..config.buffers)
                .map(|_| WaveHdr::zeroed())
                .collect::<Vec<WaveHdr>>()
                .into_boxed_slice();
            let prepared = vec![false; config.buffers];

            Ok(Session {
                handle,
                event,
                format,
                format_bytes_per_buffer: bytes_per_buffer,
                device,
                resolved_device_id,
                requested_device,
                timer_period_ms,
                timer_active,
                buffers,
                headers,
                prepared,
                cleanup: Vec::new(),
                closed: false,
            })
        }

        fn prepare(&mut self) -> Result<(), Failure> {
            let header_size = size_of::<WaveHdr>() as u32;
            let buffer_length = self.format_bytes_per_buffer;
            let count = self.headers.len();
            {
                let headers = &mut *self.headers;
                let buffers = &self.buffers;
                for index in 0..count {
                    headers[index].lp_data = buffers[index].as_ptr() as *mut u8;
                    headers[index].dw_buffer_length = buffer_length;
                    headers[index].dw_bytes_recorded = 0;
                    headers[index].dw_flags = 0;
                    headers[index].dw_loops = 0;
                    headers[index].lp_next = std::ptr::null_mut();
                    headers[index].reserved = 0;
                }
            }
            for index in 0..count {
                // SAFETY: header `index` points to a stable buffer of the declared size.
                let result = unsafe {
                    waveOutPrepareHeader(self.handle, &mut self.headers[index], header_size)
                };
                if result != 0 {
                    self.unprepare_all();
                    return Err(Failure::new(
                        "waveOutPrepareHeader",
                        Some(result),
                        format!("buffer {index} could not be prepared"),
                    ));
                }
                self.prepared[index] = true;
                let flags = self.headers[index].dw_flags;
                if flags & WHDR_PREPARED == 0 {
                    self.unprepare_all();
                    return Err(Failure::new(
                        "waveOutPrepareHeader",
                        None,
                        format!("buffer {index} was not marked WHDR_PREPARED"),
                    ));
                }
            }
            Ok(())
        }

        fn unprepare_all(&mut self) {
            let header_size = size_of::<WaveHdr>() as u32;
            for index in 0..self.headers.len() {
                if !self.prepared[index] {
                    continue;
                }
                let mut attempt = 0u32;
                loop {
                    // SAFETY: header `index` is prepared and the device will be reset
                    // before unpreparing in close().
                    let result = unsafe {
                        waveOutUnprepareHeader(self.handle, &mut self.headers[index], header_size)
                    };
                    if result == 0 {
                        self.prepared[index] = false;
                        break;
                    }
                    attempt += 1;
                    if attempt >= 3 {
                        self.cleanup.push(json!({
                            "step": "waveOutUnprepareHeader",
                            "buffer": index,
                            "code": result,
                            "code_name": mm_name(result),
                        }));
                        self.prepared[index] = false;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }

        fn reset(&mut self) {
            // SAFETY: `handle` is open; reset flushes whatever is still queued.
            let result = unsafe { waveOutReset(self.handle) };
            if result != 0 {
                self.cleanup.push(json!({
                    "step": "waveOutReset",
                    "code": result,
                    "code_name": mm_name(result),
                }));
            }
        }

        fn close(&mut self) -> Vec<Value> {
            if self.closed {
                return std::mem::take(&mut self.cleanup);
            }
            self.reset();
            self.unprepare_all();
            // SAFETY: the device was opened by this session and is not used anymore.
            let result = unsafe { waveOutClose(self.handle) };
            if result != 0 {
                self.cleanup.push(json!({
                    "step": "waveOutClose",
                    "code": result,
                    "code_name": mm_name(result),
                }));
            }
            if !self.event.is_null() {
                // SAFETY: the event handle was created by this session.
                unsafe { CloseHandle(self.event) };
                self.event = std::ptr::null_mut();
            }
            if self.timer_active {
                // SAFETY: balances timeBeginPeriod in open().
                unsafe { timeEndPeriod(self.timer_period_ms) };
                self.timer_active = false;
            }
            self.closed = true;
            std::mem::take(&mut self.cleanup)
        }

        fn info_json(&self) -> Value {
            json!({
                "device": caps_json(self.resolved_device_id, &self.device),
                "format": {
                    "sample_format": "pcm_s16le",
                    "channels": SAMPLE_CHANNELS,
                    "sample_rate": self.format.n_samples_per_sec,
                    "bits_per_sample": SAMPLE_BITS,
                    "block_align": self.format.n_block_align,
                    "avg_bytes_per_sec": self.format.n_avg_bytes_per_sec,
                    "frames_per_buffer": self.format_bytes_per_buffer / self.format.n_block_align as u32,
                    "bytes_per_buffer": self.format_bytes_per_buffer,
                    "buffers_in_rotation": self.headers.len(),
                    "period_us": (self.format_bytes_per_buffer as u64 / self.format.n_block_align as u64)
                        * 1_000_000
                        / self.format.n_samples_per_sec.max(1) as u64,
                    "queue_depth_ms": (self.headers.len() as f64
                        * (self.format_bytes_per_buffer as f64 / self.format.n_block_align as f64)
                        * 1000.0
                        / self.format.n_samples_per_sec.max(1) as f64),
                },
                "requested_device": match self.requested_device {
                    WAVE_MAPPER => json!({"kind": "wave_mapper"}),
                    index => json!({"kind": "index", "index": index}),
                },
                "timer_period_ms": self.timer_period_ms,
                "timer_begin_period_ok": self.timer_active,
            })
        }

        fn write_buffer(&mut self, index: usize) -> MmResult {
            let header_size = size_of::<WaveHdr>() as u32;
            let pointer = &mut self.headers[index] as *mut WaveHdr;
            // SAFETY: the header is prepared, its data buffer is stable and silent.
            unsafe { waveOutWrite(self.handle, pointer, header_size) }
        }

        fn position_samples(&mut self) -> Result<u32, MmResult> {
            let mut time = MmTime {
                w_type: TIME_SAMPLES,
                samples: 0,
                extra: 0,
            };
            // SAFETY: `time` is a valid MMTIME of the size passed to the driver.
            let result = unsafe {
                waveOutGetPosition(self.handle, &mut time, size_of::<MmTime>() as u32)
            };
            if result != 0 {
                Err(result)
            } else {
                Ok(time.samples)
            }
        }

        fn render(
            &mut self,
            host: Option<Arc<BlockHost>>,
            stop: &AtomicBool,
            input: &[f32],
            run_start: Instant,
            thread_start: Instant,
            capacity: usize,
            baseline_delta: f32,
        ) -> RunData {
            let frames = (self.format_bytes_per_buffer / self.format.n_block_align as u32) as usize;
            let period_us = (frames as u64 * 1_000_000) / self.format.n_samples_per_sec.max(1) as u64;
            let buffers = self.headers.len();
            let mut run = RunData::new(period_us, frames, self.format.n_samples_per_sec, buffers, baseline_delta, capacity);

            let submit_start = Instant::now();
            run.open_prepare_us = submit_start.saturating_duration_since(thread_start).as_micros() as u64;
            for index in 0..buffers {
                let result = self.write_buffer(index);
                run.writes_attempted += 1;
                if result != 0 {
                    run.write_failures += 1;
                    run.fatal_stage = Some("waveOutWrite(initial)");
                    run.fatal_code = Some(result);
                    break;
                }
            }
            if run.write_failures > 0 {
                run.elapsed_us = run_start.elapsed().as_micros() as u64;
                run.render_elapsed_us = submit_start.elapsed().as_micros() as u64;
                return run;
            }

            let mut state = DspState::default();
            let mut computed = vec![0f32; frames];
            let mut previous_sample = 0f32;
            let mut input_cursor = 0usize;
            let mut done_indices: Vec<usize> = Vec::with_capacity(buffers);
            let mut grid_anchor: Option<Instant> = None;
            let mut grid_base: u64 = 0;
            let mut previous_completion_wake: Option<Instant> = None;
            let mut total_completions: u64 = 0;
            let mut position_supported = true;
            let mut position_error: Option<u32> = None;
            let warmup_target = (2 * buffers) as u64;

            while !stop.load(Ordering::Relaxed) {
                let timeout = if total_completions == 0 {
                    FIRST_WAIT_TIMEOUT_MS
                } else {
                    WAIT_TIMEOUT_MS
                };
                // SAFETY: the event handle belongs to this session.
                let wait = unsafe { WaitForSingleObject(self.event, timeout) };
                let wake = Instant::now();
                if wait == WAIT_TIMEOUT {
                    run.wait_timeouts += 1;
                    continue;
                }
                if wait != WAIT_OBJECT_0 {
                    run.fatal_stage = Some("WaitForSingleObject");
                    run.fatal_code = Some(wait);
                    break;
                }
                run.wakes += 1;

                done_indices.clear();
                for index in 0..buffers {
                    if self.headers[index].dw_flags & WHDR_DONE != 0 {
                        done_indices.push(index);
                    }
                }
                if done_indices.is_empty() {
                    run.wakes_without_completion += 1;
                    continue;
                }
                if done_indices.len() > 1 {
                    run.wakes_with_multiple_done += 1;
                    run.coalesced_completions += (done_indices.len() - 1) as u64;
                }

                // Wake-level scheduling measurement: the oldest completion of the
                // batch is the one the wake was triggered for, so its lateness is the
                // meaningful per-wake figure. Per-completion values are kept only to
                // correlate them with swap times.
                let batch_start = total_completions;
                let warmup_left = warmup_target.saturating_sub(run.warmup_completions);
                let batch_len = done_indices.len() as u64;
                let batch_has_measured = batch_len > warmup_left;
                let mut wake_lateness: Option<i64> = None;
                let mut wake_interval: Option<u64> = None;
                if batch_has_measured {
                    let first_measured_index = batch_start + warmup_left;
                    if grid_anchor.is_none() {
                        grid_base = first_measured_index;
                        grid_anchor = Some(wake);
                    }
                    let anchor = grid_anchor.unwrap_or(wake);
                    let k = first_measured_index.saturating_sub(grid_base);
                    let grid = anchor + Duration::from_micros(period_us.saturating_mul(k));
                    wake_lateness = Some(signed_micros(wake, grid));
                    wake_interval = previous_completion_wake
                        .map(|previous| (wake - previous).as_micros() as u64);
                    previous_completion_wake = Some(wake);
                }
                let mut measured_in_batch: u64 = 0;

                for &buffer_index in &done_indices {
                    let was_measured = run.warmup_completions >= warmup_target;
                    total_completions += 1;
                    if total_completions == 1 {
                        run.first_completion_offset_us =
                            Some((wake - submit_start).as_micros() as u64);
                    }

                    let in_warmup = !was_measured;

                    // --- the DSP block under test (the only work that matters) ---
                    let work_started = Instant::now();
                    let slice = &input[input_cursor..input_cursor + frames];
                    match host.as_ref() {
                        Some(host) => {
                            host.process_rt(slice, &mut computed, &mut state);
                        }
                        None => computed.copy_from_slice(slice),
                    }
                    let work_us = work_started.elapsed().as_micros() as u64;
                    input_cursor += frames;
                    if input_cursor + frames > input.len() {
                        input_cursor = 0;
                        run.input_wraps += 1;
                    }

                    let mut block_delta = 0f32;
                    for &sample in computed.iter() {
                        if !sample.is_finite() {
                            run.non_finite_samples += 1;
                            continue;
                        }
                        let delta = (sample - previous_sample).abs();
                        if delta > block_delta {
                            block_delta = delta;
                        }
                        previous_sample = sample;
                    }
                    if block_delta > run.max_sample_delta {
                        run.max_sample_delta = block_delta;
                    }

                    if position_supported {
                        match self.position_samples() {
                            Ok(played_samples) => {
                                let expected = (total_completions as u64)
                                    .saturating_mul(frames as u64)
                                    as i64;
                                let drift = played_samples as i64 - expected;
                                if !in_warmup {
                                    run.position_drift_frames.push(drift);
                                }
                            }
                            Err(code) => {
                                position_supported = false;
                                position_error = Some(code);
                            }
                        }
                    }

                    // --- submit silence, never the computed signal ---
                    self.buffers[buffer_index].fill(0);
                    // The driver sets WHDR_DONE when the buffer completes; it must be
                    // cleared before the header is queued again.
                    self.headers[buffer_index].dw_flags &= !WHDR_DONE;
                    let result = self.write_buffer(buffer_index);
                    let callback_total_us = wake.elapsed().as_micros() as u64;
                    run.writes_attempted += 1;
                    if result != 0 {
                        run.write_failures += 1;
                        run.fatal_stage = Some("waveOutWrite");
                        run.fatal_code = Some(result);
                    }

                    if in_warmup {
                        run.warmup_completions += 1;
                        if run.warmup_completions == warmup_target {
                            // Everything before this point is warm-up (TLS, first call
                            // into the module, driver bookkeeping); count from here.
                            reset_thread_counters();
                        }
                    } else {
                        measured_in_batch += 1;
                        run.completions_measured += 1;
                        let measured_index = total_completions - 1;
                        let k = measured_index.saturating_sub(grid_base);
                        let grid = grid_anchor.unwrap_or(wake)
                            + Duration::from_micros(period_us.saturating_mul(k));
                        let completion_lateness = signed_micros(wake, grid);
                        let queue_window_us =
                            period_us.saturating_mul(buffers.saturating_sub(1) as u64) as i64;
                        let margin_us = queue_window_us
                            .saturating_sub(completion_lateness)
                            .saturating_sub(callback_total_us as i64);
                        run.work_us.push(work_us);
                        run.callback_total_us.push(callback_total_us);
                        run.completion_lateness_us.push(completion_lateness);
                        run.queue_margin_us.push(margin_us);
                        run.completion_times_us.push(run_start.elapsed().as_micros() as u64);
                        run.completion_delta.push(block_delta);
                        if margin_us < 0 {
                            run.queue_drain_risk_events += 1;
                        }
                    }

                    if run.write_failures > 0 {
                        break;
                    }
                }
                if let Some(lateness) = wake_lateness {
                    run.wake_lateness_us.push(lateness);
                    if lateness > period_us as i64 {
                        run.wakes_late_gt_one_period += 1;
                        run.buffers_late_gt_one_period += measured_in_batch;
                    }
                }
                if let Some(interval) = wake_interval {
                    run.interval_us.push(interval);
                }
                if run.write_failures > 0 {
                    break;
                }
            }

            run.position_supported = position_supported;
            run.position_error_code = position_error;
            run.allocations = allocations_on_this_thread();
            run.deallocations = deallocations_on_this_thread();
            run.render_elapsed_us = submit_start.elapsed().as_micros() as u64;
            run.elapsed_us = run_start.elapsed().as_micros() as u64;
            run.blocks_processed = total_completions;
            run.final_position_samples = if position_supported {
                self.position_samples().ok().map(u64::from)
            } else {
                None
            };
            // Completion cadence actually delivered by the device, compared to the
            // cadence the audio content requires: a ratio well below 1 means the
            // callback is not keeping real time on this device path.
            if run.render_elapsed_us > 0 {
                run.completion_rate_hz =
                    total_completions as f64 * 1_000_000.0 / run.render_elapsed_us as f64;
            }
            run.expected_completion_rate_hz = if frames > 0 {
                run.rate as f64 / frames as f64
            } else {
                0.0
            };
            run
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    fn describe_device_id(device_id: u32) -> String {
        if device_id == WAVE_MAPPER {
            "WAVE_MAPPER".to_owned()
        } else {
            format!("#{device_id}")
        }
    }

    /// Signed distance in microseconds: positive when `later` is after `earlier`.
    fn signed_micros(later: Instant, earlier: Instant) -> i64 {
        if later >= earlier {
            (later - earlier).as_micros() as i64
        } else {
            -((earlier - later).as_micros() as i64)
        }
    }

    /// One measured render session. Per-completion vectors are consumed by `execute`
    /// to build statistics; they are not serialised one by one.
    struct RunData {
        period_us: u64,
        frames: usize,
        rate: u32,
        buffers: usize,
        baseline_sample_delta: f32,
        warmup_target: u64,
        warmup_completions: u64,
        completions_measured: u64,
        work_us: Vec<u64>,
        callback_total_us: Vec<u64>,
        wake_lateness_us: Vec<i64>,
        completion_lateness_us: Vec<i64>,
        interval_us: Vec<u64>,
        queue_margin_us: Vec<i64>,
        position_drift_frames: Vec<i64>,
        position_supported: bool,
        position_error_code: Option<u32>,
        wakes: u64,
        wakes_without_completion: u64,
        wakes_with_multiple_done: u64,
        coalesced_completions: u64,
        wait_timeouts: u64,
        buffers_late_gt_one_period: u64,
        wakes_late_gt_one_period: u64,
        queue_drain_risk_events: u64,
        writes_attempted: u64,
        write_failures: u64,
        first_completion_offset_us: Option<u64>,
        max_sample_delta: f32,
        non_finite_samples: u64,
        allocations: u64,
        deallocations: u64,
        elapsed_us: u64,
        open_prepare_us: u64,
        render_elapsed_us: u64,
        blocks_processed: u64,
        final_position_samples: Option<u64>,
        completion_rate_hz: f64,
        expected_completion_rate_hz: f64,
        fatal_stage: Option<&'static str>,
        fatal_code: Option<u32>,
        input_wraps: u64,
        completion_times_us: Vec<u64>,
        completion_delta: Vec<f32>,
    }

    impl RunData {
        fn new(
            period_us: u64,
            frames: usize,
            rate: u32,
            buffers: usize,
            baseline_sample_delta: f32,
            capacity: usize,
        ) -> Self {
            Self {
                period_us,
                frames,
                rate,
                buffers,
                baseline_sample_delta,
                warmup_target: (2 * buffers) as u64,
                warmup_completions: 0,
                completions_measured: 0,
                work_us: Vec::with_capacity(capacity),
                callback_total_us: Vec::with_capacity(capacity),
                wake_lateness_us: Vec::with_capacity(capacity),
                completion_lateness_us: Vec::with_capacity(capacity),
                interval_us: Vec::with_capacity(capacity),
                queue_margin_us: Vec::with_capacity(capacity),
                position_drift_frames: Vec::with_capacity(capacity),
                position_supported: false,
                position_error_code: None,
                wakes: 0,
                wakes_without_completion: 0,
                wakes_with_multiple_done: 0,
                coalesced_completions: 0,
                wait_timeouts: 0,
                buffers_late_gt_one_period: 0,
                wakes_late_gt_one_period: 0,
                queue_drain_risk_events: 0,
                writes_attempted: 0,
                write_failures: 0,
                first_completion_offset_us: None,
                max_sample_delta: 0.0,
                non_finite_samples: 0,
                allocations: 0,
                deallocations: 0,
                elapsed_us: 0,
                open_prepare_us: 0,
                render_elapsed_us: 0,
                blocks_processed: 0,
                final_position_samples: None,
                completion_rate_hz: 0.0,
                expected_completion_rate_hz: 0.0,
                fatal_stage: None,
                fatal_code: None,
                input_wraps: 0,
                completion_times_us: Vec::with_capacity(capacity),
                completion_delta: Vec::with_capacity(capacity),
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // Hot-swap control thread
    // -----------------------------------------------------------------------------------

    struct SwapRecord {
        at_us: u64,
        requested_artifact: String,
        patch_id: Option<String>,
        status: &'static str,
        reject_reason: Option<Value>,
        load_ms: u64,
        evidence_ms: u64,
        apply_ms: u64,
        total_ms: u64,
        error: Option<String>,
    }

    #[derive(Default)]
    struct SwapOutcome {
        requested: u64,
        activations: u64,
        rejections: u64,
        errors: u64,
        records: Vec<SwapRecord>,
        swap_times_us: Vec<u64>,
        final_patch_id: Option<String>,
    }

    fn swap_json(outcome: &SwapOutcome) -> Value {
        json!({
            "requested": outcome.requested,
            "activations": outcome.activations,
            "rejections": outcome.rejections,
            "errors": outcome.errors,
            "final_patch_id": outcome.final_patch_id,
            "records": outcome.records.iter().map(|record| json!({
                "at_us": record.at_us,
                "requested_artifact": record.requested_artifact,
                "patch_id": record.patch_id,
                "status": record.status,
                "reject_reason": record.reject_reason,
                "load_ms": record.load_ms,
                "evidence_ms": record.evidence_ms,
                "apply_ms": record.apply_ms,
                "total_ms": record.total_ms,
                "error": record.error,
            })).collect::<Vec<Value>>(),
        })
    }

    fn swap_loop(
        host: Arc<BlockHost>,
        candidates: Vec<BuildOutput>,
        interval: Duration,
        stop: Arc<AtomicBool>,
        run_start: Instant,
    ) -> SwapOutcome {
        let mut outcome = SwapOutcome::default();
        let mut cursor = 0usize;
        loop {
            let deadline = Instant::now() + interval;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return outcome;
                }
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
            }
            let build = &candidates[cursor % candidates.len()];
            cursor += 1;
            let at_us = run_start.elapsed().as_micros() as u64;
            outcome.requested += 1;
            outcome.swap_times_us.push(at_us);
            let started = Instant::now();

            let prepared = match ravum_patch_host::prepare_block(
                Path::new(&build.artifact_path),
                &ExpectedAbi::dsp(),
            ) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    outcome.errors += 1;
                    outcome.records.push(SwapRecord {
                        at_us,
                        requested_artifact: build.artifact_path.clone(),
                        patch_id: None,
                        status: "error",
                        reject_reason: serde_json::to_value(&reason).ok(),
                        load_ms: 0,
                        evidence_ms: 0,
                        apply_ms: 0,
                        total_ms: started.elapsed().as_millis() as u64,
                        error: Some("prepare_block refused the candidate".to_owned()),
                    });
                    continue;
                }
            };
            let load_ms = prepared.load_ms;
            let evidence_started = Instant::now();
            let evidence = match ravum_dsp_harness::block_evidence(build) {
                Ok(evidence) => evidence,
                Err(reason) => {
                    outcome.errors += 1;
                    outcome.records.push(SwapRecord {
                        at_us,
                        requested_artifact: build.artifact_path.clone(),
                        patch_id: Some(prepared.module.manifest.patch_id.clone()),
                        status: "error",
                        reject_reason: serde_json::to_value(&reason).ok(),
                        load_ms,
                        evidence_ms: evidence_started.elapsed().as_millis() as u64,
                        apply_ms: 0,
                        total_ms: started.elapsed().as_millis() as u64,
                        error: Some("DSP evidence was refused; never activated".to_owned()),
                    });
                    continue;
                }
            };
            let evidence_ms = evidence_started.elapsed().as_millis() as u64;
            let patch_id = prepared.module.manifest.patch_id.clone();
            let apply_started = Instant::now();
            let report = host.apply(
                prepared,
                build,
                Some(&evidence),
                &build.source_snapshot_id,
            );
            let apply_ms = apply_started.elapsed().as_millis() as u64;
            match report.status {
                ravum_protocol::ActivationStatus::Activated => {
                    outcome.activations += 1;
                    outcome.final_patch_id = report.patch_id.clone();
                    outcome.records.push(SwapRecord {
                        at_us,
                        requested_artifact: build.artifact_path.clone(),
                        patch_id: report.patch_id.clone(),
                        status: "activated",
                        reject_reason: None,
                        load_ms,
                        evidence_ms,
                        apply_ms,
                        total_ms: started.elapsed().as_millis() as u64,
                        error: None,
                    });
                }
                ravum_protocol::ActivationStatus::Rejected => {
                    outcome.rejections += 1;
                    if outcome.final_patch_id.is_none() {
                        outcome.final_patch_id = Some(host.active_patch_id());
                    }
                    outcome.records.push(SwapRecord {
                        at_us,
                        requested_artifact: build.artifact_path.clone(),
                        patch_id: Some(patch_id),
                        status: "rejected",
                        reject_reason: report
                            .reject_reason
                            .as_ref()
                            .and_then(|reason| serde_json::to_value(reason).ok()),
                        load_ms,
                        evidence_ms,
                        apply_ms,
                        total_ms: started.elapsed().as_millis() as u64,
                        error: None,
                    });
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // Report assembly
    // -----------------------------------------------------------------------------------

    fn measurements_json(run: &RunData) -> Value {
        let fatal = run.fatal_stage.map(|stage| {
            json!({
                "stage": stage,
                "code": run.fatal_code,
                "code_name": run.fatal_code.map(mm_name),
            })
        });
        // Cadence measured between two observed completions, independent of the
        // device open time: this is what the callback really saw. `delivered_rate_hz`
        // is in completions per second (comparable to expected_completion_rate_hz);
        // `delivered_sample_rate_hz` converts it with the buffer size.
        let delivered = if run.completion_times_us.len() >= 2 {
            let first = run.completion_times_us[0];
            let last = run.completion_times_us[run.completion_times_us.len() - 1];
            let window_us = last.saturating_sub(first);
            let intervals = (run.completion_times_us.len() - 1) as f64;
            let delivered_rate_hz = if window_us > 0 {
                intervals * 1_000_000.0 / window_us as f64
            } else {
                0.0
            };
            json!({
                "completion_window_us": window_us,
                "delivered_rate_hz": delivered_rate_hz,
                "delivered_sample_rate_hz": delivered_rate_hz * run.frames as f64,
            })
        } else {
            json!({
                "completion_window_us": null,
                "delivered_rate_hz": null,
                "delivered_sample_rate_hz": null,
            })
        };
        json!({
            "period_us": run.period_us,
            "frames": run.frames,
            "rate": run.rate,
            "buffers": run.buffers,
            "warmup_completions": run.warmup_completions,
            "warmup_target_completions": run.warmup_target,
            "completions_measured": run.completions_measured,
            "blocks_processed": run.blocks_processed,
            "completion_rate_hz": run.completion_rate_hz,
            "expected_completion_rate_hz": run.expected_completion_rate_hz,
            "completion_window_us": delivered["completion_window_us"].clone(),
            "delivered_rate_hz": delivered["delivered_rate_hz"].clone(),
            "delivered_sample_rate_hz": delivered["delivered_sample_rate_hz"].clone(),
            "final_position_samples": run.final_position_samples,
            "open_prepare_us": run.open_prepare_us,
            "render_elapsed_us": run.render_elapsed_us,
            "work_us": stats_u64(&run.work_us),
            "callback_total_us": stats_u64(&run.callback_total_us),
            "wake_lateness_us": stats_i64(&run.wake_lateness_us),
            "completion_lateness_us": stats_i64(&run.completion_lateness_us),
            "interval_us": stats_u64(&run.interval_us),
            "queue_margin_us": stats_i64(&run.queue_margin_us),
            "position_drift_frames": if run.position_supported {
                stats_i64(&run.position_drift_frames)
            } else {
                Value::Null
            },
            "position_clock_supported": run.position_supported,
            "position_clock_error_code": run.position_error_code,
            "position_clock_error_name": run.position_error_code.map(mm_name),
            "first_completion_offset_us": run.first_completion_offset_us,
            "wakes": run.wakes,
            "wakes_without_completion": run.wakes_without_completion,
            "wait_timeouts": run.wait_timeouts,
            "wakes_with_multiple_done": run.wakes_with_multiple_done,
            "coalesced_completions": run.coalesced_completions,
            "buffers_late_gt_one_period": run.buffers_late_gt_one_period,
            "wakes_late_gt_one_period": run.wakes_late_gt_one_period,
            "queue_drain_risk_events": run.queue_drain_risk_events,
            "writes_attempted": run.writes_attempted,
            "write_failures": run.write_failures,
            "allocations_on_render_thread": run.allocations,
            "deallocations_on_render_thread": run.deallocations,
            "elapsed_us": run.elapsed_us,
            "wall_ms": run.elapsed_us / 1000,
            "fatal": fatal,
            "lateness_definition": "per wake with at least one completion: wake instant minus anchor + k*period, where anchor is the first measured completion after warm-up and k is the index of the oldest completion of the wake's batch. This removes the device start-up and first-event delivery bias; the device start-up cost is reported separately as first_completion_offset_us. waveOut delivers WOM_DONE in batches, so the value also contains the driver's event-delivery quantisation, not only OS scheduler jitter",
            "completion_lateness_definition": "per completion: wake instant minus anchor + k*period for that completion's index k; a batch of buffers observed in one wake yields one value per buffer (the newer ones are more negative by construction)",
            "queue_margin_definition": "(buffers-1)*period - lateness - callback_total; negative values are a derived queue-drain risk, not a measured XRUN",
        })
    }

    fn correlation_json(run: &RunData, swaps: &SwapOutcome, window_us: u64) -> Value {
        if swaps.requested == 0 {
            return Value::Null;
        }
        let mut swap_delta_max = 0f32;
        let mut steady_delta_max = 0f32;
        let mut swap_lateness_max = i64::MIN;
        let mut steady_lateness_max = i64::MIN;
        let mut swap_completions = 0u64;
        let mut steady_completions = 0u64;
        for (index, completion_time) in run.completion_times_us.iter().enumerate() {
            let in_swap_window = swaps
                .swap_times_us
                .iter()
                .any(|at| *completion_time >= *at && *completion_time <= at + window_us);
            let delta = run.completion_delta.get(index).copied().unwrap_or(0.0);
            let lateness = run.completion_lateness_us.get(index).copied().unwrap_or(0);
            if in_swap_window {
                swap_completions += 1;
                swap_delta_max = swap_delta_max.max(delta);
                swap_lateness_max = swap_lateness_max.max(lateness);
            } else {
                steady_completions += 1;
                steady_delta_max = steady_delta_max.max(delta);
                steady_lateness_max = steady_lateness_max.max(lateness);
            }
        }
        json!({
            "definition": "completions in [swap_time, swap_time + window_us] are attributed to the swap (publication + crossfade); the others are steady-state",
            "window_us": window_us,
            "swap_completions": swap_completions,
            "steady_completions": steady_completions,
            "max_sample_delta_at_swaps": swap_delta_max,
            "max_sample_delta_steady": steady_delta_max,
            "max_lateness_at_swaps_us": if swap_completions > 0 { Value::from(swap_lateness_max) } else { Value::Null },
            "max_lateness_steady_us": if steady_completions > 0 { Value::from(steady_lateness_max) } else { Value::Null },
        })
    }

    fn aggregates_json(devices: &[Value], device_errors: &[Value]) -> Value {
        json!({
            "num_devs": devices.len(),
            "devices": devices,
            "caps_errors": device_errors,
        })
    }

    fn failure_report(
        config: &Config,
        os: Value,
        stage: &str,
        failure: &Failure,
        status: &str,
    ) -> (Value, i32) {
        let mut error = failure.to_json();
        if let Some(object) = error.as_object_mut() {
            object.insert("stage".to_owned(), Value::from(stage));
        }
        let report = json!({
            "schema_version": 1,
            "kind": "audio-device-probe",
            "tool": TOOL,
            "tool_version": TOOL_VERSION,
            "status": status,
            "os": os,
            "config": config_json(config),
            "realtime": {
                "mmcss_requested": config.mmcss,
                "mmcss_acquired": false,
                "mmcss_priority": Value::Null,
                "mmcss_priority_set": false,
                "thread_priority_requested": config.thread_priority.map(|choice| choice.as_str()),
                "thread_priority_set": false,
                "thread_priority_observed": Value::Null,
                "avrt_error": Value::Null,
                "win32_error": Value::Null,
                "notes": ["thread priority was not applied: the probe failed before the render thread could set it"],
            },
            "error": error,
            "limits": limits_json(config),
        });
        (report, 1)
    }

    pub fn execute(config: &Config, os: Value) -> (Value, i32) {
        let mut report = json!({
            "schema_version": 1,
            "kind": "audio-device-probe",
            "tool": TOOL,
            "tool_version": TOOL_VERSION,
            "status": "running",
            "os": os,
            "config": config_json(config),
        });

        // 1) Enumerate waveOut devices (metadata only, no device opened yet).
        // SAFETY: trivial documented winmm query, no handle involved.
        let num_devs = unsafe { waveOutGetNumDevs() };
        let mut devices = Vec::new();
        let mut caps_errors = Vec::new();
        for index in 0..num_devs {
            match read_caps(index) {
                Ok(caps) => devices.push(caps_json(index, &caps)),
                Err(failure) => caps_errors.push(json!({
                    "index": index,
                    "error": failure.to_json(),
                })),
            }
        }
        report["devices"] = aggregates_json(&devices, &caps_errors);

        if let Some(requested) = config.device {
            if requested >= num_devs {
                let failure = Failure::new(
                    "device-index",
                    None,
                    format!(
                        "--device {requested} is out of range: waveOutGetNumDevs() = {num_devs}"
                    ),
                );
                return failure_report(config, os_info(), "device-index", &failure, "error");
            }
        }
        if num_devs == 0 {
            let failure = Failure::new(
                "waveOutGetNumDevs",
                None,
                "no waveOut device is available on this machine",
            );
            return failure_report(config, os_info(), "waveOutGetNumDevs", &failure, "error");
        }

        // 2) Bootstrap the block host from a prebuilt module, if one was given.
        let host: Option<Arc<BlockHost>> = match &config.module {
            Some(path) => match BlockHost::bootstrap_with_crossfade(
                ExpectedAbi::dsp(),
                Path::new(path),
                config.crossfade_frames,
            ) {
                Ok(host) => Some(Arc::new(host)),
                Err(reason) => {
                    let failure = Failure::new(
                        "BlockHost::bootstrap",
                        None,
                        format!("the DSP module was refused: {reason:?}"),
                    );
                    let mut report_failure =
                        failure_report(config, os_info(), "BlockHost::bootstrap", &failure, "error");
                    if let Some(object) = report_failure.0.as_object_mut() {
                        object.insert(
                            "module_refusal".to_owned(),
                            serde_json::to_value(&reason).unwrap_or(Value::Null),
                        );
                    }
                    return report_failure;
                }
            },
            None => None,
        };

        // Identity of the module as bootstrapped, before any hot swap: the final
        // active identity is read again after the run, so both are reported apart
        // instead of attributing the bootstrap path to the last swapped artifact.
        let bootstrap_identity = host.as_ref().map(|host| {
            (
                host.active_artifact(),
                host.active_patch_id(),
                host.active_verification().as_str().to_owned(),
            )
        });

        // 3) Load the hot-swap candidates (the JSON emitted by `ravum build --json`).
        let mut candidates: Vec<BuildOutput> = Vec::new();
        for path in &config.swap_builds {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) => {
                    let failure = Failure::new(
                        "swap-build",
                        None,
                        format!("cannot read {path}: {error}"),
                    );
                    return failure_report(config, os_info(), "swap-build", &failure, "error");
                }
            };
            // Tolerate a UTF-8 BOM: PowerShell 5.1 writes files with one by default.
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            match serde_json::from_str::<BuildOutput>(text) {
                Ok(build) => candidates.push(build),
                Err(error) => {
                    let failure = Failure::new(
                        "swap-build",
                        None,
                        format!("{path} is not a BuildOutput JSON: {error}"),
                    );
                    return failure_report(config, os_info(), "swap-build", &failure, "error");
                }
            }
        }

        // 4) Precompute the continuous test signal and the theoretical baseline delta.
        let total_input_frames = (config.seconds as usize)
            .saturating_mul(config.rate as usize)
            .saturating_add(config.rate as usize)
            .saturating_add(config.frames * (config.buffers + 8));
        let omega = 2.0 * std::f32::consts::PI * SINE_FREQUENCY_HZ / config.rate.max(1) as f32;
        let mut input = Vec::with_capacity(total_input_frames);
        for index in 0..total_input_frames {
            input.push(SINE_AMPLITUDE * (omega * index as f32).sin());
        }
        let baseline_delta = 2.0 * SINE_AMPLITUDE * (omega / 2.0).sin().abs();

        // 5) Run: one render thread owns the device, one control thread hot-swaps,
        //    the main thread only waits and then asks for a clean stop.
        let run_start = Instant::now();
        let stop = Arc::new(AtomicBool::new(false));
        let (open_tx, open_rx) = std::sync::mpsc::channel();
        let render_config = config.clone();
        let render_host = host.clone();
        let render_stop = Arc::clone(&stop);
        let capacity =
            ((config.seconds as usize).saturating_mul(config.rate as usize) / config.frames) + 4 * config.buffers + 128;
        let render_handle = std::thread::spawn(move || {
            let thread_start = Instant::now();
            // 5a) Scheduling characteristics of THIS thread, before any measurement:
            //     MMCSS first, raw Win32 priority second. A request that cannot be
            //     applied is a hard failure, never a silent no-op.
            let mut realtime = match RealtimeState::apply(&render_config) {
                Ok(state) => state,
                Err((failure, partial)) => {
                    let _ = open_tx.send(Err((failure, partial)));
                    return Err(Failure::new(
                        "realtime",
                        None,
                        "thread priority setup failed",
                    ));
                }
            };
            let mut session = match Session::open(&render_config) {
                Ok(session) => session,
                Err(failure) => {
                    let realtime_json = realtime.to_json();
                    let _ = open_tx.send(Err((failure, realtime_json)));
                    let _ = realtime.revert();
                    return Err(Failure::new("open", None, "device not opened"));
                }
            };
            if let Err(failure) = session.prepare() {
                let realtime_json = realtime.to_json();
                let _ = open_tx.send(Err((failure, realtime_json)));
                let _ = realtime.revert();
                return Err(Failure::new("prepare", None, "buffers not prepared"));
            }
            let _ = open_tx.send(Ok((session.info_json(), realtime.to_json())));
            let run = session.render(
                render_host,
                &render_stop,
                &input,
                run_start,
                thread_start,
                capacity,
                baseline_delta,
            );
            let cleanup = session.close();
            // Restore the thread characteristics explicitly before the thread ends.
            let revert = realtime.revert();
            Ok((run, cleanup, revert))
        });

        let (open_info, realtime_info) = match open_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok((info, realtime))) => (info, realtime),
            Ok(Err((failure, realtime))) => {
                stop.store(true, Ordering::SeqCst);
                let _ = render_handle.join();
                let (mut failed_report, code) =
                    failure_report(config, os_info(), failure.stage, &failure, "error");
                failed_report["realtime"] = realtime;
                return (failed_report, code);
            }
            Err(error) => {
                stop.store(true, Ordering::SeqCst);
                let _ = render_handle.join();
                let failure = Failure::new(
                    "waveOutOpen",
                    None,
                    format!("no open result from the render thread: {error}"),
                );
                return failure_report(config, os_info(), "waveOutOpen", &failure, "error");
            }
        };
        // Load sampling window: from the confirmed device open to the end of the joins.
        let load_window_started = Instant::now();
        let load_start = system_sample();
        report["realtime"] = realtime_info.clone();
        report["device"] = open_info["device"].clone();
        report["requested_device"] = open_info["requested_device"].clone();
        report["format"] = open_info["format"].clone();
        report["timer_period_ms"] = open_info["timer_period_ms"].clone();
        report["timer_begin_period_ok"] = open_info["timer_begin_period_ok"].clone();

        let swap_handle = if config.swap_every_ms > 0 && !candidates.is_empty() {
            let swap_host = host.clone();
            let swap_stop = Arc::clone(&stop);
            let interval = Duration::from_millis(config.swap_every_ms);
            Some(std::thread::spawn(move || {
                swap_loop(
                    swap_host.expect("the host is bootstrapped when swaps are requested"),
                    candidates,
                    interval,
                    swap_stop,
                    run_start,
                )
            }))
        } else {
            None
        };

        // The probe runs for the requested duration; the render thread stops at the
        // next completion (<= one period + catch-up).
        std::thread::sleep(Duration::from_secs(config.seconds));
        stop.store(true, Ordering::SeqCst);

        let render_result = render_handle.join();
        let swap_outcome = swap_handle.map(|handle| handle.join());
        let swap_outcome = match swap_outcome {
            Some(Ok(outcome)) => outcome,
            Some(Err(_)) => {
                // The audio thread already produced usable data; a panic in the
                // control thread is reported, never silently ignored.
                report["swap_thread_panicked"] = Value::Bool(true);
                SwapOutcome::default()
            }
            None => SwapOutcome::default(),
        };

        let (run, cleanup, revert) = match render_result {
            Ok(Ok((run, cleanup, revert))) => (run, cleanup, revert),
            Ok(Err(_failure)) => {
                // Should be unreachable: open/prepare failures go through the channel.
                let failure =
                    Failure::new("render-thread", None, "render thread failed after open");
                let (mut failed_report, code) =
                    failure_report(config, os_info(), "render-thread", &failure, "error");
                failed_report["realtime"] = realtime_info.clone();
                return (failed_report, code);
            }
            Err(_) => {
                let failure = Failure::new(
                    "render-thread",
                    None,
                    "render thread panicked; no measurement is reported",
                );
                let (mut failed_report, code) =
                    failure_report(config, os_info(), "render-thread", &failure, "error");
                failed_report["realtime"] = realtime_info.clone();
                return (failed_report, code);
            }
        };

        // Attach the MMCSS revert result and the machine load measured over the run.
        if let Some(realtime) = report.get_mut("realtime").and_then(Value::as_object_mut) {
            realtime.insert("revert".to_owned(), revert);
        }
        let load_window_ms = load_window_started.elapsed().as_millis() as u64;
        let load_end = system_sample();
        report["load"] = system_load_json(
            load_start,
            load_end,
            load_window_ms,
            std::env::var("RAVUM_PROBE_LOAD_NOTE").ok(),
        );

        let fatal = run.fatal_stage.is_some();
        let status = if fatal { "partial" } else if run.completions_measured == 0 {
            "error"
        } else {
            "ok"
        };

        if let Some(host) = &host {
            let (bootstrap_artifact, bootstrap_patch, bootstrap_verification) = bootstrap_identity
                .as_ref()
                .cloned()
                .unwrap_or_else(|| (host.active_artifact(), host.active_patch_id(), String::new()));
            report["module"] = json!({
                "provided": true,
                "bootstrap_path": config.module,
                "bootstrap_artifact_id": bootstrap_artifact,
                "bootstrap_patch_id": bootstrap_patch,
                "bootstrap_verification": bootstrap_verification,
                "final_artifact_id": host.active_artifact(),
                "final_patch_id": host.active_patch_id(),
                "crossfade_frames": host.crossfade_frames(),
            });
        } else {
            report["module"] = json!({"provided": false});
        }

        let measurements = measurements_json(&run);
        let delivered = measurements["delivered_rate_hz"].as_f64().unwrap_or(0.0);
        let expected = measurements["expected_completion_rate_hz"]
            .as_f64()
            .unwrap_or(0.0);
        let ratio = if expected > 0.0 {
            delivered / expected
        } else {
            0.0
        };
        report["realtime_cadence"] = json!({
            "delivered_completions_per_s": delivered,
            "expected_completions_per_s": expected,
            "ratio": ratio,
            "held_real_time": ratio >= 0.95,
            "definition": "ratio = observed buffer-completion cadence / nominal cadence (rate/frames). A ratio well below 1 means the waveOut path consumed the stream slower than real time and the playback queue drained; this is a driver/path property observed from this API, not an XRUN count",
        });
        report["measurements"] = measurements;
        report["swaps"] = swap_json(&swap_outcome);
        let crossfade_us = (config.crossfade_frames as u64) * 1_000_000 / config.rate.max(1) as u64;
        let window_us = crossfade_us + 2 * run.period_us;
        report["correlation"] = correlation_json(&run, &swap_outcome, window_us);
        if let Some(host) = &host {
            let status = host.status();
            report["host"] = json!({
                "activations": status.activations,
                "unloaded_modules": status.unloaded_modules,
                "live_modules": status.live_modules,
                "final_active_version": status.active_version,
            });
        }
        report["dsp"] = json!({
            "bootstrap_module": config.module,
            "signal": "mono 440 Hz sine, amplitude 0.5, continuous phase across blocks; computed, measured, never audible",
            "max_sample_delta": run.max_sample_delta,
            "baseline_sample_delta": run.baseline_sample_delta,
            "non_finite_samples": run.non_finite_samples,
            "input_wraps": run.input_wraps,
        });
        report["cleanup"] = Value::from(cleanup);
        report["status"] = Value::from(status);
        report["limits"] = limits_json(config);

        let exit_code = if status == "ok" { 0 } else { 1 };
        (report, exit_code)
    }
}
