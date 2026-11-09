//! Verifiers: what can actually be checked, and how failure is reported.
//!
//! Two backends ship with M2:
//! * `ExhaustiveVerifier` — enumerates the obligation's declared finite domain on
//!   the real artifact. It can honestly produce `Proved` **for that domain only**.
//! * `KaniVerifier` — detects an external prover and refuses to claim anything
//!   without a generated harness. Absence of a prover is `Unsupported`, never a
//!   silent success.

use crate::obligation::{Claim, Obligation};
use ravum_patch_host::{prepare, ExpectedAbi};
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierOutcome {
    /// The property holds for the whole declared domain.
    Proved,
    /// A concrete input violates the property.
    Failed { counterexample: String },
    /// This backend cannot decide this obligation. Never counts as success.
    Unsupported { detail: String },
    /// The budget was exhausted before the domain was covered.
    Timeout { after_ms: u64 },
    /// The backend errored, or the artifact could not be inspected.
    Unknown { detail: String },
}

impl fmt::Display for VerifierOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifierOutcome::Proved => write!(f, "proved"),
            VerifierOutcome::Failed { counterexample } => {
                write!(f, "failed: {counterexample}")
            }
            VerifierOutcome::Unsupported { detail } => write!(f, "unsupported: {detail}"),
            VerifierOutcome::Timeout { after_ms } => write!(f, "timeout after {after_ms} ms"),
            VerifierOutcome::Unknown { detail } => write!(f, "unknown: {detail}"),
        }
    }
}

pub trait Verifier {
    fn name(&self) -> &str;
    fn version(&self) -> String;
    fn check(&self, artifact: &Path, obligation: &Obligation) -> VerifierOutcome;
}

/// Enumerates the obligation's explicit finite domain on the real artifact.
#[derive(Debug, Clone)]
pub struct ExhaustiveVerifier {
    pub timeout: Duration,
}

impl Default for ExhaustiveVerifier {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl ExhaustiveVerifier {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Verifier for ExhaustiveVerifier {
    fn name(&self) -> &str {
        "ravum-exhaustive"
    }

    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    fn check(&self, artifact: &Path, obligation: &Obligation) -> VerifierOutcome {
        if obligation.inputs.is_empty() {
            return VerifierOutcome::Unsupported {
                detail: "empty domain: enumerating nothing proves nothing".to_owned(),
            };
        }
        if obligation.abi_version != ravum_protocol::PATCH_ABI_VERSION {
            return VerifierOutcome::Unsupported {
                detail: format!(
                    "obligation targets ABI {}, host supports {}",
                    obligation.abi_version,
                    ravum_protocol::PATCH_ABI_VERSION
                ),
            };
        }
        let prepared = match prepare(artifact, &ExpectedAbi::default()) {
            Ok(p) => p,
            Err(reason) => {
                return VerifierOutcome::Unknown {
                    detail: format!("cannot load the artifact for verification: {reason:?}"),
                }
            }
        };

        let started = Instant::now();
        let mut previous: Option<f64> = None;
        for input in &obligation.inputs {
            if started.elapsed() > self.timeout {
                return VerifierOutcome::Timeout {
                    after_ms: started.elapsed().as_millis() as u64,
                };
            }
            let first = prepared.module.call(*input);
            let second = prepared.module.call(*input);
            if let Some(counterexample) =
                evaluate(&obligation.claim, *input, first, second, previous)
            {
                return VerifierOutcome::Failed { counterexample };
            }
            previous = Some(first);
            // A verdict produced outside the declared budget is not a success:
            // the caller asked for an answer within a bound, not any answer.
            if started.elapsed() > self.timeout {
                return VerifierOutcome::Timeout {
                    after_ms: started.elapsed().as_millis() as u64,
                };
            }
        }
        VerifierOutcome::Proved
    }
}

fn evaluate(
    claim: &Claim,
    input: f64,
    first: f64,
    second: f64,
    previous: Option<f64>,
) -> Option<String> {
    match claim {
        Claim::Finite => {
            if first.is_finite() && second.is_finite() {
                None
            } else {
                Some(format!("f({input}) is not finite: {first} (then {second})"))
            }
        }
        Claim::Deterministic => {
            if first.to_bits() == second.to_bits() {
                None
            } else {
                Some(format!("f({input}) returned {first} then {second}"))
            }
        }
        Claim::NonNegative => {
            if first >= 0.0 {
                None
            } else {
                Some(format!("f({input}) = {first} < 0"))
            }
        }
        Claim::Equals { expected } => {
            if first.to_bits() == expected.to_bits() {
                None
            } else {
                Some(format!("f({input}) = {first}, expected {expected}"))
            }
        }
        Claim::MonotonicNonDecreasing => match previous {
            Some(previous) if first < previous => Some(format!(
                "f is decreasing: {previous} then {first} at input {input}"
            )),
            _ => None,
        },
    }
}

/// External prover adapter. Detection is real; harness generation is not implemented,
/// so this backend never claims a proof.
#[derive(Debug, Clone)]
pub struct KaniVerifier {
    pub binary: String,
    detected: Option<String>,
}

impl KaniVerifier {
    /// Probe `kani --version` once.
    pub fn detect() -> Self {
        let binary = "kani".to_owned();
        let detected = Command::new(&binary)
            .arg("--version")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_owned()
            });
        Self { binary, detected }
    }

    pub fn is_available(&self) -> bool {
        self.detected.is_some()
    }
}

impl Verifier for KaniVerifier {
    fn name(&self) -> &str {
        "kani-adapter"
    }

    fn version(&self) -> String {
        self.detected.clone().unwrap_or_else(|| "absent".to_owned())
    }

    fn check(&self, _artifact: &Path, obligation: &Obligation) -> VerifierOutcome {
        match &self.detected {
            None => VerifierOutcome::Unsupported {
                detail: format!(
                    "`{}` is not installed or not runnable: no external prover available",
                    self.binary
                ),
            },
            Some(version) => VerifierOutcome::Unsupported {
                detail: format!(
                    "external prover detected ({version}) but no harness generator exists for obligation {:?}; refusing to claim a proof",
                    obligation.id
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::Claim;

    #[test]
    fn decreasing_sequence_is_reported_with_both_values() {
        let ce = evaluate(&Claim::MonotonicNonDecreasing, 2.0, 1.0, 1.0, Some(3.0));
        assert!(ce.unwrap().contains("decreasing"));
    }

    #[test]
    fn nan_violates_finite() {
        let ce = evaluate(&Claim::Finite, 0.0, f64::NAN, f64::NAN, None);
        assert!(ce.unwrap().contains("not finite"));
    }

    #[test]
    fn non_finite_negative_zero_is_still_non_negative() {
        assert!(evaluate(&Claim::NonNegative, 0.0, -0.0, -0.0, None).is_none());
    }

    #[test]
    fn equals_is_bit_exact() {
        assert!(evaluate(&Claim::Equals { expected: 0.0 }, 0.0, 0.0, 0.0, None).is_none());
        assert!(evaluate(&Claim::Equals { expected: 0.0 }, 0.0, -0.0, -0.0, None).is_some());
    }

    #[test]
    fn empty_domain_is_unsupported_not_proved() {
        let verifier = ExhaustiveVerifier::default();
        let obligation = Obligation::new("empty", Claim::Finite, vec![]);
        // The path is never opened because the domain check happens first.
        let outcome = verifier.check(Path::new("does-not-exist.dll"), &obligation);
        assert!(matches!(outcome, VerifierOutcome::Unsupported { .. }));
    }
}
