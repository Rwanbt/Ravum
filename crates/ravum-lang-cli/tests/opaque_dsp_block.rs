use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ravum_dsp_harness::{
    allocations_on_this_thread, deallocations_on_this_thread, reset_thread_counter,
};
use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_dsp, DEFAULT_STEPS};
use ravum_lang_jit::hot::HotEngine;
use ravum_patch_host::{
    prepare_opaque_block, OpaqueBlock, OpaqueEditor, OpaqueUpdate, PreparedOpaqueBlock,
};
use ravum_protocol::{BuildContextId, BuildOutput, DiagnosticSet, RejectReason, SnapshotId};

const FRAMES: usize = 64;
const SOURCE: &str = r#"
struct Filter { gain: f32, enabled: bool }
struct State { taps: [f32; 4], filter: Filter }
fn init() -> State effects() {
    return State { taps: [1.0, 0.0, 0.0, 0.0], filter: Filter { gain: 0.5, enabled: true } };
}
fn process_sample(state: &mut State, input: f32) -> f32 effects() {
    let y: f32 = INPUT_GAIN * input * state.filter.gain + state.taps[0] * 0.5;
    state.taps[0] = y;
    return y;
}
fn main() -> () effects() {}
"#;

fn project(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("ravum-dsp3-{}-{name}", std::process::id()));
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(
        path.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    fs::write(path.join("src/main.rv"), source).unwrap();
    path
}

fn cli(path: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(arguments)
        .current_dir(path)
        .output()
        .unwrap()
}

