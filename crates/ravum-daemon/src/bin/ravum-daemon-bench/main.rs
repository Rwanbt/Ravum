//! MVP-0b incremental-analysis measurement harness.
//!
//! Produces the raw evidence files quoted by `docs/mvp/MVP-0b-incremental.md`:
//!
//! * `--suite campaign` — 7 edit categories × 3 modes × 10 samples;
//! * `--suite scale` — cold vs persistent-incremental at three unit sizes;
//! * `--suite invariants` — negative/boundary checks (stale, cancel, corrupt,
//!   restart, budgets, JSON contract, user-file safety);
//! * `--suite validate` — applies every edit chain without compiling anything;
//! * `--suite grouping` — MVP-0c L2: N sequential analysis cycles vs one grouped
//!   snapshot (windows 0/25/50/100 ms) + incompatible-batch rejection/recovery.
//!
//! All modes use the same public `rustc` CLI arguments and the same fixture
//! content. Nothing here is a mock: every measurement spawns a real compiler.

mod campaign;
mod compaction;
mod edits;
mod gc;
mod grouping;
mod harness;
mod invariants;
mod multi_soak;
mod scale;
mod soak;

use std::path::{Path, PathBuf};

struct Args {
    suite: String,
    samples: usize,
    scale_reps: usize,
    work_dir: PathBuf,
    out_campaign: PathBuf,
    out_scale: PathBuf,
    out_invariants: PathBuf,
    out_gc: PathBuf,
    gc_edits: usize,
    out_grouping: PathBuf,
    grouping_reps: usize,
    out_soak: PathBuf,
    out_compaction: PathBuf,
    compaction_edits: usize,
    out_multi: PathBuf,
    multi_clients: usize,
    multi_seconds: u64,
    multi_pace_ms: u64,
    multi_control_seconds: u64,
    multi_control_pace_ms: u64,
    soak_requests: usize,
    soak_seconds: u64,
    soak_pace_ms: u64,
    soak_sample_every: usize,
    soak_control_requests: usize,
    soak_control_seconds: u64,
    soak_control_pace_ms: u64,
    soak_min_total_bytes: u64,
    soak_max_total_bytes: u64,
    clean: bool,
}

