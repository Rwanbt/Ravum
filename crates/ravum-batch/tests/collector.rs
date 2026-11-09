//! Tests déterministes du collecteur de lots (aucun rustc, aucun sleep :
//! les timestamps sont injectés).

use ravum_batch::{
    Analyzer, BatchCollector, BatchConfig, BatchOutcome, BatchProposal, BatchReason, FeedbackError,
    SubmitError, Verdict,
};
use std::collections::HashSet;

#[derive(Debug, Clone)]
struct DemoProposal {
    id: String,
    paths: Vec<String>,
    symbols: Vec<String>,
    base: String,
}

impl DemoProposal {
    fn new(id: &str, path: &str, symbol: &str) -> Self {
        Self {
            id: id.to_owned(),
            paths: vec![path.to_owned()],
            symbols: vec![symbol.to_owned()],
            base: "base-1".to_owned(),
        }
    }

    fn with_base(mut self, base: &str) -> Self {
        self.base = base.to_owned();
        self
    }
}

impl BatchProposal for DemoProposal {
    fn proposal_id(&self) -> &str {
        &self.id
    }
    fn paths(&self) -> &[String] {
        &self.paths
    }
    fn symbols(&self) -> &[String] {
        &self.symbols
    }
    fn base_id(&self) -> &str {
        &self.base
    }
}

#[derive(Debug, Default)]
struct FakeAnalyzer {
    calls: Vec<Vec<String>>,
    diagnose_calls: Vec<String>,
    reject_ids: HashSet<String>,
    /// Quand vrai, toute analyse groupée (>1 id) échoue même sans id rejeté —
    /// modélise un verdict de lot refusé dont les analyses individuelles passent.
    reject_batches: bool,
}

impl Analyzer for FakeAnalyzer {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        let ids: Vec<String> = proposal_ids.iter().map(|id| (*id).to_owned()).collect();
        self.calls.push(ids.clone());
        let rejected = ids.iter().find(|id| self.reject_ids.contains(id.as_str()));
        if let Some(id) = rejected {
            return Verdict::failed(format!("rejected: {id}"));
        }
        if self.reject_batches && ids.len() > 1 {
            return Verdict::failed("grouped analysis refused (simulated)");
        }
        Verdict::passed()
    }

    fn diagnose(&mut self, proposal_id: &str) -> Verdict {
        self.diagnose_calls.push(proposal_id.to_owned());
        if self.reject_ids.contains(proposal_id) {
            Verdict::failed(format!("diagnostic: {proposal_id}"))
        } else {
            Verdict::passed()
        }
    }
}

fn collector(window_ms: u64, max_batch: usize) -> BatchCollector<DemoProposal, FakeAnalyzer> {
    BatchCollector::new(
        BatchConfig::new(window_ms, max_batch).expect("valid config"),
        FakeAnalyzer::default(),
    )
}

#[test]
fn six_compatible_proposals_share_one_analysis() {
    let mut c = collector(50, 6);
    for index in 0..6 {
        c.submit(
            DemoProposal::new(
                &format!("agent-{index}"),
                &format!("src/f{index}.rs"),
                &format!("f{index}"),
            ),
            index as u64,
        )
        .unwrap();
    }
    let reports = c.poll(5);
    assert_eq!(reports.len(), 1, "one full batch, one analysis");
    assert_eq!(reports[0].reason, BatchReason::Full);
    assert_eq!(reports[0].proposal_ids.len(), 6);
    assert_eq!(reports[0].analyses_used, 1);
    assert_eq!(reports[0].outcome, BatchOutcome::AcceptedAll);
    assert_eq!(reports[0].wait_ms, 5, "closed when the 6th item arrived");
    assert_eq!(c.telemetry().batches_formed, 1);
    assert_eq!(c.telemetry().analyses_saved_net, 5);
    assert_eq!(c.into_analyzer().calls.len(), 1);
}

