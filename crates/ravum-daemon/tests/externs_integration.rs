//! MVP-0c L1.2 — real integration tests for content-addressed externs in
//! `Daemon::analyze_with_externs`.
//!
//! Every dependency `.rmeta` is produced by a previous real analysis of this
//! daemon (no fabricated metadata), and every dependent is compiled by a real
//! `rustc --extern` invocation. Compiler-spawning tests hold [`RUSTC_LOCK`].

use ravum_daemon::snapshot::verify_snapshot;
use ravum_daemon::{
    AnalysisRequest, BuildContext, Daemon, DaemonConfig, ErrorKind, ExternInput, Priority,
    SnapshotStore, SourceFile, ToolchainInfo,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static RUSTC_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
static REVISION: AtomicU64 = AtomicU64::new(0);

fn rustc_guard() -> std::sync::MutexGuard<'static, ()> {
    match RUSTC_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn rustc() -> PathBuf {
    std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "ravum-daemon-externs-{name}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn daemon(work_dir: &Path) -> Daemon {
    Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        ..Default::default()
    })
    .expect("daemon open")
}

fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

fn context(crate_name: &str) -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: crate_name.to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn request(label: &str, crate_name: &str, content: &str) -> AnalysisRequest {
    AnalysisRequest {
        revision: next_revision(),
        label: label.to_owned(),
        root: "src/lib.rs".to_owned(),
        files: vec![SourceFile {
            path: "src/lib.rs".to_owned(),
            content: content.to_owned(),
        }],
        context: context(crate_name),
        priority: Priority::Normal,
    }
}

fn core_v1() -> &'static str {
    "pub fn scale(x: f64) -> f64 { x * 2.0 }\n"
}

fn core_v2_compatible() -> &'static str {
    "pub fn scale(x: f64) -> f64 { x * 2.0 }\npub fn cube(x: f64) -> f64 { x * x * x }\n"
}

fn core_v3_breaking() -> &'static str {
    "pub fn scale(x: f64, factor: f64) -> f64 { x * factor }\n"
}

fn api_v1() -> &'static str {
    "pub fn scaled(x: f64) -> f64 { core::scale(x) }\n"
}

/// Analyse the dependency and return its real, content-addressed `.rmeta` bytes.
fn analyze_dependency(daemon: &Daemon, content: &str) -> (String, Vec<u8>, u64) {
    let report = daemon
        .analyze(&request("core", "core", content))
        .expect("dependency analysis");
    assert!(report.success, "dependency must compile: {report:?}");
    assert!(report.is_current());
    let interface_id = report
        .artifact_sha256
        .clone()
        .expect("successful analysis must expose its artifact");
    let bytes = daemon
        .artifact(&interface_id)
        .expect("artifact must be readable");
    assert_eq!(
        ravum_daemon::hash::sha256_hex(&bytes),
        interface_id,
        "artifact bytes must match their interface id"
    );
    (interface_id, bytes, report.timings.analysis_ms)
}

