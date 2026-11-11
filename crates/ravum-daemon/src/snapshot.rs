//! Immutable content-addressed snapshots and mutable stable compile units.
//!
//! Two distinct concepts, deliberately separated:
//!
//! * [`SnapshotStore`] keeps an **immutable** copy of every submitted unit under
//!   its content identity (source bytes + build context + toolchain). It is the
//!   proof of what was analysed and is never overwritten.
//! * [`sync_unit_dir`] mirrors the latest content into a **stable path** so
//!   repeated `rustc` invocations see the same file names, like an editor that
//!   keeps saving the same file. `-C incremental` reuses work across edits only
//!   when file identities are stable; recompiling from a fresh directory per
//!   edit would throw that reuse away.
//!
//! User files are never written: everything lives below the daemon's own
//! `work_dir`.

use crate::config::{
    validate_extern_name, validate_interface_id, BuildContext, Priority, ToolchainInfo,
    DAEMON_SCHEMA_VERSION, MAX_EXTERNS_PER_UNIT, MAX_EXTERN_BYTES, MAX_FILES_PER_UNIT,
    MAX_FILE_BYTES, MAX_LABEL_BYTES, MAX_LOGICAL_PATH_BYTES, MAX_TOTAL_BYTES,
    SNAPSHOT_MANIFEST_SCHEMA_VERSION,
};
use crate::diagnostics::normalize_path;
use crate::error::{DaemonError, Result};
use crate::fsutil::{atomic_write, safe_join};
use crate::hash::{push_field, sha256_hex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// One source file submitted by the caller (unsaved editor buffers included).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

/// One analysis request. `revision` is the caller's monotonic edit counter; a
/// response for a superseded revision is never returned as current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub revision: u64,
    pub label: String,
    pub root: String,
    pub files: Vec<SourceFile>,
    pub context: BuildContext,
    #[serde(default)]
    pub priority: Priority,
}

impl AnalysisRequest {
    /// Validates the request contract before any filesystem write or process
    /// spawn. Returns the normalized (sorted, `/`-separated) file list.
    pub fn validate(&self) -> Result<()> {
        self.normalized_files().map(|_| ())
    }

    /// Normalized, sorted `(logical_path, content)` pairs.
    pub fn normalized_files(&self) -> Result<Vec<(String, &str)>> {
        if self.label.is_empty() || self.label.len() > MAX_LABEL_BYTES {
            return Err(DaemonError::invalid(format!(
                "label must be 1..={MAX_LABEL_BYTES} bytes"
            )));
        }
        if self.label.chars().any(char::is_control) {
            return Err(DaemonError::invalid(
                "label must not contain control characters".to_owned(),
            ));
        }
        self.context.validate()?;
        if self.files.is_empty() {
            return Err(DaemonError::invalid("unit has no source file".to_owned()));
        }
        if self.files.len() > MAX_FILES_PER_UNIT {
            return Err(DaemonError::invalid(format!(
                "unit has too many files ({} > {MAX_FILES_PER_UNIT})",
                self.files.len()
            )));
        }
        let mut normalized: Vec<(String, &str)> = Vec::with_capacity(self.files.len());
        let mut total_bytes: usize = 0;
        for file in &self.files {
            let path = validate_logical_path(&file.path)?;
            let bytes = file.content.len();
            if bytes > MAX_FILE_BYTES {
                return Err(DaemonError::invalid(format!(
                    "file {path:?} is too large ({bytes} > {MAX_FILE_BYTES} bytes)"
                )));
            }
            total_bytes = total_bytes.saturating_add(bytes);
            normalized.push((path, file.content.as_str()));
        }
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(DaemonError::invalid(format!(
                "unit is too large ({total_bytes} > {MAX_TOTAL_BYTES} bytes)"
            )));
        }
        normalized.sort_by(|a, b| a.0.cmp(&b.0));
        for pair in normalized.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(DaemonError::invalid(format!(
                    "duplicate logical path {:?}",
                    pair[0].0
                )));
            }
        }
        let root = validate_logical_path(&self.root)?;
        if !normalized.iter().any(|(path, _)| *path == root) {
            return Err(DaemonError::invalid(format!(
                "root {root:?} is not one of the submitted files"
            )));
        }
        Ok(normalized)
    }

    /// Content identity of the request: source bytes + build context + toolchain.
    /// No externs: equivalent to [`AnalysisRequest::identity_with_externs`] with an
    /// empty extern list (legacy behaviour preserved byte-for-byte).
    pub fn identity(&self, toolchain: &ToolchainInfo) -> Result<SnapshotIdentity> {
        self.identity_with_externs(toolchain, &[])
    }

    /// Content identity including the content-addressed externs: two requests with
    /// identical sources but different extern bytes/interfaces get different
    /// `snapshot_id`s, so every cache keyed by it (memory, disk, snapshot store)
    /// misses when an extern changes. The hash of the zero-extern case is
    /// unchanged, preserving legacy snapshot ids.
    pub fn identity_with_externs(
        &self,
        toolchain: &ToolchainInfo,
        externs: &[ExternInput],
    ) -> Result<SnapshotIdentity> {
        validate_externs(externs)?;
        let files = self.normalized_files()?;
        let context_id = self.context.id(toolchain);
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-snapshot-v1");
        push_field(&mut hasher, validate_logical_path(&self.root)?.as_bytes());
        push_field(&mut hasher, context_id.as_bytes());
        push_field(&mut hasher, toolchain.id().as_bytes());
        let mut file_ids = Vec::with_capacity(files.len());
        let mut total_bytes = 0u64;
        for (path, content) in &files {
            let sha256 = sha256_hex(content.as_bytes());
            push_field(&mut hasher, path.as_bytes());
            push_field(&mut hasher, sha256.as_bytes());
            push_field(&mut hasher, content.as_bytes());
            total_bytes = total_bytes.saturating_add(content.len() as u64);
            file_ids.push(SnapshotFileId {
                path: (*path).clone(),
                sha256,
                len: content.len() as u64,
            });
        }
        let mut extern_refs: Vec<SnapshotExtern> =
            externs.iter().map(ExternInput::reference).collect();
        if !extern_refs.is_empty() {
            extern_refs.sort_by(|a, b| a.name.cmp(&b.name));
            push_field(&mut hasher, b"ravum-daemon-externs-v1");
            for extern_ref in &extern_refs {
                push_field(&mut hasher, extern_ref.name.as_bytes());
                push_field(&mut hasher, extern_ref.crate_name.as_bytes());
                push_field(&mut hasher, extern_ref.interface_id.as_bytes());
                push_field(&mut hasher, &extern_ref.len.to_le_bytes());
            }
        }
        Ok(SnapshotIdentity {
            snapshot_id: hex(&hasher.finalize()),
            context_id,
            toolchain_id: toolchain.id(),
            root: validate_logical_path(&self.root)?,
            files: file_ids,
            total_bytes,
            externs: extern_refs,
        })
    }
}