#[test]
fn max_batch_is_bounded_and_ids_are_monotonic() {
    let mut c = collector(50, 2);
    let mut ids = Vec::new();
    for index in 0..5 {
        ids.push(
            c.submit(
                DemoProposal::new(
                    &format!("a{index}"),
                    &format!("src/a{index}.rs"),
                    &format!("s{index}"),
                ),
                index as u64,
            )
            .unwrap(),
        );
    }
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    let reports = c.poll(100);
    let sizes: Vec<usize> = reports.iter().map(|r| r.proposal_ids.len()).collect();
    assert_eq!(sizes, vec![2, 2, 1]);
    assert!(reports.iter().all(|r| r.proposal_ids.len() <= 2));
    assert_eq!(c.telemetry().batches_formed, 3);
    assert_eq!(c.telemetry().analyses_used, 3);
    assert!(c.telemetry().no_loss());
}

#[test]
fn window_expiry_separates_batches_and_records_wait() {
    let mut c = collector(50, 6);
    c.submit(DemoProposal::new("a", "src/a.rs", "a"), 0)
        .unwrap();
    let first = c.poll(50);
    assert_eq!(first.len(), 1, "the first batch expires at t=50");
    assert_eq!(first[0].proposal_ids, vec!["a"]);
    assert_eq!(first[0].reason, BatchReason::WindowExpired);
    assert_eq!(first[0].wait_ms, 50);
    // b arrives after the first batch closed: it opens a distinct window.
    c.submit(DemoProposal::new("b", "src/b.rs", "b"), 60)
        .unwrap();
    let second = c.poll(110);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].proposal_ids, vec!["b"]);
    assert_eq!(second[0].reason, BatchReason::WindowExpired);
    assert_eq!(second[0].wait_ms, 50);
    assert_eq!(c.telemetry().wait_p50_ms(), Some(50));
}

#[test]
fn a_full_batch_does_not_wait_for_the_window() {
    let mut c = collector(100, 3);
    c.submit(DemoProposal::new("a", "src/a.rs", "a"), 10)
        .unwrap();
    c.submit(DemoProposal::new("b", "src/b.rs", "b"), 20)
        .unwrap();
    c.submit(DemoProposal::new("c", "src/c.rs", "c"), 30)
        .unwrap();
    let reports = c.poll(30);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].reason, BatchReason::Full);
    assert_eq!(reports[0].wait_ms, 20);
}

#[test]
fn a_textual_symbol_conflict_is_isolated_never_merged() {
    let mut c = collector(50, 6);
    c.submit(DemoProposal::new("a", "src/x.rs", "shared"), 0)
        .unwrap();
    c.submit(DemoProposal::new("b", "src/x.rs", "shared"), 10)
        .unwrap();
    let reports = c.poll(60);
    assert_eq!(reports.len(), 2, "the conflicting proposal is isolated");
    let telemetry = c.telemetry().clone();
    assert_eq!(telemetry.preflight_isolations, 1);
    assert!(telemetry.symbol_conflicts >= 1);
    assert_eq!(telemetry.analyses_used, 2);
    assert!(reports.iter().all(|r| r.proposal_ids.len() == 1));
    assert!(telemetry.no_loss());
}

#[test]
fn a_shared_path_is_isolated() {
    let mut c = collector(50, 6);
    c.submit(DemoProposal::new("a", "src/lib.rs", "alpha"), 0)
        .unwrap();
    c.submit(DemoProposal::new("b", "src/lib.rs", "beta"), 0)
        .unwrap();
    let reports = c.poll(50);
    assert_eq!(reports.len(), 2);
    assert!(c.telemetry().path_conflicts >= 1);
    assert_eq!(c.telemetry().preflight_isolations, 1);
}

#[test]
fn a_different_cas_base_is_never_merged() {
    let mut c = collector(50, 6);
    c.submit(
        DemoProposal::new("a", "src/a.rs", "a").with_base("base-1"),
        0,
    )
    .unwrap();
    c.submit(
        DemoProposal::new("b", "src/b.rs", "b").with_base("base-2"),
        0,
    )
    .unwrap();
    let reports = c.poll(50);
    assert_eq!(reports.len(), 2);
    assert!(c.telemetry().base_mismatches >= 1);
    assert_eq!(c.telemetry().preflight_isolations, 1);
}