fn main() {
    let repo_root = repo_root();
    let args = match parse_args(&repo_root) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("ravum-daemon-bench: {message}");
            eprintln!(
                "usage: ravum-daemon-bench [--suite all|campaign|scale|invariants|gc|validate|grouping] \
                 [--samples N] [--scale-reps N] [--work-dir PATH] [--out PATH] \
                 [--out-scale PATH] [--out-invariants PATH] [--out-grouping PATH] [--grouping-reps N] [--no-clean]"
            );
            std::process::exit(2);
        }
    };
    if args.clean {
        if let Err(error) =
            ravum_daemon::fsutil::remove_dir_if_marked(&args.work_dir, "ravum-daemon-bench")
        {
            eprintln!("ravum-daemon-bench: cannot clean work dir: {error}");
            std::process::exit(2);
        }
    }
    if let Err(error) = std::fs::create_dir_all(&args.work_dir) {
        eprintln!("ravum-daemon-bench: cannot create work dir: {error}");
        std::process::exit(2);
    }

    let mut invalid = false;
    match args.suite.as_str() {
        "validate" => {
            if let Err(error) = validate_edits(&repo_root, &args) {
                eprintln!("validation failed: {error}");
                std::process::exit(1);
            }
            println!("edit chains validated");
            return;
        }
        "campaign" | "all" => match campaign_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("campaign failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "scale" | "all" => match scale_run(&repo_root, &args) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("scale suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "gc" | "all" => match gc_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("gc suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "multi-soak" | "all" => match multi_soak_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("multi-soak suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "compaction" | "all" => match compaction_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("compaction suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "soak" | "all" => match soak_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("soak suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "grouping" | "all" => match grouping_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("grouping suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }
    match args.suite.as_str() {
        "invariants" | "all" => match invariants_run(&repo_root, &args) {
            Ok(valid) => invalid |= !valid,
            Err(error) => {
                eprintln!("invariants suite failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {}
    }

    if invalid {
        eprintln!("MEASUREMENT INVALID: see the evidence files for the failing check");
        std::process::exit(1);
    }
}

fn campaign_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "campaign: {} categories x 3 modes x {} samples",
        7, args.samples
    );
    let value = campaign::run(&campaign::CampaignOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("campaign"),
        samples: args.samples,
    })?;
    campaign::write(&args.out_campaign, &value)?;
    print_campaign_table(&value);
    println!("written: {}", args.out_campaign.display());
    Ok(value["valid"].as_bool().unwrap_or(false))
}

fn scale_run(repo_root: &Path, args: &Args) -> Result<(), String> {
    println!(
        "scale: 3 generated unit sizes x 2 cases x {} reps",
        args.scale_reps
    );
    let value = scale::run(&scale::ScaleOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("scale-suite"),
        reps: args.scale_reps,
    })?;
    scale::write(&args.out_scale, &value)?;
    for entry in value["summary"].as_array().into_iter().flatten() {
        println!(
            "  {:<7} lines={:<5} identical cold={:>5}ms incr={:>5}ms | body cold={:>5}ms incr={:>5}ms",
            entry["size"].as_str().unwrap_or("?"),
            entry["line_count"].as_u64().unwrap_or(0),
            entry["identical"]["cold_p50_ms"].as_u64().unwrap_or(0),
            entry["identical"]["incremental_p50_ms"].as_u64().unwrap_or(0),
            entry["body_edit"]["cold_p50_ms"].as_u64().unwrap_or(0),
            entry["body_edit"]["incremental_p50_ms"].as_u64().unwrap_or(0),
        );
    }
    println!("written: {}", args.out_scale.display());
    Ok(())
}

fn gc_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "gc: {} real edits, then explicit retention GC (cache+snapshots, incremental, dry-run)",
        args.gc_edits
    );
    let value = gc::run(&gc::GcOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("gc-suite"),
        edits: args.gc_edits,
    })?;
    gc::write(&args.out_gc, &value)?;
    let audit = &value["audit_before"]["sizes"];
    let gc1 = &value["gc1_cache_snapshots"];
    let gc2 = &value["gc2_incremental"];
    let dry = &value["dry_run_audit"];
    println!(
        "  accumulated: edits={} rustc_runs={} work_dir={}KB cache={}B snapshots={}B incremental={}B contexts={}",
        value["accumulation"]["distinct_edits_analysed"],
        value["accumulation"]["rustc_runs"],
        value["audit_before"]["work_dir_bytes"].as_u64().unwrap_or(0) / 1024,
        audit["cache_bytes"].as_u64().unwrap_or(0),
        audit["snapshots_bytes"].as_u64().unwrap_or(0),
        audit["incremental_bytes"].as_u64().unwrap_or(0),
        audit["incremental_contexts"].as_u64().unwrap_or(0),
    );
    println!(
        "  gc1 cache+snapshots: duration={}ms freed={}B disk {}KB->{}KB cache_entries {}->{} snapshots {}->{}",
        gc1["duration_ms"].as_u64().unwrap_or(0),
        gc1["report"]["freed_bytes"].as_u64().unwrap_or(0),
        gc1["disk_bytes_before"].as_u64().unwrap_or(0) / 1024,
        gc1["disk_bytes_after"].as_u64().unwrap_or(0) / 1024,
        gc1["report"]["before"]["cache_entries"].as_u64().unwrap_or(0),
        gc1["report"]["after"]["cache_entries"].as_u64().unwrap_or(0),
        gc1["report"]["before"]["snapshots"].as_u64().unwrap_or(0),
        gc1["report"]["after"]["snapshots"].as_u64().unwrap_or(0),
    );
    println!(
        "  gc1 verification: recent_hit={} ({}ms, analysis={}ms) old_miss={} rebuild={}ms rustc_runs {}->{}",
        gc1["verification"]["recent_cache_hit"],
        gc1["verification"]["recent_wall_ms"],
        gc1["verification"]["recent_analysis_ms"],
        gc1["verification"]["old_cache_miss"],
        gc1["verification"]["old_wall_ms"],
        gc1["verification"]["rustc_runs_before"],
        gc1["verification"]["rustc_runs_after"],
    );
    println!(
        "  gc2 incremental: duration={}ms contexts deleted={:?} contexts kept={:?} disk {}KB->{}KB",
        gc2["duration_ms"].as_u64().unwrap_or(0),
        gc2["verification"]["collected_contexts"],
        gc2["verification"]["kept_contexts"],
        gc2["disk_bytes_before"].as_u64().unwrap_or(0) / 1024,
        gc2["disk_bytes_after"].as_u64().unwrap_or(0) / 1024,
    );
    println!(
        "  dry-run: plan_non_empty={} planned_freed={}B filesystem_unchanged={}",
        dry["plan_non_empty"],
        dry["planned_freed_bytes"].as_u64().unwrap_or(0),
        dry["filesystem_unchanged"],
    );
    println!("written: {}", args.out_gc.display());
    let ok = gc1["verification"]["recent_cache_hit"].as_bool() == Some(true)
        && gc1["verification"]["old_cache_miss"].as_bool()
            == gc1["verification"]["old_cache_miss_expected"].as_bool()
        && gc1["verification"]["no_compiler_for_recent"].as_bool() == Some(true)
        && gc2["verification"]["new_content_in_collected_context_compiled"].as_bool() == Some(true)
        && gc2["verification"]["incremental_context_recreated"].as_bool() == Some(true)
        && dry["plan_non_empty"].as_bool() == Some(true)
        && dry["filesystem_unchanged"].as_bool() == Some(true);
    Ok(ok)
}

