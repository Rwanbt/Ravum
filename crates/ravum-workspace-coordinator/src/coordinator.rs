//! COLLAB-1/2/3 coordinator: proposals in, decisions out, one current snapshot.
//!
//! The coordinator is the only component allowed to turn a proposal into state.
//! Agents never write the workspace; they submit a proposal against a base
//! snapshot. Refusals are journaled with their reasons and change nothing.
//!
//! Decision semantics implemented here (no more, no less):
//! * `REJECT` — malformed patch, unknown/undeclared path, a precondition that
//!   cannot be explained by an already integrated proposal (lost-update risk), or
//!   a semantic verification that *failed* with concrete evidence.
//! * `CONFLICT` — overlapping symbol or hunk with an already integrated change.
//! * `MANUAL_REQUIRED` — contract/ABI impact, generated/lockfile path, declared
//!   cross-module dependency on a path another agent already modified, an
//!   undeclared semantic dependency, or a required semantic verification that
//!   could not run (fail-closed).
//! * `REBASE_REVALIDATE` — stale base *and* incomplete compare-and-swap coverage:
//!   replay is possible but must be re-validated, so nothing is integrated.
//! * `AUTO_SAFE` — every precondition holds, the patch applies exactly on the
//!   current content, nothing overlaps, and — when a `MergeVerifier` is injected —
//!   the candidate merged snapshot passed every required semantic check.
//!
//! Two modes exist on purpose:
//! * `Coordinator::new` is the COLLAB-1 compatibility mode (textual only): it is
//!   kept so the existing COLLAB-1 call sites and their 47 tests keep their exact
//!   meaning. An accepted change is recorded with
//!   `VerificationRecord::not_required()`, never with a fake pass.
//! * `Coordinator::with_merge_verifier` (and `Coordinator::semantic`) is the
//!   COLLAB-2/3 mode used by the `ravum-collab` binary: a textually clean merge is
//!   never `AUTO_SAFE` without proof.

use crate::patch::{self, touched_ranges};
use crate::path_policy;
use crate::records::{
    AcceptedChange, MergeCheckKind, VerificationRecord, COLLAB_RECORD_SCHEMA_VERSION,
};
use crate::semantics::{self, PatchFacts};
use crate::verifier::{
    self, MergeVerifier, VerificationOutcome, VerificationRequest,
};
use ravum_protocol::{ChangeProposal, DecisionKind, JournalEntry, ProposalDecision, SnapshotId};
use ravum_snapshot::SourceSnapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct Integrated {
    proposal_id: String,
    agent_id: String,
    paths: Vec<String>,
    symbols: Vec<String>,
    ranges: Vec<(String, usize, usize)>,
    modified_symbols: BTreeSet<String>,
}

/// Internal outcome of the semantic gate.
enum Gate {
    Accept {
        record: VerificationRecord,
        reasons: Vec<String>,
    },
    Refuse {
        kind: DecisionKind,
        reasons: Vec<String>,
    },
}

/// Owns the current snapshot and the set of already integrated proposals.
pub struct Coordinator {
    base: SourceSnapshot,
    snapshot: SourceSnapshot,
    integrated: Vec<Integrated>,
    explained_paths: BTreeSet<String>,
    journal: Vec<JournalEntry>,
    journal_path: Option<std::path::PathBuf>,
    journal_error: Option<String>,
    accepted: Vec<AcceptedChange>,
    verifier: Option<Arc<dyn MergeVerifier>>,
    seq: u64,
}

impl std::fmt::Debug for Coordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coordinator")
            .field("base", &self.base.id())
            .field("snapshot", &self.snapshot.id())
            .field("integrated", &self.integrated.len())
            .field("journal", &self.journal.len())
            .field("accepted", &self.accepted.len())
            .field("verifier", &self.verifier.as_ref().map(|v| v.name()))
            .finish()
    }
}

