//! Evidence for a block module, produced by actually running it.
//!
//! This is `tested`, not `proved`: it exercises a fixed 64-frame block and checks
//! three observable properties. It says nothing about audio quality, real-time
//! behaviour, or inputs outside that block.

use ravum_patch_host::{prepare_block, prepare_opaque_block, ExpectedAbi, OpaqueBlock};
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

    let (mut first, mut second) = (DspState::default(), DspState::default());
    let module = &prepared.module;
    Ok(smoke(
        build,
        "state is host-owned and starts at zero",
        |input, output| {
            module.process_block(input, output, &mut first);
        },
        |input, output| {
            module.process_block(input, output, &mut second);
        },
    ))
}

/// Exercise a trusted native ABI 3 artifact from two initialized states.
///
/// # Safety
/// The artifact must come from a trusted build that implements DSP ABI 3.
pub unsafe fn opaque_block_evidence(build: &BuildOutput) -> Result<EvidenceRecord, RejectReason> {
    let path = Path::new(&build.artifact_path);
    let load = || {
        // SAFETY: the caller vouches for this native artifact.
        let prepared = unsafe { prepare_opaque_block(path) }?;
        if prepared.artifact_id != build.artifact_id {
            return Err(RejectReason::ArtifactMismatch {
                expected: build.artifact_id.clone(),
                found: prepared.artifact_id,
            });
        }
        OpaqueBlock::start(prepared)
    };
    let (mut first, mut second) = (load()?, load()?);
    Ok(smoke(
        build,
        "state is host-owned and initialized by the module",
        |input, output| {
            first.process(input, output);
        },
        |input, output| {
            second.process(input, output);
        },
    ))
}

fn smoke(
    build: &BuildOutput,
    initialization: &str,
    mut primary: impl FnMut(&[f32], &mut [f32]),
    mut independent: impl FnMut(&[f32], &mut [f32]),
) -> EvidenceRecord {
    const FRAMES: usize = 64;
    let input = std::array::from_fn::<_, FRAMES, _>(|i| i as f32 / FRAMES as f32 - 0.5);
    let (mut first, mut second, mut third) = ([0.0; FRAMES], [0.0; FRAMES], [0.0; FRAMES]);
    primary(&input, &mut first);
    independent(&input, &mut second);
    // Exercise the next block with the advanced state, not a fresh one.
    primary(&input, &mut third);

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
        initialization.to_owned(),
        "does not measure audio quality, latency, real-time behaviour or state migration"
            .to_owned(),
        format!("trust base: rustc-built artifact, the native loader, and {tool} {tool_version}"),
    ];
    assumptions.extend(failures);

    EvidenceRecord {
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
    }
}
