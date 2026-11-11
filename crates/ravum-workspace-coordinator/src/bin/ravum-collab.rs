//! `ravum-collab` — run the COLLAB-2/3 coordinator over a directory of proposals.
//!
//! Usage:
//!   ravum-collab integrate --workspace <dir> --proposals <dir> [--journal <path>]
//!                          [--publication-journal <path>]
//!                          [--commit] [--author NAME] [--email MAIL] [--json]
//!                          [--verify] [--verify-tests] [--protocol v1|v2]
//!                          [--rustc <path>] [--work <dir>]
//!   ravum-collab base --workspace <dir>
//!
//! The workspace is read into an immutable snapshot. Proposals are applied to an
//! in-memory overlay; nothing is written back unless `--commit` is passed, and
//! even then only through the single `GitWriter`, as one transactional publication
//! (isolated index, unpublished commit, compare-and-swap, then reconciliation of
//! the published paths only).
//!
//! Semantic verification is **opt-in and defaults to the exact COLLAB-1
//! behaviour**, because existing COLLAB-1 callers and the historical demo are
//! workspaces of loose `.rs` files, not crates: they must keep producing their
//! historical decisions.
//! * `--verify` turns semantic verification on: before any `AUTO_SAFE` proposal is
//!   integrated, the candidate merged snapshot is compiled with the public `rustc`
//!   CLI through the injected `MergeVerifier`. A required check that cannot run
//!   (no inferable crate root, missing rustc, ...) yields `MANUAL_REQUIRED`, never
//!   a silent pass.
//! * `--verify-tests` implies `--verify` and additionally compiles and executes
//!   the merged snapshot's test harness (it executes code, hence the explicit
//!   flag).
//! * `--protocol v2` switches to the MVP-0c strict protocol: a textually
//!   applicable patch is never `AUTO_SAFE` without a declared verification; the
//!   decision journal and the accepted-change records carry versioned
//!   `EvaluationRecord`s, and a default textual `SemanticOracle` reports the
//!   contract findings it can see. `--protocol v1` (default) is the historical
//!   COLLAB-1 compatibility profile and is marked as such in every record.

mod common;

use common::RustcMergeVerifier;
use ravum_protocol::{ChangeProposal, DecisionKind, ProposalDecision};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    Coordinator, EvaluationProtocol, GitWriter, IntegrationSummary, MergeCheckStatus, PathUpdate,
    PublishRequest, COORDINATOR_SCHEMA_VERSION,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("integrate") => integrate(&args[1..]),
        Some("base") => base(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  ravum-collab integrate --workspace <dir> --proposals <dir> [--journal <path>] [--commit] [--json] [--fail-on-conflict]\n                        [--verify] [--verify-tests] [--protocol v1|v2] [--rustc <path>] [--work <dir>]\n  ravum-collab base --workspace <dir>"
            );
            2
        }
    };
    std::process::exit(code);
}