impl Coordinator {
    /// COLLAB-1 compatibility mode: textual checks only, no semantic verifier.
    ///
    /// `AUTO_SAFE` here means "the patch applies exactly and does not overlap an
    /// integrated hunk". It does **not** mean the merged snapshot compiles. Use
    /// [`Coordinator::with_merge_verifier`] (or [`Coordinator::semantic`]) for the
    /// COLLAB-2/3 contract, which is what the `ravum-collab` binary does.
    pub fn new(base: SourceSnapshot) -> Self {
        Self {
            snapshot: base.clone(),
            base,
            integrated: Vec::new(),
            explained_paths: BTreeSet::new(),
            journal: Vec::new(),
            journal_path: None,
            journal_error: None,
            accepted: Vec::new(),
            verifier: None,
            seq: 0,
        }
    }

    /// Semantic mode: a candidate merge only becomes `AUTO_SAFE` after the
    /// injected verifier passed every required check on the exact candidate
    /// snapshot. If a required check cannot run, the decision is
    /// `MANUAL_REQUIRED`, never a silent success.
    pub fn with_merge_verifier(mut self, verifier: Arc<dyn MergeVerifier>) -> Self {
        self.verifier = Some(verifier);
        self
    }

    /// Convenience constructor for the semantic mode.
    pub fn semantic(base: SourceSnapshot, verifier: Arc<dyn MergeVerifier>) -> Self {
        Self::new(base).with_merge_verifier(verifier)
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

    /// True when semantic verification gates every integration.
    pub fn verification_required(&self) -> bool {
        self.verifier.is_some()
    }

    pub fn verifier_name(&self) -> Option<&'static str> {
        self.verifier.as_ref().map(|verifier| verifier.name())
    }

    /// One record per accepted change, in integration order (L2 / COLLAB-4 API).
    pub fn accepted_changes(&self) -> &[AcceptedChange] {
        &self.accepted
    }

    pub fn accepted_change_for(&self, proposal_id: &str) -> Option<&AcceptedChange> {
        self.accepted
            .iter()
            .find(|change| change.proposal_id == proposal_id)
    }

