//! Evidence for a block module, produced by actually running it.
//!
//! This is `tested`, not `proved`: it exercises a fixed 64-frame block and checks
//! three observable properties. It says nothing about audio quality, real-time
//! behaviour, or inputs outside that block.

use ravum_patch_host::{prepare_block, ExpectedAbi};
use ravum_protocol::{
    sha256_hex, BuildOutput, DspState, EvidenceRecord, RejectReason, VerificationKey,
    VerificationStatus, EVIDENCE_SCHEMA_VERSION,
};
use std::path::Path;

pub const EVIDENCE_PROPERTY: &str =
    "dsp block: finite + deterministic + non-silent on a fixed 64-frame test block";

pub fn dsp_evidence_key(tool: &str, tool_version: &str) -> VerificationKey {
    VerificationKey::new(sha256_hex(
        format!("ravum-dsp-smoke-v1\0{EVIDENCE_PROPERTY}\0{tool}\0{tool_version}").as_bytes(),
    ))
}

/// Load the candidate as a block module and run one fixed test block.
pub fn block_evidence(build: &BuildOutput) -> Result<EvidenceRecord, RejectReason> {
    let prepared = prepare_block(Path::new(&build.artifact_path), &ExpectedAbi::dsp())?;
    if prepared.module.artifact_id != build.artifact_id {
        return Err(RejectReason::ArtifactMismatch {
            expected: build.artifact_id.clone(),
            found: prepared.module.artifact_id.clone(),
        });
    }

    let frames = 64usize;
    let input: Vec<f32> = (0..frames)
        .map(|i| ((i as f32) / (frames as f32)) - 0.5)
        .collect();

    let mut first = vec![0f32; frames];
    let mut first_state = DspState::default();
    prepared.module.process_block(&input, &mut first, &mut first_state);

    let mut second = vec![0f32; frames];
    let mut second_state = DspState::default();
    prepared.module.process_block(&input, &mut second, &mut second_state);

    // A second block from the *same advanced state* must be requested from the same
    // module, otherwise a stateful filter would be compared from a fresh state.
    let mut third = vec![0f32; frames];
    prepared.module.process_block(&input, &mut third, &mut first_state);

    let finite = first.iter().all(|s| s.is_finite()) && third.iter().all(|s| s.is_finite());
    let deterministic = first == second;
    let non_silent = first.iter().any(|s| s.abs() > f32::EPSILON);

    let mut failures = Vec::new();
    if !finite {
        failures.push("non-finite sample produced".to_owned());
    }
    if !deterministic {
        failures.push("two runs from the same state diverged".to_owned());
    }
    if !non_silent {
        failures.push("the module produced silence for a non-silent input".to_owned());
    }

    let tool = "ravum-dsp-harness";
    let tool_version = env!("CARGO_PKG_VERSION");
    let mut assumptions = vec![
        "single fixed 64-frame block of a deterministic ramp".to_owned(),
        "state is host-owned and starts at zero".to_owned(),
        "does not measure audio quality, latency, real-time behaviour or state migration".to_owned(),
        format!("trust base: rustc-built artifact, the native loader, and {tool} {tool_version}"),
    ];
    assumptions.extend(failures);

    Ok(EvidenceRecord {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        verification_key: dsp_evidence_key(tool, tool_version),
        status: VerificationStatus::Tested,
        property: EVIDENCE_PROPERTY.to_owned(),
        assumptions,
        tool: tool.to_owned(),
        tool_version: tool_version.to_owned(),
        source_snapshot_id: build.source_snapshot_id.clone(),
        build_context_id: build.build_context_id.clone(),
        artifact_id: build.artifact_id.clone(),
        passed: finite && deterministic && non_silent,
        counterexample: None,
        recorded_at_unix: ravum_protocol::now_unix(),
    })
}
