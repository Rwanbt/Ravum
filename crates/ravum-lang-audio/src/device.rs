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
    rt: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// Outcome of the real-time promotion of the callback thread
/// (feature `rt-priority`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtStatus {
    /// Feature off: the thread keeps the backend's policy.
    Disabled,
    /// No callback has run yet.
    Pending,
    /// The OS granted real-time scheduling.
    Promoted,
    /// The OS refused (no RTKit, no permission...): audio still runs,
    /// at normal priority. Never reported as promoted.
    Refused,
}

const RT_DISABLED: u8 = 0;
#[cfg_attr(not(feature = "rt-priority"), allow(dead_code))]
const RT_PENDING: u8 = 1;
#[cfg_attr(not(feature = "rt-priority"), allow(dead_code))]
const RT_PROMOTED: u8 = 2;
#[cfg_attr(not(feature = "rt-priority"), allow(dead_code))]
const RT_REFUSED: u8 = 3;

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
        let initial = if cfg!(feature = "rt-priority") {
            RT_PENDING
        } else {
            RT_DISABLED
        };
        let rt = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(initial));
        let rt_cb = rt.clone();
        // Promotion happens ONCE, on the first callback, before the first
        // block: it is the only point that runs on the backend's thread. It
        // may talk to RTKit over D-Bus (allocates, blocks): a documented
        // one-time exception to INV-05, before any audio is produced. The
        // handle is kept for the life of the stream.
        #[cfg(feature = "rt-priority")]
        let mut rt_handle: Option<Option<audio_thread_priority::RtPriorityHandle>> = None;
        let stream =
            device
                .build_output_stream(
                    &config.into(),
                    move |data: &mut [f32], _| {
                        #[cfg(feature = "rt-priority")]
                        if rt_handle.is_none() {
                            let frames = (data.len() / channels.max(1) as usize) as u32;
                            let state =
                                match audio_thread_priority::promote_current_thread_to_real_time(
                                    frames,
                                    sample_rate,
                                ) {
                                    Ok(h) => {
                                        rt_handle = Some(Some(h));
                                        RT_PROMOTED
                                    }
                                    Err(_) => {
                                        rt_handle = Some(None);
                                        RT_REFUSED
                                    }
                                };
                            rt_cb.store(state, std::sync::atomic::Ordering::Release);
                        }
                        #[cfg(not(feature = "rt-priority"))]
                        let _ = &rt_cb;
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
            rt,
        })
    }

    /// Real-time status of the callback thread (see [`RtStatus`]).
    pub fn rt_status(&self) -> RtStatus {
        match self.rt.load(std::sync::atomic::Ordering::Acquire) {
            RT_DISABLED => RtStatus::Disabled,
            RT_PENDING => RtStatus::Pending,
            RT_PROMOTED => RtStatus::Promoted,
            _ => RtStatus::Refused,
        }
    }
}
