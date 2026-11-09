//! Fenêtre de groupement adaptative : bornes, réaction à la densité et à la
//! rareté, absence de perte, et comparaison déterministe avec les fenêtres
//! fixes 0/50/100 ms sur une trace d'arrivées identique.
//!
//! Aucun `sleep`, aucun rustc : les horodatages sont injectés, donc la mesure
//! est reproductible. Ce n'est pas une mesure de bout en bout : c'est le coût
//! du collecteur seul, sur une trace synthétique déclarée comme telle.

use ravum_batch::{
    AdaptiveWindow, Analyzer, BatchCollector, BatchConfig, BatchProposal, ConfigError, Verdict,
    DEFAULT_ADAPTIVE_STEP_MS, MAX_WINDOW_MS,
};

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
struct CountingAnalyzer {
    analyses: usize,
    items: usize,
}

impl Analyzer for CountingAnalyzer {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        self.analyses += 1;
        self.items += proposal_ids.len();
        Verdict::passed()
    }
}

struct TraceOutcome {
    batches: usize,
    analyses: usize,
    items: usize,
    effective_window_ms: u64,
    wait_total_ms: u64,
    no_loss: bool,
}

fn run_trace(config: BatchConfig, arrivals_ms: &[u64]) -> TraceOutcome {
    let mut collector = BatchCollector::new(config, CountingAnalyzer::default());
    let mut batches = 0;
    let last = *arrivals_ms.last().unwrap_or(&0);
    for (index, at) in arrivals_ms.iter().enumerate() {
        // Un ordonnanceur réel expire d'abord les fenêtres échues, puis soumet :
        // l'ordre inverse permettrait à une proposition tardive de rejoindre un
        // lot déjà hors fenêtre.
        batches += collector.poll(*at).len();
        collector
            .submit(
                DemoProposal::new(
                    &format!("agent-{index}/task"),
                    &format!("src/f{index}.rs"),
                    &format!("f{index}"),
                ),
                *at,
            )
            .expect("submit");
    }
    batches += collector
        .flush(last + MAX_WINDOW_MS + DEFAULT_ADAPTIVE_STEP_MS * 4)
        .len();
    TraceOutcome {
        batches,
        analyses: collector.telemetry().analyses_used as usize,
        items: collector.telemetry().items_reported as usize,
        effective_window_ms: collector.effective_window_ms(),
        wait_total_ms: collector.telemetry().wait_samples_ms.iter().sum(),
        no_loss: collector.telemetry().no_loss(),
    }
}

#[test]
fn adaptive_window_bounds_are_validated() {
    assert!(AdaptiveWindow::new(0, 100, 25).is_ok());
    assert!(AdaptiveWindow::new(50, 50, 10).is_ok());
    assert_eq!(
        AdaptiveWindow::new(80, 40, 10),
        Err(ConfigError::InvalidAdaptiveWindow {
            min_ms: 80,
            max_ms: 40
        })
    );
    assert_eq!(
        AdaptiveWindow::new(0, 101, 10),
        Err(ConfigError::InvalidAdaptiveWindow {
            min_ms: 0,
            max_ms: 101
        })
    );
    assert_eq!(
        AdaptiveWindow::new(0, 100, 0),
        Err(ConfigError::AdaptiveStepZero)
    );
    assert!(BatchConfig::adaptive(10, 100, 20, 6).is_ok());
    assert!(BatchConfig::adaptive(10, 100, 20, 0).is_err());
}

#[test]
fn density_grows_the_window_and_rarity_shrinks_it() {
    let mut collector = BatchCollector::new(
        BatchConfig::adaptive(10, 100, 20, 8).unwrap(),
        CountingAnalyzer::default(),
    );
    assert_eq!(collector.effective_window_ms(), 10, "starts at the minimum");

    // Densité : chaque proposition (chemins et symboles distincts, donc
    // compatibles) rejoint le lot ouvert, la fenêtre grandit.
    let mut at = 0;
    for index in 0..5 {
        collector
            .submit(
                DemoProposal::new(
                    &format!("dense-{index}"),
                    &format!("src/d{index}.rs"),
                    &format!("symbol{index}"),
                ),
                at,
            )
            .expect("submit");
        collector.poll(at);
        at += 5;
    }
    let grown = collector.effective_window_ms();
    assert!(
        grown > 10 && grown <= 100,
        "density must grow the window within bounds, got {grown}"
    );
    assert!(collector.telemetry().window_growths > 0);

    // Rareté : après la phase dense, un lot d'une seule proposition expire sans
    // arrivée et la fenêtre rétrécit (bornée par `min_ms`).
    collector.flush(at);
    collector
        .submit(DemoProposal::new("alone", "src/a.rs", "a"), at)
        .expect("submit");
    let before = collector.effective_window_ms();
    assert!(before > 10, "the dense phase must have grown the window");
    collector.poll(at + before + 1);
    assert!(
        collector.effective_window_ms() < before,
        "a sparse expiry must shrink the window"
    );
    assert!(collector.effective_window_ms() >= 10, "never below min");
    assert!(collector.telemetry().window_shrinks > 0);
}