    /// Decide and, only for `AUTO_SAFE`, integrate into the in-memory overlay.
    pub fn submit(&mut self, proposal: &ChangeProposal) -> ProposalDecision {
        let before = self.current_id();
        let (mut decision, integrate) = self.evaluate(proposal);
        let mut accepted: Option<AcceptedChange> = None;

        if integrate {
            let parsed = patch::parse(&proposal.unified_patch).unwrap_or_default();
            if let Some(overlay) = self.build_overlay(proposal) {
                let candidate = self.snapshot.with_overlay(overlay);
                let facts = semantics::analyse(&self.snapshot, &parsed);
                let gate = self.semantic_gate(proposal, &candidate, &facts);
                match gate {
                    Gate::Accept { record, reasons } => {
                        self.snapshot = candidate;
                        for path in &proposal.target_paths {
                            self.explained_paths.insert(path.clone());
                        }
                        self.integrated.push(Integrated {
                            proposal_id: proposal.proposal_id(),
                            agent_id: proposal.agent_id.clone(),
                            paths: proposal.target_paths.clone(),
                            symbols: proposal.target_symbols.clone(),
                            ranges: self.ranges_of(proposal),
                            modified_symbols: facts.modified_symbols.clone(),
                        });
                        decision.reasons.extend(reasons);
                        accepted = Some(AcceptedChange {
                            schema_version: COLLAB_RECORD_SCHEMA_VERSION,
                            proposal_id: proposal.proposal_id(),
                            agent_id: proposal.agent_id.clone(),
                            task_id: proposal.task_id.clone(),
                            decision: DecisionKind::AutoSafe,
                            journal_seq: self.seq + 1,
                            snapshot_before: before.clone(),
                            snapshot_after: self.snapshot.id().clone(),
                            paths: decision.integrated_paths.clone(),
                            declared_symbols: proposal.target_symbols.clone(),
                            modified_symbols: facts.modified_symbols.iter().cloned().collect(),
                            verification_required: self.verifier.is_some(),
                            evidence_ids: record.evidence_ids(),
                            verification: record,
                            recorded_at_unix: ravum_protocol::now_unix(),
                        });
                    }
                    Gate::Refuse { kind, reasons } => {
                        let mut refusal = ProposalDecision::new(proposal, kind);
                        refusal.reasons = reasons;
                        decision = refusal;
                    }
                }
            }
        }

        let after = self.current_id();
        decision.resulting_snapshot_id = if accepted.is_some() {
            Some(after.clone())
        } else {
            None
        };
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
        if let Some(mut record) = accepted {
            record.journal_seq = self.seq;
            self.accepted.push(record);
        }
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

    /// Semantic gate: policy refusals first (contracts, undeclared dependencies),
    /// then the injected verifier on the exact candidate snapshot.
    fn semantic_gate(
        &self,
        proposal: &ChangeProposal,
        candidate: &SourceSnapshot,
        facts: &PatchFacts,
    ) -> Gate {
        let Some(verifier) = &self.verifier else {
            return Gate::Accept {
                record: VerificationRecord::not_required(),
                reasons: Vec::new(),
            };
        };

        // 1. Policy refusals: things a local compile cannot prove.
        let mut policy_reasons = facts.contract_reasons();
        if let Some(reason) = self.undeclared_dependency_reason(facts) {
            policy_reasons.push(reason);
        }
        if !policy_reasons.is_empty() {
            policy_reasons.push(format!(
                "semantic policy evidence: {}",
                facts.evidence_id(candidate.id())
            ));
            return Gate::Refuse {
                kind: DecisionKind::ManualRequired,
                reasons: policy_reasons,
            };
        }

        // 2. Required checks are derived from the change, not from the agent's
        //    claims: a behavioural change in a snapshot that ships tests must have
        //    its tests compiled and executed.
        let mut required_checks = vec![MergeCheckKind::MetadataCheck];
        if semantics::has_test_code(&self.snapshot) && facts.behavioural_change() {
            required_checks.push(MergeCheckKind::TestCompile);
            required_checks.push(MergeCheckKind::TestRun);
        }

        let request = VerificationRequest {
            proposal_id: proposal.proposal_id(),
            agent_id: proposal.agent_id.clone(),
            snapshot_before: self.snapshot.clone(),
            snapshot_after: candidate.clone(),
            changed_paths: facts.changed_paths(),
            declared_symbols: proposal.target_symbols.clone(),
            required_checks: required_checks.clone(),
        };
        match verifier.verify(&request) {
            VerificationOutcome::Passed { checks } => {
                let missing = verifier::missing_required_checks(&required_checks, &checks);
                if !missing.is_empty() {
                    let names: Vec<&str> = missing.iter().map(|kind| kind.as_str()).collect();
                    return Gate::Refuse {
                        kind: DecisionKind::ManualRequired,
                        reasons: vec![
                            format!(
                                "semantic verification incomplete: required check(s) {} did not run or did not pass",
                                names.join(", ")
                            ),
                            format!(
                                "verifier {:?} returned Passed without a complete proof; refusing a textually clean merge (fail-closed)",
                                verifier.name()
                            ),
                        ],
                    };
                }
                let record = VerificationRecord::from_checks(required_checks, checks);
                let mut reasons = vec![record.summary_line()];
                for check in &record.checks {
                    reasons.push(format!(
                        "{}: {} [{}] evidence {}",
                        check.kind.as_str(),
                        check.summary,
                        check.status.as_str(),
                        check.evidence_id
                    ));
                }
                Gate::Accept { record, reasons }
            }
            VerificationOutcome::Failed { check, reason } => {
                let mut reasons = vec![format!("semantic verification failed: {reason}")];
                reasons.push(format!(
                    "failed check {} on snapshot {} (evidence {})",
                    check.kind.as_str(),
                    check.snapshot_id.short(),
                    check.evidence_id
                ));
                for diagnostic in check.diagnostics.iter().take(5) {
                    reasons.push(format!(
                        "diagnostic: {} [{}] {}",
                        diagnostic.code.clone().unwrap_or_else(|| "no-code".to_owned()),
                        diagnostic.level,
                        diagnostic.message
                    ));
                }
                Gate::Refuse {
                    kind: DecisionKind::Reject,
                    reasons,
                }
            }
            VerificationOutcome::Unavailable { reason } => Gate::Refuse {
                kind: DecisionKind::ManualRequired,
                reasons: vec![
                    format!("semantic verification required but unavailable: {reason}"),
                    format!(
                        "verifier {:?} could not run the required check; a textually clean merge is not accepted without proof (fail-closed)",
                        verifier.name()
                    ),
                ],
            },
        }
    }

    /// A changed line references a symbol whose definition was modified by an
    /// already integrated proposal, and the proposal did not declare that
    /// dependency (declared dependencies are refused earlier as `MANUAL_REQUIRED`).
    fn undeclared_dependency_reason(&self, facts: &PatchFacts) -> Option<String> {
        let mut modified: semantics::ModifiedSymbols = BTreeMap::new();
        for integrated in &self.integrated {
            let path = integrated
                .paths
                .first()
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_owned());
            for symbol in &integrated.modified_symbols {
                modified
                    .entry(symbol.clone())
                    .or_insert_with(|| (path.clone(), integrated.proposal_id.clone(), integrated.agent_id.clone()));
            }
        }
        let dependency = semantics::undeclared_dependency(facts, &modified)?;
        Some(format!(
            "undeclared semantic dependency: a changed line references symbol {:?}, whose definition was modified by {} (agent {}); declare the dependency on {:?} or request arbitration",
            dependency.symbol, dependency.other_proposal, dependency.other_agent, dependency.path
        ))
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
    use crate::records::{MergeCheck, MergeCheckStatus};
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

    // ------------------------------------------------------------------
    // COLLAB-1 textual-mode tests (kept exactly: these are the 47 baseline
    // behaviours the crate must not lose).
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // COLLAB-2/3 semantic-gate tests (stub verifiers; the real rustc verifier
    // is exercised end-to-end by tests/collab2_two_functions.rs).
    // ------------------------------------------------------------------

    enum Stub {
        Pass,
        FailDiagnostic,
        Unavailable,
        PassOnlyMetadata,
    }

    struct StubVerifier(Stub);

    impl StubVerifier {
        fn check(&self, request: &VerificationRequest, kind: MergeCheckKind, status: MergeCheckStatus) -> MergeCheck {
            MergeCheck::new(
                kind,
                status,
                "stub-rustc",
                "1.95.0",
                "host",
                request.snapshot_after.id().clone(),
                "stub verdict",
                Vec::new(),
            )
        }
    }

    impl MergeVerifier for StubVerifier {
        fn name(&self) -> &'static str {
            "stub-verifier"
        }

        fn verify(&self, request: &VerificationRequest) -> VerificationOutcome {
            match self.0 {
                Stub::Pass => VerificationOutcome::Passed {
                    checks: request
                        .required_checks
                        .iter()
                        .map(|kind| self.check(request, *kind, MergeCheckStatus::Passed))
                        .collect(),
                },
                Stub::PassOnlyMetadata => VerificationOutcome::Passed {
                    checks: vec![self.check(request, MergeCheckKind::MetadataCheck, MergeCheckStatus::Passed)],
                },
                Stub::FailDiagnostic => {
                    let check = MergeCheck::new(
                        MergeCheckKind::MetadataCheck,
                        MergeCheckStatus::Failed,
                        "stub-rustc",
                        "1.95.0",
                        "host",
                        request.snapshot_after.id().clone(),
                        "linked crate does not compile",
                        vec![crate::records::MergeDiagnostic {
                            code: Some("E0428".to_owned()),
                            level: "error".to_owned(),
                            message: "the name `helper2` is defined multiple times".to_owned(),
                            line: Some(3),
                        }],
                    );
                    VerificationOutcome::Failed {
                        check,
                        reason: "merged snapshot does not compile (1 error)".to_owned(),
                    }
                }
                Stub::Unavailable => VerificationOutcome::Unavailable {
                    reason: "rustc not found".to_owned(),
                },
            }
        }
    }