/// Identity of one immutable snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIdentity {
    pub snapshot_id: String,
    pub context_id: String,
    pub toolchain_id: String,
    pub root: String,
    pub files: Vec<SnapshotFileId>,
    pub total_bytes: u64,
    /// Content-addressed direct externs of this snapshot (empty for legacy/no-extern
    /// requests). Mixed into `snapshot_id`, never kept outside it.
    #[serde(default)]
    pub externs: Vec<SnapshotExtern>,
}

/// Ordered digest of the extern set; used by the verdict cache in addition to the
/// snapshot id (defence in depth: an extern change can never be a HIT).
pub fn externs_digest(externs: &[SnapshotExtern]) -> String {
    let mut ordered: Vec<&SnapshotExtern> = externs.iter().collect();
    ordered.sort_by(|a, b| a.name.cmp(&b.name));
    let mut hasher = Sha256::new();
    hasher.update(b"ravum-daemon-externs-digest-v1\0");
    for extern_ref in ordered {
        push_field(&mut hasher, extern_ref.name.as_bytes());
        push_field(&mut hasher, extern_ref.crate_name.as_bytes());
        push_field(&mut hasher, extern_ref.interface_id.as_bytes());
        push_field(&mut hasher, &extern_ref.len.to_le_bytes());
    }
    hex(&hasher.finalize())
}

