//! Block-session measurement: block wall time, overruns, allocations and
//! sample-to-sample discontinuity.
//!
//! The "period" is simulated (frames / sample_rate). This is **not** an audio device:
//! no driver, no buffer underrun, no real-time priority. Every number below is
//! labelled accordingly in the report.

use ravum_protocol::{DspBlockTimings, DspSessionReport, DspState};

use crate::alloc::{allocations_on_this_thread, reset_thread_counter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionConfig {
    pub blocks: u64,
    pub frames: usize,
    pub sample_rate: u32,
    /// Amplitude of the generated sine, in [-1, 1].
    pub amplitude: f32,
    pub frequency_hz: f32,
    /// Index of the first generated sample; lets two sessions form one continuous
    /// signal instead of restarting the phase at the swap boundary.
    pub start_sample: u64,
    /// Sleep between blocks to emulate a real-time cadence. `0` = run as fast as
    /// possible (measures capacity, not scheduling).
    pub period_sleep_us: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            blocks: 200,
            frames: 256,
            sample_rate: 48_000,
            amplitude: 0.5,
            frequency_hz: 440.0,
            start_sample: 0,
            period_sleep_us: 0,
        }
    }
}

impl SessionConfig {
    pub fn period_us(&self) -> u64 {
        if self.sample_rate == 0 {
            0
        } else {
            (self.frames as u64 * 1_000_000) / self.sample_rate as u64
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionMeasurement {
    pub blocks: u64,
    pub frames: u64,
    pub period_us: u64,
    pub timings: DspBlockTimings,
    pub overruns: u64,
    pub allocations: u64,
    pub max_sample_delta: f32,
    pub baseline_sample_delta: f32,
}

impl SessionMeasurement {
    pub fn into_report(
        self,
        active_patch_id: impl Into<String>,
        swaps: u64,
        unloaded_modules: u64,
    ) -> DspSessionReport {
        DspSessionReport {
            schema_version: 1,
            blocks: self.blocks,
            frames: self.frames,
            period_us: self.period_us,
            block_timings: self.timings,
            overruns: self.overruns,
            allocations_on_audio_thread: self.allocations,
            max_sample_delta: self.max_sample_delta,
            baseline_sample_delta: self.baseline_sample_delta,
            swaps,
            active_patch_id: active_patch_id.into(),
            unloaded_modules,
        }
    }
}

/// Result of a measured session, with the state and the last sample so a caller can
/// chain two sessions and measure the discontinuity **at the swap boundary**.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionOutcome {
    pub measurement: SessionMeasurement,
    pub final_state: DspState,
    pub last_sample: f32,
}

/// Run a measured session from a zeroed state.
pub fn measure<F>(config: SessionConfig, process: F) -> SessionMeasurement
where
    F: FnMut(&[f32], &mut [f32], &mut DspState),
{
    measure_from(config, DspState::default(), 0.0, process).measurement
}

/// Run a measured session from an existing state and previous output sample.
///
/// `process` is called once per block with the generated input; it must be the
/// **audio path under test** (it should not allocate, and the counter will tell).
pub fn measure_from<F>(
    config: SessionConfig,
    initial_state: DspState,
    previous_output_sample: f32,
    mut process: F,
) -> SessionOutcome
where
    F: FnMut(&[f32], &mut [f32], &mut DspState),
{
    let frames = config.frames.max(1);
    let mut input = vec![0f32; frames];
    let mut output = vec![0f32; frames];
    let mut state = initial_state;
    let mut samples: Vec<u64> = Vec::with_capacity(config.blocks as usize);
    let mut allocations = 0u64;
    let mut overruns = 0u64;
    let mut max_delta = 0f32;
    let mut previous_sample = previous_output_sample;
    let period_us = config.period_us();

    for block in 0..config.blocks {
        fill_sine(&mut input, block, &config);
        reset_thread_counter();
        let started = std::time::Instant::now();
        process(&input, &mut output, &mut state);
        let elapsed_us = started.elapsed().as_micros() as u64;
        allocations = allocations.saturating_add(allocations_on_this_thread());
        samples.push(elapsed_us);
        if period_us > 0 && elapsed_us > period_us {
            overruns += 1;
        }
        for sample in output.iter() {
            if !sample.is_finite() {
                max_delta = f32::INFINITY;
                continue;
            }
            let delta = (sample - previous_sample).abs();
            if delta > max_delta {
                max_delta = delta;
            }
            previous_sample = *sample;
        }
        if config.period_sleep_us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(config.period_sleep_us));
        }
    }

