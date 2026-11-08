//! COLLAB-1 coordinator: proposals in, decisions out, one current snapshot.
//!
//! The coordinator is the only component allowed to turn a proposal into state.
//! Agents never write the workspace; they submit a proposal against a base
//! snapshot. Refusals are journaled with their reasons and change nothing.
//!
//! Decision semantics implemented here (no more, no less):
//! * `REJECT` — malformed patch, unknown/undeclared path, or a precondition that
//!   cannot be explained by an already integrated proposal (lost-update risk).
//! * `CONFLICT` — overlapping symbol or hunk with an already integrated change.
//! * `MANUAL_REQUIRED` — contract impact, generated/lockfile path, or a declared
//!   cross-module dependency on a path another agent already modified.
//! * `REBASE_REVALIDATE` — stale base *and* incomplete compare-and-swap coverage:
//!   replay is possible but must be re-validated, so nothing is integrated.
//! * `AUTO_SAFE` — every precondition holds, the patch applies exactly on the
//!   current content, and nothing overlaps.

use crate::patch::{self, touched_ranges};
use crate::path_policy;
use ravum_protocol::{ChangeProposal, DecisionKind, JournalEntry, ProposalDecision, SnapshotId};
use ravum_snapshot::SourceSnapshot;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
struct Integrated {
    proposal_id: String,
    agent_id: String,
    paths: Vec<String>,
    symbols: Vec<String>,
    ranges: Vec<(String, usize, usize)>,
}

/// Owns the current snapshot and the set of already integrated proposals.
#[derive(Debug)]
pub struct Coordinator {
    base: SourceSnapshot,
    snapshot: SourceSnapshot,
    integrated: Vec<Integrated>,
    explained_paths: BTreeSet<String>,
    journal: Vec<JournalEntry>,
    journal_path: Option<std::path::PathBuf>,
    journal_error: Option<String>,
    seq: u64,
}

impl Coordinator {
    pub fn new(base: SourceSnapshot) -> Self {
        Self {
            snapshot: base.clone(),
            base,
            integrated: Vec::new(),
            explained_paths: BTreeSet::new(),
            journal: Vec::new(),
            journal_path: None,
            journal_error: None,
            seq: 0,
        }
    }

    /// Write every decision to disk as it is taken, instead of flushing at the end.
    ///
    /// A crash then loses at most the decision in progress, never the history.
    pub fn with_journal(mut self, path: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::File::create(&path)?;
        self.journal_path = Some(path);
        Ok(self)
    }

    pub fn journal_path(&self) -> Option<&std::path::Path> {
        self.journal_path.as_deref()
    }

    /// First persistence failure, if any. A caller must treat it as fatal.
    pub fn journal_error(&self) -> Option<&str> {
        self.journal_error.as_deref()
    }

    pub fn current(&self) -> &SourceSnapshot {
        &self.snapshot
    }

    pub fn base(&self) -> &SourceSnapshot {
        &self.base
    }

