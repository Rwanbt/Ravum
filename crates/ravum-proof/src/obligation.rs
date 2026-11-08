//! Obligations: what must be true, over which explicit finite domain, under which
//! declared assumptions.
//!
//! A verification key is derived from the *complete* context: obligation content,
//! declared assumptions, ABI, source snapshot, build context, artifact bytes and
//! verifier identity. Any mutation of any of these produces a different key, which
//! is what makes cache invalidation mechanical instead of hopeful (ADR-013).

use ravum_protocol::{sha256_hex, ArtifactId, BuildContextId, SnapshotId, VerificationKey};
use serde::{Deserialize, Serialize};

pub const OBLIGATION_SCHEMA_VERSION: u32 = 1;

/// A property of the patchable entry point `f64 -> f64`.
///
/// The vocabulary is deliberately small: each claim must be checkable, and a claim
/// we cannot check must yield `Unknown`, never a silent success.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Claim {
    /// The result is finite for every input of the domain.
    Finite,
    /// Two consecutive calls with the same input return the same bits.
    Deterministic,
    /// The result is `>= 0` for every input of the domain.
    NonNegative,
    /// The result equals `expected` (bit-exact) for every input of the domain.
    Equals { expected: f64 },
    /// Results do not decrease when the domain is read in the declared order.
    MonotonicNonDecreasing,
}

impl Claim {
    pub fn label(&self) -> &'static str {
        match self {
            Claim::Finite => "finite",
            Claim::Deterministic => "deterministic",
            Claim::NonNegative => "non_negative",
            Claim::Equals { .. } => "equals",
            Claim::MonotonicNonDecreasing => "monotonic_non_decreasing",
        }
    }

    /// Canonical encoding used in the verification key.
    pub fn canonical(&self) -> String {
        match self {
            Claim::Equals { expected } => format!("equals:{:016x}", expected.to_bits()),
            other => other.label().to_owned(),
        }
    }
}

/// An obligation is data, not code: it can be reviewed, diffed, cached and mutated
/// by an adversarial test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Obligation {
    pub schema_version: u32,
    /// Stable identifier of the property, e.g. `compute.non_negative`.
    pub id: String,
    pub claim: Claim,
    /// The **entire** domain that will be enumerated. An empty domain proves nothing.
    pub inputs: Vec<f64>,
    pub abi_version: u32,
    /// Assumptions the verdict depends on. They are part of the cache key.
    pub assumptions: Vec<String>,
}

impl Obligation {
    pub fn new(id: impl Into<String>, claim: Claim, inputs: Vec<f64>) -> Self {
        Self {
            schema_version: OBLIGATION_SCHEMA_VERSION,
            id: id.into(),
            claim,
            inputs,
            abi_version: ravum_protocol::PATCH_ABI_VERSION,
            assumptions: Vec::new(),
        }
    }

    pub fn with_assumption(mut self, assumption: impl Into<String>) -> Self {
        self.assumptions.push(assumption.into());
        self
    }

    /// Human-readable statement of what is claimed.
    pub fn describe(&self) -> String {
        format!(
            "{}: {} over {} declared input(s)",
            self.id,
            match &self.claim {
                Claim::Finite => "finite result".to_owned(),
                Claim::Deterministic => "deterministic result".to_owned(),
                Claim::NonNegative => "result >= 0".to_owned(),
                Claim::Equals { expected } => format!("result == {expected}"),
                Claim::MonotonicNonDecreasing => "non-decreasing results".to_owned(),
            },
            self.inputs.len()
        )
    }
}

/// Everything a verdict depends on besides the obligation itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofContext {
    pub source_snapshot_id: SnapshotId,
    pub build_context_id: BuildContextId,
    pub artifact_id: ArtifactId,
    pub tool: String,
    pub tool_version: String,
}

impl ProofContext {
    pub fn new(
        source_snapshot_id: SnapshotId,
        build_context_id: BuildContextId,
        artifact_id: ArtifactId,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Self {
        Self {
            source_snapshot_id,
            build_context_id,
            artifact_id,
            tool: tool.into(),
            tool_version: tool_version.into(),
        }
    }
}

/// Derive the verification key from the full context.
///
/// The encoding is explicit and versioned so that adding a field cannot silently
/// reuse an old key: the `ravum-verification-v1` prefix must be bumped instead.
pub fn verification_key(obligation: &Obligation, ctx: &ProofContext) -> VerificationKey {
    let mut assumptions: Vec<String> = obligation.assumptions.clone();
    assumptions.sort();
    let inputs: Vec<String> = obligation
        .inputs
        .iter()
        .map(|v| format!("{:016x}", v.to_bits()))
        .collect();

    let canonical = format!(
        "ravum-verification-v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        obligation.id,
        obligation.claim.canonical(),
        inputs.join(","),
        assumptions.join("\u{1f}"),
        obligation.abi_version,
        ctx.source_snapshot_id,
        ctx.build_context_id,
        ctx.artifact_id,
        ctx.tool,
        ctx.tool_version,
        obligation.schema_version,
    );
    VerificationKey::new(sha256_hex(canonical.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ProofContext {
        ProofContext::new(
            SnapshotId::new("snap"),
            BuildContextId::new("build"),
            ArtifactId::new("artifact"),
            "ravum-exhaustive",
            "0.1.0",
        )
    }

    fn obligation() -> Obligation {
        Obligation::new("compute.non_negative", Claim::NonNegative, vec![0.0, 1.0, 2.0])
            .with_assumption("single-threaded call")
    }

    #[test]
    fn key_is_stable_for_identical_inputs() {
        assert_eq!(
            verification_key(&obligation(), &ctx()),
            verification_key(&obligation(), &ctx())
        );
    }

    #[test]
    fn key_changes_with_every_mutated_field() {
        let base = verification_key(&obligation(), &ctx());

        let mut other_domain = obligation();
        other_domain.inputs.push(3.0);
        assert_ne!(base, verification_key(&other_domain, &ctx()));

        let mut other_claim = obligation();
        other_claim.claim = Claim::Finite;
        assert_ne!(base, verification_key(&other_claim, &ctx()));

        let mut other_assumption = obligation();
        other_assumption.assumptions.push("x >= 0".to_owned());
        assert_ne!(base, verification_key(&other_assumption, &ctx()));

        let mut other_snapshot = ctx();
        other_snapshot.source_snapshot_id = SnapshotId::new("snap2");
        assert_ne!(base, verification_key(&obligation(), &other_snapshot));

        let mut other_artifact = ctx();
        other_artifact.artifact_id = ArtifactId::new("tampered");
        assert_ne!(base, verification_key(&obligation(), &other_artifact));

        let mut other_tool = ctx();
        other_tool.tool_version = "1.96.0".to_owned();
        assert_ne!(base, verification_key(&obligation(), &other_tool));

        let mut other_id = obligation();
        other_id.id = "compute.other".to_owned();
        assert_ne!(base, verification_key(&other_id, &ctx()));
    }

    #[test]
    fn assumption_order_does_not_matter() {
        let a = obligation()
            .with_assumption("a")
            .with_assumption("b");
        let b = obligation()
            .with_assumption("b")
            .with_assumption("a");
        assert_eq!(verification_key(&a, &ctx()), verification_key(&b, &ctx()));
    }
}
