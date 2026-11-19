//! Deterministic replay of a hot-swap session (gate T17, first slice).
//!
//! A live session is not reproducible by re-running it: *when* the real-time
//! thread adopts a version depends on thread timing. What is reproducible is
//! the session's **log**: the input samples, the starting module, and, for
//! every version the runner adopted, the IR module and the index of the first
//! sample it processed. Replaying that log offline drives the same
//! [`HotEngine`]/[`RtRunner`] pair (same rollback rule HR-1.5, same state
//! handling), so it must give the live output **bit for bit**.
//!
//! Model and limits (stated, not hidden):
//! * the entry is a stateless `fn(f32) -> f32` or the DSP block
//!   `process_sample(&mut State, f32) -> f32`;
//! * a version published but replaced before the runner took it never ran and
//!   is not in the log;
//! * determinism rests on the IR semantics (no I/O allowed on a real-time
//!   entry, no clock, no randomness) and on Cranelift emitting IEEE-754 code
//!   without fast-math; it is checked by tests on one platform, not proved.

use ravum_lang_ir::ir::Module;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hot::{HotEngine, Refusal};

/// Log format version; a reader refuses any other.
pub const LOG_VERSION: &str = "ravum-hot-session-1";

/// One version adopted by the runner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Swap {
    /// Index of the first input sample processed by this version.
    pub at_sample: u64,
    pub module: Module,
}

/// Everything needed to reproduce a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionLog {
    pub version: String,
    pub entry: String,
    pub inputs: Vec<f32>,
    pub start: Module,
    /// Strictly increasing `at_sample`.
    pub swaps: Vec<Swap>,
}

impl SessionLog {
    pub fn new(entry: &str, start: Module) -> Self {
        SessionLog {
            version: LOG_VERSION.into(),
            entry: entry.into(),
            inputs: Vec::new(),
            start,
            swaps: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a session log always serializes")
    }

    /// Parse a log; an unknown format version is refused (fail-closed).
    pub fn from_json(text: &str) -> Result<SessionLog, String> {
        let log: SessionLog = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if log.version != LOG_VERSION {
            return Err(format!("unknown session log version `{}`", log.version));
        }
        Ok(log)
    }
}

/// Why a log cannot be replayed.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayError {
    /// The log itself is inconsistent (unordered or out-of-range swap).
    BadLog(String),
    /// A version is refused exactly as it would be live.
    Refused { swap: usize, refusal: String },
}

/// Replay `log` and return one output per input sample.
pub fn replay(log: &SessionLog) -> Result<Vec<f32>, ReplayError> {
    let n = log.inputs.len() as u64;
    let mut last = 0u64;
    for (k, s) in log.swaps.iter().enumerate() {
        if (k > 0 && s.at_sample <= last) || s.at_sample > n {
            return Err(ReplayError::BadLog(format!(
                "swap {k} at sample {} (previous {last}, {n} inputs)",
                s.at_sample
            )));
        }
        last = s.at_sample;
    }
    let refused = |swap: usize, r: Refusal| ReplayError::Refused {
        swap,
        refusal: r.to_string(),
    };
    let (mut engine, mut rt) =
        HotEngine::start(&log.start, &log.entry).map_err(|r| refused(0, r))?;
    let dsp = log.start.state.is_some();
    let mut out = Vec::with_capacity(log.inputs.len());
    let mut next = 0;
    for (i, x) in log.inputs.iter().enumerate() {
        while next < log.swaps.len() && log.swaps[next].at_sample == i as u64 {
            engine
                .update(&log.swaps[next].module)
                .map_err(|r| refused(next + 1, r))?;
            engine.collect();
            if !rt.poll() {
                return Err(ReplayError::BadLog(format!(
                    "swap {next}: version not adoptable"
                )));
            }
            engine.collect();
            next += 1;
        }
        out.push(if dsp {
            rt.process_sample(*x)
        } else {
            rt.process_f32(*x)
        });
    }
    Ok(out)
}

/// SHA-256 of the output bits (little-endian), as hex.
pub fn digest(out: &[f32]) -> String {
    let mut h = Sha256::new();
    for v in out {
        h.update(v.to_bits().to_le_bytes());
    }
    h.finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}