/// Print the snapshot id and per-file hashes of a workspace, so a proposal author
/// can fill `base_snapshot_id` and `expected_hashes` with real values.
fn base(args: &[String]) -> i32 {
    let Some(workspace) = flag(args, "--workspace").map(PathBuf::from) else {
        eprintln!("missing --workspace");
        return 2;
    };
    match read_workspace(&workspace) {
        Ok(snapshot) => {
            let files: serde_json::Map<String, serde_json::Value> = snapshot
                .paths()
                .filter_map(|p| {
                    snapshot
                        .file_hash(p)
                        .map(|h| (p.to_owned(), serde_json::Value::String(h.to_owned())))
                })
                .collect();
            let payload = serde_json::json!({
                "snapshot_id": snapshot.id(),
                "files": files,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
            0
        }
        Err(e) => {
            eprintln!("cannot read workspace {}: {e}", workspace.display());
            2
        }
    }
}

fn integrate(args: &[String]) -> i32 {
    let Some(workspace) = flag(args, "--workspace").map(PathBuf::from) else {
        eprintln!("missing --workspace");
        return 2;
    };
    let Some(proposals_dir) = flag(args, "--proposals").map(PathBuf::from) else {
        eprintln!("missing --proposals");
        return 2;
    };
    let journal_path = flag(args, "--journal")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/ravum/collab-journal.jsonl"));
    let json = args.iter().any(|a| a == "--json");
    let fail_on_conflict = args.iter().any(|a| a == "--fail-on-conflict");
    let verify_tests = args.iter().any(|a| a == "--verify-tests");
    // Opt-in only: COLLAB-1 callers and the historical demo keep their exact
    // decisions unless semantic verification is explicitly requested.
    let semantic = verify_tests || args.iter().any(|a| a == "--verify");
    let protocol = match flag(args, "--protocol").as_deref() {
        None | Some("v1") => EvaluationProtocol::V1CollabCompat,
        Some("v2") => EvaluationProtocol::V2SemanticStatuses,
        Some(other) => {
            eprintln!("unknown --protocol {other:?} (expected v1 or v2)");
            return 2;
        }
    };

    let base = read_workspace(&workspace);
    let base = match base {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read workspace {}: {e}", workspace.display());
            return 2;
        }
    };
    let mut coordinator = Coordinator::new(base).with_evaluation_protocol(protocol);
    if semantic {
        let rustc = flag(args, "--rustc")
            .or_else(|| std::env::var("RUSTC").ok())
            .unwrap_or_else(|| "rustc".to_owned());
        let work_dir = flag(args, "--work")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/ravum/verify"));
        let verifier = RustcMergeVerifier::new(rustc, work_dir, verify_tests);
        coordinator = coordinator.with_merge_verifier(Arc::new(verifier));
    }
    let mut coordinator = match coordinator.with_journal(journal_path.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open journal {}: {e}", journal_path.display());
            return 2;
        }
    };
    let initial = coordinator.current_id();

    let mut decisions: Vec<ProposalDecision> = Vec::new();
    let mut summary = IntegrationSummary::default();
    for (index, path) in proposal_files(&proposals_dir).iter().enumerate() {
        let proposal = match load_proposal(path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("proposal {} is unusable: {e}", path.display());
                return 2;
            }
        };
        let decision = coordinator.submit(&proposal);
        if let Some(err) = coordinator.journal_error() {
            eprintln!("journal write failed: {err}");
            eprintln!(
                "aborting: a decision that cannot be recorded must not be acted upon further"
            );
            return 2;
        }
        summary.record(&decision);
        let level = coordinator
            .evaluation_for(&decision.proposal_id)
            .map(|evaluation| evaluation.level.as_str())
            .unwrap_or("UNKNOWN");
        if !json {
            println!(
                "#{} {:<17} {:<24} {:<24} {}",
                index + 1,
                decision.decision.as_str(),
                decision.proposal_id,
                level,
                decision.reasons.first().cloned().unwrap_or_default()
            );
            for reason in decision.reasons.iter().skip(1) {
                println!("   - {reason}");
            }
        }
        decisions.push(decision);
    }

    if !json {
        println!(
            "\nAUTO_SAFE={} CONFLICT={} REJECT={} MANUAL_REQUIRED={} REBASE_REVALIDATE={}",
            summary.auto_safe, summary.conflicts, summary.rejected, summary.manual, summary.rebase
        );
        println!(
            "snapshot {} -> {}",
            initial.short(),
            coordinator.current_id().short()
        );
        println!(
            "evaluation protocol: {} (coordinator schema v{})",
            coordinator.protocol().as_str(),
            COORDINATOR_SCHEMA_VERSION
        );
        match coordinator.verifier_name() {
            Some(name) => println!(
                "verification: semantic (verifier {name}); {} accepted change record(s)",
                coordinator.accepted_changes().len()
            ),
            None => println!(
                "verification: textual only (COLLAB-1 default; pass --verify to require semantic verification)"
            ),
        }
        for change in coordinator.accepted_changes() {
            let level = change
                .evaluation
                .as_ref()
                .map(|evaluation| evaluation.level.as_str())
                .unwrap_or("UNKNOWN");
            println!(
                "  record {} -> {} [{} / {}] {}",
                change.snapshot_before.short(),
                change.snapshot_after.short(),
                change.verification.status.as_str(),
                level,
                change.evidence_ids.join(",")
            );
        }
    }

    let mut publication: Option<serde_json::Value> = None;
    if args.iter().any(|a| a == "--commit") {
        match commit(&workspace, &coordinator, args) {
            Ok(info) => {
                if json {
                    publication = Some(info.as_json());
                } else {
                    println!(
                        "committed {} (parent {}) paths={} warnings={}",
                        info.short_sha, info.short_parent, info.paths, info.warnings
                    );
                }
            }
            Err(e) => {
                eprintln!("commit refused: {e}");
                return 1;
            }
        }
    }

    if json {
        let payload = serde_json::json!({
            "schema_version": 2,
            "coordinator_schema_version": COORDINATOR_SCHEMA_VERSION,
            "protocol": coordinator.protocol().as_str(),
            "snapshot_before": initial,
            "snapshot_after": coordinator.current_id(),
            "verification": {
                "mode": if coordinator.verification_required() { "semantic" } else { "textual_only" },
                "requested": semantic,
                "verifier": coordinator.verifier_name(),
                "test_execution": verify_tests,
            },
            "oracle": coordinator.oracle_name(),
            "summary": {
                "auto_safe": summary.auto_safe,
                "conflict": summary.conflicts,
                "reject": summary.rejected,
                "manual_required": summary.manual,
                "rebase_revalidate": summary.rebase,
            },
            "decisions": decisions,
            "evaluations": coordinator.evaluations(),
            "accepted_changes": coordinator.accepted_changes(),
            "publication": publication,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    }

    if fail_on_conflict
        && (summary.conflicts + summary.rejected + summary.manual + summary.rebase > 0)
    {
        return 1;
    }
    0
}

/// Result of one transactional publication.
struct CommitInfo {
    sha: String,
    parent: String,
    short_sha: String,
    short_parent: String,
    paths: usize,
    warnings: usize,
}

impl CommitInfo {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sha": self.sha,
            "parent": self.parent,
            "paths": self.paths,
            "warnings": self.warnings,
        })
    }
}

