//! Injectable semantic oracle (MVP-0c L3).
//!
//! The coordinator is an episodic merger: it can prove *textual* applicability
//! (exact patch application, compare-and-swap preconditions, symbol/line
//! overlap). It cannot know whether a patch breaks an interface used by another
//! crate, hides a weakened contract, or introduces an ambiguous import. That is
//! the job of an **oracle**: a component allowed to bring external semantic
//! knowledge (L1 inter-crate analysis, a compiler, a proof artefact, a human
//! review, ...) into the merge decision.
//!
//! Design constraints:
//! * the coordinator depends on the `SemanticOracle` **trait only** — the real
//!   L1-backed implementation is wired later, outside this crate;
//! * the default oracle is the current textual heuristics
//!   ([`TextualSemanticsOracle`]) and makes **no strong claim**: it only reports
//!   what the textual layer can see (public surface, ABI markers, removed
//!   assertions);
//! * an oracle that cannot answer returns `Unavailable`; the coordinator then
//!   fails closed (`MANUAL_REQUIRED`), it never promotes absence of findings to a
//!   pass;
//! * a `PROVED` status is only reachable through an explicit
//!   [`BoundedProof`] with provenance (property, scope, assumptions, tool,
//!   version, target snapshot). An oracle cannot ask for `PROVED` implicitly.
//!
//! Findings and proofs are content-addressed (`evidence_id`) so the decision
//! journal and the accepted-change records can be audited without re-running the
//! oracle.

use crate::records::MergeCheckKind;
use crate::semantics::{self, ContractRisk};
use ravum_protocol::sha256_hex;
use ravum_snapshot::SourceSnapshot;
use serde::{Deserialize, Serialize};

/// What kind of semantic problem a finding reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OracleFindingKind {
    /// A public interface changed while other crates may use it (L1 reports the
    /// actual dependents; the textual default only sees the surface change).
    InterfaceBreak,
    /// A changed line depends on a symbol another agent modified without declaring
    /// the dependency.
    UndeclaredDependency,
    /// A contract assertion was removed or a declared contract was weakened.
    WeakenedContract,
    /// An ABI marker (`extern "C"`, `#[no_mangle]`, `#[repr(C)]`, ...) changed.
    AbiChange,
    /// The merge introduces an ambiguous import (glob overlap, duplicate names).
    AmbiguousImport,
    /// A generated file / lockfile would be modified.
    LockfileChange,
    /// Behaviour changed in code that still compiles (a test or a proof failed).
    FunctionalRegression,
    /// The base snapshot is stale (kept for oracle symmetry; the coordinator
    /// normally detects this earlier).
    StaleSnapshot,
    /// Any other semantic concern an external oracle wants to report.
    Other,
}

impl OracleFindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OracleFindingKind::InterfaceBreak => "INTERFACE_BREAK",
            OracleFindingKind::UndeclaredDependency => "UNDECLARED_DEPENDENCY",
            OracleFindingKind::WeakenedContract => "WEAKENED_CONTRACT",
            OracleFindingKind::AbiChange => "ABI_CHANGE",
            OracleFindingKind::AmbiguousImport => "AMBIGUOUS_IMPORT",
            OracleFindingKind::LockfileChange => "LOCKFILE_CHANGE",
            OracleFindingKind::FunctionalRegression => "FUNCTIONAL_REGRESSION",
            OracleFindingKind::StaleSnapshot => "STALE_SNAPSHOT",
            OracleFindingKind::Other => "OTHER",
        }
    }
}

/// Severity of a finding. Only `Blocking` prevents integration; `Info`/`Warning`
/// are journaled so a future daemon or a human can see them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleSeverity {
    Info,
    Warning,
    Blocking,
}

impl OracleSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            OracleSeverity::Info => "info",
            OracleSeverity::Warning => "warning",
            OracleSeverity::Blocking => "blocking",
        }
    }

    pub fn is_blocking(self) -> bool {
        matches!(self, OracleSeverity::Blocking)
    }
}

/// One structured semantic finding, content-addressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleFinding {
    pub kind: OracleFindingKind,
    pub severity: OracleSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    /// Deterministic digest of the finding; the same finding reported twice gets
    /// the same id.
    pub evidence_id: String,
}

impl OracleFinding {
    pub fn new(
        kind: OracleFindingKind,
        severity: OracleSeverity,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let evidence_id = finding_evidence_id(kind, severity, &message, &[], &[]);
        Self {
            kind,
            severity,
            message,
            paths: Vec::new(),
            symbols: Vec::new(),
            evidence_id,
        }
    }

