//! `ravum-collab` — run the COLLAB-1 coordinator over a directory of proposals.
//!
//! Usage:
//!   ravum-collab integrate --workspace <dir> --proposals <dir> [--journal <path>]
//!                          [--commit] [--author NAME] [--email MAIL] [--json]
//!
//! The workspace is read into an immutable snapshot. Proposals are applied to an
//! in-memory overlay; nothing is written back unless `--commit` is passed, and
//! even then only through the single `GitWriter`.

use ravum_protocol::{ChangeProposal, DecisionKind, ProposalDecision};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{Coordinator, GitWriter, IntegrationSummary};
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
    let mut coordinator = Coordinator::new(base);
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

    write_journal(&journal_path, &coordinator);

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
    let mut integrated: Vec<String> = coordinator
        .journal()
        .iter()
        .filter(|entry| entry.decision.decision == DecisionKind::AutoSafe)
        .flat_map(|entry| entry.decision.integrated_paths.clone())
        .collect();
    integrated.sort();
    integrated.dedup();
    if integrated.is_empty() {
        return Err("no integrated proposal to commit".to_owned());
    }
    coordinator
        .current()
        .materialize_to(workspace)
        .map_err(|e| format!("cannot materialize snapshot: {e}"))?;

    let writer = GitWriter::new(
        workspace,
        flag(args, "--author").unwrap_or_else(|| "ravum-coordinator".to_owned()),
        flag(args, "--email").unwrap_or_else(|| "coordinator@ravum.invalid".to_owned()),
    );
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
    let result = writer
        .commit_paths(
            &integrated,
            &format!(
                "chore(collab): integrate {} proposal(s) touching {} path(s)",
                coordinator
                    .journal()
                    .iter()
                    .filter(|e| e.decision.decision == DecisionKind::AutoSafe)
                    .count(),
                integrated.len()
            ),
            &trailers,
        )
        .map_err(|e| e.to_string())?;
    println!("committed {} (parent {})", &result.sha[..12.min(result.sha.len())], &result.parent[..12.min(result.parent.len())]);
    Ok(())
}

fn write_journal(path: &Path, coordinator: &Coordinator) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut text = String::new();
    for entry in coordinator.journal() {
        if let Ok(line) = serde_json::to_string(entry) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    if let Err(e) = std::fs::write(path, text) {
        eprintln!("cannot write journal {}: {e}", path.display());
    }
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
