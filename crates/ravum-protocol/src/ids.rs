//! The five distinct identities required by M0/RAV-005.
//!
//! They are deliberately *different Rust types* around the same string payload so
//! that a build context can never be silently passed where a source snapshot is
//! expected. They are all content-derived hex digests except `ActiveVersionId`,
//! which is a monotonic activation counter.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! digest_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn short(&self) -> &str {
                let s = &self.0;
                if s.len() > 12 {
                    &s[..12]
                } else {
                    s
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

digest_id!(
    SnapshotId,
    "Identity of the exact canonical source tree (paths + contents). Changes when any file changes."
);
digest_id!(
    BuildContextId,
    "Identity of the toolchain + target + build flags used to produce an artifact."
);
digest_id!(
    VerificationKey,
    "Identity of a verification obligation: property, assumptions, tool, version, contracts."
);
digest_id!(
    ArtifactId,
    "Identity of the produced native artifact bytes (content-addressed)."
);

/// Monotonic counter of an activation performed by the patch host.
///
/// It is not a content hash: two different content snapshots can be activated
/// with the same generation number only if the host was restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActiveVersionId(pub u64);

impl fmt::Display for ActiveVersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_types_do_not_share_construction_macro_state() {
        let snap = SnapshotId::new("abc");
        let artifact = ArtifactId::new("abc");
        assert_eq!(snap.as_str(), artifact.as_str());
        assert_eq!(snap.to_string(), "abc");
    }

    #[test]
    fn short_is_bounded() {
        assert_eq!(SnapshotId::new("0123456789abcdef").short(), "0123456789ab");
        assert_eq!(SnapshotId::new("tiny").short(), "tiny");
    }
}