#[test]
fn real_chain_dependency_rmeta_then_dependent_then_hit() {
    let _guard = rustc_guard();
    let work = temp_dir("real-chain");
    let daemon = daemon(&work);
    let (core_interface, core_bytes, core_ms) = analyze_dependency(&daemon, core_v1());
    assert!(core_ms > 0);
    let extern_core =
        ExternInput::new("core", "core", core_bytes.clone()).expect("valid extern input");
    assert_eq!(extern_core.interface_id, core_interface);

    let dependent = request("api", "api", api_v1());
    let report = daemon
        .analyze_with_externs(&dependent, &[extern_core.clone()])
        .expect("dependent analysis");
    assert!(report.success, "chained analysis must succeed: {report:?}");
    assert!(report.is_current());
    assert!(!report.cache.hit, "first chained run must be a real miss");
    assert!(report.timings.analysis_ms > 0, "rustc must have run");
    assert_eq!(report.externs.len(), 1);
    assert_eq!(report.externs[0].name, "core");
    assert_eq!(report.externs[0].interface_id, core_interface);
    assert_eq!(report.externs[0].rmeta_sha256, core_interface);
    assert!(report.externs[0].validates());
    let snapshot = report.snapshot.as_ref().expect("snapshot report");
    assert_eq!(snapshot.externs, 1);
    let api_artifact = report
        .artifact_sha256
        .clone()
        .expect("dependent artifact id");

    // Repeat with identical sources and identical extern: real cache HIT, but the
    // interface artifact identity is still served.
    let repeat = daemon
        .analyze_with_externs(&request("api", "api", api_v1()), &[extern_core])
        .expect("repeat analysis");
    assert!(repeat.cache.hit, "identical externs must hit: {repeat:?}");
    assert_eq!(
        repeat.artifact_sha256.as_deref(),
        Some(api_artifact.as_str())
    );
    assert_eq!(repeat.externs.len(), 1);
    assert_eq!(repeat.timings.analysis_ms, 0);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn changed_extern_without_source_change_is_a_real_miss_with_correct_diagnostic() {
    let _guard = rustc_guard();
    let work = temp_dir("extern-change");
    let daemon = daemon(&work);
    let (interface_v1, bytes_v1, _) = analyze_dependency(&daemon, core_v1());
    let extern_v1 = ExternInput::new("core", "core", bytes_v1).expect("extern v1");
    let first = daemon
        .analyze_with_externs(&request("api", "api", api_v1()), &[extern_v1.clone()])
        .expect("first chained analysis");
    assert!(first.success && !first.cache.hit);
    let snapshot_v1 = first.snapshot_id.clone();

    // New core interface (compatible addition): different .rmeta bytes, same api
    // sources. The request must MISS (extern identity changed), not hit.
    let (interface_v2, bytes_v2, _) = analyze_dependency(&daemon, core_v2_compatible());
    assert_ne!(interface_v1, interface_v2, "interface must change");
    let extern_v2 = ExternInput::new("core", "core", bytes_v2).expect("extern v2");
    let second = daemon
        .analyze_with_externs(&request("api", "api", api_v1()), &[extern_v2.clone()])
        .expect("second chained analysis");
    assert!(
        !second.cache.hit,
        "a changed extern must never be a cache HIT: {second:?}"
    );
    assert!(second.success, "compatible extern must compile: {second:?}");
    assert_eq!(second.externs[0].interface_id, interface_v2);
    assert_ne!(
        snapshot_v1, second.snapshot_id,
        "snapshot identity must include the extern identity"
    );

    // Breaking interface: same api sources + new extern => real recompilation and
    // the actual rustc diagnostic.
    let (interface_v3, bytes_v3, _) = analyze_dependency(&daemon, core_v3_breaking());
    assert_ne!(interface_v2, interface_v3);
    let breaking = daemon
        .analyze_with_externs(
            &request("api", "api", api_v1()),
            &[ExternInput::new("core", "core", bytes_v3).expect("extern v3")],
        )
        .expect("breaking chained analysis");
    assert!(!breaking.cache.hit, "changed extern must miss");
    assert!(
        !breaking.success,
        "breaking signature must fail: {breaking:?}"
    );
    assert!(
        breaking.diagnostics.iter().any(|diagnostic| diagnostic
            .rendered
            .as_deref()
            .unwrap_or("")
            .contains("E0061")),
        "expected a real E0061 diagnostic: {:?}",
        breaking.diagnostics
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn extern_validation_is_fail_closed() {
    let _guard = rustc_guard();
    let work = temp_dir("validation");
    let daemon = daemon(&work);
    let (_, bytes, _) = analyze_dependency(&daemon, core_v1());
    let valid = ExternInput::new("core", "core", bytes.clone()).expect("valid extern");
    let request = request("api", "api", api_v1());

    // Duplicate names are refused before any compilation.
    let duplicate = ExternInput::new("core", "core_v2", bytes.clone()).expect("second bytes");
    let error = daemon
        .analyze_with_externs(&request, &[valid.clone(), duplicate])
        .err()
        .expect("duplicate extern names must be refused");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);

    // A tampered interface id is refused (fail-closed).
    let mut tampered = valid.clone();
    tampered.interface_id = "a".repeat(64);
    let error = daemon
        .analyze_with_externs(&request, &[tampered])
        .err()
        .expect("tampered interface must be refused");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);

    // Bytes and names are validated at construction time too.
    assert!(ExternInput::new("core", "core", Vec::new()).is_err());
    assert!(ExternInput::new("../core", "core", bytes.clone()).is_err());

    // The daemon artifact store verifies the bytes it serves.
    let (interface, _, _) = analyze_dependency(&daemon, core_v1());
    let artifact_path = work.join("artifacts").join(format!("{interface}.rmeta"));
    assert!(artifact_path.is_file(), "artifact must be stored");
    std::fs::write(&artifact_path, b"tampered artifact").expect("tamper artifact");
    let error = daemon
        .artifact(&interface)
        .err()
        .expect("tampered artifact must be refused");
    assert_eq!(error.kind, ErrorKind::SnapshotCorrupt);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn legacy_schema_one_snapshot_still_verifies_without_externs() {
    let toolchain = ToolchainInfo {
        release: "1.95.0".to_owned(),
        host: "x86_64-pc-windows-msvc".to_owned(),
        commit_hash: "legacy-test".to_owned(),
    };
    let request = request("legacy", "legacy_unit", "pub fn legacy() {}\n");
    let identity = request.identity(&toolchain).expect("identity");
    assert!(identity.externs.is_empty());
    let files = request.normalized_files().expect("files");
    let root = temp_dir("legacy-store");
    let store = SnapshotStore::new(root.join("snapshots")).expect("store");
    let materialized = store.materialize(&identity, &files).expect("materialize");
    let manifest_path = materialized.dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("manifest")).expect("json");
    manifest["schema_version"] = serde_json::json!(1);
    if let Some(object) = manifest.as_object_mut() {
        object.remove("externs");
    }
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("json"),
    )
    .expect("write legacy manifest");
    assert_eq!(
        verify_snapshot(&materialized.dir, &identity).expect("legacy schema 1 must verify"),
        1
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&temp_dir("legacy-store-unused"));
}