    let timings = summarize(&samples);
    let baseline = baseline_delta(&config);
    SessionOutcome {
        measurement: SessionMeasurement {
            blocks: config.blocks,
            frames: config.blocks.saturating_mul(frames as u64),
            period_us,
            timings,
            overruns,
            allocations,
            max_sample_delta: max_delta,
            baseline_sample_delta: baseline,
        },
        final_state: state,
        last_sample: previous_sample,
    }
}

fn fill_sine(buffer: &mut [f32], block: u64, config: &SessionConfig) {
    let omega = 2.0 * std::f32::consts::PI * config.frequency_hz / config.sample_rate.max(1) as f32;
    let offset = config
        .start_sample
        .saturating_add(block.saturating_mul(buffer.len() as u64)) as f32;
    for (index, sample) in buffer.iter_mut().enumerate() {
        *sample = config.amplitude * (omega * (offset + index as f32)).sin();
    }
}

/// Largest sample-to-sample delta of the *input* sine: the reference below which an
/// output delta is not a discontinuity.
///
/// For `A·sin(ωn)`, the max difference between consecutive samples is
/// `2·A·sin(ω/2)`.
fn baseline_delta(config: &SessionConfig) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * config.frequency_hz / config.sample_rate.max(1) as f32;
    2.0 * config.amplitude * (omega / 2.0).sin().abs()
}

fn summarize(samples: &[u64]) -> DspBlockTimings {
    if samples.is_empty() {
        return DspBlockTimings::default();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    DspBlockTimings {
        count: sorted.len() as u64,
        p50_us: nearest_rank(&sorted, 50.0),
        p95_us: nearest_rank(&sorted, 95.0),
        p99_us: nearest_rank(&sorted, 99.0),
        max_us: *sorted.last().unwrap_or(&0),
    }
}

fn nearest_rank(sorted: &[u64], percentile: f64) -> u64 {
    let rank = (percentile / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_noop_processor_is_measured_and_allocates_nothing() {
        let config = SessionConfig {
            blocks: 20,
            frames: 64,
            ..SessionConfig::default()
        };
        let measurement = measure(config, |input, output, _state| {
            output.copy_from_slice(input);
        });
        assert_eq!(measurement.blocks, 20);
        assert_eq!(measurement.frames, 20 * 64);
        assert_eq!(
            measurement.allocations, 0,
            "copying a block must not allocate"
        );
        assert_eq!(measurement.timings.count, 20);
        // A pass-through cannot exceed the input's own sample-to-sample delta.
        assert!(
            measurement.max_sample_delta <= measurement.baseline_sample_delta * 1.01,
            "delta {} above baseline {}",
            measurement.max_sample_delta,
            measurement.baseline_sample_delta
        );
    }

    #[test]
    fn chaining_sessions_preserves_state_and_exposes_the_boundary_delta() {
        let config = SessionConfig {
            blocks: 5,
            frames: 32,
            ..SessionConfig::default()
        };
        let first = measure_from(config, DspState::default(), 0.0, |input, output, _state| {
            output.copy_from_slice(input);
        });
        assert_eq!(first.final_state, DspState::default());
        let continued = SessionConfig {
            start_sample: config.blocks * config.frames as u64,
            ..config
        };
        let second = measure_from(continued, first.final_state, first.last_sample, |input, output, _state| {
            output.copy_from_slice(input);
        });
        // The same signal continues, so the boundary is not a discontinuity.
        assert!(
            second.measurement.max_sample_delta <= second.measurement.baseline_sample_delta * 1.01
        );
    }

    #[test]
    fn a_discontinuity_shows_up_above_the_baseline() {
        let config = SessionConfig {
            blocks: 4,
            frames: 32,
            amplitude: 0.5,
            ..SessionConfig::default()
        };
        let measurement = measure(config, |input, output, _state| {
            for (i, sample) in input.iter().enumerate() {
                output[i] = if i == 16 { 0.9 } else { *sample };
            }
        });
        assert!(measurement.max_sample_delta > measurement.baseline_sample_delta);
        assert!(measurement.max_sample_delta > 0.5);
    }

    #[test]
    fn period_is_derived_from_frames_and_rate() {
        let config = SessionConfig {
            frames: 256,
            sample_rate: 48_000,
            ..SessionConfig::default()
        };
        assert_eq!(config.period_us(), 5333);
    }
}