impl SnapshotIdentity {
    /// Short form used in file/directory names (never as the sole identity).
    pub fn short(&self) -> &str {
        &self.snapshot_id[..12.min(self.snapshot_id.len())]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFileId {
    pub path: String,
    pub sha256: String,
    pub len: u64,
}

/// One direct extern interface artifact stored inside a snapshot (schema 2).
/// The artifact bytes are content-addressed by `interface_id` (their SHA-256) and
/// the `.rmeta` identity is what rustc reads to validate dependents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotExtern {
    /// Name used by the dependent (`--extern NAME=PATH`).
    pub name: String,
    /// Crate name of the dependency target.
    pub crate_name: String,
    /// SHA-256 of the `.rmeta` bytes.
    pub interface_id: String,
    pub len: u64,
}

/// Caller-provided extern artifact bytes for one materialization.
///
/// Wire format (JSON-lines clients): `{ "name", "crate_name", "interface_id",
/// "len", "rmeta_hex" }`. Deserialization is fail-closed: the interface id must
/// be the exact SHA-256 of the decoded bytes, names must be Rust identifiers and
/// the size is bounded. Rust callers use [`ExternInput::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternInput {
    pub name: String,
    pub crate_name: String,
    pub interface_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternInputWire {
    name: String,
    crate_name: String,
    interface_id: String,
    #[serde(default)]
    len: Option<u64>,
    #[serde(default, alias = "rmeta_hex")]
    bytes_hex: String,
}

impl Serialize for ExternInput {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        // Exact wire schema: {name, crate_name, interface_id, bytes_hex}.
        #[derive(Serialize)]
        struct Wire<'a> {
            name: &'a str,
            crate_name: &'a str,
            interface_id: &'a str,
            bytes_hex: String,
        }
        Wire {
            name: &self.name,
            crate_name: &self.crate_name,
            interface_id: &self.interface_id,
            bytes_hex: hex_encode(&self.bytes),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExternInput {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as DeError;
        let wire = ExternInputWire::deserialize(deserializer)?;
        let bytes = hex_decode(&wire.bytes_hex).map_err(DeError::custom)?;
        if let Some(len) = wire.len {
            if bytes.len() as u64 != len {
                return Err(DeError::custom(format!(
                    "extern {:?}: len {} does not match {} decoded bytes",
                    wire.name,
                    len,
                    bytes.len()
                )));
            }
        }
        let input = ExternInput {
            name: wire.name,
            crate_name: wire.crate_name,
            interface_id: wire.interface_id,
            bytes,
        };
        validate_extern_one(&input).map_err(DeError::custom)?;
        Ok(input)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn hex_decode(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        return Err(DaemonError::invalid("rmeta_hex has an odd length"));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let raw = text.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        let high = hex_digit(raw[index])?;
        let low = hex_digit(raw[index + 1])?;
        bytes.push((high << 4) | low);
        index += 2;
    }
    Ok(bytes)
}

fn hex_digit(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(DaemonError::invalid(format!(
            "rmeta_hex contains a non-hex character {:?}",
            byte as char
        ))),
    }
}

impl ExternInput {
    /// Builds and validates one extern input; `interface_id` must be the exact
    /// SHA-256 of `bytes` (a mismatched label is refused, never trusted).
    pub fn new(
        name: impl Into<String>,
        crate_name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        let name = name.into();
        let crate_name = crate_name.into();
        validate_extern_name(&name)?;
        validate_extern_name(&crate_name)?;
        if bytes.is_empty() || bytes.len() > MAX_EXTERN_BYTES {
            return Err(DaemonError::invalid(format!(
                "extern {name:?} has invalid size {} (1..={MAX_EXTERN_BYTES})",
                bytes.len()
            )));
        }
        Ok(Self {
            interface_id: sha256_hex(&bytes),
            name,
            crate_name,
            bytes,
        })
    }

