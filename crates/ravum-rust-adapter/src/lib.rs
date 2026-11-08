//! Rust analysis/build adapter built exclusively on the public `rustc` CLI.
//!
//! INV-11 compliance: no `rustc_private`, no `RUSTC_BOOTSTRAP`, and ambient
//! `RUSTFLAGS`/wrapper variables are removed from the child environment so a
//! build result can be attributed to the declared context.
//!
//! Builds are cached on disk by `(SourceSnapshotId, BuildContextId)`. This is not
//! an optimisation detail: on Windows, re-linking over a DLL that the running host
//! has loaded fails, and MSVC output is **not** byte-reproducible (measured: two
//! builds of the same source differ). The cache gives a stable artifact per
//! content identity without ever rewriting a loaded file.

use ravum_protocol::{
    sha256_hex, ArtifactId, BuildContextId, BuildOutput, DiagnosticSet, PATCH_SCHEMA_VERSION,
};
use ravum_snapshot::SourceSnapshot;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

mod parse;

pub use parse::parse_rustc_json;

/// Disambiguates artifact file names within one process; never reused across runs,
/// so a stale file is always a cache miss, never a silent overwrite.
static BUILD_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct AdapterError(pub String);

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AdapterError {}

impl From<std::io::Error> for AdapterError {
    fn from(e: std::io::Error) -> Self {
        AdapterError(format!("io error: {e}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainInfo {
    pub release: String,
    pub host: String,
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub diagnostics: DiagnosticSet,
    pub check_ms: u64,
}

#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub build: BuildOutput,
    pub success: bool,
}

/// Provenance of one cached artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBuild {
    schema_version: u32,
    artifact_path: String,
    artifact_id: String,
    source_snapshot_id: String,
    build_context_id: String,
    tool_version: String,
    target: String,
    built_at_unix: u64,
}

pub struct RustAdapter {
    rustc: PathBuf,
    edition: String,
    work_dir: PathBuf,
    artifact_cache: bool,
    toolchain: OnceLock<ToolchainInfo>,
}

struct Captured {
    stdout: String,
    stderr: String,
    code: i32,
}

impl RustAdapter {
    pub fn new(rustc: impl Into<PathBuf>, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            rustc: rustc.into(),
            edition: "2021".to_owned(),
            work_dir: work_dir.into(),
            artifact_cache: true,
            toolchain: OnceLock::new(),
        }
    }

    /// Disable artifact reuse. Measurement harnesses use this so that a "build"
    /// number always means a real `rustc` invocation.
    pub fn with_artifact_cache(mut self, enabled: bool) -> Self {
        self.artifact_cache = enabled;
        self
    }

    pub fn set_artifact_cache(&mut self, enabled: bool) {
        self.artifact_cache = enabled;
    }

