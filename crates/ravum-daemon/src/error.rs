//! Typed errors for the analysis daemon.
//!
//! Fail-closed policy: a corrupt snapshot, a corrupt cache entry or an
//! unparsable result is never served as if it were a valid analysis. Cache
//! corruption degrades to a *miss* (safe: a real `rustc` run happens), while
//! snapshot corruption is an error (the daemon must not compile bytes that are
//! not the ones the caller submitted).

use std::fmt;

/// Category of a daemon failure. Tests match on the category, not on message
/// text, so wording can evolve without silently weakening assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Filesystem or process I/O failure.
    Io,
    /// The request violates the daemon contract (bad logical path, oversized
    /// unit, reserved flag...). Rejected *before* any compilation.
    InvalidRequest,
    /// The configured `rustc` could not be queried or spawned.
    Toolchain,
    /// `rustc` was spawned but the daemon could not collect its result.
    Driver,
    /// An immutable snapshot exists but no longer matches its manifest. This is
    /// a hard error: the daemon refuses to compile substituted bytes.
    SnapshotCorrupt,
    /// The daemon state is inconsistent (poisoned lock, invalid cache policy...).
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ErrorKind {
    /// Stable machine-readable label used by the JSON-lines daemon protocol.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Io => "io",
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::Toolchain => "toolchain",
            ErrorKind::Driver => "driver",
            ErrorKind::SnapshotCorrupt => "snapshot_corrupt",
            ErrorKind::Internal => "internal",
        }
    }
}

impl DaemonError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    pub fn driver(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Driver, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, message)
    }

    pub fn toolchain(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Toolchain, message)
    }

    pub fn snapshot_corrupt(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::SnapshotCorrupt, message)
    }
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for DaemonError {}

impl From<std::io::Error> for DaemonError {
    fn from(error: std::io::Error) -> Self {
        DaemonError::io(format!("io error: {error}"))
    }
}

pub type Result<T> = std::result::Result<T, DaemonError>;