    pub fn reference(&self) -> SnapshotExtern {
        SnapshotExtern {
            name: self.name.clone(),
            crate_name: self.crate_name.clone(),
            interface_id: self.interface_id.clone(),
            len: self.bytes.len() as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotManifest {
    schema_version: u32,
    snapshot_id: String,
    root: String,
    context_id: String,
    toolchain_id: String,
    total_bytes: u64,
    files: Vec<SnapshotFileId>,
    /// Schema 2: content-addressed direct-dependency interface artifacts. Schema 1
    /// snapshots simply have none.
    #[serde(default)]
    externs: Vec<SnapshotExtern>,
}

/// What materializing a snapshot actually did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedSnapshot {
    /// Immutable snapshot root (`<work>/snapshots/<snapshot_id>`).
    pub dir: PathBuf,
    /// Checkout directory holding the immutable source bytes.
    pub checkout: PathBuf,
    /// Content-addressed extern interface artifacts (`externs/<interface_id>.rmeta`).
    pub externs_dir: PathBuf,
    /// Verified extern references of this snapshot (empty for schema 1 / no externs).
    pub externs: Vec<SnapshotExtern>,
    /// `false` when the snapshot already existed and was verified.
    pub created: bool,
    /// Number of files whose bytes were hash-verified during this call.
    pub verified_files: usize,
}

/// Immutable, content-addressed snapshot store.
pub struct SnapshotStore {
    root: PathBuf,
    write_lock: Mutex<()>,
    seq: AtomicU64,
}

impl SnapshotStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            write_lock: Mutex::new(()),
            seq: AtomicU64::new(0),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir_of(&self, snapshot_id: &str) -> PathBuf {
        self.root.join(snapshot_id)
    }

    /// Writes (or verifies) the immutable snapshot. Never overwrites existing
    /// bytes: an identity that exists with different content is an error, not a
    /// silent replacement. Legacy schema 1 snapshots (no externs) stay readable.
    pub fn materialize(
        &self,
        identity: &SnapshotIdentity,
        files: &[(String, &str)],
    ) -> Result<MaterializedSnapshot> {
        self.materialize_with_externs(identity, files, &[])
    }

    /// Schema 2: also stores the direct-dependency `.rmeta` artifacts by their
    /// interface SHA-256, so a dependent can later be compiled with `--extern`
    /// against the exact bytes that were verified. A mismatched interface id,
    /// duplicate extern name or oversized artifact is refused before any write.
    pub fn materialize_with_externs(
        &self,
        identity: &SnapshotIdentity,
        files: &[(String, &str)],
        externs: &[ExternInput],
    ) -> Result<MaterializedSnapshot> {
        validate_snapshot_id(&identity.snapshot_id)?;
        validate_externs(externs)?;
        let extern_refs: Vec<SnapshotExtern> = externs.iter().map(ExternInput::reference).collect();
        let final_dir = self.root.join(&identity.snapshot_id);
        let checkout = final_dir.join("checkout");
        let externs_dir = final_dir.join("externs");
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| DaemonError::internal("snapshot write lock poisoned"))?;
        if final_dir.exists() {
            let verified = verify_snapshot_with_externs(&final_dir, identity, &extern_refs)?;
            return Ok(MaterializedSnapshot {
                dir: final_dir,
                checkout,
                externs_dir,
                externs: extern_refs,
                created: false,
                verified_files: verified,
            });
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let tmp_dir = self.root.join(format!(
            ".tmp-{}-{seq}-{}",
            std::process::id(),
            identity.short()
        ));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir)?;
        let tmp_checkout = tmp_dir.join("checkout");
        for (path, content) in files {
            let target = safe_join(&tmp_checkout, path)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, content.as_bytes())?;
        }
        if !externs.is_empty() {
            let tmp_externs = tmp_dir.join("externs");
            std::fs::create_dir_all(&tmp_externs)?;
            for extern_input in externs {
                atomic_write(
                    &tmp_externs.join(format!("{}.rmeta", extern_input.interface_id)),
                    &extern_input.bytes,
                )?;
            }
        }
        let manifest = SnapshotManifest {
            schema_version: SNAPSHOT_MANIFEST_SCHEMA_VERSION,
            snapshot_id: identity.snapshot_id.clone(),
            root: identity.root.clone(),
            context_id: identity.context_id.clone(),
            toolchain_id: identity.toolchain_id.clone(),
            total_bytes: identity.total_bytes,
            files: identity.files.clone(),
            externs: extern_refs.clone(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
            DaemonError::internal(format!("cannot serialize snapshot manifest: {e}"))
        })?;
        atomic_write(&tmp_dir.join("manifest.json"), &manifest_bytes)?;
        match std::fs::rename(&tmp_dir, &final_dir) {
            Ok(()) => Ok(MaterializedSnapshot {
                dir: final_dir,
                checkout,
                externs_dir,
                externs: extern_refs,
                created: true,
                verified_files: identity.files.len(),
            }),
            Err(rename_error) => {
                // A concurrent writer (another daemon process) may have won the
                // race. Verify its snapshot instead of clobbering it.
                let _ = std::fs::remove_dir_all(&tmp_dir);
                if final_dir.exists() {
                    let verified =
                        verify_snapshot_with_externs(&final_dir, identity, &extern_refs)?;
                    Ok(MaterializedSnapshot {
                        dir: final_dir,
                        checkout,
                        externs_dir,
                        externs: extern_refs,
                        created: false,
                        verified_files: verified,
                    })
                } else {
                    Err(DaemonError::io(format!(
                        "cannot publish snapshot {}: {rename_error}",
                        identity.snapshot_id
                    )))
                }
            }
        }
    }
}

/// One extern as it appears on the JSON-lines wire:
/// `{ "name", "crate_name", "interface_id", "bytes_hex" }`.
///
/// This is a *protocol* type, deliberately separate from [`AnalysisRequest`] so
/// the Rust API keeps its exact legacy shape (no struct literal breakage) while
/// clients can version the wire schema independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExtern {
    pub name: String,
    pub crate_name: String,
    pub interface_id: String,
    pub bytes_hex: String,
}

impl WireExtern {
    /// Validates and converts one wire extern into the internal [`ExternInput`].
    /// Fail-closed: hex validity, SHA-256 match, name and size bounds.
    pub fn into_extern_input(self) -> Result<ExternInput> {
        let bytes = hex_decode(&self.bytes_hex)?;
        let input = ExternInput::new(self.name, self.crate_name, bytes)?;
        if input.interface_id != self.interface_id {
            return Err(DaemonError::invalid(format!(
                "extern {:?}: interface_id does not match the SHA-256 of bytes_hex",
                input.name
            )));
        }
        Ok(input)
    }
}

/// Complete wire request: the legacy [`AnalysisRequest`] (flattened, exactly the
/// same JSON fields as before) plus an optional `externs` array.
///
/// `#[serde(default)]` makes every existing client request valid unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAnalyzeRequest {
    #[serde(flatten)]
    pub request: AnalysisRequest,
    #[serde(default)]
    pub externs: Vec<WireExtern>,
}