#[test]
fn a_refused_batch_falls_back_to_n_plus_one_without_partial_acceptance() {
    let mut c = BatchCollector::new(
        BatchConfig::new(50, 6).unwrap(),
        FakeAnalyzer {
            reject_batches: true,
            ..FakeAnalyzer::default()
        },
    );

    for index in 0..3 {
        c.submit(
            DemoProposal::new(
                &format!("a{index}"),
                &format!("src/a{index}.rs"),
                &format!("s{index}"),
            ),
            0,
        )
        .unwrap();
    }
    let reports = c.poll(50);
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert!(
        !report.batch_verdict.ok,
        "the batch verdict is kept as failed"
    );
    assert_eq!(report.individual.len(), 3);
    assert_eq!(report.accepted_ids.len(), 3);
    assert!(report.rejected_ids.is_empty());
    assert_eq!(report.analyses_used, 4, "1 refused batch + 3 individual");
    assert!(matches!(report.outcome, BatchOutcome::FellBack { .. }));
    let telemetry = c.telemetry().clone();
    assert_eq!(telemetry.batches_fell_back, 1);
    assert_eq!(telemetry.individual_analyses, 3);
    assert_eq!(telemetry.analyses_saved_net, -1);
    assert!(telemetry.no_loss());
    let analyzer = c.into_analyzer();
    assert_eq!(analyzer.calls.len(), 4);
    assert_eq!(analyzer.calls[0].len(), 3, "batch analysis first");
    assert!(analyzer.calls[1..].iter().all(|call| call.len() == 1));
}

#[test]
fn fallback_rejects_only_the_failing_proposal() {
    let mut analyzer = FakeAnalyzer::default();
    analyzer.reject_ids.insert("bad".to_owned());
    let mut c = BatchCollector::new(BatchConfig::new(50, 6).unwrap(), analyzer);
    for id in ["a", "bad", "c"] {
        c.submit(DemoProposal::new(id, &format!("src/{id}.rs"), id), 0)
            .unwrap();
    }
    let reports = c.poll(50);
    let report = &reports[0];
    assert_eq!(report.accepted_ids, vec!["a", "c"]);
    assert_eq!(report.rejected_ids, vec!["bad"]);
    assert_eq!(report.analyses_used, 4);
    assert_eq!(c.telemetry().analyses_saved_net, -1);
    assert!(c.telemetry().no_loss());
}

#[test]
fn no_proposal_is_lost_in_a_mixed_stream() {
    let mut c = collector(50, 6);
    let mut submitted = Vec::new();
    let mut now = 0u64;
    for index in 0..24 {
        let symbol = format!("s{index}");
        let path = if index % 4 == 0 {
            "src/shared.rs".to_owned()
        } else {
            format!("src/f{index}.rs")
        };
        let base = if index % 7 == 0 { "base-2" } else { "base-1" };
        let id = format!("agent-{index}");
        submitted.push(id.clone());
        c.submit(
            DemoProposal {
                id,
                paths: vec![path],
                symbols: vec![symbol],
                base: base.to_owned(),
            },
            now,
        )
        .unwrap();
        now += 7;
    }
    let mut reports = c.poll(now + 100);
    reports.extend(c.flush(now + 100));
    let mut reported: Vec<String> = reports
        .iter()
        .flat_map(|report| report.accepted_ids.iter().chain(report.rejected_ids.iter()))
        .cloned()
        .collect();
    reported.sort();
    submitted.sort();
    assert_eq!(
        reported, submitted,
        "every proposal is reported exactly once"
    );
    assert_eq!(c.telemetry().proposals_received, 24);
    assert_eq!(c.telemetry().items_reported, 24);
    assert!(c.telemetry().no_loss());
    assert_eq!(c.pending_items(), 0);
    assert_eq!(c.pending_batches(), 0);
}

#[test]
fn immediate_feedback_is_available_before_the_batch() {
    let mut c = collector(50, 6);
    let item = c
        .submit(DemoProposal::new("a", "src/a.rs", "a"), 0)
        .unwrap();
    let verdict = c.feedback(item).expect("feedback before the batch");
    assert!(verdict.ok);
    assert_eq!(c.telemetry().feedback_calls, 1);
    assert_eq!(c.pending_batches(), 1);
    let reports = c.poll(50);
    assert_eq!(reports.len(), 1);
    assert_eq!(
        c.feedback(item),
        Err(FeedbackError::AlreadyAnalyzed(item)),
        "after analysis the immediate feedback is no longer available"
    );
    assert_eq!(c.feedback(999), Err(FeedbackError::UnknownItem(999)));
}