    pub fn with_paths(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.paths = paths.into_iter().map(Into::into).collect();
        self.recompute_evidence_id();
        self
    }

    pub fn with_symbols(mut self, symbols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.symbols = symbols.into_iter().map(Into::into).collect();
        self.recompute_evidence_id();
        self
    }

    fn recompute_evidence_id(&mut self) {
        self.evidence_id = finding_evidence_id(
            self.kind,
            self.severity,
            &self.message,
            &self.paths,
            &self.symbols,
        );
    }

    pub fn is_blocking(&self) -> bool {
        self.severity.is_blocking()
    }
}

/// Deterministic identity of one finding.
pub fn finding_evidence_id(
    kind: OracleFindingKind,
    severity: OracleSeverity,
    message: &str,
    paths: &[String],
    symbols: &[String],
) -> String {
    let mut buf = format!(
        "ravum-oracle-finding-v1\0{}\0{}\0{}",
        kind.as_str(),
        severity.as_str(),
        message
    );
    for path in paths {
        buf.push('\0');
        buf.push_str(path);
    }
    for symbol in symbols {
        buf.push('\0');
        buf.push_str(symbol);
    }
    sha256_hex(buf.as_bytes())
}

/// A proof artefact with **explicit provenance**. `PROVED` requires one of these;
/// an empty/incomplete proof is never accepted as a proof (it would turn a
/// bounded check into a general claim).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedProof {
    /// What exactly was proven (not "the program is correct").
    pub property: String,
    /// The paths/symbols the property covers. Empty is not a valid scope.
    pub scope: Vec<String>,
    /// The assumptions the proof depends on. Empty is not "no assumptions".
    pub assumptions: Vec<String>,
    pub tool: String,
    pub tool_version: String,
    /// The exact candidate snapshot the proof applies to.
    pub target_snapshot: ravum_protocol::SnapshotId,
    /// Deterministic digest of the proof content.
    pub evidence_id: String,
}

impl BoundedProof {
    pub fn new(
        property: impl Into<String>,
        scope: impl IntoIterator<Item = impl Into<String>>,
        assumptions: impl IntoIterator<Item = impl Into<String>>,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
        target_snapshot: ravum_protocol::SnapshotId,
    ) -> Self {
        let property = property.into();
        let scope: Vec<String> = scope.into_iter().map(Into::into).collect();
        let assumptions: Vec<String> = assumptions.into_iter().map(Into::into).collect();
        let tool = tool.into();
        let tool_version = tool_version.into();
        let evidence_id = proof_evidence_id(
            &property,
            &scope,
            &assumptions,
            &tool,
            &tool_version,
            &target_snapshot,
        );
        Self {
            property,
            scope,
            assumptions,
            tool,
            tool_version,
            target_snapshot,
            evidence_id,
        }
    }

    /// A proof is only usable when its provenance is complete.
    pub fn is_explicit(&self) -> bool {
        !self.property.trim().is_empty()
            && !self.scope.is_empty()
            && self.scope.iter().all(|item| !item.trim().is_empty())
            && !self.assumptions.is_empty()
            && self.assumptions.iter().all(|item| !item.trim().is_empty())
            && !self.tool.trim().is_empty()
            && !self.tool_version.trim().is_empty()
            && self.evidence_id.len() == 64
    }

    /// A proof only applies to the exact snapshot it names.
    pub fn applies_to(&self, snapshot: &ravum_protocol::SnapshotId) -> bool {
        &self.target_snapshot == snapshot
    }
}

/// Deterministic identity of one proof.
pub fn proof_evidence_id(
    property: &str,
    scope: &[String],
    assumptions: &[String],
    tool: &str,
    tool_version: &str,
    target_snapshot: &ravum_protocol::SnapshotId,
) -> String {
    let mut buf = format!(
        "ravum-bounded-proof-v1\0{}\0{}\0{}",
        property, tool, tool_version
    );
    buf.push('\0');
    buf.push_str(target_snapshot.as_str());
    for item in scope {
        buf.push('\0');
        buf.push_str(item);
    }
    for assumption in assumptions {
        buf.push('\0');
        buf.push_str(assumption);
    }
    sha256_hex(buf.as_bytes())
}