impl WireAnalyzeRequest {
    /// Splits and validates the wire request into the Rust request and externs.
    pub fn into_parts(self) -> Result<(AnalysisRequest, Vec<ExternInput>)> {
        let externs = self
            .externs
            .into_iter()
            .map(WireExtern::into_extern_input)
            .collect::<Result<Vec<_>>>()?;
        validate_externs(&externs)?;
        Ok((self.request, externs))
    }
}
fn validate_extern_one(extern_input: &ExternInput) -> Result<()> {
    validate_extern_name(&extern_input.name)?;
    validate_extern_name(&extern_input.crate_name)?;
    validate_interface_id(&extern_input.interface_id)?;
    if extern_input.bytes.is_empty() || extern_input.bytes.len() > MAX_EXTERN_BYTES {
        return Err(DaemonError::invalid(format!(
            "extern {:?} has invalid size {} (1..={MAX_EXTERN_BYTES})",
            extern_input.name,
            extern_input.bytes.len()
        )));
    }
    if sha256_hex(&extern_input.bytes) != extern_input.interface_id {
        return Err(DaemonError::invalid(format!(
            "extern {:?} interface_id is not the SHA-256 of its bytes",
            extern_input.name
        )));
    }
    Ok(())
}

/// Validates a whole extern set (names, sizes, interface ids, uniqueness).
/// Public so callers (daemon, tests) can fail before touching the filesystem.
pub fn validate_externs(externs: &[ExternInput]) -> Result<()> {
    if externs.len() > MAX_EXTERNS_PER_UNIT {
        return Err(DaemonError::invalid(format!(
            "snapshot has too many externs ({} > {MAX_EXTERNS_PER_UNIT})",
            externs.len()
        )));
    }
    let mut names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for extern_input in externs {
        validate_extern_one(extern_input)?;
        if !names.insert(extern_input.name.as_str()) {
            return Err(DaemonError::invalid(format!(
                "duplicate extern name {:?}",
                extern_input.name
            )));
        }
    }
    Ok(())
}

/// Verifies an existing snapshot against its expected identity: manifest
/// fields, file list and every on-disk byte hash, including the self-consistency
/// of any stored extern artifacts. Returns the verified file count.
pub fn verify_snapshot(dir: &Path, identity: &SnapshotIdentity) -> Result<usize> {
    verify_snapshot_internal(dir, identity, None)
}

/// Same as [`verify_snapshot`], and additionally requires the stored extern
/// references to equal `expected` (fail-closed when a caller expects externs and
/// the snapshot has others, or none).
pub fn verify_snapshot_with_externs(
    dir: &Path,
    identity: &SnapshotIdentity,
    expected: &[SnapshotExtern],
) -> Result<usize> {
    verify_snapshot_internal(dir, identity, Some(expected))
}

fn verify_snapshot_internal(
    dir: &Path,
    identity: &SnapshotIdentity,
    expected_externs: Option<&[SnapshotExtern]>,
) -> Result<usize> {
    let manifest_path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| {
        DaemonError::snapshot_corrupt(format!(
            "snapshot {} has no readable manifest: {e}",
            identity.snapshot_id
        ))
    })?;
    let manifest: SnapshotManifest = serde_json::from_str(&text).map_err(|e| {
        DaemonError::snapshot_corrupt(format!(
            "snapshot {} manifest is unparsable: {e}",
            identity.snapshot_id
        ))
    })?;
    // Schema 1 (legacy, no externs) and schema 2 (current) are readable; any
    // other schema is a corruption, never a guess.
    if manifest.schema_version != SNAPSHOT_MANIFEST_SCHEMA_VERSION
        && manifest.schema_version != DAEMON_SCHEMA_VERSION
    {
        return Err(DaemonError::snapshot_corrupt(format!(
            "snapshot {} uses schema {} (expected {} or legacy {})",
            identity.snapshot_id,
            manifest.schema_version,
            SNAPSHOT_MANIFEST_SCHEMA_VERSION,
            DAEMON_SCHEMA_VERSION
        )));
    }
    if manifest.snapshot_id != identity.snapshot_id
        || manifest.root != identity.root
        || manifest.context_id != identity.context_id
        || manifest.toolchain_id != identity.toolchain_id
        || manifest.total_bytes != identity.total_bytes
        || manifest.files != identity.files
        || manifest.externs != identity.externs
    {
        return Err(DaemonError::snapshot_corrupt(format!(
            "snapshot {} manifest does not match the requested identity",
            identity.snapshot_id
        )));
    }
    let checkout = dir.join("checkout");
    for file in &identity.files {
        let path = safe_join(&checkout, &file.path)?;
        let bytes = std::fs::read(&path).map_err(|e| {
            DaemonError::snapshot_corrupt(format!(
                "snapshot {} is missing {}: {e}",
                identity.snapshot_id, file.path
            ))
        })?;
        if bytes.len() as u64 != file.len || sha256_hex(&bytes) != file.sha256 {
            return Err(DaemonError::snapshot_corrupt(format!(
                "snapshot {} file {} does not match its manifest hash",
                identity.snapshot_id, file.path
            )));
        }
    }
    let externs_dir = dir.join("externs");
    for extern_ref in &manifest.externs {
        let path = externs_dir.join(format!("{}.rmeta", extern_ref.interface_id));
        let bytes = std::fs::read(&path).map_err(|e| {
            DaemonError::snapshot_corrupt(format!(
                "snapshot {} is missing extern {}: {e}",
                identity.snapshot_id, extern_ref.name
            ))
        })?;
        if bytes.len() as u64 != extern_ref.len || sha256_hex(&bytes) != extern_ref.interface_id {
            return Err(DaemonError::snapshot_corrupt(format!(
                "snapshot {} extern {} does not match its manifest hash",
                identity.snapshot_id, extern_ref.name
            )));
        }
    }
    if let Some(expected) = expected_externs {
        if manifest.externs != expected {
            return Err(DaemonError::snapshot_corrupt(format!(
                "snapshot {} extern set does not match the requested externs",
                identity.snapshot_id
            )));
        }
    }
    Ok(identity.files.len())
}
/// Filesystem work performed by [`sync_unit_dir`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SyncStats {
    pub written_files: usize,
    pub written_bytes: u64,
    pub removed_files: usize,
}

