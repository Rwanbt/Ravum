//! `CpalHost`: the default output device (feature `cpal`, off by default).
//!
//! NOT TESTED in CI (no audio device). The callback only calls
//! [`Processor::process`] on a preallocated scratch buffer sized for the
//! largest block seen at start; a larger device block is processed in
//! chunks, never by allocating.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::Processor;

/// A running output stream; dropping it stops the audio.
pub struct CpalHost {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    pub channels: u16,
}

impl CpalHost {
    /// Open the default output device and start `processor` (mono, copied
    /// to every channel, silence as input).
    /// Stack budget given to compiled code on the device callback thread:
    /// that thread belongs to the backend (ALSA: a Rust thread, 2 MiB;
    /// CoreAudio/WASAPI: an OS thread, possibly ~512 KiB).
    pub const CALLBACK_STACK_BUDGET: usize = 256 << 10;

    /// Open the default output device and start a JIT runner, with the
    /// conservative [`Self::CALLBACK_STACK_BUDGET`].
    pub fn start_runner(mut runner: ravum_lang_jit::hot::RtRunner) -> Result<CpalHost, String> {
        runner.set_stack_budget(Self::CALLBACK_STACK_BUDGET);
        Self::start(Box::new(runner))
    }

    pub fn start(mut processor: Box<dyn Processor>) -> Result<CpalHost, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "output format {:?} is not f32 (only f32 is supported)",
                config.sample_format()
            ));
        }
        let channels = config.channels();
        let sample_rate = config.sample_rate().0;
        const CHUNK: usize = 256;
        let silence = [0.0f32; CHUNK];
        let mut mono = [0.0f32; CHUNK];
        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    let ch = channels as usize;
                    for frame_chunk in data.chunks_mut(CHUNK * ch) {
                        let n = frame_chunk.len() / ch;
                        processor.process(&silence[..n], &mut mono[..n]);
                        for (i, frame) in frame_chunk.chunks_mut(ch).enumerate() {
                            frame.fill(mono[i]);
                        }
                    }
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(CpalHost {
            _stream: stream,
            sample_rate,
            channels,
        })
    }
}
