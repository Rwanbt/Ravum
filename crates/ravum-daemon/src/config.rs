//! Build context, toolchain identity, priority and daemon configuration.
//!
//! A result is valid only for one `(source content, build context, toolchain)`
//! triple. All three are reduced to SHA-256 identities *before* any compilation
//! so cache invalidation is a pure identity comparison, never a guess.

use crate::auto_gc::AutoGcPolicy;
use crate::error::{DaemonError, Result};
use crate::hash::push_field;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Version of the daemon's on-disk cache/snapshot formats. Changing any stored
/// schema requires bumping this constant so old entries are misses, not
/// misinterpreted data.
pub const DAEMON_SCHEMA_VERSION: u32 = 1;

/// Version of the analysis pipeline itself (arguments, normalization rules).
/// Included in cache identities: a daemon upgrade invalidates cached verdicts.
pub const ANALYSIS_ENGINE_VERSION: &str = "rustc-cli/metadata-v1";

/// Hard limits that keep one runaway request from exhausting the reference
/// machine. They are contract limits: requests above them are rejected before
/// any filesystem write or process spawn.
pub const MAX_FILES_PER_UNIT: usize = 64;
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_LOGICAL_PATH_BYTES: usize = 512;
pub const MAX_FLAGS: usize = 64;
pub const MAX_FLAG_BYTES: usize = 512;

/// Toolchain identity, measured once per daemon through the public `-vV` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainInfo {
    pub release: String,
    pub host: String,
    pub commit_hash: String,
}

impl ToolchainInfo {
    /// Queries `rustc -vV`. The ambient `RUSTFLAGS`/wrapper variables are
    /// removed so the identity cannot be silently distorted by the caller's
    /// shell (same INV-11 discipline as `ravum-rust-adapter`).
    pub fn query(rustc: &Path) -> Result<Self> {
        let output = Command::new(rustc)
            .arg("-vV")
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .output()
            .map_err(|e| {
                DaemonError::toolchain(format!("cannot spawn {} -vV: {e}", rustc.display()))
            })?;
        if !output.status.success() {
            return Err(DaemonError::toolchain(format!(
                "{} -vV exited with {:?}",
                rustc.display(),
                output.status.code()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key).map(|v| v.trim().to_owned()))
                .unwrap_or_else(|| "unknown".to_owned())
        };
        Ok(ToolchainInfo {
            release: field("release:"),
            host: field("host:"),
            commit_hash: field("commit-hash:"),
        })
    }

    /// Stable identity of this toolchain inside every cache key.
    pub fn id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-toolchain-v1");
        push_field(&mut hasher, self.release.as_bytes());
        push_field(&mut hasher, self.host.as_bytes());
        push_field(&mut hasher, self.commit_hash.as_bytes());
        hex(&hasher.finalize())
    }
}

/// Declared build context of an analysis request. Flags are *part of the
/// semantics*: `-D warnings` changes the result, so it must change the cache
/// key as well.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildContext {
    pub edition: String,
    pub crate_name: String,
    pub crate_type: String,
    pub flags: Vec<String>,
}

impl Default for BuildContext {
    fn default() -> Self {
        Self {
            edition: "2021".to_owned(),
            crate_name: "ravum_unit".to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        }
    }
}

impl BuildContext {
    /// Rejects a context the daemon cannot honour safely. In particular the
    /// daemon owns `--out-dir`, `--emit`, the error format, the crate identity
    /// and the incremental directory: a caller flag that would override one of
    /// those is not a build context, it is an attempt to escape the contract.
    pub fn validate(&self) -> Result<()> {
        if !is_valid_edition(&self.edition) {
            return Err(DaemonError::invalid(format!(
                "unsupported edition {:?} (expected one of 2015, 2018, 2021, 2024)",
                self.edition
            )));
        }
        if !is_valid_crate_name(&self.crate_name) {
            return Err(DaemonError::invalid(format!(
                "invalid crate_name {:?}: expected [A-Za-z_][A-Za-z0-9_]*",
                self.crate_name
            )));
        }
        if self.crate_type != "lib" && self.crate_type != "rlib" {
            return Err(DaemonError::invalid(format!(
                "unsupported crate_type {:?}: this daemon only emits metadata for library units",
                self.crate_type
            )));
        }
        if self.flags.len() > MAX_FLAGS {
            return Err(DaemonError::invalid(format!(
                "too many build flags ({} > {MAX_FLAGS})",
                self.flags.len()
            )));
        }
        for flag in &self.flags {
            if flag.is_empty() || flag.len() > MAX_FLAG_BYTES {
                return Err(DaemonError::invalid(
                    "empty or oversized build flag".to_owned(),
                ));
            }
            if flag.chars().any(char::is_control) {
                return Err(DaemonError::invalid(
                    "build flags must not contain control characters".to_owned(),
                ));
            }
            if is_reserved_flag(flag) {
                return Err(DaemonError::invalid(format!(
                    "build flag {flag:?} is reserved by the daemon contract"
                )));
            }
        }
        Ok(())
    }

    /// Identity of `(context, toolchain)`, used in the snapshot and cache keys.
    pub fn id(&self, toolchain: &ToolchainInfo) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-context-v1");
        push_field(&mut hasher, ANALYSIS_ENGINE_VERSION.as_bytes());
        push_field(&mut hasher, self.edition.as_bytes());
        push_field(&mut hasher, self.crate_name.as_bytes());
        push_field(&mut hasher, self.crate_type.as_bytes());
        for flag in &self.flags {
            push_field(&mut hasher, flag.as_bytes());
        }
        push_field(&mut hasher, toolchain.id().as_bytes());
        hex(&hasher.finalize())
    }
}