fn multi_soak_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "multi-soak: {} clients x {} s over loopback TCP + mono-client control x {} s",
        args.multi_clients, args.multi_seconds, args.multi_control_seconds
    );
    let value = multi_soak::run(&multi_soak::MultiSoakOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("multi-soak-suite"),
        clients: args.multi_clients,
        seconds: args.multi_seconds,
        pace_ms: args.multi_pace_ms,
        control_seconds: args.multi_control_seconds,
        control_pace_ms: args.multi_control_pace_ms,
    })?;
    multi_soak::write(&args.out_multi, &value)?;
    let multi = &value["multi"];
    let control = &value["control"];
    println!(
        "  multi: clients={} requests={} errors={} timeouts={} latency p50={}ms p95={}ms p99={}ms rss drift={}B disk end={}B exit={}",
        multi["clients"], multi["requests_total"], multi["errors"], multi["timeouts"],
        multi["latency_ms"]["p50"], multi["latency_ms"]["p95"], multi["latency_ms"]["p99"],
        multi["server"]["rss_drift_bytes"], multi["server"]["disk_end_bytes"], multi["server"]["daemon_exit_code"]
    );
    println!(
        "  control: requests={} errors={} timeouts={} latency p50={}ms p95={}ms rss drift={}B disk end={}B exit={}",
        control["requests_total"], control["errors"], control["timeouts"],
        control["latency_ms"]["p50"], control["latency_ms"]["p95"],
        control["server"]["rss_drift_bytes"], control["server"]["disk_end_bytes"], control["server"]["daemon_exit_code"]
    );
    println!(
        "  verification (multi): recent_hit={} valid={}",
        multi["verification"]["recent_hit_without_compiler"], value["valid"]
    );
    println!("written: {}", args.out_multi.display());
    Ok(value["valid"].as_bool().unwrap_or(false))
}

fn compaction_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "compaction: {} real edits, then compact_incremental keep=1 and measure reuse",
        args.compaction_edits
    );
    let value = compaction::run(&compaction::CompactionOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("compaction-suite"),
        edits: args.compaction_edits,
    })?;
    compaction::write(&args.out_compaction, &value)?;
    let before = &value["before"];
    let after = &value["after"];
    let gc = &value["gc"];
    let verification = &value["verification"];
    println!(
        "  sessions {}->{}, disk {}->{} B (freed {} B), gc={} ms, compaction deleted={} kept={} freed={} B",
        before["sessions"], after["sessions"],
        before["disk_bytes"], after["disk_bytes"], after["disk_freed_bytes"],
        gc["duration_ms"], gc["compaction"]["sessions_deleted"],
        gc["compaction"]["sessions_kept"], gc["compaction"]["bytes_freed"]
    );
    println!(
        "  analysis before={}ms after-compaction={}ms cold={}ms reuse_retained={} cache_intact={} valid={}",
        verification["baseline_before_compaction"]["analysis_ms"],
        verification["after_compaction"]["analysis_ms"],
        verification["cold_reference_no_state"]["analysis_ms"],
        verification["reuse_retained"],
        verification["cache_intact_after_compaction"],
        value["valid"]
    );
    println!("written: {}", args.out_compaction.display());
    Ok(value["valid"].as_bool().unwrap_or(false))
}