    pub fn current_id(&self) -> SnapshotId {
        self.snapshot.id().clone()
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn integrated_proposals(&self) -> impl Iterator<Item = &str> {
        self.integrated.iter().map(|i| i.proposal_id.as_str())
    }

    /// Decide and, only for `AUTO_SAFE`, integrate into the in-memory overlay.
    pub fn submit(&mut self, proposal: &ChangeProposal) -> ProposalDecision {
        let before = self.current_id();
        let (mut decision, integrate) = self.evaluate(proposal);

        if integrate {
            if let Some(overlay) = self.build_overlay(proposal) {
                self.snapshot = self.snapshot.with_overlay(overlay);
                for path in &proposal.target_paths {
                    self.explained_paths.insert(path.clone());
                }
                self.integrated.push(Integrated {
                    proposal_id: proposal.proposal_id(),
                    agent_id: proposal.agent_id.clone(),
                    paths: proposal.target_paths.clone(),
                    symbols: proposal.target_symbols.clone(),
                    ranges: self.ranges_of(proposal),
                });
            }
        }

        let after = self.current_id();
        decision.resulting_snapshot_id = if integrate { Some(after.clone()) } else { None };
        decision.decided_at_unix = ravum_protocol::now_unix();
        self.seq += 1;
        let entry = JournalEntry {
            seq: self.seq,
            decision: decision.clone(),
            snapshot_before: before,
            snapshot_after: after,
        };
        self.persist(&entry);
        self.journal.push(entry);
        decision
    }

    fn persist(&mut self, entry: &JournalEntry) {
        let Some(path) = self.journal_path.clone() else {
            return;
        };
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
            writeln!(file, "{}", serde_json::to_string(entry).unwrap_or_default())?;
            file.flush()
        })();
        if let Err(e) = result {
            if self.journal_error.is_none() {
                self.journal_error = Some(format!("{}: {e}", path.display()));
            }
        }
    }

    fn evaluate(&self, proposal: &ChangeProposal) -> (ProposalDecision, bool) {
        use DecisionKind::*;
        let decide = |kind: DecisionKind, reason: String| {
            (ProposalDecision::new(proposal, kind).with_reason(reason), false)
        };

        if proposal.target_paths.is_empty() {
            return decide(Reject, "proposal declares no target path".to_owned());
        }
        // Path safety comes first: a proposal that names repository metadata, an
        // absolute path or a `..` traversal is refused before any content is read,
        // so "an agent tried to write into .git" is a recorded refusal.
        if let Some((path, reason)) = proposal
            .target_paths
            .iter()
            .find_map(|path| path_policy::unsafe_path_reason(path).map(|reason| (path, reason)))
        {
            return decide(
                Reject,
                format!("refuses an unsafe target path {path:?}: {reason}"),
            );
        }
        if let Some((path, _)) = proposal
            .expected_hashes
            .iter()
            .find(|(path, _)| path_policy::unsafe_path_reason(path).is_some())
        {
            return decide(
                Reject,
                format!("refuses an unsafe precondition path: {path:?}"),
            );
        }
        if proposal
            .impacted_contracts
            .iter()
            .any(|c| !c.trim().is_empty())
        {
            return decide(
                ManualRequired,
                "proposal declares an impacted contract: contract changes require human arbitration (INV-02)"
                    .to_owned(),
            );
        }
        if let Some(path) = proposal
            .target_paths
            .iter()
            .find(|p| path_policy::is_generated_path(&path_policy::canonical_relative_path(p)))
        {
            return decide(
                ManualRequired,
                format!("target path {path:?} is generated or a lockfile: never auto-integrated"),
            );
        }

        let files = match patch::parse(&proposal.unified_patch) {
            Ok(f) => f,
            Err(e) => return decide(Reject, format!("invalid patch: {e}")),
        };
        for file in &files {
            if !proposal.target_paths.iter().any(|p| p == &file.path) {
                return decide(
                    Reject,
                    format!("patch touches {:?} which is absent from target_paths", file.path),
                );
            }
            if self.snapshot.content_of(&file.path).is_none() {
                return decide(Reject, format!("patch targets unknown file {:?}", file.path));
            }
        }

        // Compare-and-swap preconditions.
        for (path, expected) in &proposal.expected_hashes {
            let current = self.snapshot.file_hash(path);
            if current == Some(expected.as_str()) {
                continue;
            }
            let explained = self.base.file_hash(path) == Some(expected.as_str())
                && self.explained_paths.contains(path);
            if explained {
                continue;
            }
            return decide(
                Reject,
                format!(
                    "stale precondition on {path}: expected {expected}, current {}",
                    current.unwrap_or("<missing>")
                ),
            );
        }

        // Overlap with work already integrated: classified before "does it apply",
        // because two agents competing for the same unit is a conflict by intent,
        // not merely a textual disagreement.
        let proposal_ranges = self.ranges_of(proposal);
        for other in &self.integrated {
            if let Some(symbol) = intersect(&proposal.target_symbols, &other.symbols) {
                return decide(
                    Conflict,
                    format!(
                        "symbol {symbol:?} is already modified by {} (agent {})",
                        other.proposal_id, other.agent_id
                    ),
                );
            }
            if let Some((path, line)) = range_overlap(&proposal_ranges, &other.ranges) {
                return decide(
                    Conflict,
                    format!(
                        "{path}:{line} overlaps an already integrated hunk from {} (agent {})",
                        other.proposal_id, other.agent_id
                    ),
                );
            }
        }

        // Exact application is the only merge we allow.
        for file in &files {
            let Some(original) = self.snapshot.content_of(&file.path) else {
                return decide(Reject, format!("patch targets unknown file {:?}", file.path));
            };
            if let Err(e) = patch::apply(file, original) {
                return decide(
                    Conflict,
                    format!("patch does not apply on the current content: {e}"),
                );
            }
        }

        if let Some(dep) = proposal
            .declared_dependencies
            .iter()
            .find(|dep| other_path_touched(&self.integrated, dep))
        {
            return decide(
                ManualRequired,
                format!(
                    "proposal depends on {dep:?}, which another agent already modified: cross-module arbitration required"
                ),
            );
        }

        let base_is_current = proposal.base_snapshot_id == *self.snapshot.id();
        let missing_cas = proposal
            .target_paths
            .iter()
            .any(|p| !proposal.expected_hashes.contains_key(p));
        if !base_is_current && missing_cas {
            return decide(
                RebaseRevalidate,
                format!(
                    "base snapshot {} is stale and expected hashes do not cover every target path: replay requires fresh validation",
                    proposal.base_snapshot_id.short()
                ),
            );
        }

        let mut decision = ProposalDecision::new(proposal, AutoSafe).with_reason(
            "preconditions satisfied, patch applies exactly, no overlap with integrated work",
        );
        decision.integrated_paths = files.iter().map(|f| f.path.clone()).collect();
        (decision, true)
    }

    fn ranges_of(&self, proposal: &ChangeProposal) -> Vec<(String, usize, usize)> {
        patch::parse(&proposal.unified_patch)
            .map(|files| touched_ranges(&files))
            .unwrap_or_default()
    }

    fn build_overlay(&self, proposal: &ChangeProposal) -> Option<Vec<(String, String)>> {
        let files = patch::parse(&proposal.unified_patch).ok()?;
        let mut overlay = Vec::new();
        for file in files {
            let original = self.snapshot.content_of(&file.path)?;
            let applied = patch::apply(&file, original).ok()?;
            overlay.push((file.path, applied));
        }
        Some(overlay)
    }
}