fn is_valid_edition(edition: &str) -> bool {
    matches!(edition, "2015" | "2018" | "2021" | "2024")
}

fn is_valid_crate_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    name.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.is_empty()
}

/// Flags the daemon must keep sole control of to preserve provenance and
/// isolation. Rejecting them is fail-closed: better a clear error than a result
/// attributed to the wrong context.
fn is_reserved_flag(flag: &str) -> bool {
    let lower = flag.to_ascii_lowercase();
    lower.starts_with("--out-dir")
        || lower == "-o"
        || lower.starts_with("-o=")
        || lower.starts_with("--emit")
        || lower.starts_with("--error-format")
        || lower.starts_with("--json")
        || lower.starts_with("--crate-name")
        || lower.starts_with("--crate-type")
        || lower.starts_with("--edition")
        || lower.starts_with("incremental=")
        || lower.starts_with("-cincremental")
        || lower == "@"
        || flag.starts_with('@')
        || lower.contains("incremental=")
}

/// Interactive priority. Interactive requests are admitted before background
/// ones when the job gate is saturated; the comparison is explicit so a future
/// scheduler cannot silently reorder them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Bulk/background work: yields to everything else.
    Background,
    /// Default editing-request priority.
    #[default]
    Normal,
    /// Keystroke-latency work; admitted first.
    Interactive,
}

/// Daemon configuration. `work_dir` is the daemon's private directory: user
/// files are never written, only read by the caller before the request.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// `rustc` executable, defaulting to `RAVUM_RUSTC` or `rustc` from `PATH`.
    pub rustc: PathBuf,
    /// Private working directory (snapshots, cache, incremental state, output).
    pub work_dir: PathBuf,
    /// Maximum number of concurrent `rustc` processes. The reference machine
    /// has ~2 Go free: default 1, never above 2 for measurement campaigns.
    pub max_concurrent_jobs: usize,
    /// In-memory L2 cache size; the on-disk cache is content-addressed and
    /// bounded by the filesystem, not by this value.
    pub max_cache_entries: usize,
    /// Cancellation and stale-check polling interval for a running `rustc`.
    pub poll_interval_ms: u64,
    /// Hard wall-clock cap for one analysis; a runaway compile is killed and
    /// reported as `timed_out`.
    pub analysis_timeout_ms: u64,
    /// Test seam: force the toolchain identity instead of querying `rustc`.
    /// Used by the invalidation tests to prove that a toolchain change is a
    /// cache miss without installing a second toolchain.
    pub toolchain_override: Option<ToolchainInfo>,
    /// Opt-in automatic background retention GC. Disabled by default; when
    /// enabled, `Daemon::open` starts one maintenance thread that evaluates the
    /// configured byte thresholds off the analysis path.
    pub auto_gc: AutoGcPolicy,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            rustc: std::env::var_os("RAVUM_RUSTC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("rustc")),
            work_dir: PathBuf::from("target").join("ravum-daemon"),
            max_concurrent_jobs: 1,
            max_cache_entries: 256,
            poll_interval_ms: 2,
            analysis_timeout_ms: 30_000,
            toolchain_override: None,
            auto_gc: AutoGcPolicy::default(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Convenience: identity of `(context, toolchain)` without a daemon instance.
pub fn context_id(context: &BuildContext, toolchain: &ToolchainInfo) -> String {
    context.id(toolchain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolchain() -> ToolchainInfo {
        ToolchainInfo {
            release: "1.95.0".to_owned(),
            host: "x86_64-pc-windows-msvc".to_owned(),
            commit_hash: "59807616e".to_owned(),
        }
    }

    #[test]
    fn context_identity_changes_with_flags() {
        let tc = toolchain();
        let mut a = BuildContext::default();
        let b = BuildContext::default();
        let id_a = a.id(&tc);
        assert_eq!(id_a, b.id(&tc), "same context must be stable");
        a.flags.push("-D".to_owned());
        a.flags.push("warnings".to_owned());
        assert_ne!(id_a, a.id(&tc), "flags are semantic: identity must change");
    }

    #[test]
    fn context_identity_changes_with_toolchain() {
        let a = BuildContext::default();
        let mut other = toolchain();
        other.release = "1.98.1".to_owned();
        assert_ne!(a.id(&toolchain()), a.id(&other));
    }

    #[test]
    fn reserved_flags_are_rejected() {
        let mut context = BuildContext::default();
        for flag in [
            "--out-dir=elsewhere",
            "--emit=link",
            "--error-format=human",
            "--crate-name=other",
            "-C",
        ] {
            context.flags = vec![flag.to_owned()];
            // `-C` alone is not reserved; it is rejected by rustc, not by us.
            if flag == "-C" {
                continue;
            }
            assert!(
                context.validate().is_err(),
                "reserved flag {flag:?} must be rejected"
            );
        }
        context.flags = vec!["-C".to_owned(), "incremental=/tmp/x".to_owned()];
        assert!(
            context.validate().is_err(),
            "incremental directory is daemon-owned"
        );
        context.flags = vec!["-D".to_owned(), "warnings".to_owned()];
        assert!(context.validate().is_ok(), "legitimate flags must pass");
    }

    #[test]
    fn invalid_crate_name_and_edition_are_rejected() {
        let mut context = BuildContext::default();
        context.edition = "1999".to_owned();
        assert!(context.validate().is_err());
        context.edition = "2021".to_owned();
        context.crate_name = "not-a-name".to_owned();
        assert!(context.validate().is_err());
    }

    #[test]
    fn priority_orders_interactive_first() {
        let mut levels = vec![Priority::Normal, Priority::Interactive, Priority::Background];
        levels.sort();
        assert_eq!(
            levels,
            vec![
                Priority::Background,
                Priority::Normal,
                Priority::Interactive
            ]
        );
    }
}
