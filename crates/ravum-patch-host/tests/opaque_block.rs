use std::path::Path;

use ravum_patch_host::{prepare_block, prepare_opaque_block, ExpectedAbi, OpaqueBlock};
use ravum_protocol::dsp::DspStateLayout;
use ravum_protocol::{BuildOutput, RejectReason, SnapshotId};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;

fn fixture(tag: &str, abi: u32, size: u32, digest_digit: char, gain: u32) -> BuildOutput {
    let dir = std::env::temp_dir().join(format!("ravum-abi3-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.rs");
    let digest = digest_digit.to_string().repeat(64);
    std::fs::write(
        &path,
        format!(r#"
#[repr(C)]
pub struct Layout {{ size: u32, align: u32 }}
#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {{ {abi} }}
#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const u8 {{
    b"{{\"schema_version\":1,\"patch_id\":\"{tag}\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:opaque:v3\",\"author\":\"tests\"}}\0".as_ptr()
}}
#[no_mangle]
pub extern "C" fn ravum_state_schema() -> *const u8 {{ b"{digest}\0".as_ptr() }}
#[no_mangle]
pub extern "C" fn ravum_state_layout() -> Layout {{ Layout {{ size: {size}, align: 4 }} }}
#[no_mangle]
pub unsafe extern "C" fn ravum_state_init(state: *mut f32) {{ state.write(2.0); }}
#[no_mangle]
pub unsafe extern "C" fn ravum_process_block(state: *mut f32, input: *const f32, output: *mut f32, frames: u32) {{
    for i in 0..frames as usize {{
        *state += *input.add(i) * {gain} as f32;
        output.add(i).write(*state);
    }}
}}
"#),
    ).unwrap();
    let source = SourceSnapshot::from_file(&path).unwrap();
    let outcome = RustAdapter::new("rustc", dir.join("work"))
        .build_cdylib(&source, &dir.join("artifacts"))
        .unwrap();
    assert!(outcome.success, "{:?}", outcome.build.diagnostics);
    outcome.build
}

fn load(build: &BuildOutput) -> Result<ravum_patch_host::PreparedOpaqueBlock, RejectReason> {
    // SAFETY: tests build these controlled fixtures just above.
    unsafe { prepare_opaque_block(Path::new(&build.artifact_path)) }
}

fn replace(block: &mut OpaqueBlock, build: &BuildOutput) -> Result<(), RejectReason> {
    // SAFETY: the native code is built from the trusted fixture above.
    let evidence = unsafe { ravum_dsp_harness::evidence::opaque_block_evidence(build) }?;
    block.replace(
        load(build)?,
        build,
        Some(&evidence),
        &build.source_snapshot_id,
    )
}

#[test]
fn compatible_replacement_keeps_initialized_state() {
    let first = load(&fixture("first", 3, 4, 'a', 1)).unwrap();
    assert_eq!(first.layout(), DspStateLayout { size: 4, align: 4 });
    let mut block = OpaqueBlock::start(first).unwrap();
    let mut output = [0.0; 2];
    assert_eq!(block.process(&[1.0, 1.0], &mut output), 2);
    assert_eq!(output, [3.0, 4.0]);

    let incompatible = fixture("wrong-state", 3, 4, 'b', 5);
    assert!(matches!(
        replace(&mut block, &incompatible),
        Err(RejectReason::StateSchemaChanged { .. })
    ));
    block.process(&[1.0, 1.0], &mut output);
    assert_eq!(output, [5.0, 6.0]);

    let wrong_layout = fixture("wrong-layout", 3, 8, 'a', 5);
    assert!(matches!(
        replace(&mut block, &wrong_layout),
        Err(RejectReason::StateLayout { size: 8, align: 4 })
    ));
    let next = fixture("second", 3, 4, 'a', 2);
    assert!(matches!(
        block.replace(load(&next).unwrap(), &next, None, &next.source_snapshot_id),
        Err(RejectReason::NotVerified { .. })
    ));
    assert!(matches!(
        block.replace(load(&next).unwrap(), &next, None, &SnapshotId::new("newer")),
        Err(RejectReason::StaleBuild { .. })
    ));
    replace(&mut block, &next).unwrap();
    block.process(&[1.0, 1.0], &mut output);
    assert_eq!(output, [8.0, 10.0]);
}

#[test]
fn oversized_state_and_cross_abi_loads_are_refused() {
    let opaque = fixture("too-large", 3, 65_540, 'a', 1);
    assert!(matches!(
        load(&opaque),
        Err(RejectReason::StateLayout {
            size: 65_540,
            align: 4
        })
    ));
    assert!(matches!(
        prepare_block(Path::new(&opaque.artifact_path), &ExpectedAbi::dsp()),
        Err(RejectReason::AbiMismatch {
            expected: 2,
            found: 3
        })
    ));
    let legacy = fixture("abi2", 2, 4, 'a', 1);
    assert!(matches!(
        load(&legacy),
        Err(RejectReason::AbiMismatch {
            expected: 3,
            found: 2
        })
    ));
}