#[test]
fn invalid_duplicate_and_overfull_submissions_are_rejected_explicitly() {
    let mut c = BatchCollector::new(
        BatchConfig::new(100, 6)
            .unwrap()
            .with_max_pending(2)
            .unwrap(),
        FakeAnalyzer::default(),
    );
    assert_eq!(
        c.submit(DemoProposal::new("", "src/a.rs", "a"), 0),
        Err(SubmitError::EmptyProposalId)
    );
    let mut no_base = DemoProposal::new("x", "src/x.rs", "x");
    no_base.base = String::new();
    assert_eq!(c.submit(no_base, 0), Err(SubmitError::EmptyBaseId));
    c.submit(DemoProposal::new("a", "src/a.rs", "a"), 0)
        .unwrap();
    assert_eq!(
        c.submit(DemoProposal::new("a", "src/a.rs", "a"), 0),
        Err(SubmitError::DuplicateProposal("a".to_owned()))
    );
    c.submit(DemoProposal::new("b", "src/b.rs", "b"), 0)
        .unwrap();
    assert_eq!(
        c.submit(DemoProposal::new("c", "src/c.rs", "c"), 0),
        Err(SubmitError::PendingLimit(2))
    );
    let telemetry = c.telemetry().clone();
    assert_eq!(telemetry.proposals_received, 2);
    assert_eq!(telemetry.proposals_rejected, 4);
    assert_eq!(telemetry.duplicates_rejected, 1);
    assert_eq!(telemetry.queue_full_rejected, 1);
}

#[test]
fn next_batch_returns_batches_in_batch_id_order() {
    let mut c = collector(50, 6);
    c.submit(DemoProposal::new("a", "src/x.rs", "shared"), 0)
        .unwrap();
    c.submit(DemoProposal::new("b", "src/x.rs", "shared"), 0)
        .unwrap();
    let first = c.next_batch(50).expect("first batch due");
    assert_eq!(first.batch_id, 1);
    let second = c.next_batch(50).expect("second batch due");
    assert_eq!(second.batch_id, 2);
    assert!(second.batch_id > first.batch_id);
    assert!(c.next_batch(50).is_none());
}

#[test]
fn zero_window_expires_immediately() {
    let mut c = collector(0, 6);
    c.submit(DemoProposal::new("a", "src/a.rs", "a"), 7)
        .unwrap();
    let reports = c.poll(7);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].reason, BatchReason::WindowExpired);
    assert_eq!(reports[0].wait_ms, 0);
}

#[test]
fn mixed_scenario_about_half_of_the_batches_fall_back() {
    // 8 vagues de 5 propositions (max_batch=6 => fenêtre) ; 4 vagues contiennent
    // une proposition rejetée par l'analyseur (conflit sémantique imprévu, non
    // détectable par le pré-vol textuel) => repli N+1.
    let mut analyzer = FakeAnalyzer::default();
    for wave in (1..8).step_by(2) {
        analyzer.reject_ids.insert(format!("wave{wave}-bad"));
    }
    let mut c = BatchCollector::new(BatchConfig::new(50, 6).unwrap(), analyzer);
    for wave in 0..8 {
        let start = wave as u64 * 100;
        for item in 0..5 {
            let id = if wave % 2 == 1 && item == 2 {
                format!("wave{wave}-bad")
            } else {
                format!("wave{wave}-item{item}")
            };
            c.submit(
                DemoProposal::new(&id, &format!("src/w{wave}-{item}.rs"), &id),
                start + item as u64,
            )
            .unwrap();
        }
        let reports = c.poll(start + 50);
        assert_eq!(reports.len(), 1, "one batch per wave");
    }
    let telemetry = c.telemetry().clone();
    assert_eq!(telemetry.batches_formed, 8);
    assert_eq!(telemetry.batches_fell_back, 4);
    assert_eq!(telemetry.batches_accepted_all, 4);
    assert_eq!(telemetry.analyses_used, 4 * 1 + 4 * 6);
    assert_eq!(telemetry.analyses_saved_net, 4 * 4 + 4 * -1);
    assert_eq!(telemetry.wait_p50_ms(), Some(50));
    assert_eq!(telemetry.proposals_received, 40);
    assert_eq!(telemetry.items_reported, 40);
    assert!(telemetry.no_loss());
}