fn soak_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "soak: auto campaign {} requests / {} s (pace {} ms) + control campaign {} requests / {} s (pace {} ms)",
        args.soak_requests,
        args.soak_seconds,
        args.soak_pace_ms,
        args.soak_control_requests,
        args.soak_control_seconds,
        args.soak_control_pace_ms
    );
    let value = soak::run(&soak::SoakOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("soak-suite"),
        requests: args.soak_requests,
        seconds: args.soak_seconds,
        pace_ms: args.soak_pace_ms,
        sample_every: args.soak_sample_every,
        control_requests: args.soak_control_requests,
        control_seconds: args.soak_control_seconds,
        control_pace_ms: args.soak_control_pace_ms,
        min_total_bytes: args.soak_min_total_bytes,
        max_total_bytes: args.soak_max_total_bytes,
    })?;
    soak::write(&args.out_soak, &value)?;
    let auto = &value["auto"];
    let control = &value["control"];
    println!(
        "  auto: requests={} duration={}s failures={} latency p50={}ms p95={}ms auto_runs={} auto_failures={} disk {}->{}B (max {}) rss drift={}B journal={}",
        auto["requests"], auto["duration_s"], auto["failures"],
        auto["latency_ms"]["p50"], auto["latency_ms"]["p95"],
        auto["auto_gc"]["runs"], auto["auto_gc"]["failures"],
        auto["disk_bytes"]["start"], auto["disk_bytes"]["end"], auto["disk_bytes"]["max"],
        auto["rss_bytes"]["drift"], auto["journal_lines"]
    );
    println!(
        "  control: requests={} duration={}s failures={} latency p50={}ms p95={}ms disk {}->{}B (max {}) rss drift={}B",
        control["requests"], control["duration_s"], control["failures"],
        control["latency_ms"]["p50"], control["latency_ms"]["p95"],
        control["disk_bytes"]["start"], control["disk_bytes"]["end"], control["disk_bytes"]["max"],
        control["rss_bytes"]["drift"]
    );
    println!(
        "  verification: recent_hit={} old_success={} settled={} valid={}",
        auto["verification"]["recent"]["hit_without_compiler"],
        auto["verification"]["old"]["success"],
        auto["settled"],
        value["valid"]
    );
    println!("written: {}", args.out_soak.display());
    Ok(value["valid"].as_bool().unwrap_or(false))
}

