//! `ravum-collab` — run the COLLAB-1 coordinator over a directory of proposals.
//!
//! Usage:
//!   ravum-collab integrate --workspace <dir> --proposals <dir> [--journal <path>]
//!                          [--publication-journal <path>]
//!                          [--commit] [--author NAME] [--email MAIL] [--json]
//!
//! The workspace is read into an immutable snapshot. Proposals are applied to an
//! in-memory overlay; nothing is written back unless `--commit` is passed, and
//! even then only through the single `GitWriter`, as one transactional publication
//! (isolated index, unpublished commit, compare-and-swap, then reconciliation of
//! the published paths only).

use ravum_protocol::{ChangeProposal, DecisionKind, ProposalDecision};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    Coordinator, GitWriter, IntegrationSummary, PathUpdate, PublishRequest,
};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("integrate") => integrate(&args[1..]),
        Some("base") => base(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  ravum-collab integrate --workspace <dir> --proposals <dir> [--journal <path>] [--commit] [--json] [--fail-on-conflict]\n  ravum-collab base --workspace <dir>"
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
            println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
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

    let base = read_workspace(&workspace);
    let base = match base {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read workspace {}: {e}", workspace.display());
            return 2;
        }
    };
    let mut coordinator = match Coordinator::new(base).with_journal(journal_path.clone()) {
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
            eprintln!("aborting: a decision that cannot be recorded must not be acted upon further");
            return 2;
        }
        summary.record(&decision);
        if !json {
            println!(
                "#{} {:<17} {:<24} {}",
                index + 1,
                decision.decision.as_str(),
                decision.proposal_id,
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
        println!("snapshot {} -> {}", initial.short(), coordinator.current_id().short());
    }

    if args.iter().any(|a| a == "--commit") {
        if let Err(e) = commit(&workspace, &coordinator, args) {
            eprintln!("commit refused: {e}");
            return 1;
        }
    }

    if json {
        let payload = serde_json::json!({
            "schema_version": 1,
            "snapshot_before": initial,
            "snapshot_after": coordinator.current_id(),
            "summary": {
                "auto_safe": summary.auto_safe,
                "conflict": summary.conflicts,
                "reject": summary.rejected,
                "manual_required": summary.manual,
                "rebase_revalidate": summary.rebase,
            },
            "decisions": decisions,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    }

    if fail_on_conflict && (summary.conflicts + summary.rejected + summary.manual + summary.rebase > 0) {
        return 1;
    }
    0
}

fn commit(workspace: &Path, coordinator: &Coordinator, args: &[String]) -> Result<(), String> {
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

    let trailers = coordinator
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
    println!(
        "committed {} (parent {}) paths={} warnings={}",
        &result.sha[..12.min(result.sha.len())],
        &result.parent[..12.min(result.parent.len())],
        result.paths.len(),
        result.warnings.len()
    );
    for warning in &result.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
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