/// Everything an oracle may look at. It receives both snapshots and the textual
/// patch, so it can re-derive anything it needs; it cannot mutate coordinator
/// state.
#[derive(Debug, Clone)]
pub struct OracleRequest {
    pub proposal_id: String,
    pub agent_id: String,
    pub snapshot_before: SourceSnapshot,
    pub snapshot_after: SourceSnapshot,
    pub changed_paths: Vec<String>,
    pub declared_symbols: Vec<String>,
    pub declared_dependencies: Vec<String>,
    pub impacted_contracts: Vec<String>,
    pub unified_patch: String,
    /// Checks the coordinator requires before accepting this merge. A future L1
    /// oracle can use this to scope its work; it cannot remove a required check.
    pub required_checks: Vec<MergeCheckKind>,
}

/// Verdict of one oracle evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleVerdict {
    /// No finding. An optional bounded proof may accompany a clear verdict.
    Clear { bounded_proof: Option<BoundedProof> },
    /// Structured findings; the coordinator refuses if any is `Blocking`.
    Findings(Vec<OracleFinding>),
    /// The oracle could not answer; the coordinator fails closed.
    Unavailable { reason: String },
}

/// Injected semantic oracle. Implementations must be side-effect free with
/// respect to the workspace (read-only observation; scratch dirs only).
pub trait SemanticOracle: Send + Sync {
    fn name(&self) -> &'static str {
        "unnamed-oracle"
    }

    fn evaluate(&self, request: &OracleRequest) -> OracleVerdict;
}

/// Serialisable summary of one oracle evaluation, kept in the decision journal
/// and in the accepted-change record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleReport {
    pub oracle: String,
    /// `clear`, `findings`, `blocking_findings` or `unavailable`.
    pub verdict: String,
    pub findings: Vec<OracleFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounded_proof: Option<BoundedProof>,
    pub evidence_id: String,
}

impl OracleReport {
    pub fn from_verdict(oracle: &str, verdict: &OracleVerdict) -> Self {
        let mut report = Self {
            oracle: oracle.to_owned(),
            verdict: "clear".to_owned(),
            findings: Vec::new(),
            bounded_proof: None,
            evidence_id: String::new(),
        };
        match verdict {
            OracleVerdict::Clear { bounded_proof } => {
                report.bounded_proof = bounded_proof.clone();
            }
            OracleVerdict::Findings(findings) => {
                report.findings = findings.clone();
                report.verdict = if findings.iter().any(OracleFinding::is_blocking) {
                    "blocking_findings".to_owned()
                } else {
                    "findings".to_owned()
                };
            }
            OracleVerdict::Unavailable { .. } => {
                report.verdict = "unavailable".to_owned();
            }
        }
        report.recompute_evidence_id();
        report
    }

    /// Coordinator-owned findings (undeclared dependency needs the integrated
    /// history, which the oracle request does not carry) are merged here before
    /// the refusal decision is taken.
    pub fn merge_findings(&mut self, findings: impl IntoIterator<Item = OracleFinding>) {
        let mut added = false;
        for finding in findings {
            if !self
                .findings
                .iter()
                .any(|existing| existing.evidence_id == finding.evidence_id)
            {
                self.findings.push(finding);
                added = true;
            }
        }
        if added {
            self.verdict = if self.findings.iter().any(OracleFinding::is_blocking) {
                "blocking_findings".to_owned()
            } else {
                "findings".to_owned()
            };
            self.recompute_evidence_id();
        }
    }

    pub fn blocking_findings(&self) -> impl Iterator<Item = &OracleFinding> {
        self.findings.iter().filter(|finding| finding.is_blocking())
    }

    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(OracleFinding::is_blocking)
    }

    /// The blocking decision is "at least revalidate" only when the *only*
    /// blocking findings are interface breaks (a public surface change used
    /// elsewhere); any other blocking finding needs human arbitration, and an ABI
    /// change alongside an interface break is strictly worse than revalidation.
    pub fn only_interface_breaks_are_blocking(&self) -> bool {
        let blocking: Vec<&OracleFinding> = self.blocking_findings().collect();
        !blocking.is_empty()
            && blocking
                .iter()
                .all(|finding| finding.kind == OracleFindingKind::InterfaceBreak)
    }

    pub fn warning_findings(&self) -> impl Iterator<Item = &OracleFinding> {
        self.findings
            .iter()
            .filter(|finding| !finding.is_blocking())
    }

    pub fn summary_line(&self) -> String {
        format!(
            "semantic oracle: {} ({}, {} finding(s), evidence {})",
            self.oracle,
            self.verdict,
            self.findings.len(),
            self.evidence_id
        )
    }

    fn recompute_evidence_id(&mut self) {
        let mut buf = format!(
            "ravum-oracle-report-v1\0{}\0{}\0",
            self.oracle, self.verdict
        );
        for finding in &self.findings {
            buf.push_str(&finding.evidence_id);
            buf.push('\0');
        }
        if let Some(proof) = &self.bounded_proof {
            buf.push_str(&proof.evidence_id);
            buf.push('\0');
        }
        self.evidence_id = sha256_hex(buf.as_bytes());
    }
}