#[test]
fn extern_digest_is_order_independent_and_interface_sensitive() {
    let _guard = rustc_guard();
    let work = temp_dir("digest");
    let daemon = daemon(&work);
    let (_, bytes_a, _) = analyze_dependency(&daemon, core_v1());
    let (_, bytes_b, _) = analyze_dependency(&daemon, core_v2_compatible());
    let a = ExternInput::new("core", "core", bytes_a).expect("a");
    let b = ExternInput::new("core", "core", bytes_b).expect("b");
    let digest_a = ravum_daemon::externs_digest(&[a.reference()]);
    let digest_b = ravum_daemon::externs_digest(&[b.reference()]);
    assert_ne!(digest_a, digest_b);
    // The same set in any order has the same digest.
    let x = ExternInput::new("alpha", "alpha", vec![1u8; 16])
        .expect("x")
        .reference();
    let y = ExternInput::new("beta", "beta", vec![2u8; 16])
        .expect("y")
        .reference();
    assert_eq!(
        ravum_daemon::externs_digest(&[x.clone(), y.clone()]),
        ravum_daemon::externs_digest(&[y, x])
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn extern_inputs_roundtrip_through_json_protocol_shape() {
    // A wire-format extern (hex payload) deserializes fail-closed and is accepted
    // by an analysis; a mismatched interface id is refused at deserialization.
    let _guard = rustc_guard();
    let work = temp_dir("wire");
    let daemon = daemon(&work);
    let (interface, bytes, _) = analyze_dependency(&daemon, core_v1());
    let wire = serde_json::json!({
        "name": "core",
        "crate_name": "core",
        "interface_id": interface,
        "len": bytes.len(),
        "rmeta_hex": bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
    });
    let extern_input: ExternInput =
        serde_json::from_value(wire.clone()).expect("wire extern must deserialize");
    assert_eq!(extern_input.bytes, bytes);
    let mut bad_wire = wire;
    bad_wire["interface_id"] = serde_json::json!("b".repeat(64));
    assert!(
        serde_json::from_value::<ExternInput>(bad_wire).is_err(),
        "mismatched interface id must be refused at deserialization"
    );
    // The serde path is what the JSON-lines protocol uses; the analysis succeeds.
    let report = daemon
        .analyze_with_externs(&request("api", "api", api_v1()), &[extern_input])
        .expect("wire chained analysis");
    assert!(report.success);
    let _ = std::fs::remove_dir_all(&work);
}
#[test]
fn transitive_extern_closure_decodes_re_exports() {
    let _guard = rustc_guard();
    let work = temp_dir("closure");
    let daemon = daemon(&work);
    // core exposes a type, api re-exports it, consumer uses the re-export. To
    // decode `api::Point`, rustc needs core's metadata too: this exercises the
    // transitive interface closure, not just the direct extern.
    let core_source = "pub struct Point { pub x: f64 }\npub fn scale(x: f64) -> f64 { x * 2.0 }\n";
    let (_, core_bytes, _) = analyze_dependency(&daemon, core_source);
    let core_extern = ExternInput::new("core", "core", core_bytes).expect("core extern");
    let api_source =
        "pub use core::Point;\npub fn midpoint(x: f64) -> Point { Point { x: core::scale(x) } }\n";
    let api_report = daemon
        .analyze_with_externs(&request("api", "api", api_source), &[core_extern])
        .expect("api analysis");
    assert!(api_report.success, "api must compile: {api_report:?}");
    let api_interface = api_report.artifact_sha256.clone().expect("api artifact");
    let api_bytes = daemon.artifact(&api_interface).expect("api artifact bytes");
    let consumer_content = "pub fn use_re_export(x: f64) -> api::Point { api::midpoint(x) }\n";
    let consumer = daemon
        .analyze_with_externs(
            &request("consumer", "consumer", consumer_content),
            &[ExternInput::new("api", "api", api_bytes).expect("api extern")],
        )
        .expect("consumer analysis");
    assert!(
        consumer.success,
        "the transitive interface closure must let consumer decode api::Point: {consumer:?}"
    );
    assert!(consumer.timings.analysis_ms > 0);
    let _ = std::fs::remove_dir_all(&work);
}
