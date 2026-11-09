//! Separated identities (INV-04): content, context, interface, externs, verdict.
//!
//! None of these are interchangeable: a dependent is never considered up to date
//! from its own sources alone, and a compiler artifact is identified by the bytes
//! rustc actually consumed (`interface_id` = SHA-256 of the `.rmeta`).

use crate::archive::ExternArtifact;
use crate::graph::Unit;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::process::Command;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Compilation context, shared by every unit of one campaign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSpec {
    /// SHA-256 of the exact `rustc -vV` output used for this campaign.
    pub toolchain_id: String,
    /// Host target (`host:` line of `rustc -vV`).
    pub host_target: String,
    /// Requested target; `None` means host. A non-host target forces fallback.
    pub target: Option<String>,
    /// Extra rustc flags of the harness (validated, never arbitrary).
    pub flags: Vec<String>,
}

impl ContextSpec {
    /// Reads the local toolchain identity from the public `rustc -vV` output.
    pub fn from_rustc(flags: Vec<String>) -> Result<Self, String> {
        let rustc = std::env::var_os("RAVUM_RUSTC").unwrap_or_else(|| "rustc".into());
        let output = Command::new(rustc)
            .arg("-vV")
            .output()
            .map_err(|error| format!("cannot spawn `rustc -vV`: {error}"))?;
        if !output.status.success() {
            return Err(format!("`rustc -vV` failed with {}", output.status));
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let host_target = text
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .map(str::trim)
            .ok_or_else(|| "`rustc -vV` output has no host line".to_owned())?
            .to_owned();
        let context = Self {
            toolchain_id: sha256_hex(text.as_bytes()),
            host_target,
            target: None,
            flags,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.toolchain_id.len() != 64
            || !self.toolchain_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("context: toolchain_id must be a 64-character hex SHA-256".to_owned());
        }
        if self.host_target.trim().is_empty() {
            return Err("context: host_target is empty".to_owned());
        }
        if let Some(target) = &self.target {
            if target.trim().is_empty() {
                return Err("context: target is empty".to_owned());
            }
        }
        for flag in &self.flags {
            if flag.trim().is_empty() || flag.contains('\0') || flag.contains('\n') {
                return Err(format!("context: invalid flag {flag:?}"));
            }
            let allowed_passthrough = flag.starts_with("-A")
                || flag.starts_with("-W")
                || flag.starts_with("-D")
                || flag == "--cap-lints=allow"
                || flag.starts_with("--cap-lints=");
            let allowed_codegen = [
                "-C opt-level=",
                "-C debuginfo=",
                "-C overflow-checks=",
                "-C force-frame-pointers=",
                "-C panic=",
            ]
            .iter()
            .any(|prefix| flag.starts_with(prefix));
            if !allowed_passthrough && !allowed_codegen {
                return Err(format!(
                    "context: flag {flag:?} is not allowed (identity-bearing flags such as \
                     --extern/-L/--cfg/--target/--emit/-C incremental are controlled by the runner)"
                ));
            }
        }
        Ok(())
    }
}

/// Canonical content identity of one unit: `Cargo.toml`, `build.rs` and every
/// `.rs` file, in sorted relative-path order, with length-separated bytes.
pub fn content_id(files: &[(String, Vec<u8>)]) -> String {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    hasher.update(b"ravum-content-v1\0");
    for (path, content) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
        hasher.update(b"\0");
    }
    hex(&hasher.finalize())
}

/// Context identity of one unit: toolchain + target + edition + crate name +
/// features + flags. Never mixed with the content identity.
pub fn context_id(context: &ContextSpec, unit: &Unit) -> String {
    let canonical = serde_json::json!({
        "v": 1,
        "toolchain_id": context.toolchain_id,
        "host_target": context.host_target,
        "target": context.target,
        "edition": unit.edition,
        "crate_name": unit.crate_name,
        "features": Vec::<String>::new(),
        "flags": context.flags,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    sha256_hex(&bytes)
}

/// Ordered digest of the direct externs, binding each extern name to the exact
/// interface rustc will read.
pub fn externs_digest(externs: &[ExternArtifact]) -> String {
    let mut sorted: Vec<&ExternArtifact> = externs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut hasher = Sha256::new();
    hasher.update(b"ravum-externs-v1\0");
    for extern_artifact in sorted {
        hasher.update(extern_artifact.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(extern_artifact.interface_id.as_bytes());
        hasher.update(b"\0");
    }
    hex(&hasher.finalize())
}

/// Cache key: `(content_id, context_id, externs, toolchain)`. The toolchain is
/// repeated in the context identity; keeping it explicit makes the invariant
/// visible and prevents "same sources" shortcuts.
pub fn verdict_key(
    content_id: &str,
    context_id: &str,
    externs_digest: &str,
    toolchain_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ravum-verdict-key-v1\0");
    for part in [content_id, context_id, externs_digest, toolchain_id] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hex(&hasher.finalize())
}

fn hex(digest: &[u8]) -> String {
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