    pub fn artifact_cache_enabled(&self) -> bool {
        self.artifact_cache
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    pub fn toolchain(&self) -> Result<ToolchainInfo, AdapterError> {
        if let Some(info) = self.toolchain.get() {
            return Ok(info.clone());
        }
        let out = self.execute(&["-vV".to_owned()])?;
        let release = field(&out.stdout, "release:").unwrap_or_else(|| "unknown".to_owned());
        let host = field(&out.stdout, "host:").unwrap_or_else(|| "unknown".to_owned());
        Ok(self
            .toolchain
            .get_or_init(|| ToolchainInfo { release, host })
            .clone())
    }

    /// Analysis only: `--emit=metadata`, no machine code, no link.
    pub fn check(&self, snapshot: &SourceSnapshot) -> Result<CheckOutcome, AdapterError> {
        let toolchain = self.toolchain()?;
        let source = self.write_source(snapshot, "check")?;
        let out_dir = self.work_dir.join("check").join(snapshot.id().short());
        std::fs::create_dir_all(&out_dir)?;

        let args = vec![
            "--edition".to_owned(),
            self.edition.clone(),
            "--crate-name".to_owned(),
            "ravum_check".to_owned(),
            "--crate-type".to_owned(),
            "lib".to_owned(),
            "--emit=metadata".to_owned(),
            "--error-format=json".to_owned(),
            "--out-dir".to_owned(),
            out_dir.to_string_lossy().to_string(),
            source.to_string_lossy().to_string(),
        ];
        let started = Instant::now();
        let out = self.execute(&args)?;
        let check_ms = started.elapsed().as_millis() as u64;
        Ok(CheckOutcome {
            diagnostics: parse::parse_rustc_json(
                &out.stderr,
                snapshot.id(),
                "rustc",
                &toolchain.release,
                out.code == 0,
            ),
            check_ms,
        })
    }

    /// Codegen + link into a `cdylib`.
    ///
    /// Reuses the artifact previously built for the same snapshot and build
    /// context; otherwise invokes `rustc` once and records the provenance.
    pub fn build_cdylib(
        &self,
        snapshot: &SourceSnapshot,
        out_dir: &Path,
    ) -> Result<BuildOutcome, AdapterError> {
        std::fs::create_dir_all(out_dir)?;
        let toolchain = self.toolchain()?;
        let flags = self.build_flags();
        let build_context_id = build_context_id(&toolchain, &self.edition, &flags);
        let cache_path = out_dir.join(format!(
            "build-{}-{}.json",
            snapshot.id().short(),
            build_context_id.short()
        ));

        if self.artifact_cache {
            if let Some(cached) = self.reuse_cached(
                &cache_path,
                snapshot,
                &build_context_id,
                &toolchain,
                &flags,
            )? {
                return Ok(cached);
            }
        }

        let source = self.write_source(snapshot, "build")?;
        let stamp = BUILD_SEQ.fetch_add(1, Ordering::Relaxed);
        let artifact = out_dir.join(format!(
            "patch-{}-{}-{stamp}.{}",
            snapshot.id().short(),
            build_context_id.short(),
            std::env::consts::DLL_EXTENSION
        ));

        let mut args = vec![
            "--edition".to_owned(),
            self.edition.clone(),
            "--crate-name".to_owned(),
            "ravum_patch".to_owned(),
            "--crate-type".to_owned(),
            "cdylib".to_owned(),
            "--error-format=json".to_owned(),
        ];
        args.extend(flags.iter().cloned());
        args.push("-o".to_owned());
        args.push(artifact.to_string_lossy().to_string());
        args.push(source.to_string_lossy().to_string());

        let started = Instant::now();
        let out = self.execute(&args)?;
        let build_ms = started.elapsed().as_millis() as u64;
        let diagnostics = parse::parse_rustc_json(
            &out.stderr,
            snapshot.id(),
            "rustc",
            &toolchain.release,
            out.code == 0,
        );
        let success = out.code == 0 && !diagnostics.has_errors() && artifact.is_file();

        let bytes = std::fs::read(&artifact).unwrap_or_default();
        let artifact_id = ArtifactId::new(sha256_hex(&bytes));
        if success {
            let entry = CachedBuild {
                schema_version: 1,
                artifact_path: artifact.to_string_lossy().to_string(),
                artifact_id: artifact_id.to_string(),
                source_snapshot_id: snapshot.id().to_string(),
                build_context_id: build_context_id.to_string(),
                tool_version: toolchain.release.clone(),
                target: toolchain.host.clone(),
                built_at_unix: ravum_protocol::now_unix(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&entry) {
                let _ = std::fs::write(&cache_path, json);
            }
        }

        Ok(BuildOutcome {
            build: BuildOutput {
                schema_version: PATCH_SCHEMA_VERSION,
                artifact_path: artifact.to_string_lossy().to_string(),
                artifact_id,
                source_snapshot_id: snapshot.id().clone(),
                build_context_id,
                tool: "rustc".to_owned(),
                tool_version: toolchain.release,
                target: toolchain.host,
                flags,
                build_ms,
                cached_build: false,
                diagnostics,
            },
            success,
        })
    }

    fn reuse_cached(
        &self,
        cache_path: &Path,
        snapshot: &SourceSnapshot,
        build_context_id: &BuildContextId,
        toolchain: &ToolchainInfo,
        flags: &[String],
    ) -> Result<Option<BuildOutcome>, AdapterError> {
        let Ok(text) = std::fs::read_to_string(cache_path) else {
            return Ok(None);
        };
        let Ok(entry) = serde_json::from_str::<CachedBuild>(&text) else {
            return Ok(None);
        };
        if entry.source_snapshot_id != snapshot.id().as_str()
            || entry.build_context_id != build_context_id.as_str()
        {
            return Ok(None);
        }
        let path = PathBuf::from(&entry.artifact_path);
        if !path.is_file() {
            return Ok(None);
        }
        let started = Instant::now();
        let bytes = std::fs::read(&path)?;
        let artifact_id = ArtifactId::new(sha256_hex(&bytes));
        // A modified artifact is a miss: never serve bytes that no longer match
        // the verdict they were produced with.
        if artifact_id.as_str() != entry.artifact_id {
            return Ok(None);
        }
        let build_ms = started.elapsed().as_millis() as u64;
        Ok(Some(BuildOutcome {
            build: BuildOutput {
                schema_version: PATCH_SCHEMA_VERSION,
                artifact_path: entry.artifact_path,
                artifact_id,
                source_snapshot_id: snapshot.id().clone(),
                build_context_id: build_context_id.clone(),
                tool: "rustc".to_owned(),
                tool_version: toolchain.release.clone(),
                target: toolchain.host.clone(),
                flags: flags.to_vec(),
                build_ms,
                cached_build: true,
                diagnostics: DiagnosticSet::new(
                    snapshot.id().clone(),
                    "rustc",
                    &toolchain.release,
                ),
            },
            success: true,
        }))
    }

    pub fn build_flags(&self) -> Vec<String> {
        vec![
            "-C".to_owned(),
            "opt-level=1".to_owned(),
            "-C".to_owned(),
            "debuginfo=0".to_owned(),
            // A panic must never unwind across the FFI boundary: abort is the
            // documented, fail-closed behaviour of the M1 patch ABI.
            "-C".to_owned(),
            "panic=abort".to_owned(),
        ]
    }

    fn write_source(&self, snapshot: &SourceSnapshot, phase: &str) -> Result<PathBuf, AdapterError> {
        let dir = self.work_dir.join(phase);
        std::fs::create_dir_all(&dir)?;
        let path_name = snapshot
            .paths()
            .next()
            .ok_or_else(|| AdapterError("snapshot is empty".to_owned()))?;
        let content = snapshot
            .content_of(path_name)
            .ok_or_else(|| AdapterError("snapshot file vanished".to_owned()))?;
        let path = dir.join(format!("unit-{}.rs", snapshot.id().short()));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn execute(&self, args: &[String]) -> Result<Captured, AdapterError> {
        let out = Command::new(&self.rustc)
            .args(args)
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .output()
            .map_err(|e| AdapterError(format!("failed to spawn {}: {e}", self.rustc.display())))?;
        Ok(Captured {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            code: out.status.code().unwrap_or(-1),
        })
    }
}

/// Identity of the build context, derived before any compilation happens so the
/// artifact cache can be keyed on it.
pub fn build_context_id(
    toolchain: &ToolchainInfo,
    edition: &str,
    flags: &[String],
) -> BuildContextId {
    BuildContextId::new(sha256_hex(
        format!(
            "ravum-build-v1\0{}\0{}\0{edition}\0cdylib\0{}",
            toolchain.release,
            toolchain.host,
            flags.join(" ")
        )
        .as_bytes(),
    ))
}

fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_owned()))
}