#[test]
fn a_dense_trace_coalesces_more_than_a_zero_window() {
    // 8 propositions à 5 ms d'intervalle : la fenêtre fixe 0 ne peut rien
    // regrouper, la fenêtre adaptative regroupe.
    let arrivals: Vec<u64> = (0..8).map(|index| index * 5).collect();
    let fixed_zero = run_trace(BatchConfig::new(0, 8).unwrap(), &arrivals);
    let adaptive = run_trace(BatchConfig::adaptive(10, 100, 20, 8).unwrap(), &arrivals);

    assert_eq!(fixed_zero.batches, 8, "a zero window never coalesces");
    assert!(
        adaptive.batches < fixed_zero.batches,
        "adaptive must form fewer batches on a dense trace: adaptive={} fixed0={}",
        adaptive.batches,
        fixed_zero.batches
    );
    assert!(adaptive.no_loss && fixed_zero.no_loss);
}

#[test]
fn a_sparse_trace_does_not_wait_for_the_maximum_window() {
    // 3 propositions très espacées : une fenêtre fixe 100 attendrait 100 ms à
    // chaque fois, l'adaptative revient vite au minimum.
    let arrivals = [0u64, 500, 1000];
    let fixed_max = run_trace(BatchConfig::new(100, 8).unwrap(), &arrivals);
    let adaptive = run_trace(BatchConfig::adaptive(10, 100, 40, 8).unwrap(), &arrivals);

    assert_eq!(fixed_max.batches, 3);
    assert_eq!(adaptive.batches, 3);
    assert_eq!(
        adaptive.effective_window_ms, 10,
        "after sparse arrivals the adaptive window sits at its minimum"
    );
    assert!(adaptive.no_loss);
}

#[test]
fn fixed_windows_0_50_100_and_adaptive_are_compared_on_one_trace() {
    // Trace déclarée : 12 propositions, densité par paquets de 3, silences de
    // 200 ms entre les paquets (simule trois agents qui écrivent par rafales).
    let mut arrivals: Vec<u64> = Vec::new();
    let mut at = 0;
    for burst in 0..4 {
        for index in 0..3 {
            arrivals.push(at + index * 5);
        }
        at += 200 + burst * 0;
    }
    let fixed0 = run_trace(BatchConfig::new(0, 8).unwrap(), &arrivals);
    let fixed50 = run_trace(BatchConfig::new(50, 8).unwrap(), &arrivals);
    let fixed100 = run_trace(BatchConfig::new(100, 8).unwrap(), &arrivals);
    let adaptive = run_trace(BatchConfig::adaptive(10, 100, 20, 8).unwrap(), &arrivals);

    for outcome in [&fixed0, &fixed50, &fixed100, &adaptive] {
        assert!(outcome.no_loss, "no proposal may be lost");
        assert_eq!(outcome.items, arrivals.len(), "every proposal reported");
    }
    assert!(fixed0.batches >= fixed50.batches);
    assert!(adaptive.batches <= fixed100.batches);
    println!(
        "MEASURE batches: fixed0={} fixed50={} fixed100={} adaptive={} | analyses: {} {} {} {} | wait_total_ms: {} {} {} {}",
        fixed0.batches,
        fixed50.batches,
        fixed100.batches,
        adaptive.batches,
        fixed0.analyses,
        fixed50.analyses,
        fixed100.analyses,
        adaptive.analyses,
        fixed0.wait_total_ms,
        fixed50.wait_total_ms,
        fixed100.wait_total_ms,
        adaptive.wait_total_ms
    );
}