/// Mirrors `files` into the stable `dir`, writing only files whose content
/// changed and removing files that are no longer part of the unit. This keeps
/// source file identities stable across edits, which is what makes persistent
/// `-C incremental` directories reusable.
pub fn sync_unit_dir(dir: &Path, files: &[(String, &str)]) -> Result<SyncStats> {
    std::fs::create_dir_all(dir)?;
    let mut stats = SyncStats::default();
    let mut expected: Vec<PathBuf> = Vec::with_capacity(files.len());
    for (path, content) in files {
        let target = safe_join(dir, path)?;
        expected.push(target.clone());
        let unchanged = match std::fs::read(&target) {
            Ok(existing) => existing == content.as_bytes(),
            Err(_) => false,
        };
        if !unchanged {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, content.as_bytes())?;
            stats.written_files += 1;
            stats.written_bytes = stats.written_bytes.saturating_add(content.len() as u64);
        }
    }
    stats.removed_files = remove_untracked_files(dir, &expected)?;
    Ok(stats)
}

fn remove_untracked_files(dir: &Path, expected: &[PathBuf]) -> Result<usize> {
    let mut removed = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if !expected.contains(&path) {
                // Only daemon-owned unit trees are passed here; never a user path.
                std::fs::remove_file(&path)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn validate_logical_path(raw: &str) -> Result<String> {
    if raw.is_empty() {
        return Err(DaemonError::invalid("empty logical path".to_owned()));
    }
    if raw.len() > MAX_LOGICAL_PATH_BYTES {
        return Err(DaemonError::invalid(format!(
            "logical path is too long ({} > {MAX_LOGICAL_PATH_BYTES} bytes)",
            raw.len()
        )));
    }
    if raw.chars().any(char::is_control) {
        return Err(DaemonError::invalid(
            "logical path must not contain control characters".to_owned(),
        ));
    }
    if raw.contains(':') {
        return Err(DaemonError::invalid(format!(
            "logical path {raw:?} must not contain ':'"
        )));
    }
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(DaemonError::invalid(format!(
            "logical path {raw:?} must be relative"
        )));
    }
    let normalized = normalize_path(raw);
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() {
            return Err(DaemonError::invalid(format!(
                "logical path {raw:?} has an empty component"
            )));
        }
        if part == "." || part == ".." {
            return Err(DaemonError::invalid(format!(
                "logical path {raw:?} has a non-normal component"
            )));
        }
        if is_windows_reserved(part) {
            return Err(DaemonError::invalid(format!(
                "logical path {raw:?} uses a reserved device name"
            )));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn is_windows_reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn validate_snapshot_id(snapshot_id: &str) -> Result<()> {
    if snapshot_id.len() != 64 || !snapshot_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DaemonError::internal(format!(
            "invalid snapshot id {snapshot_id:?}"
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, content: &str) -> AnalysisRequest {
        AnalysisRequest {
            revision: 1,
            label: "unit".to_owned(),
            root: path.to_owned(),
            files: vec![SourceFile {
                path: path.to_owned(),
                content: content.to_owned(),
            }],
            context: BuildContext::default(),
            priority: Priority::Normal,
        }
    }

    fn toolchain() -> ToolchainInfo {
        ToolchainInfo {
            release: "1.95.0".to_owned(),
            host: "x86_64-pc-windows-msvc".to_owned(),
            commit_hash: "abc".to_owned(),
        }
    }

    #[test]
    fn identity_is_stable_and_content_addressed() {
        let toolchain = toolchain();
        let a = request("src/lib.rs", "pub fn a() {}")
            .identity(&toolchain)
            .unwrap();
        let b = request("src/lib.rs", "pub fn a() {}")
            .identity(&toolchain)
            .unwrap();
        assert_eq!(a.snapshot_id, b.snapshot_id);
        let c = request("src/lib.rs", "pub fn b() {}")
            .identity(&toolchain)
            .unwrap();
        assert_ne!(a.snapshot_id, c.snapshot_id);
        let mut changed_context = request("src/lib.rs", "pub fn a() {}");
        changed_context.context.flags = vec!["-D".to_owned(), "warnings".to_owned()];
        let d = changed_context.identity(&toolchain).unwrap();
        assert_ne!(a.snapshot_id, d.snapshot_id, "context must change identity");
    }

    #[test]
    fn label_is_display_only_and_does_not_change_identity() {
        let toolchain = toolchain();
        let a = request("src/lib.rs", "pub fn a() {}");
        let mut b = request("src/lib.rs", "pub fn a() {}");
        b.label = "another display label".to_owned();
        assert_eq!(
            a.identity(&toolchain).unwrap().snapshot_id,
            b.identity(&toolchain).unwrap().snapshot_id,
            "the display label is not part of the content identity"
        );
    }

    #[test]
    fn traversal_and_reserved_paths_are_rejected() {
        for path in [
            "../escape.rs",
            "/abs.rs",
            "C:/abs.rs",
            "src//lib.rs",
            "src/./lib.rs",
            "src/../lib.rs",
            "NUL.rs",
        ] {
            assert!(
                request(path, "fn main() {}").validate().is_err(),
                "path {path:?} must be rejected"
            );
        }
    }

    #[test]
    fn root_must_be_a_submitted_file() {
        let mut value = request("src/lib.rs", "pub fn a() {}");
        value.root = "src/other.rs".to_owned();
        assert!(value.validate().is_err());
    }

    #[test]
    fn materialize_verifies_existing_snapshot_and_detects_tampering() {
        let dir =
            std::env::temp_dir().join(format!("ravum-daemon-snapshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SnapshotStore::new(dir.join("snapshots")).unwrap();
        let toolchain = toolchain();
        let value = request("src/lib.rs", "pub fn a() {}");
        let identity = value.identity(&toolchain).unwrap();
        let files = value.normalized_files().unwrap();
        let first = store.materialize(&identity, &files).unwrap();
        assert!(first.created);
        let second = store.materialize(&identity, &files).unwrap();
        assert!(
            !second.created,
            "second materialization must verify, not rewrite"
        );

        // Tamper with an immutable snapshot: the daemon must refuse it.
        let target = first.checkout.join("src").join("lib.rs");
        std::fs::write(&target, "pub fn tampered() {}").unwrap();
        let error = store.materialize(&identity, &files).unwrap_err();
        assert_eq!(error.kind, crate::error::ErrorKind::SnapshotCorrupt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_unit_dir_writes_only_changes_and_removes_stale_files() {
        let dir = std::env::temp_dir().join(format!("ravum-daemon-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = vec![
            ("src/lib.rs".to_owned(), "pub fn a() {}"),
            ("src/math.rs".to_owned(), "pub fn m() {}"),
        ];
        let first = sync_unit_dir(&dir, &files).unwrap();
        assert_eq!(first.written_files, 2);
        let second = sync_unit_dir(&dir, &files).unwrap();
        assert_eq!(
            second.written_files, 0,
            "unchanged content must not be rewritten"
        );
        let changed = vec![
            ("src/lib.rs".to_owned(), "pub fn a() {}"),
            ("src/math.rs".to_owned(), "pub fn m2() {}"),
        ];
        let third = sync_unit_dir(&dir, &changed).unwrap();
        assert_eq!(third.written_files, 1);
        let removed =
            sync_unit_dir(&dir, &vec![("src/lib.rs".to_owned(), "pub fn a() {}")]).unwrap();
        assert_eq!(removed.removed_files, 1);
        assert!(!dir.join("src").join("math.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "ravum-daemon-snapshot-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn make_extern(bytes: &[u8]) -> ExternInput {
        ExternInput::new("core", "core", bytes.to_vec()).unwrap()
    }

    #[test]
    fn schema2_snapshot_stores_content_addressed_externs() {
        let root = temp_root("schema2");
        let store = SnapshotStore::new(root.join("snapshots")).unwrap();
        let request = request("src/lib.rs", "pub fn a() {}");
        let extern_input = make_extern(b"dep-rmeta-bytes-1");
        let extern_ref = extern_input.reference();
        let identity = request
            .identity_with_externs(&toolchain(), &[extern_input.clone()])
            .unwrap();
        assert_eq!(identity.externs, vec![extern_ref.clone()]);
        let plain_identity = request.identity(&toolchain()).unwrap();
        assert_ne!(
            plain_identity.snapshot_id, identity.snapshot_id,
            "the snapshot identity must include the externs"
        );
        let files = request.normalized_files().unwrap();
        let materialized = store
            .materialize_with_externs(&identity, &files, &[extern_input.clone()])
            .unwrap();
        assert!(materialized.created);
        assert_eq!(materialized.externs, vec![extern_ref.clone()]);
        let extern_path = materialized
            .externs_dir
            .join(format!("{}.rmeta", extern_ref.interface_id));
        assert_eq!(std::fs::read(&extern_path).unwrap(), extern_input.bytes);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(materialized.dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["schema_version"], 2);
        assert_eq!(manifest["externs"][0]["name"], "core");
        assert_eq!(
            manifest["externs"][0]["interface_id"],
            serde_json::json!(extern_ref.interface_id)
        );
        assert_eq!(verify_snapshot(&materialized.dir, &identity).unwrap(), 1);
        assert_eq!(
            verify_snapshot_with_externs(&materialized.dir, &identity, &[extern_ref.clone()])
                .unwrap(),
            1
        );
        // Idempotent re-materialization: verified, not overwritten.
        let again = store
            .materialize_with_externs(&identity, &files, &[extern_input.clone()])
            .unwrap();
        assert!(!again.created);
        assert_eq!(again.verified_files, 1);
        // A different extern set for the same identity is refused (fail-closed).
        let other = make_extern(b"dep-rmeta-bytes-2");
        assert!(store
            .materialize_with_externs(&identity, &files, &[other])
            .is_err());
        // Expecting externs when the stored set differs is a corruption, not a HIT.
        assert!(verify_snapshot_with_externs(&materialized.dir, &identity, &[]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_schema1_manifest_stays_readable() {
        let root = temp_root("legacy");
        let store = SnapshotStore::new(root.join("snapshots")).unwrap();
        let request = request("src/lib.rs", "pub fn a() {}");
        let identity = request.identity(&toolchain()).unwrap();
        let files = request.normalized_files().unwrap();
        let materialized = store.materialize(&identity, &files).unwrap();
        let manifest_path = materialized.dir.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["schema_version"] = serde_json::json!(1);
        manifest.as_object_mut().unwrap().remove("externs");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(verify_snapshot(&materialized.dir, &identity).unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tampered_extern_bytes_are_detected() {
        let root = temp_root("tamper");
        let store = SnapshotStore::new(root.join("snapshots")).unwrap();
        let request = request("src/lib.rs", "pub fn a() {}");
        let extern_input = make_extern(b"dep-rmeta-bytes");
        let extern_ref = extern_input.reference();
        let identity = request
            .identity_with_externs(&toolchain(), &[extern_input.clone()])
            .unwrap();
        let files = request.normalized_files().unwrap();
        let materialized = store
            .materialize_with_externs(&identity, &files, &[extern_input])
            .unwrap();
        std::fs::write(
            materialized
                .externs_dir
                .join(format!("{}.rmeta", extern_ref.interface_id)),
            b"tampered",
        )
        .unwrap();
        assert!(verify_snapshot(&materialized.dir, &identity).is_err());
        assert!(verify_snapshot_with_externs(&materialized.dir, &identity, &[extern_ref]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extern_inputs_are_validated_before_any_write() {
        let root = temp_root("invalid");
        let store = SnapshotStore::new(root.join("snapshots")).unwrap();
        let request = request("src/lib.rs", "pub fn a() {}");
        let identity = request.identity(&toolchain()).unwrap();
        let files = request.normalized_files().unwrap();
        // Mismatched interface id: the bytes must be addressed by their own hash.
        let mut bad = make_extern(b"dep-rmeta-bytes");
        bad.interface_id = "a".repeat(64);
        assert!(store
            .materialize_with_externs(&identity, &files, &[bad])
            .is_err());
        // Invalid extern name (path characters) is refused.
        let mut bad_name = make_extern(b"dep-rmeta-bytes");
        bad_name.name = "../core".to_owned();
        assert!(store
            .materialize_with_externs(&identity, &files, &[bad_name])
            .is_err());
        // Duplicate extern names are refused.
        let duplicate = make_extern(b"other-rmeta");
        let first = make_extern(b"dep-rmeta-bytes");
        assert!(store
            .materialize_with_externs(&identity, &files, &[first, duplicate])
            .is_err());
        // Empty artifact is refused.
        assert!(ExternInput::new("core", "core", Vec::new()).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