    fn semantic_mode(stub: Stub) -> Coordinator {
        Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), FILE.to_string())]),
            Arc::new(StubVerifier(stub)),
        )
    }

    #[test]
    fn semantic_mode_accepts_only_with_a_complete_proof_and_records_it() {
        let mut c = semantic_mode(Stub::Pass);
        let h = hash(&c);
        let d = c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::AutoSafe, "reasons: {:?}", d.reasons);
        assert!(d.reasons.iter().any(|reason| reason.contains("semantic verification: passed")));

        let records = c.accepted_changes();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.journal_seq, 1);
        assert!(record.verification_required);
        assert_eq!(record.verification.status, MergeCheckStatus::Passed);
        assert_eq!(record.verification.required_checks, vec![MergeCheckKind::MetadataCheck]);
        assert_eq!(record.verification.checks.len(), 1);
        assert_eq!(record.verification.checks[0].evidence_id.len(), 64);
        assert!(record.snapshot_before != record.snapshot_after);
    }

    #[test]
    fn semantic_mode_turns_a_compile_failure_into_a_reject_with_evidence() {
        let mut c = semantic_mode(Stub::FailDiagnostic);
        let h = hash(&c);
        let before = c.current_id();
        let d = c.submit(&proposal(&c, "agent-b", &["beta"], PATCH_BETA, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::Reject);
        assert_eq!(c.current_id(), before, "a failed verification integrates nothing");
        assert!(c.accepted_changes().is_empty());
        assert!(
            d.reasons.iter().any(|reason| reason.contains("E0428")),
            "refusal must name the compiler diagnostic: {:?}",
            d.reasons
        );
        assert!(d.reasons.iter().any(|reason| reason.contains("evidence")));
    }

    #[test]
    fn semantic_mode_refuses_when_verification_is_unavailable() {
        let mut c = semantic_mode(Stub::Unavailable);
        let h = hash(&c);
        let before = c.current_id();
        let d = c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::ManualRequired);
        assert_eq!(c.current_id(), before);
        assert!(d.reasons.iter().any(|reason| reason.contains("unavailable")));
        assert!(d.reasons.iter().any(|reason| reason.contains("fail-closed")));
    }

    #[test]
    fn semantic_mode_rejects_an_incomplete_proof() {
        // The snapshot ships tests and the change is behavioural: TestRun is
        // required. A verifier that only ran the metadata check is not enough.
        let source = format!("{FILE}\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn t() {{ assert_eq!(super::alpha(), 1); }}\n}}\n");
        let mut c = Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), source)]),
            Arc::new(StubVerifier(Stub::PassOnlyMetadata)),
        );
        let h = hash(&c);
        let d = c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::ManualRequired);
        assert!(
            d.reasons.iter().any(|reason| reason.contains("incomplete")),
            "reasons: {:?}",
            d.reasons
        );
        assert!(c.accepted_changes().is_empty());
    }

    #[test]
    fn semantic_mode_requires_test_execution_when_tests_exist() {
        let source = format!("{FILE}\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn t() {{ assert_eq!(super::alpha(), 1); }}\n}}\n");
        struct Capture(std::sync::Mutex<Option<Vec<MergeCheckKind>>>);
        impl MergeVerifier for Capture {
            fn verify(&self, request: &VerificationRequest) -> VerificationOutcome {
                *self.0.lock().unwrap() = Some(request.required_checks.clone());
                VerificationOutcome::Passed { checks: Vec::new() }
            }
        }
        let capture = Arc::new(Capture(std::sync::Mutex::new(None)));
        let mut c = Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), source)]),
            capture.clone(),
        );
        let h = hash(&c);
        c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]));
        let required = capture.0.lock().unwrap().clone().expect("verifier called");
        assert!(required.contains(&MergeCheckKind::MetadataCheck));
        assert!(required.contains(&MergeCheckKind::TestCompile));
        assert!(required.contains(&MergeCheckKind::TestRun));
    }

    #[test]
    fn semantic_mode_never_auto_integrates_a_public_signature_change() {
        let source = "pub fn area(w: u32) -> u32 {\n    w * 2\n}\n";
        let mut c = Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), source.to_string())]),
            Arc::new(StubVerifier(Stub::Pass)),
        );
        let h = c.base().file_hash("lib.rs").unwrap().to_owned();
        let patch = "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n-pub fn area(w: u32) -> u32 {\n-    w * 2\n+pub fn area(w: u32, h: u32) -> u32 {\n+    w * h\n }\n";
        let d = c.submit(&proposal(&c, "agent-a", &["area"], patch, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::ManualRequired);
        assert!(d.reasons.iter().any(|reason| reason.contains("public API")));
        assert!(c.accepted_changes().is_empty());
    }

    #[test]
    fn semantic_mode_never_auto_integrates_an_abi_change() {
        let source = "#[no_mangle]\npub extern \"C\" fn entry(v: u32) -> u32 {\n    v + 1\n}\n";
        let mut c = Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), source.to_string())]),
            Arc::new(StubVerifier(Stub::Pass)),
        );
        let h = c.base().file_hash("lib.rs").unwrap().to_owned();
        let patch = "+++ b/lib.rs\n@@ -2,3 +2,3 @@\n-pub extern \"C\" fn entry(v: u32) -> u32 {\n+pub extern \"C\" fn entry(v: u64) -> u32 {\n     v + 1\n }\n";
        let d = c.submit(&proposal(&c, "agent-a", &["entry"], patch, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::ManualRequired);
        assert!(d.reasons.iter().any(|reason| reason.contains("ABI")));
    }

    #[test]
    fn semantic_mode_detects_an_undeclared_dependency_between_agents() {
        let source = "fn helper(x: u32) -> u32 {\n    x + 1\n}\n\nfn compose(x: u32) -> u32 {\n    helper(x) * 2\n}\n";
        let mut c = Coordinator::semantic(
            SourceSnapshot::new([("lib.rs".to_string(), source.to_string())]),
            Arc::new(StubVerifier(Stub::Pass)),
        );
        let h = c.base().file_hash("lib.rs").unwrap().to_owned();
        let patch_a = "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn helper(x: u32) -> u32 {\n-    x + 1\n+    x + 5\n }\n";
        let d = c.submit(&proposal(&c, "agent-a", &["helper"], patch_a, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::AutoSafe, "reasons: {:?}", d.reasons);

        let patch_b = "+++ b/lib.rs\n@@ -5,3 +5,3 @@\n fn compose(x: u32) -> u32 {\n-    helper(x) * 2\n+    helper(x) * 3\n }\n";
        let d = c.submit(&proposal(&c, "agent-b", &["compose"], patch_b, &[("lib.rs", &h)]));
        assert_eq!(d.decision, DecisionKind::ManualRequired);
        assert!(
            d.reasons.iter().any(|reason| reason.contains("undeclared semantic dependency")),
            "reasons: {:?}",
            d.reasons
        );
        assert_eq!(c.accepted_changes().len(), 1);
    }

    #[test]
    fn legacy_mode_records_a_verification_that_was_never_run() {
        let mut c = base();
        let h = hash(&c);
        assert_eq!(
            c.submit(&proposal(&c, "agent-a", &["alpha"], PATCH_ALPHA, &[("lib.rs", &h)]))
                .decision,
            DecisionKind::AutoSafe
        );
        let record = &c.accepted_changes()[0];
        assert!(!record.verification_required);
        assert_eq!(record.verification.status, MergeCheckStatus::Skipped);
        assert_eq!(record.journal_seq, 1);
        assert!(!c.verification_required());
    }
}