fn grouping_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!(
        "grouping: 6 modes (sequential, grouped windows 0/25/50/100 ms, incompatible) x {} recorded reps",
        args.grouping_reps
    );
    let value = grouping::run(&grouping::GroupingOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("grouping-suite"),
        reps: args.grouping_reps,
    })?;
    harness::write_evidence(&args.out_grouping, &value)?;
    let modes = &value["summary"]["modes"];
    for (spec, mode) in modes.as_object().into_iter().flatten() {
        let total = &mode["total_ms"];
        let rustc = &mode["rustc_runs"];
        println!(
            "  {:<14} total p50={:>6}ms (min {:>6} / max {:>6})  analyses={}  rustc={}  valid={}",
            spec,
            total["p50"].as_u64().unwrap_or(0),
            total["min"].as_u64().unwrap_or(0),
            total["max"].as_u64().unwrap_or(0),
            mode["analyses"]["p50"].as_u64().unwrap_or(0),
            rustc["p50"].as_u64().unwrap_or(0),
            mode["valid"].as_bool().unwrap_or(false),
        );
    }
    for comparison in value["summary"]["comparison"]
        .as_array()
        .into_iter()
        .flatten()
    {
        println!(
            "  gain grouped-{:>3}ms: séquentiel {}ms -> groupé {}ms, gain apparié p50={}ms ({}%), analyses économisées={}",
            comparison["window_ms"].as_u64().unwrap_or(0),
            comparison["sequential_total_ms"]["p50"].as_u64().unwrap_or(0),
            comparison["grouped_total_ms"]["p50"].as_u64().unwrap_or(0),
            comparison["gain_ms_paired"]["p50"].as_u64().unwrap_or(0),
            comparison["gain_percent_paired"]["p50"].as_u64().unwrap_or(0),
            comparison["analyses_saved_per_cycle"].as_u64().unwrap_or(0),
        );
    }
    let incompatible = &value["summary"]["incompatible"];
    println!(
        "  incompatible: rejets={}/{} identifiés={}/{} reprise p50={}ms (lot refusé p50={}ms) rustc/rep={}",
        incompatible["batch_rejected_count"].as_u64().unwrap_or(0),
        incompatible["observations"].as_u64().unwrap_or(0),
        incompatible["incompatible_identified_count"].as_u64().unwrap_or(0),
        incompatible["observations"].as_u64().unwrap_or(0),
        incompatible["recovery_total_ms"]["p50"].as_u64().unwrap_or(0),
        incompatible["batch_latency_ms"]["p50"].as_u64().unwrap_or(0),
        incompatible["rustc_runs_per_rep"]["p50"].as_u64().unwrap_or(0),
    );
    let break_even = &value["summary"]["break_even"];
    println!(
        "  break-even: fenêtre max mesurée à gain positif={}ms, estimation dérivée={}ms ({}), durée campagne={}s",
        break_even["max_measured_window_with_positive_median_gain_ms"]
            .as_u64()
            .unwrap_or(0),
        break_even["estimated_break_even_window_ms"]
            .as_u64()
            .unwrap_or(0),
        break_even["status"].as_str().unwrap_or("?"),
        value["campaign"]["duration_ms"].as_u64().unwrap_or(0) / 1000,
    );
    println!("  valid={}", value["valid"].as_bool().unwrap_or(false));
    println!("written: {}", args.out_grouping.display());
    Ok(value["valid"].as_bool().unwrap_or(false))
}

fn invariants_run(repo_root: &Path, args: &Args) -> Result<bool, String> {
    println!("invariants: negative/boundary checks");
    let value = invariants::run(&invariants::InvariantOptions {
        repo_root: repo_root.to_path_buf(),
        work_root: args.work_dir.join("invariants-suite"),
    })?;
    invariants::write(&args.out_invariants, &value)?;
    for check in value["checks"].as_array().into_iter().flatten() {
        println!(
            "  {:<42} {}",
            check["name"].as_str().unwrap_or("?"),
            if check["passed"].as_bool() == Some(true) {
                "PASS"
            } else {
                "FAIL"
            }
        );
    }
    let summary = &value["summary"];
    println!(
        "  checks: {}/{} passed, failed={}",
        summary["passed"].as_u64().unwrap_or(0),
        summary["total"].as_u64().unwrap_or(0),
        summary["failed"].as_u64().unwrap_or(0)
    );
    println!("written: {}", args.out_invariants.display());
    Ok(value["summary"]["valid"].as_bool().unwrap_or(false))
}

fn validate_edits(repo_root: &Path, args: &Args) -> Result<(), String> {
    let fixture = harness::load_fixture(repo_root)?;
    let base = harness::base_files(&fixture);
    for category in edits::categories() {
        let states = edits::build_states(&base, 10, &category)?;
        let valid_states = match category.kind {
            edits::CategoryKind::InvalidRepair => states.len() / 2,
            _ => states.len(),
        };
        println!(
            "  {:<16} kind={:<13} states={} (recorded samples>=10: {})",
            category.name,
            format!("{:?}", category.kind),
            states.len(),
            valid_states >= 10
        );
    }
    let _ = args;
    Ok(())
}

