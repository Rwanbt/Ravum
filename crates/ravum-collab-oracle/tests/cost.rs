//! Cost measurement of one real L1 oracle evaluation on `fixtures/multicrate`.
//!
//! Five evaluations of the same (content-addressed) snapshot pair. The first one
//! is cold (empty scratch state); the next four reuse the content-addressed
//! scratch directories, so their numbers show the steady-state cost of the
//! adapter. The numbers are observations on this machine, not a performance
//! claim; they are reported in `evidence/mvp0c-collab-oracle-2026-11-09.json`.

mod support;

use ravum_collab_oracle::UnitGraphOracle;
use ravum_workspace_coordinator::{OracleVerdict, SemanticOracle};
use std::time::Instant;
use support::*;

#[test]
fn cost_of_five_real_evaluations_on_the_multicrate_fixture() {
    let oracle = UnitGraphOracle::new(state_dir("cost-multicrate"));
    let request = body_only_request(CORE_DEPENDENTS);

    let mut times = Vec::new();
    let mut costs = Vec::new();
    for run in 0..5 {
        let started = Instant::now();
        let verdict = oracle.evaluate(&request);
        let elapsed = started.elapsed().as_millis() as u64;
        let cost = oracle.last_cost().expect("cost recorded");
        assert!(
            matches!(verdict, OracleVerdict::Clear { .. }),
            "the measured request must stay Clear (declared dependents, compatible change): {verdict:?}"
        );
        println!(
            "run {}: {} ms | rustc={} cargo_fallback={} cargo_metadata={} cache_hits={} fallback_units={} changed={:?}",
            run + 1,
            elapsed,
            cost.rustc_invocations,
            cost.cargo_fallback_invocations,
            cost.cargo_metadata_invocations,
            cost.cache_hits,
            cost.fallback_units,
            cost.changed_units
        );
        times.push(elapsed);
        costs.push(cost);
    }

    let cold = times[0];
    let mut sorted = times.clone();
    sorted.sort_unstable();
    let median = sorted[2];
    let max = sorted[4];
    println!("cold_start_ms={cold} median_ms={median} max_ms={max} runs={times:?}");

    assert_eq!(times.len(), 5);
    for cost in &costs {
        assert_eq!(
            cost.cargo_metadata_invocations, 2,
            "each evaluation discovers the before and after graph"
        );
        assert_eq!(cost.changed_units, vec!["core".to_owned()]);
        assert!(
            cost.cargo_fallback_invocations > 0,
            "the fixture has fallback units"
        );
    }
    assert!(
        costs[0].rustc_invocations > 0,
        "the cold campaign must invoke rustc: {:?}",
        costs[0]
    );
    assert!(
        costs[1].cache_hits > 0,
        "later evaluations reuse the content-addressed verdict archive: {:?}",
        costs[1]
    );
    assert!(
        costs[0].rustc_invocations >= costs[1].rustc_invocations,
        "the cold run cannot compile fewer units than a warm cached run: {:?}",
        costs
            .iter()
            .map(|cost| (cost.rustc_invocations, cost.cache_hits))
            .collect::<Vec<_>>()
    );
    assert!(median > 0, "a real evaluation takes measurable time");
}
