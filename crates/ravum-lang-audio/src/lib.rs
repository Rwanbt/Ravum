//! Audio host interface (V1-A1): a [`Processor`] turns input blocks into
//! output blocks on the audio thread; a host drives it. Two hosts:
//!
//! - [`WavHost`]: offline, renders blocks into a 16-bit PCM WAV file — the
//!   host tested in CI (no device needed);
//! - `CpalHost` (feature `cpal`, off by default): the default output
//!   device. **Not tested here** (no audio device in the container).
//!
//! INV-05: [`Processor::process`] runs on the audio thread. The
//! [`RtRunner`] implementation only polls the mailbox (lock-free swap) once
//! per block and calls the compiled entry per sample: no compilation,
//! allocation, lock or I/O (checked by `tests/wav.rs` with a counting
//! allocator). Compilation and migration stay on the editor side
//! ([`ravum_lang_jit::hot::HotEngine`]).

#![forbid(unsafe_code)]

use std::io::Write;
use std::path::Path;

use ravum_lang_jit::hot::{EntryKind, RtRunner};

/// Block processing on the audio thread. Mono; `input.len() == output.len()`.
pub trait Processor: Send {
    fn process(&mut self, input: &[f32], output: &mut [f32]);
}

impl Processor for RtRunner {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        // Safepoint: a published version is adopted between blocks only.
        self.poll();
        match self.kind() {
            EntryKind::DspF32 => {
                for (o, &x) in output.iter_mut().zip(input) {
                    *o = self.process_sample(x);
                }
            }
            EntryKind::F32ToF32 => {
                for (o, &x) in output.iter_mut().zip(input) {
                    *o = self.process_f32(x);
                }
            }
            EntryKind::F64ToF64 => {
                for (o, &x) in output.iter_mut().zip(input) {
                    *o = self.process_f64(x as f64) as f32;
                }
            }
            // A no-argument entry is an editor-side run, never audio work:
            // silence.
            EntryKind::Nullary(_) => output.fill(0.0),
        }
    }
}

/// Stream parameters shared by the hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    pub sample_rate: u32,
    pub block: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        StreamConfig {
            sample_rate: 48_000,
            block: 64,
        }
    }
}

/// Offline host: feeds `input` block by block to a processor and keeps the
/// output. Buffers are allocated once, before processing starts.
pub struct WavHost {
    pub config: StreamConfig,
    rendered: Vec<f32>,
}

impl WavHost {
    pub fn new(config: StreamConfig) -> Self {
        assert!(config.block > 0, "block size");
        WavHost {
            config,
            rendered: Vec::new(),
        }
    }

    /// Process `input` (padded with silence to whole blocks). `between`
    /// runs on the host side after each block (e.g. an editor-side hot
    /// update); it is the only place where work other than processing may
    /// happen.
    pub fn render(
        &mut self,
        processor: &mut dyn Processor,
        input: &[f32],
        mut between: impl FnMut(usize),
    ) -> &[f32] {
        let b = self.config.block;
        let blocks = input.len().div_ceil(b);
        let mut inbuf = vec![0.0f32; b];
        self.rendered = vec![0.0f32; blocks * b];
        for k in 0..blocks {
            inbuf.fill(0.0);
            let src = &input[k * b..((k + 1) * b).min(input.len())];
            inbuf[..src.len()].copy_from_slice(src);
            processor.process(&inbuf, &mut self.rendered[k * b..(k + 1) * b]);
            between(k);
        }
        &self.rendered
    }

    pub fn rendered(&self) -> &[f32] {
        &self.rendered
    }

    /// Write the last render as mono 16-bit PCM WAV (samples clamped to
    /// [-1, 1]).
    pub fn write_wav(&self, path: &Path) -> std::io::Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        f.write_all(&wav_bytes(&self.rendered, self.config.sample_rate))?;
        f.flush()
    }
}

/// A mono 16-bit PCM WAV file holding `samples`.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(feature = "cpal")]
pub mod device;
