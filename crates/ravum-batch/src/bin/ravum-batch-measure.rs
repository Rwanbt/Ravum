//! Mesure du collecteur de lots sur des arrivées simulées, sans rustc et sans
//! cargo dans la boucle : les timestamps sont injectés et l'analyseur est
//! déterministe. Écrit `evidence/mvp0c-batch-2026-11-09.json`.
//!
//! Comparaison au modèle L2.2 calibré (coûts mesurés en phase 1 : cycle d'analyse
//! 191 ms, reprise de lot refusé 1596 ms) : le collecteur ne promet pas un code
//! plus correct, il réduit le **nombre d'analyses** ; la conversion en temps est
//! un modèle explicitement étiqueté.

use ravum_batch::{
    percentile, Analyzer, BatchCollector, BatchConfig, BatchOutcome, BatchProposal, BatchReport,
    Verdict,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;

/// Coût mesuré phase 1 (docs/mvp/MVP-0c-build-performance.md).
const CYCLE_MS: u64 = 191;
/// Coût mesuré d'un lot refusé + reprise individuelle (phase 1, N=6).
const RECOVERY_MS: u64 = 1596;
/// Fenêtre par défaut mesurée.
const WINDOW_MS: u64 = 50;

#[derive(Debug, Clone)]
struct DemoProposal {
    id: String,
    paths: Vec<String>,
    symbols: Vec<String>,
    base: String,
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

struct DemoAnalyzer {
    reject_ids: HashSet<String>,
    analyses_calls: u64,
}

impl Analyzer for DemoAnalyzer {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        self.analyses_calls += 1;
        match proposal_ids
            .iter()
            .find(|id| self.reject_ids.contains(**id))
        {
            Some(id) => Verdict::failed(format!("simulated semantic rejection: {id}")),
            None => Verdict::passed(),
        }
    }
}

fn demo(id: &str, wave: usize, item: usize) -> DemoProposal {
    DemoProposal {
        id: id.to_owned(),
        paths: vec![format!("src/w{wave}-{item}.rs")],
        symbols: vec![id.to_owned()],
        base: "base-1".to_owned(),
    }
}

struct Scenario {
    name: &'static str,
    description: &'static str,
    reports: Vec<BatchReport>,
    telemetry: ravum_batch::BatchTelemetry,
    proposals: usize,
    analyzer_calls: u64,
}

fn run_scenario(
    name: &'static str,
    description: &'static str,
    waves: &[(u64, Vec<DemoProposal>)],
    reject_ids: HashSet<String>,
) -> Scenario {
    let mut analyzer = DemoAnalyzer {
        reject_ids,
        analyses_calls: 0,
    };
    let mut collector = BatchCollector::new(
        BatchConfig::new(WINDOW_MS, 6).expect("default window"),
        &mut analyzer,
    );
    let mut reports = Vec::new();
    let mut proposals = 0usize;
    let mut last_start = 0u64;
    for (start, items) in waves {
        last_start = *start;
        for (index, proposal) in items.iter().enumerate() {
            collector
                .submit(proposal.clone(), *start + index as u64)
                .expect("unique proposal id");
            proposals += 1;
        }
        // +2 ms covers the last arrival of a wave (item index <= 2 in the
        // textual scenario) so no half-open window leaks into the next wave.
        reports.extend(collector.poll(*start + WINDOW_MS + 2));
    }
    reports.extend(collector.flush(last_start + WINDOW_MS + 2));
    let telemetry = collector.telemetry().clone();
    let analyzer_calls = collector.into_analyzer().analyses_calls;
    Scenario {
        name,
        description,
        reports,
        telemetry,
        proposals,
        analyzer_calls,
    }
}

fn summarize(scenario: &Scenario) -> Value {
    let telemetry = &scenario.telemetry;
    let waits: Vec<u64> = telemetry.wait_samples_ms.clone();
    let mut wait_sorted = waits.clone();
    wait_sorted.sort_unstable();
    let accepted_all = scenario
        .reports
        .iter()
        .filter(|report| matches!(&report.outcome, BatchOutcome::AcceptedAll))
        .count();
    let fell_back = telemetry.batches_fell_back as usize;
    let analyses_used: usize = scenario
        .reports
        .iter()
        .map(|report| report.analyses_used)
        .sum();
    let sequential_analyses = scenario.proposals;
    json!({
        "name": scenario.name,
        "description": scenario.description,
        "config": { "window_ms": WINDOW_MS, "max_batch": 6 },
        "proposals": scenario.proposals,
        "batches": scenario.reports.len(),
        "batches_accepted_all": accepted_all,
        "batches_fell_back": fell_back,
        "fallback_rate": if scenario.reports.is_empty() { 0.0 } else { fell_back as f64 / scenario.reports.len() as f64 },
        "sequential_analyses_baseline": sequential_analyses,
        "analyses_used": analyses_used,
        "analyses_saved_net": telemetry.analyses_saved_net,
        "analyzer_calls_observed": scenario.analyzer_calls,
        "preflight_isolations": telemetry.preflight_isolations,
        "path_conflicts": telemetry.path_conflicts,
        "symbol_conflicts": telemetry.symbol_conflicts,
        "base_mismatches": telemetry.base_mismatches,
        "wait_ms": {
            "samples": waits,
            "min": wait_sorted.first().copied(),
            "max": wait_sorted.last().copied(),
            "p50": percentile(&waits, 50),
            "p95": percentile(&waits, 95),
        },
        "no_loss": telemetry.no_loss() && telemetry.proposals_received == scenario.proposals as u64,
        "items_reported": telemetry.items_reported,
        "proposals_received": telemetry.proposals_received,
        "batches_detail": scenario.reports.iter().map(|report| json!({
            "batch_id": report.batch_id,
            "reason": report.reason,
            "proposals": &report.proposal_ids,
            "wait_ms": report.wait_ms,
            "outcome": &report.outcome,
            "analyses_used": report.analyses_used,
        })).collect::<Vec<_>>(),
    })
}

/// Conversion modèle : coût séquentiel N×cycle vs coût collecté
/// (fenêtre + cycle si lot accepté, fenêtre + reprise mesurée si repli).
fn model_cost(scenario: &Scenario) -> (u64, u64) {
    let sequential = scenario.proposals as u64 * CYCLE_MS;
    let mut collected = 0u64;
    for report in &scenario.reports {
        collected += report.wait_ms;
        collected += match &report.outcome {
            BatchOutcome::AcceptedAll => CYCLE_MS,
            BatchOutcome::FellBack { .. } => RECOVERY_MS,
        };
    }
    (sequential, collected)
}

fn scenario_a() -> Scenario {
    let mut waves = Vec::new();
    for wave in 0..5 {
        let start = wave as u64 * 100;
        let items: Vec<DemoProposal> = (0..6)
            .map(|item| demo(&format!("a-w{wave}-i{item}"), wave, item))
            .collect();
        waves.push((start, items));
    }
    run_scenario(
        "compatible-5x6",
        "5 vagues de 6 propositions disjointes (max_batch=6) : un lot plein par vague, une analyse",
        &waves,
        HashSet::new(),
    )
}

fn scenario_b_mixed() -> Scenario {
    let mut waves = Vec::new();
    let mut rejected = HashSet::new();
    for wave in 0..8 {
        let start = wave as u64 * 100;
        let items: Vec<DemoProposal> = (0..5)
            .map(|item| {
                let id = if wave % 2 == 1 && item == 2 {
                    format!("b-w{wave}-unexpected-reject")
                } else {
                    format!("b-w{wave}-i{item}")
                };
                if wave % 2 == 1 && item == 2 {
                    rejected.insert(id.clone());
                }
                demo(&id, wave, item)
            })
            .collect();
        waves.push((start, items));
    }
    run_scenario(
        "mixed-p-50",
        "8 vagues de 5 propositions ; 4 vagues sur 8 contiennent un rejet sémantique imprévu (non détectable par le pré-vol) => p≈50 % de lots en repli",
        &waves,
        rejected,
    )
}

fn scenario_c_textual() -> Scenario {
    // 6 vagues de 3 propositions : deux partagent le même symbole (conflit
    // textuel détecté par le pré-vol), la troisième est indépendante.
    let mut waves = Vec::new();
    for wave in 0..6 {
        let start = wave as u64 * 100;
        let items = vec![
            DemoProposal {
                id: format!("c-w{wave}-shared-a"),
                paths: vec![format!("src/shared-{wave}.rs")],
                symbols: vec!["shared_fn".to_owned()],
                base: "base-1".to_owned(),
            },
            DemoProposal {
                id: format!("c-w{wave}-shared-b"),
                paths: vec![format!("src/shared-{wave}.rs")],
                symbols: vec!["shared_fn".to_owned()],
                base: "base-1".to_owned(),
            },
            demo(&format!("c-w{wave}-independent"), wave, 2),
        ];
        waves.push((start, items));
    }
    run_scenario(
        "textual-conflicts",
        "6 vagues : un symbole partagé par deux propositions est isolé par le pré-vol (jamais fusionné); la troisième reste groupable",
        &waves,
        HashSet::new(),
    )
}

fn main() {
    let started = std::time::Instant::now();
    let scenarios = [scenario_a(), scenario_b_mixed(), scenario_c_textual()];
    let summaries: Vec<Value> = scenarios.iter().map(summarize).collect();

    let mixed = &scenarios[1];
    let (sequential_model, collected_model) = model_cost(mixed);
    let batches = mixed.reports.len() as f64;
    let conflict_rate = if batches == 0.0 {
        0.0
    } else {
        mixed
            .reports
            .iter()
            .filter(|r| !matches!(&r.outcome, BatchOutcome::AcceptedAll))
            .count() as f64
            / batches
    };
    let expected_batch_cost = WINDOW_MS as f64
        + (1.0 - conflict_rate) * CYCLE_MS as f64
        + conflict_rate * RECOVERY_MS as f64;
    let model_sequential_batch_cost = 5.0 * CYCLE_MS as f64;
    let model_gain_per_batch = model_sequential_batch_cost - expected_batch_cost;

    // Balayage du modèle L2.2 à p=50 % : le coût d'un repli (1596 ms, mesuré à
    // N=6) est conservateur pour les petits lots ; le seuil de rentabilité en N
    // se lit directement dans ce tableau.
    let break_even_rate = |n: f64| {
        ((n - 1.0) * CYCLE_MS as f64 - WINDOW_MS as f64) / (RECOVERY_MS as f64 - CYCLE_MS as f64)
    };
    let model_sweep: Vec<Value> = (3..=8)
        .map(|n| {
            let sequential = n as f64 * CYCLE_MS as f64;
            json!({
                "n": n,
                "sequential_cost_ms": sequential,
                "expected_grouped_cost_ms_at_p50": expected_batch_cost,
                "gain_ms": sequential - expected_batch_cost,
                "profitable_at_p50": sequential > expected_batch_cost,
                "break_even_conflict_rate": break_even_rate(n as f64),
            })
        })
        .collect();
    let model_comparison = json!({
        "conflict_rate_observed": conflict_rate,
        "sequential_cost_model_ms": sequential_model,
        "collected_cost_model_ms": collected_model,
        "gain_model_ms": sequential_model - collected_model,
        "gain_percent": if sequential_model == 0 { 0.0 } else { 100.0 * (sequential_model - collected_model) as f64 / sequential_model as f64 },
        "expected_batch_cost_ms": expected_batch_cost,
        "sequential_batch_cost_ms": model_sequential_batch_cost,
        "gain_per_batch_ms": model_gain_per_batch,
        "model_sweep_p50_window50": model_sweep,
        "note": "coût collecté = fenêtre mesurée + 191 ms par lot accepté + 1596 ms par lot en repli (reprise mesurée à N=6, conservatrice pour N=5) ; le balayage N=3..8 applique le modèle L2.2 à p=50 % et w=50 ms",
    });

    let checks = json!({
        "compatible_wave_uses_one_analysis_each": summaries[0]["batches"] == 5 && summaries[0]["analyses_used"] == 5,
        "compatible_wave_saves_25_analyses": summaries[0]["analyses_saved_net"] == 25,
        "mixed_has_about_half_fallbacks": summaries[1]["batches_fell_back"] == 4 && summaries[1]["batches"] == 8,
        "mixed_analyses_used_matches_fallback_model": summaries[1]["analyses_used"] == 28,
        "mixed_no_loss": summaries[1]["no_loss"] == true,
        "mixed_wait_p50_is_window": summaries[1]["wait_ms"]["p50"] == WINDOW_MS,
        "textual_conflict_is_never_merged": summaries[2]["preflight_isolations"] == 6
            && summaries[2]["batches"] == 12
            && summaries[2]["analyses_used"] == 12
            && summaries[2]["path_conflicts"] == 6
            && summaries[2]["symbol_conflicts"] == 6,
        "textual_scenario_no_loss": summaries[2]["no_loss"] == true,
        "all_analyzer_calls_accounted": summaries.iter().all(|summary| summary["analyzer_calls_observed"] == summary["analyses_used"]),
        "no_proposal_lost": summaries.iter().all(|summary| summary["no_loss"] == true),
    });
    let valid = checks
        .as_object()
        .map(|map| map.values().all(|value| value == &json!(true)))
        .unwrap_or(false);

    let machine = json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "logical_parallelism": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
    });

    let evidence = json!({
        "schema_version": 1,
        "suite": "batch",
        "generated_unix": ravum_batch_measure_unix(),
        "duration_ms": started.elapsed().as_millis() as u64,
        "machine": machine,
        "protocol": {
            "goal": "mesurer le collecteur de lots borné sur des arrivées simulées et le comparer au modèle L2.2 calibré",
            "no_rustc": true,
            "no_cargo_in_loops": true,
            "timestamps": "injectés (ms); aucun sleep",
            "analyzer": "déterministe: rejette un lot si l'un de ses ids est marqué; rejette l'item marqué en analyse individuelle",
            "config": { "window_ms": WINDOW_MS, "max_batch": 6, "max_pending": ravum_batch::DEFAULT_MAX_PENDING },
            "calibrated_costs_source": "docs/mvp/MVP-0c-build-performance.md (phase 1) + evidence/mvp0c-grouping-2026-11-09.json",
            "calibrated_costs": { "cycle_ms": CYCLE_MS, "recovery_ms": RECOVERY_MS },
            "model_label": "MODÈLE CALIBRÉ: le collecteur réduit le nombre d'analyses; la conversion en ms n'est pas une mesure bout en bout",
        },
        "scenarios": summaries,
        "model_comparison_mixed": model_comparison,
        "checks": checks,
        "valid": valid,
    });

    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../evidence/mvp0c-batch-2026-11-09.json")
        });
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string_pretty(&evidence).expect("serialize evidence");
    std::fs::write(&out, format!("{text}\n")).expect("write evidence");
    println!(
        "batch measurement: valid={valid} scenarios={} duration={}ms",
        summaries.len(),
        started.elapsed().as_millis()
    );
    for summary in &summaries {
        println!(
            "  {:<18} proposals={} batches={} fallbacks={} analyses={} saved={} wait p50={:?} p95={:?}",
            summary["name"].as_str().unwrap_or("?"),
            summary["proposals"],
            summary["batches"],
            summary["batches_fell_back"],
            summary["analyses_used"],
            summary["analyses_saved_net"],
            summary["wait_ms"]["p50"],
            summary["wait_ms"]["p95"],
        );
    }
    println!("  written: {}", out.display());
    if !valid {
        std::process::exit(1);
    }
}

fn ravum_batch_measure_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
