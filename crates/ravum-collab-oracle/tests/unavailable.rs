//! Fail-closed behaviour when the local analysis environment is not reliable.
//!
//! This file is a **separate integration-test binary** on purpose: it mutates
//! process-wide environment variables (`CARGO`, `RAVUM_RUSTC`) to simulate missing
//! tools, and no other test of this crate can observe those mutations. All
//! assertions are on the real adapter code path (`UnitGraphOracle::evaluate`),
//! which must return `Unavailable` — never `Clear` — when the analysis cannot be
//! trusted.

mod support;

use ravum_collab_oracle::UnitGraphOracle;
use ravum_workspace_coordinator::{OracleVerdict, SemanticOracle};
use std::ffi::OsString;
use support::*;

struct EnvGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[test]
fn missing_tools_yield_unavailable_never_clear() {
    let request = body_only_request(CORE_DEPENDENTS);

    // 1. `cargo` cannot be spawned: the whole interface analysis is impossible.
    {
        let oracle = UnitGraphOracle::new(state_dir("unavailable-cargo"));
        let _guard = EnvGuard::set("CARGO", "/nonexistent/ravum-cargo-does-not-exist");
        let verdict = oracle.evaluate(&request);
        let reason = unavailable_reason(&verdict)
            .unwrap_or_else(|| panic!("missing cargo must be Unavailable: {verdict:?}"));
        assert!(
            reason.contains("cargo is unavailable"),
            "reason must name the missing cargo: {reason}"
        );
        assert!(!matches!(verdict, OracleVerdict::Clear { .. }));
    }

    // 2. `rustc -vV` cannot be spawned: no toolchain identity, no campaign.
    {
        let oracle = UnitGraphOracle::new(state_dir("unavailable-rustc"));
        let _guard = EnvGuard::set("RAVUM_RUSTC", "/nonexistent/ravum-rustc-does-not-exist");
        let verdict = oracle.evaluate(&request);
        let reason = unavailable_reason(&verdict)
            .unwrap_or_else(|| panic!("missing rustc must be Unavailable: {verdict:?}"));
        assert!(
            reason.contains("toolchain context is unavailable"),
            "reason must name the missing toolchain: {reason}"
        );
        assert!(!matches!(verdict, OracleVerdict::Clear { .. }));
    }

    // 3. Sanity: with the tools restored, the same request is analysable again.
    {
        let oracle = UnitGraphOracle::new(state_dir("unavailable-restored"));
        let verdict = oracle.evaluate(&request);
        assert!(
            !matches!(verdict, OracleVerdict::Unavailable { .. }),
            "with the tools restored the adapter must analyse: {verdict:?}"
        );
    }
}
