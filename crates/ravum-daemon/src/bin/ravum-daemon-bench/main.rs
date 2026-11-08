//! MVP-0b incremental-analysis measurement harness.
//!
//! Produces the raw evidence files quoted by `docs/mvp/MVP-0b-incremental.md`:
//!
//! * `--suite campaign` — 7 edit categories × 3 modes × 10 samples;
//! * `--suite scale` — cold vs persistent-incremental at three unit sizes;
//! * `--suite invariants` — negative/boundary checks (stale, cancel, corrupt,
//!   restart, budgets, JSON contract, user-file safety);
//! * `--suite validate` — applies every edit chain without compiling anything.
//!
//! All modes use the same public `rustc` CLI arguments and the same fixture
//! content. Nothing here is a mock: every measurement spawns a real compiler.

mod campaign;
mod edits;
mod gc;
mod harness;
mod invariants;
mod scale;

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
    clean: bool,
}

fn main() {
    let repo_root = repo_root();
    let args = match parse_args(&repo_root) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("ravum-daemon-bench: {message}");
            eprintln!(
                "usage: ravum-daemon-bench [--suite all|campaign|scale|invariants|gc|validate] \
                 [--samples N] [--scale-reps N] [--work-dir PATH] [--out PATH] \
                 [--out-scale PATH] [--out-invariants PATH] [--no-clean]"
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
    println!("scale: 3 generated unit sizes x 2 cases x {} reps", args.scale_reps);
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
        value["valid"],
        value["consistency_failures"],
        value["unexpected_failures"]
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
            "--out-invariants" => {
                args.out_invariants = PathBuf::from(take("--out-invariants")?)
            }
            "--out-gc" => args.out_gc = PathBuf::from(take("--out-gc")?),
            "--gc-edits" => {
                args.gc_edits = take("--gc-edits")?
                    .parse()
                    .map_err(|_| "--gc-edits must be an integer".to_owned())?
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
        "all" | "campaign" | "scale" | "invariants" | "gc" | "validate"
    ) {
        return Err(format!("unknown suite {:?}", args.suite));
    }
    Ok(args)
}