fn short(sha: &str) -> String {
    sha[..12.min(sha.len())].to_owned()
}

fn commit(
    workspace: &Path,
    coordinator: &Coordinator,
    args: &[String],
) -> Result<CommitInfo, String> {
    // Fail closed: in semantic mode nothing is published without a recorded,
    // passed verification bound to the exact accepted snapshot.
    for change in coordinator.accepted_changes() {
        if change.verification_required && change.verification.status != MergeCheckStatus::Passed {
            return Err(format!(
                "accepted change {} has no passed semantic verification (status {})",
                change.proposal_id,
                change.verification.status.as_str()
            ));
        }
    }
    let integrated: Vec<String> = coordinator
        .journal()
        .iter()
        .filter(|entry| entry.decision.decision == DecisionKind::AutoSafe)
        .flat_map(|entry| entry.decision.integrated_paths.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if integrated.is_empty() {
        return Err("no integrated proposal to commit".to_owned());
    }
    // Content comes from the coordinator's overlay, never from a blind rewrite of
    // the whole workspace: the writer validates each path against the base snapshot
    // hash before publishing, and writes only what it actually publishes.
    let mut changes = Vec::with_capacity(integrated.len());
    for path in &integrated {
        let Some(content) = coordinator.current().content_of(path) else {
            return Err(format!(
                "integrated path {path} has no content in the coordinator snapshot"
            ));
        };
        // An empty expected hash means "the path must be absent from the base".
        let expected_old = coordinator
            .base()
            .file_hash(path)
            .map(ToOwned::to_owned)
            .unwrap_or_default();
        changes.push(
            PathUpdate::write(path.clone(), content.to_owned())
                .with_expected_old_sha256(expected_old),
        );
    }

    let publication_journal = flag(args, "--publication-journal")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut path = coordinator
                .journal_path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("target/ravum/collab-journal.jsonl"));
            path.set_extension("publications.jsonl");
            path
        });
    let writer = GitWriter::new(
        workspace,
        flag(args, "--author").unwrap_or_else(|| "ravum-coordinator".to_owned()),
        flag(args, "--email").unwrap_or_else(|| "coordinator@ravum.invalid".to_owned()),
    )
    .with_publication_journal(&publication_journal)
    .map_err(|e| format!("cannot open publication journal: {e}"))?;

    let mut trailers = coordinator
        .journal()
        .iter()
        .filter(|entry| entry.decision.decision == DecisionKind::AutoSafe)
        .flat_map(|entry| {
            vec![
                (
                    "Ravum-Proposal".to_owned(),
                    entry.decision.proposal_id.clone(),
                ),
                ("Ravum-Agent".to_owned(), entry.decision.agent_id.clone()),
            ]
        })
        .collect::<Vec<_>>();
    // One evidence trailer per accepted change: the commit body then carries the
    // exact verification proof ids, so an auditor can re-derive what was checked.
    for change in coordinator.accepted_changes() {
        trailers.push((
            "Ravum-Evidence".to_owned(),
            change.verification.evidence_id.clone(),
        ));
        for evidence_id in &change.evidence_ids {
            if *evidence_id != change.verification.evidence_id {
                trailers.push(("Ravum-Check-Evidence".to_owned(), evidence_id.clone()));
            }
        }
    }
    let proposal_ids = coordinator
        .journal()
        .iter()
        .filter(|entry| entry.decision.decision == DecisionKind::AutoSafe)
        .map(|entry| entry.decision.proposal_id.clone())
        .collect::<Vec<_>>();
    // Stable across a restart: the same integrated proposal set maps to the same
    // publication id, so recovery refuses a second publication instead of redoing it.
    let request = PublishRequest::new(format!(
        "chore(collab): integrate {} proposal(s) touching {} path(s)",
        proposal_ids.len(),
        integrated.len()
    ))
    .with_changes(changes)
    .with_publication_id(format!("collab:{}", proposal_ids.join(",")))
    .with_trailers(trailers);

    let result = writer
        .publish_request(&request)
        .map_err(|e| e.to_string())?;
    for warning in &result.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(CommitInfo {
        short_sha: short(&result.sha),
        short_parent: short(&result.parent),
        sha: result.sha,
        parent: result.parent,
        paths: result.paths.len(),
        warnings: result.warnings.len(),
    })
}

fn read_workspace(root: &Path) -> std::io::Result<SourceSnapshot> {
    SourceSnapshot::from_directory(root, |path| {
        !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with(".collab") || n == "collab-journal.jsonl")
            .unwrap_or(false)
    })
}

fn proposal_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn load_proposal(path: &Path) -> Result<ChangeProposal, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Tolerate a UTF-8 BOM produced by common editors/tools.
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).map_err(|e| e.to_string())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