fn print_campaign_table(value: &serde_json::Value) {
    println!(
        "{:<16} {:>18} {:>22} {:>18} {:>12}",
        "category", "cold p50/p95", "incremental p50/p95", "daemon p50/p95", "consistency"
    );
    for category in value["categories"].as_array().into_iter().flatten() {
        let stats = &category["stats_ms"];
        println!(
            "{:<16} {:>8}/{:<8} {:>10}/{:<10} {:>8}/{:<8} {:>12}",
            category["name"].as_str().unwrap_or("?"),
            stats["cold"]["p50"].as_u64().unwrap_or(0),
            stats["cold"]["p95"].as_u64().unwrap_or(0),
            stats["incremental"]["p50"].as_u64().unwrap_or(0),
            stats["incremental"]["p95"].as_u64().unwrap_or(0),
            stats["daemon"]["p50"].as_u64().unwrap_or(0),
            stats["daemon"]["p95"].as_u64().unwrap_or(0),
            if category["consistency"]["failures"].as_u64().unwrap_or(1) == 0 {
                "ok".to_owned()
            } else {
                "MISMATCH".to_owned()
            }
        );
    }
    println!(
        "valid={} consistency_failures={} unexpected_failures={}",
        value["valid"], value["consistency_failures"], value["unexpected_failures"]
    );
}

fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_args(repo_root: &Path) -> Result<Args, String> {
    let mut args = Args {
        suite: "all".to_owned(),
        samples: 10,
        scale_reps: 5,
        work_dir: repo_root.join("target").join("ravum-daemon-bench"),
        out_campaign: repo_root
            .join("evidence")
            .join("mvp0b-incremental-bench.json"),
        out_scale: repo_root
            .join("evidence")
            .join("mvp0b-incremental-scale.json"),
        out_invariants: repo_root
            .join("evidence")
            .join("mvp0b-incremental-invariants.json"),
        out_gc: repo_root
            .join("evidence")
            .join("mvp0b-daemon-gc-2026-11-08.json"),
        gc_edits: 40,
        out_grouping: repo_root
            .join("evidence")
            .join("mvp0c-grouping-2026-11-09.json"),
        grouping_reps: 5,
        out_soak: repo_root
            .join("evidence")
            .join("mvp0b-daemon-gc-auto-2026-11-08.json"),
        out_compaction: repo_root
            .join("evidence")
            .join("mvp0b-daemon-compaction-2026-11-08.json"),
        compaction_edits: 20,
        out_multi: repo_root
            .join("evidence")
            .join("mvp0b-daemon-soak-multi-2026-11-08.json"),
        multi_clients: 3,
        multi_seconds: 120,
        multi_pace_ms: 300,
        multi_control_seconds: 120,
        multi_control_pace_ms: 300,
        soak_requests: 150,
        soak_seconds: 480,
        soak_pace_ms: 1900,
        soak_sample_every: 5,
        soak_control_requests: 40,
        soak_control_seconds: 120,
        soak_control_pace_ms: 800,
        soak_min_total_bytes: 180_000,
        soak_max_total_bytes: 120_000,
        clean: true,
    };
    let mut index = 1;
    let argv: Vec<String> = std::env::args().collect();
    while index < argv.len() {
        let flag = argv[index].clone();
        let mut take = |name: &str| -> Result<String, String> {
            index += 1;
            argv.get(index)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--suite" => args.suite = take("--suite")?,
            "--samples" => {
                args.samples = take("--samples")?
                    .parse()
                    .map_err(|_| "--samples must be an integer".to_owned())?
            }
            "--scale-reps" => {
                args.scale_reps = take("--scale-reps")?
                    .parse()
                    .map_err(|_| "--scale-reps must be an integer".to_owned())?
            }
            "--work-dir" => args.work_dir = PathBuf::from(take("--work-dir")?),
            "--out" => args.out_campaign = PathBuf::from(take("--out")?),
            "--out-scale" => args.out_scale = PathBuf::from(take("--out-scale")?),
            "--out-invariants" => args.out_invariants = PathBuf::from(take("--out-invariants")?),
            "--out-gc" => args.out_gc = PathBuf::from(take("--out-gc")?),
            "--gc-edits" => {
                args.gc_edits = take("--gc-edits")?
                    .parse()
                    .map_err(|_| "--gc-edits must be an integer".to_owned())?
            }
            "--out-grouping" => args.out_grouping = PathBuf::from(take("--out-grouping")?),
            "--grouping-reps" => {
                args.grouping_reps = take("--grouping-reps")?
                    .parse()
                    .map_err(|_| "--grouping-reps must be an integer".to_owned())?
            }
            "--out-soak" => args.out_soak = PathBuf::from(take("--out-soak")?),
            "--out-compaction" => args.out_compaction = PathBuf::from(take("--out-compaction")?),
            "--clients" => {
                args.multi_clients = take("--clients")?
                    .parse()
                    .map_err(|_| "--clients must be an integer".to_owned())?
            }
            "--multi-seconds" => {
                args.multi_seconds = take("--multi-seconds")?
                    .parse()
                    .map_err(|_| "--multi-seconds must be an integer".to_owned())?
            }
            "--multi-pace-ms" => {
                args.multi_pace_ms = take("--multi-pace-ms")?
                    .parse()
                    .map_err(|_| "--multi-pace-ms must be an integer".to_owned())?
            }
            "--multi-control-seconds" => {
                args.multi_control_seconds = take("--multi-control-seconds")?
                    .parse()
                    .map_err(|_| "--multi-control-seconds must be an integer".to_owned())?
            }
            "--multi-control-pace-ms" => {
                args.multi_control_pace_ms = take("--multi-control-pace-ms")?
                    .parse()
                    .map_err(|_| "--multi-control-pace-ms must be an integer".to_owned())?
            }
            "--out-multi" => args.out_multi = PathBuf::from(take("--out-multi")?),
            "--compaction-edits" => {
                args.compaction_edits = take("--compaction-edits")?
                    .parse()
                    .map_err(|_| "--compaction-edits must be an integer".to_owned())?
            }
            "--soak-requests" => {
                args.soak_requests = take("--soak-requests")?
                    .parse()
                    .map_err(|_| "--soak-requests must be an integer".to_owned())?
            }
            "--soak-seconds" => {
                args.soak_seconds = take("--soak-seconds")?
                    .parse()
                    .map_err(|_| "--soak-seconds must be an integer".to_owned())?
            }
            "--soak-pace-ms" => {
                args.soak_pace_ms = take("--soak-pace-ms")?
                    .parse()
                    .map_err(|_| "--soak-pace-ms must be an integer".to_owned())?
            }
            "--soak-sample-every" => {
                args.soak_sample_every = take("--soak-sample-every")?
                    .parse()
                    .map_err(|_| "--soak-sample-every must be an integer".to_owned())?
            }
            "--soak-control-requests" => {
                args.soak_control_requests = take("--soak-control-requests")?
                    .parse()
                    .map_err(|_| "--soak-control-requests must be an integer".to_owned())?
            }
            "--soak-control-seconds" => {
                args.soak_control_seconds = take("--soak-control-seconds")?
                    .parse()
                    .map_err(|_| "--soak-control-seconds must be an integer".to_owned())?
            }
            "--soak-control-pace-ms" => {
                args.soak_control_pace_ms = take("--soak-control-pace-ms")?
                    .parse()
                    .map_err(|_| "--soak-control-pace-ms must be an integer".to_owned())?
            }
            "--soak-min-total-bytes" => {
                args.soak_min_total_bytes = take("--soak-min-total-bytes")?
                    .parse()
                    .map_err(|_| "--soak-min-total-bytes must be an integer".to_owned())?
            }
            "--soak-max-total-bytes" => {
                args.soak_max_total_bytes = take("--soak-max-total-bytes")?
                    .parse()
                    .map_err(|_| "--soak-max-total-bytes must be an integer".to_owned())?
            }
            "--no-clean" => args.clean = false,
            "--help" | "-h" => {
                return Err("help requested".to_owned());
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
        index += 1;
    }
    if !matches!(
        args.suite.as_str(),
        "all"
            | "campaign"
            | "scale"
            | "invariants"
            | "gc"
            | "soak"
            | "compaction"
            | "multi-soak"
            | "grouping"
            | "validate"
    ) {
        return Err(format!("unknown suite {:?}", args.suite));
    }
    Ok(args)
}
