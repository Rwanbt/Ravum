//! RFC-E11-DSP-BLOCK §3: a `.rv` DSP block built by `ravum-lang build --emit
//! dsp-block`, loaded by the real block host (`ravum-patch-host`).
//!
//! * differential: the block equals a Rust reference filter bit for bit on 10
//!   fixed signals;
//! * 0 allocation and 0 deallocation on the callback over 10 000 blocks with
//!   20 compatible swaps, the state carried across each swap;
//! * the host refuses a module whose state schema differs, or that exports none.

use ravum_dsp_harness::{
    allocations_on_this_thread, block_evidence, deallocations_on_this_thread, reset_thread_counters,
};
use ravum_patch_host::{prepare_block, BlockHost, ExpectedAbi};
use ravum_protocol::{
    ActivationStatus, BuildContextId, BuildOutput, DiagnosticSet, DspState, RejectReason,
    SnapshotId,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FRAMES: usize = 64;

fn source(alpha: &str) -> String {
    format!(
        r#"struct State {{ z1: f32, z2: f32 }}

fn process_sample(state: &mut State, input: f32) -> f32 effects() {{
    let alpha: f32 = {alpha};
    let y: f32 = alpha * input + (1.0 - alpha) * state.z1;
    state.z1 = y;
    state.z2 = input;
    y
}}

fn main() -> () effects() {{
    let mut s: State = State {{ z1: 0.0, z2: 0.0 }};
    drop(process_sample(&mut s, 1.0));
}}
"#
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ravum-dsp-block-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Build one `.rv` block and return the path of its `cdylib`.
fn build_block(dir: &Path, name: &str, alpha: &str) -> PathBuf {
    let package = dir.join(name);
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(
        package.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    fs::write(package.join("src/main.rv"), source(alpha)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["build", "--emit", "dsp-block", "."])
        .current_dir(&package)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "build failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let built = stdout
        .lines()
        .find_map(|l| l.strip_prefix("built: "))
        .expect("built: line");
    PathBuf::from(built.trim())
}

fn build_output(path: &Path) -> BuildOutput {
    let prepared = prepare_block(path, &ExpectedAbi::dsp_with_state_schema()).expect("prepare");
    let snapshot = SnapshotId::new("dsp-block-test");
    BuildOutput {
        schema_version: 1,
        artifact_path: path.display().to_string(),
        artifact_id: prepared.module.artifact_id.clone(),
        source_snapshot_id: snapshot.clone(),
        build_context_id: BuildContextId::new("ravum-lang"),
        tool: "ravum-lang".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        target: "host".into(),
        flags: Vec::new(),
        build_ms: 0,
        cached_build: false,
        diagnostics: DiagnosticSet {
            schema_version: 1,
            source_snapshot_id: snapshot,
            tool: "ravum-lang".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            diagnostics: Vec::new(),
        },
    }
}

fn signal(kind: usize, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let t = i as f32;
            match kind % 5 {
                0 => (t * 0.031 * (kind as f32 + 1.0)).sin() * 0.5,
                1 => {
                    if (i / 7) % 2 == 0 {
                        0.8
                    } else {
                        -0.8
                    }
                }
                2 => {
                    if i == 0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                3 => (t / frames as f32) * 2.0 - 1.0,
                _ => ((i * 7919 + kind * 104_729) % 1000) as f32 / 500.0 - 1.0,
            }
        })
        .collect()
}

fn reference(alpha: f32, input: &[f32], state: &mut DspState) -> Vec<f32> {
    input
        .iter()
        .map(|&x| {
            let y = alpha * x + (1.0 - alpha) * state.z1;
            state.z1 = y;
            state.z2 = x;
            y
        })
        .collect()
}

#[test]
fn the_rv_block_equals_the_reference_filter_bit_for_bit() {
    let dir = scratch("diff");
    let path = build_block(&dir, "lowpass", "0.1");
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp_with_state_schema(), &path).expect("bootstrap");
    for kind in 0..10 {
        let input = signal(kind, FRAMES * 16);
        let mut state = DspState::default();
        let mut expected_state = DspState::default();
        let expected = reference(0.1, &input, &mut expected_state);
        let mut output = vec![0f32; input.len()];
        for (block_in, block_out) in input.chunks(FRAMES).zip(output.chunks_mut(FRAMES)) {
            assert_eq!(host.process_rt(block_in, block_out, &mut state), FRAMES);
        }
        let same = output
            .iter()
            .zip(&expected)
            .all(|(a, b)| a.to_bits() == b.to_bits());
        assert!(same, "signal {kind}: the block differs from the reference");
        assert_eq!(state.z1.to_bits(), expected_state.z1.to_bits());
        assert_eq!(state.z2.to_bits(), expected_state.z2.to_bits());
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn twenty_swaps_keep_the_state_without_allocating_on_the_callback() {
    let dir = scratch("swap");
    let a = build_block(&dir, "lowpass_a", "0.1");
    let b = build_block(&dir, "lowpass_b", "0.3");
    let builds = [(build_output(&a), 0.1f32), (build_output(&b), 0.3f32)];
    let evidence: Vec<_> = builds
        .iter()
        .map(|(build, _)| block_evidence(build).expect("evidence"))
        .collect();
    let host = BlockHost::bootstrap(ExpectedAbi::dsp_with_state_schema(), &a).expect("bootstrap");

    let mut state = DspState::default();
    let mut expected_state = DspState::default();
    let mut active = 0usize;
    let mut allocations = 0u64;
    let mut deallocations = 0u64;
    let mut swaps = 0u64;
    let input = signal(0, FRAMES);
    let mut output = vec![0f32; FRAMES];
    for block in 0..10_000usize {
        if block % 500 == 499 {
            let next = 1 - active;
            let (build, _) = &builds[next];
            let prepared = prepare_block(Path::new(&build.artifact_path), host.expected_abi())
                .expect("prepare");
            let report = host.apply(
                prepared,
                build,
                Some(&evidence[next]),
                &build.source_snapshot_id,
            );
            assert_eq!(
                report.status,
                ActivationStatus::Activated,
                "{:?}",
                report.reject_reason
            );
            active = next;
            swaps += 1;
        }
        reset_thread_counters();
        host.process_rt(&input, &mut output, &mut state);
        allocations += allocations_on_this_thread();
        deallocations += deallocations_on_this_thread();
        let expected = reference(builds[active].1, &input, &mut expected_state);
        assert!(
            output
                .iter()
                .zip(&expected)
                .all(|(x, y)| x.to_bits() == y.to_bits()),
            "block {block}: output differs after {swaps} swaps (state not carried?)"
        );
    }
    assert_eq!(swaps, 20);
    assert_eq!(allocations, 0, "allocations on the callback");
    assert_eq!(deallocations, 0, "deallocations on the callback");
    let _ = fs::remove_dir_all(&dir);
}

/// Compile a hand-written block module with `rustc`, to forge what a Ravum
/// build never produces.
fn forged(dir: &Path, name: &str, schema_export: Option<&str>) -> PathBuf {
    let mut text = String::from(
        "use std::os::raw::c_char;\n\
         const MANIFEST: &[u8] = b\"{\\\"schema_version\\\":1,\\\"patch_id\\\":\\\"forged\\\",\\\"export\\\":\\\"ravum_process_block\\\",\\\"signature\\\":\\\"block:f32->f32+state:v1\\\"}\\0\";\n\
         #[no_mangle] pub extern \"C\" fn ravum_abi_version() -> u32 { 2 }\n\
         #[no_mangle] pub extern \"C\" fn ravum_manifest_v1() -> *const c_char { MANIFEST.as_ptr() as *const c_char }\n\
         #[no_mangle] pub extern \"C\" fn ravum_process_block(_i: *const f32, _o: *mut f32, _n: u32, _s: *mut u8) {}\n",
    );
    if let Some(schema) = schema_export {
        text.push_str(&format!(
            "#[no_mangle] pub extern \"C\" fn ravum_state_schema() -> *const c_char {{ b\"{schema}\\0\".as_ptr() as *const c_char }}\n"
        ));
    }
    let src = dir.join(format!("{name}.rs"));
    fs::write(&src, text).unwrap();
    let out = dir.join(format!(
        "{}{name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".into());
    let status = Command::new(rustc)
        .args(["--edition", "2021", "--crate-type", "cdylib", "-o"])
        .arg(&out)
        .arg(&src)
        .status()
        .unwrap();
    assert!(status.success());
    out
}

#[test]
fn the_host_refuses_another_state_schema() {
    let dir = scratch("refuse");
    let other = forged(&dir, "other_schema", Some(&"0".repeat(64)));
    match prepare_block(&other, &ExpectedAbi::dsp_with_state_schema()) {
        Err(RejectReason::StateSchemaChanged { expected, found }) => {
            assert_eq!(expected, ravum_protocol::dsp_state_schema_v1_digest());
            assert_eq!(found, "0".repeat(64));
        }
        Err(other) => panic!("unexpected refusal {other:?}"),
        Ok(_) => panic!("a module with another state schema was accepted"),
    }
    let none = forged(&dir, "no_schema", None);
    match prepare_block(&none, &ExpectedAbi::dsp_with_state_schema()) {
        Err(RejectReason::MissingExport { symbol }) => assert_eq!(symbol, "ravum_state_schema"),
        Err(other) => panic!("unexpected refusal {other:?}"),
        Ok(_) => panic!("a module without a state schema was accepted"),
    }
    // The historical DSP contract still loads it: the check is opt-in.
    assert!(prepare_block(&none, &ExpectedAbi::dsp()).is_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn checker_and_host_agree_on_the_state_schema() {
    assert_eq!(
        ravum_lang_sema::reload::DSP_STATE_SCHEMA_V1_TEXT,
        ravum_protocol::DSP_STATE_SCHEMA_V1_TEXT
    );
    assert_eq!(
        ravum_lang_sema::loader::sha256_hex(ravum_protocol::DSP_STATE_SCHEMA_V1_TEXT.as_bytes()),
        ravum_protocol::dsp_state_schema_v1_digest()
    );
}