fn build(name: &str, source: &str) -> BuildOutput {
    let package = project(name, source);
    let output = cli(&package, &["build", "--emit", "dsp-block", "."]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = stdout
        .lines()
        .find_map(|line| line.strip_prefix("built: "))
        .unwrap();
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(format!("{path}.build.json")).unwrap()).unwrap();
    let snapshot = SnapshotId::new(meta["source_digest"].as_str().unwrap());
    BuildOutput {
        schema_version: 1,
        artifact_path: path.into(),
        artifact_id: load_path(Path::new(path)).artifact_id,
        source_snapshot_id: snapshot.clone(),
        build_context_id: BuildContextId::new(meta["identity"].as_str().unwrap()),
        tool: "rustc".into(),
        tool_version: meta["rustc"].as_str().unwrap().into(),
        target: "host".into(),
        flags: vec!["--emit=dsp-block".into()],
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

fn load(build: &BuildOutput) -> PreparedOpaqueBlock {
    load_path(Path::new(&build.artifact_path))
}

fn load_path(path: &Path) -> PreparedOpaqueBlock {
    // SAFETY: this is the trusted compiler's artifact built from our fixture.
    unsafe { prepare_opaque_block(path) }.unwrap()
}

fn update(editor: &mut OpaqueEditor, build: &BuildOutput) -> Result<OpaqueUpdate, RejectReason> {
    // SAFETY: build is produced by the trusted compiler from a test fixture.
    let evidence = unsafe { ravum_dsp_harness::evidence::opaque_block_evidence(build) }?;
    editor.update(
        load(build),
        build,
        Some(&evidence),
        &build.source_snapshot_id,
    )
}

#[test]
fn opaque_state_matches_jit_and_interpreter_bit_for_bit() {
    let source = SOURCE.replace("INPUT_GAIN", "1.0f32");
    let module = compile_source(&source).unwrap();
    let artifact = build("opaque-signals", &source);
    for seed in 0..10 {
        let input = std::array::from_fn::<_, FRAMES, _>(|i| {
            (((i * 17 + seed * 7) % 31) as f32 - 15.0) / 16.0
        });
        let mut block = OpaqueBlock::start(load(&artifact)).unwrap();
        let (_engine, mut jit) = HotEngine::start(&module, "process_sample").unwrap();
        let interpreted = run_dsp(
            &module,
            Some("init"),
            "process_sample",
            &input,
            DEFAULT_STEPS,
        )
        .unwrap();
        let mut native = [0.0; FRAMES];
        assert_eq!(block.process(&input, &mut native), FRAMES);
        for ((sample, native), interpreted) in input.into_iter().zip(native).zip(interpreted) {
            assert_eq!(native.to_bits(), interpreted.to_bits(), "signal {seed}");
            assert_eq!(
                native.to_bits(),
                jit.process_sample(sample).to_bits(),
                "signal {seed}"
            );
        }
    }
}

#[test]
fn same_digest_swaps_keep_state_without_callback_allocation() {
    let v1 = SOURCE.replace("INPUT_GAIN", "1.0f32");
    let v2 = SOURCE.replace("INPUT_GAIN", "2.0f32");
    let first = build("opaque-swap-first", &v1);
    let second = build("opaque-swap-second", &v2);
    let active = load(&first);
    let digest = active.schema().to_owned();
    let layout = active.layout();
    let (mut editor, mut runner) = OpaqueBlock::start(active).unwrap().split();
    let input = [0.125; FRAMES];
    let mut output = [0.0; FRAMES];
    let mut expected = 1.0f32;
    let mut gain = 1.0f32;
    for index in 0..10_000 {
        if index != 0 && index % 480 == 0 {
            let build = if gain == 1.0 { &second } else { &first };
            let next = load(build);
            assert_eq!(next.schema(), digest);
            assert_eq!(next.layout(), layout);
            assert_eq!(update(&mut editor, build).unwrap(), OpaqueUpdate::Published);
            gain = if gain == 1.0 { 2.0 } else { 1.0 };
        }
        reset_thread_counter();
        let frames = runner.process(&input, &mut output);
        let allocations = allocations_on_this_thread();
        let frees = deallocations_on_this_thread();
        assert_eq!((frames, allocations, frees), (FRAMES, 0, 0));
        for value in output {
            expected = gain * input[0] * 0.5 + expected * 0.5;
            assert_eq!(value.to_bits(), expected.to_bits());
        }
    }
}

#[test]
fn heap_owning_opaque_state_is_rejected_by_the_checker() {
    let source = SOURCE
        .replace("INPUT_GAIN", "1.0f32")
        .replace("taps: [f32; 4]", "taps: [f32; 4], text: String")
        .replace(
            "taps: [1.0, 0.0, 0.0, 0.0]",
            "taps: [1.0, 0.0, 0.0, 0.0], text: String::new()",
        )
        .replace(
            "fn init() -> State effects()",
            "fn init() -> State effects(alloc)",
        );
    let package = project("opaque-heap", &source);
    let output = cli(&package, &["check", "--dsp-block", "."]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E-DSP-STATE-TYPE"));
}

#[test]
fn migration_adopts_new_state_or_preserves_the_active_block_on_trap() {
    let original = SOURCE.replace("INPUT_GAIN", "1.0f32");
    let upgrade = original
        .replace(
            "taps: [f32; 4], filter: Filter",
            "taps: [f32; 4], filter: Filter, offset: f32",
        )
        .replace("enabled: true } }", "enabled: true }, offset: 0.0 }")
        .replace("return y;", "return y + state.offset;");
    let from = "struct OldState { taps: [f32; 4], filter: Filter }";
    let trapping = format!(
        r#"{upgrade}
{from}
fn migrate(old: OldState) -> State effects(panic) {{ panic("test migration trap") }}
"#
    );
    let migrated = format!(
        r#"{upgrade}
{from}
fn migrate(old: OldState) -> State effects() {{
    return State {{ taps: old.taps, filter: old.filter, offset: 1.0 }};
}}
"#
    );

    let initial = build("opaque-migrate-original", &original);
    let block = OpaqueBlock::start(load(&initial)).unwrap();
    let (mut editor, mut runner) = block.split();
    let trap = build("opaque-migrate-trap", &trapping);
    let good = build("opaque-migrate-success", &migrated);

    std::thread::scope(|scope| {
        let (input, next) = std::sync::mpsc::channel::<f32>();
        let (send, output) = std::sync::mpsc::channel();
        let worker = scope.spawn(move || {
            while let Ok(sample) = next.recv() {
                let mut out = [0.0];
                reset_thread_counter();
                let frames = runner.process(&[sample], &mut out);
                let counts = (
                    frames,
                    allocations_on_this_thread(),
                    deallocations_on_this_thread(),
                );
                send.send((out[0], counts)).unwrap();
            }
        });
        let render = |sample| {
            input.send(sample).unwrap();
            let (sample, counts) = output.recv().unwrap();
            assert_eq!(counts, (1, 0, 0));
            sample
        };

        assert_eq!(render(1.0), 1.0);
        assert_eq!(
            update(&mut editor, &trap).unwrap(),
            OpaqueUpdate::WaitingForBlock
        );
        assert_eq!(
            editor.finish_migration(&trap.source_snapshot_id).unwrap(),
            Some(OpaqueUpdate::WaitingForBlock)
        );
        assert_eq!(render(0.0), 0.5);
        assert!(matches!(
            editor.finish_migration(&trap.source_snapshot_id),
            Err(RejectReason::StateMigrationFailed)
        ));

        assert_eq!(
            update(&mut editor, &good).unwrap(),
            OpaqueUpdate::WaitingForBlock
        );
        assert_eq!(render(0.0), 0.25);
        assert_eq!(render(0.0), 0.125);
        assert_eq!(
            editor.finish_migration(&good.source_snapshot_id).unwrap(),
            Some(OpaqueUpdate::Published)
        );
        assert!(matches!(
            update(&mut editor, &good),
            Err(RejectReason::StateUpdateInFlight)
        ));
        assert_eq!(render(0.0), 1.25);

        assert_eq!(
            update(&mut editor, &initial).unwrap(),
            OpaqueUpdate::WaitingForBlock
        );
        assert!(matches!(
            editor.finish_migration(&SnapshotId::new("stale")),
            Err(RejectReason::StaleBuild { .. })
        ));
        assert_eq!(render(0.0), 1.125);
        drop(input);
        worker.join().unwrap();
    });
}

#[test]
fn same_digest_implies_the_same_c_layout_for_generated_states() {
    for count in [1, 2, 5, 9] {
        let source = SOURCE
            .replace("taps: [f32; 4], filter: Filter", &format!("taps: [f32; 4], filter: Filter, counter: u32, enabled: bool, padding: [u32; {count}]"))
            .replace("enabled: true } }", &format!("enabled: true }}, counter: 0u32, enabled: false, padding: [0u32; {count}] }}"));
        let first = load(&build(
            &format!("opaque-layout-first-{count}"),
            &source.replace("INPUT_GAIN", "1.0f32"),
        ));
        let second = load(&build(
            &format!("opaque-layout-second-{count}"),
            &source.replace("INPUT_GAIN", "2.0f32"),
        ));
        assert_eq!(first.schema(), second.schema());
        assert_eq!(first.layout(), second.layout());
        assert_eq!(first.layout().size, 32 + 4 * count);
        assert_eq!(first.layout().align, 4);
    }
}

#[test]
fn trapping_initializer_is_rejected_before_native_codegen() {
    let source = SOURCE.replace("INPUT_GAIN", "1.0f32").replace(
        "return State { taps: [1.0",
        "let zero: i32 = 0; let bad: i32 = 1 / zero; return State { taps: [bad as f32",
    );
    let package = project("opaque-init-trap", &source);
    let output = cli(&package, &["check", "--dsp-block", "."]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E-EFF-PROFILE"));
}