/// Default oracle: the textual heuristics already present in the coordinator
/// (no AST, no inter-crate knowledge, no strong claim). It reports the contract
/// risks it can see; everything else is "clear", which is only meaningful when a
/// declared verifier also ran.
#[derive(Debug, Default, Clone, Copy)]
pub struct TextualSemanticsOracle;

impl SemanticOracle for TextualSemanticsOracle {
    fn name(&self) -> &'static str {
        "textual-semantics-v1"
    }

    fn evaluate(&self, request: &OracleRequest) -> OracleVerdict {
        let files = match crate::patch::parse(&request.unified_patch) {
            Ok(files) => files,
            Err(error) => {
                return OracleVerdict::Unavailable {
                    reason: format!("textual oracle could not parse the candidate patch: {error}"),
                };
            }
        };
        let facts = semantics::analyse(&request.snapshot_before, &files);
        let mut findings = Vec::new();
        for risk in &facts.contract_risks {
            match risk {
                ContractRisk::PublicSurface(line) => findings.push(
                    OracleFinding::new(
                        OracleFindingKind::InterfaceBreak,
                        OracleSeverity::Blocking,
                        format!(
                            "public interface surface changed at {}:{}; usage by other crates is not known to the textual oracle, revalidation is required",
                            line.path, line.old_line
                        ),
                    )
                    .with_paths([line.path.clone()])
                    .with_symbols(line.symbol.clone()),
                ),
                ContractRisk::AbiSurface(line) => findings.push(
                    OracleFinding::new(
                        OracleFindingKind::AbiChange,
                        OracleSeverity::Blocking,
                        format!(
                            "ABI surface changed at {}:{}; ABI compatibility cannot be proven by a local compile",
                            line.path, line.old_line
                        ),
                    )
                    .with_paths([line.path.clone()]),
                ),
                ContractRisk::RemovedAssertion(line) => findings.push(
                    OracleFinding::new(
                        OracleFindingKind::WeakenedContract,
                        OracleSeverity::Blocking,
                        format!(
                            "contract assertion removed at {}:{}; weakening a contract requires human arbitration",
                            line.path, line.old_line
                        ),
                    )
                    .with_paths([line.path.clone()]),
                ),
            }
        }
        if findings.is_empty() {
            OracleVerdict::Clear {
                bounded_proof: None,
            }
        } else {
            OracleVerdict::Findings(findings)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_protocol::SnapshotId;

    fn request(base: &str, patch: &str) -> OracleRequest {
        OracleRequest {
            proposal_id: "agent-a/t".to_owned(),
            agent_id: "agent-a".to_owned(),
            snapshot_before: SourceSnapshot::new([("lib.rs".to_string(), base.to_string())]),
            snapshot_after: SourceSnapshot::new([(
                "lib.rs".to_string(),
                format!("{base}\n// candidate"),
            )]),
            changed_paths: vec!["lib.rs".to_owned()],
            declared_symbols: Vec::new(),
            declared_dependencies: Vec::new(),
            impacted_contracts: Vec::new(),
            unified_patch: patch.to_owned(),
            required_checks: vec![MergeCheckKind::MetadataCheck],
        }
    }

    #[test]
    fn the_default_oracle_is_clear_on_a_body_only_change() {
        let oracle = TextualSemanticsOracle;
        let request = request(
            "fn alpha() -> u32 {\n    1\n}\n",
            "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn alpha() -> u32 {\n-    1\n+    10\n }\n",
        );
        match oracle.evaluate(&request) {
            OracleVerdict::Clear { bounded_proof } => assert!(bounded_proof.is_none()),
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn the_default_oracle_reports_a_public_surface_change_as_interface_break() {
        let oracle = TextualSemanticsOracle;
        let request = request(
            "pub fn area(w: u32) -> u32 {\n    w * 2\n}\n",
            "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n-pub fn area(w: u32) -> u32 {\n-    w * 2\n+pub fn area(w: u32, h: u32) -> u32 {\n+    w * h\n }\n",
        );
        match oracle.evaluate(&request) {
            OracleVerdict::Findings(findings) => {
                assert_eq!(findings.len(), 1);
                assert_eq!(findings[0].kind, OracleFindingKind::InterfaceBreak);
                assert!(findings[0].is_blocking());
                assert_eq!(findings[0].paths, vec!["lib.rs".to_owned()]);
            }
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn the_default_oracle_reports_abi_and_removed_assertions() {
        let oracle = TextualSemanticsOracle;
        let abi = request(
            "#[no_mangle]\npub extern \"C\" fn entry(v: u32) -> u32 {\n    v\n}\n",
            "+++ b/lib.rs\n@@ -2,3 +2,3 @@\n-pub extern \"C\" fn entry(v: u32) -> u32 {\n+pub extern \"C\" fn entry(v: u64) -> u32 {\n     v\n }\n",
        );
        match oracle.evaluate(&abi) {
            OracleVerdict::Findings(findings) => {
                assert!(findings
                    .iter()
                    .any(|finding| finding.kind == OracleFindingKind::AbiChange
                        && finding.is_blocking()));
            }
            other => panic!("unexpected verdict: {other:?}"),
        }
        let weakened = request(
            "fn checked(v: u32) -> u32 {\n    assert!(v != 0);\n    v\n}\n",
            "+++ b/lib.rs\n@@ -1,4 +1,3 @@\n fn checked(v: u32) -> u32 {\n-    assert!(v != 0);\n     v\n }\n",
        );
        match oracle.evaluate(&weakened) {
            OracleVerdict::Findings(findings) => {
                assert!(findings.iter().any(|finding| {
                    finding.kind == OracleFindingKind::WeakenedContract && finding.is_blocking()
                }));
            }
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn a_report_decision_rule_only_revalidates_pure_interface_breaks() {
        let interface = OracleReport::from_verdict(
            "textual-semantics-v1",
            &OracleVerdict::Findings(vec![OracleFinding::new(
                OracleFindingKind::InterfaceBreak,
                OracleSeverity::Blocking,
                "public surface changed",
            )]),
        );
        assert!(interface.has_blocking());
        assert!(interface.only_interface_breaks_are_blocking());

        let abi = OracleReport::from_verdict(
            "textual-semantics-v1",
            &OracleVerdict::Findings(vec![
                OracleFinding::new(
                    OracleFindingKind::InterfaceBreak,
                    OracleSeverity::Blocking,
                    "public surface changed",
                ),
                OracleFinding::new(
                    OracleFindingKind::AbiChange,
                    OracleSeverity::Blocking,
                    "ABI changed",
                ),
            ]),
        );
        assert!(!abi.only_interface_breaks_are_blocking());
    }

    #[test]
    fn report_evidence_ids_are_deterministic() {
        let verdict = OracleVerdict::Findings(vec![OracleFinding::new(
            OracleFindingKind::AmbiguousImport,
            OracleSeverity::Blocking,
            "glob overlap",
        )]);
        let first = OracleReport::from_verdict("oracle", &verdict);
        let second = OracleReport::from_verdict("oracle", &verdict);
        assert_eq!(first.evidence_id, second.evidence_id);
        assert_eq!(first.evidence_id.len(), 64);
        assert_eq!(first.verdict, "blocking_findings");
    }

    #[test]
    fn a_bounded_proof_requires_complete_provenance_and_the_exact_snapshot() {
        let target = SnapshotId::new("snapshot-candidate");
        let valid = BoundedProof::new(
            "no data race on the audio buffer",
            ["src/dsp.rs"],
            ["single-threaded callback"],
            "kani",
            "0.40.0",
            target.clone(),
        );
        assert!(valid.is_explicit());
        assert!(valid.applies_to(&target));
        assert!(!valid.applies_to(&SnapshotId::new("another-snapshot")));

        let no_assumptions = BoundedProof::new(
            "some property",
            ["src/dsp.rs"],
            Vec::<String>::new(),
            "kani",
            "0.40.0",
            target.clone(),
        );
        assert!(!no_assumptions.is_explicit());
        let no_scope = BoundedProof::new(
            "some property",
            Vec::<String>::new(),
            ["assumption"],
            "kani",
            "0.40.0",
            target,
        );
        assert!(!no_scope.is_explicit());
    }
}