fn intersect(left: &[String], right: &[String]) -> Option<String> {
    let right: BTreeSet<&String> = right.iter().collect();
    left.iter().find(|s| right.contains(s)).cloned()
}

fn range_overlap(
    left: &[(String, usize, usize)],
    right: &[(String, usize, usize)],
) -> Option<(String, usize)> {
    for (lpath, lstart, lend) in left {
        for (rpath, rstart, rend) in right {
            if lpath == rpath && lstart <= rend && rstart <= lend {
                return Some((lpath.clone(), (*lstart).max(*rstart)));
            }
        }
    }
    None
}

fn other_path_touched(integrated: &[Integrated], dependency: &str) -> bool {
    integrated
        .iter()
        .any(|i| i.paths.iter().any(|p| p == dependency))
}

/// Aggregate view used by the demo binary.
#[derive(Debug, Clone, Default)]
pub struct IntegrationSummary {
    pub auto_safe: usize,
    pub conflicts: usize,
    pub rejected: usize,
    pub manual: usize,
    pub rebase: usize,
    pub integrated_paths: BTreeMap<String, usize>,
}

impl IntegrationSummary {
    pub fn record(&mut self, decision: &ProposalDecision) {
        match decision.decision {
            DecisionKind::AutoSafe => {
                self.auto_safe += 1;
                for p in &decision.integrated_paths {
                    *self.integrated_paths.entry(p.clone()).or_insert(0) += 1;
                }
            }
            DecisionKind::Conflict => self.conflicts += 1,
            DecisionKind::Reject => self.rejected += 1,
            DecisionKind::ManualRequired => self.manual += 1,
            DecisionKind::RebaseRevalidate => self.rebase += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_protocol::COLLAB_SCHEMA_VERSION;

    const FILE: &str = "fn alpha() -> u32 {\n    1\n}\n\nfn beta() -> u32 {\n    2\n}\n";

    fn base() -> Coordinator {
        Coordinator::new(SourceSnapshot::new([(
            "lib.rs".to_string(),
            FILE.to_string(),
        )]))
    }

    fn proposal(
        coordinator: &Coordinator,
        agent: &str,
        symbols: &[&str],
        patch: &str,
        hashes: &[(&str, &str)],
    ) -> ChangeProposal {
        ChangeProposal {
            schema_version: COLLAB_SCHEMA_VERSION,
            agent_id: agent.to_owned(),
            task_id: "t1".to_owned(),
            base_snapshot_id: coordinator.current_id(),
            target_paths: vec!["lib.rs".to_owned()],
            target_symbols: symbols.iter().map(|s| s.to_string()).collect(),
            expected_hashes: hashes
                .iter()
                .map(|(p, h)| (p.to_string(), h.to_string()))
                .collect(),
            declared_dependencies: vec![],
            impacted_contracts: vec![],
            unified_patch: patch.to_owned(),
            note: None,
        }
    }

    const PATCH_ALPHA: &str =
        "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn alpha() -> u32 {\n-    1\n+    10\n }\n";
    const PATCH_BETA: &str =
        "+++ b/lib.rs\n@@ -5,3 +5,3 @@\n fn beta() -> u32 {\n-    2\n+    20\n }\n";

    fn hash(coordinator: &Coordinator) -> String {
        coordinator.base().file_hash("lib.rs").unwrap().to_owned()
    }

    #[test]
    fn independent_hunks_in_one_file_are_auto_safe() {
        let mut c = base();
        let h = hash(&c);
        let a = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        assert_eq!(c.submit(&a).decision, DecisionKind::AutoSafe);
        // Second agent based on the SAME original base: explained by the integrated change.
        let b = proposal(&c, "agent-b", &["beta"], PATCH_BETA, &[("lib.rs", &h)]);
        let d = c.submit(&b);
        assert_eq!(d.decision, DecisionKind::AutoSafe, "reasons: {:?}", d.reasons);
        let content = c.current().content_of("lib.rs").unwrap();
        assert!(content.contains("10") && content.contains("20"));
    }

    #[test]
    fn same_symbol_twice_is_a_conflict_and_nothing_is_lost() {
        let mut c = base();
        let h = hash(&c);
        let a = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        assert_eq!(c.submit(&a).decision, DecisionKind::AutoSafe);
        let snapshot_after_a = c.current_id();
        let b = proposal(&c, "agent-b", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        let d = c.submit(&b);
        assert_eq!(d.decision, DecisionKind::Conflict);
        assert_eq!(c.current_id(), snapshot_after_a, "a refusal must not change state");
        assert!(c.current().content_of("lib.rs").unwrap().contains("10"));
    }

    #[test]
    fn unexplainable_precondition_is_rejected() {
        let mut c = base();
        let a = proposal(&c, "agent-a", &[], PATCH_ALPHA, &[("lib.rs", "deadbeef")]);
        assert_eq!(c.submit(&a).decision, DecisionKind::Reject);
    }

    #[test]
    fn git_metadata_paths_are_refused_before_any_state_change() {
        let mut c = base();
        let base_id = c.base().id().clone();

        let mut metadata_target = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[]);
        metadata_target.target_paths = vec![".git/config".to_owned()];
        let decision = c.submit(&metadata_target);
        assert_eq!(decision.decision, DecisionKind::Reject);
        assert!(
            decision
                .reasons
                .iter()
                .any(|reason| reason.contains(".git")),
            "refusal must name the metadata path: {:?}",
            decision.reasons
        );

        let mut traversal = proposal(&c, "agent-b", &["alpha"], PATCH_ALPHA, &[]);
        traversal.target_paths = vec!["../outside.rs".to_owned()];
        assert_eq!(c.submit(&traversal).decision, DecisionKind::Reject);

        let mut unsafe_precondition = proposal(&c, "agent-c", &["alpha"], PATCH_ALPHA, &[]);
        unsafe_precondition
            .expected_hashes
            .insert(".git/HEAD".to_owned(), "0".repeat(64));
        let decision = c.submit(&unsafe_precondition);
        assert_eq!(decision.decision, DecisionKind::Reject);
        assert!(
            decision
                .reasons
                .iter()
                .any(|reason| reason.contains("unsafe precondition")),
            "refusal must name the unsafe precondition: {:?}",
            decision.reasons
        );

        assert_eq!(
            c.current_id(),
            base_id,
            "refusals must not change the snapshot"
        );
    }

    #[test]
    fn stale_base_without_full_cas_coverage_requires_revalidation() {
        let mut c = base();
        let h = hash(&c);
        let a = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        assert_eq!(c.submit(&a).decision, DecisionKind::AutoSafe);
        let mut b = proposal(&c, "agent-b", &["beta"], PATCH_BETA, &[]);
        b.base_snapshot_id = SnapshotId::new("an-older-base");
        let d = c.submit(&b);
        assert_eq!(d.decision, DecisionKind::RebaseRevalidate);
        assert!(d.resulting_snapshot_id.is_none());
    }

    #[test]
    fn contract_impact_and_lockfiles_are_never_auto_integrated() {
        let mut c = base();
        let h = hash(&c);
        let mut a = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        a.impacted_contracts = vec!["alpha: n > 0".to_owned()];
        assert_eq!(c.submit(&a).decision, DecisionKind::ManualRequired);

        let mut b = proposal(&c, "agent-b", &["beta"], PATCH_BETA, &[("Cargo.lock", &h)]);
        b.target_paths = vec!["Cargo.lock".to_owned()];
        assert_eq!(c.submit(&b).decision, DecisionKind::ManualRequired);
    }

    #[test]
    fn declared_dependency_on_a_modified_path_needs_arbitration() {
        let mut c = base();
        let h = hash(&c);
        let a = proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]);
        assert_eq!(c.submit(&a).decision, DecisionKind::AutoSafe);
        let mut b = proposal(&c, "agent-b", &["beta"], PATCH_BETA, &[("lib.rs", &h)]);
        b.declared_dependencies = vec!["lib.rs".to_owned()];
        assert_eq!(c.submit(&b).decision, DecisionKind::ManualRequired);
    }

    #[test]
    fn journal_keeps_every_decision_in_order() {
        let mut c = base();
        let h = hash(&c);
        c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]));
        c.submit(&proposal(&c, "agent-b", &[], "not a patch", &[("lib.rs", &h)]));
        assert_eq!(c.journal().len(), 2);
        assert_eq!(c.journal()[0].seq, 1);
        assert_eq!(c.journal()[1].decision.decision, DecisionKind::Reject);
        assert_eq!(
            c.journal()[1].snapshot_before,
            c.journal()[1].snapshot_after,
            "a refused proposal never advances the snapshot"
        );
    }
}
