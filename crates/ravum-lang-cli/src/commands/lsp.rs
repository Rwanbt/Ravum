//! `ravum-lang lsp` — explicitly UNSUPPORTED in this build.
//!
//! No language server exists (`crates/ravum-lang-lsp` is T7 and absent). The
//! command is listed as UNSUPPORTED in `--help`, exits 3, and never pretends to
//! start a server.

use serde_json::json;

use crate::json;
use crate::util;

pub fn run(json_output: bool) -> i32 {
    if json_output {
        let summary = json!({
            "status": "UNSUPPORTED",
            "reason": "no language server is implemented in this build",
            "tracking": "crates/ravum-lang-lsp (T7, optional)",
        });
        let meta = json::ReportMeta {
            command: "lsp",
            success: false,
            exit_code: util::EXIT_TOOL,
            target_kind: "workspace",
            target_path: ".".to_string(),
            profile: None,
            dependencies_digest: None,
            lockfile_stale: None,
        };
        let envelope = json::envelope(meta, Vec::new(), summary);
        json::print(&envelope);
    } else {
        eprintln!(
            "lsp: UNSUPPORTED — no language server is implemented in this build \
             (crates/ravum-lang-lsp, T7, is absent/optional)"
        );
    }
    util::EXIT_TOOL
}
