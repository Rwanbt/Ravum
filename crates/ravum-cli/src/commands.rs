//! One-shot commands: check, build, patch, run, status.

use crate::hostproc::HostProc;
use ravum_coordinator::Pipeline;
use ravum_patch_host::{inspect_artifact, ExpectedAbi};
use ravum_proof::{Claim, EvidenceLedger, ExhaustiveVerifier, Obligation, ProofBroker};
use ravum_protocol::DiagnosticSet;
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn work_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("ravum")
}

pub fn state_path() -> PathBuf {
    work_dir().join("state.json")
}

pub fn pipeline() -> Pipeline {
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    Pipeline::new(rustc, work_dir())
}

/// First positional (non-flag, non-flag-value) argument.
pub fn positional(args: &[String], index: usize) -> Option<String> {
    let mut seen = 0usize;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg.starts_with("--") {
            if takes_value(arg) {
                iter.next();
            }
            continue;
        }
        if seen == index {
            return Some(arg.clone());
        }
        seen += 1;
    }
    None
}

fn takes_value(flag: &str) -> bool {
    !matches!(flag, "--json" | "--once" | "--help")
}

pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

pub fn flag_u64(args: &[String], name: &str, default: u64) -> u64 {
    flag(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

pub fn load_snapshot(file: &str) -> Result<SourceSnapshot, String> {
    SourceSnapshot::from_file(Path::new(file)).map_err(|e| format!("cannot read {file}: {e}"))
}

pub fn render_diagnostics(set: &DiagnosticSet) -> String {
    if set.diagnostics.is_empty() {
        return format!("no diagnostic ({} {})", set.tool, set.tool_version);
    }
    set.diagnostics
        .iter()
        .map(|d| {
            let code = d.code.clone().unwrap_or_default();
            let place = d
                .span
                .as_ref()
                .map(|s| format!("{}:{}:{}", s.file, s.line_start, s.column_start))
                .unwrap_or_else(|| "<no span>".to_owned());
            format!("{}[{}] {}: {}", d.level.as_str(), code, place, d.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn check(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!("usage: ravum check <file.rs> [--json]");
        return 2;
    };
    let snapshot = match load_snapshot(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let pipeline = pipeline();
    let outcome = match pipeline.analyze(&snapshot) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("adapter error: {e}");
            return 2;
        }
    };
    if has(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&outcome.diagnostics).unwrap());
    } else {
        println!(
            "snapshot {}  check {} ms\n{}",
            snapshot.id().short(),
            outcome.check_ms,
            render_diagnostics(&outcome.diagnostics)
        );
    }
    if outcome.diagnostics.has_errors() {
        1
    } else {
        0
    }
}

pub fn build(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!("usage: ravum build <file.rs> [--json]");
        return 2;
    };
    let snapshot = match load_snapshot(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let pipeline = pipeline();
    match pipeline.analyze(&snapshot) {
        Ok(check) if check.diagnostics.has_errors() => {
            eprintln!("{}", render_diagnostics(&check.diagnostics));
            eprintln!("refused: analysis reported errors, nothing was built");
            return 1;
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("adapter error: {e}");
            return 2;
        }
    }
    let outcome = match pipeline.build(&snapshot) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("adapter error: {e}");
            return 2;
        }
    };
    if has(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&outcome.build).unwrap());
    } else {
        println!("snapshot      {}", snapshot.id());
        println!("artifact      {}", outcome.build.artifact_path);
        println!("artifact_id   {}", outcome.build.artifact_id);
        println!("build_context {}", outcome.build.build_context_id);
        println!("build_ms      {}", outcome.build.build_ms);
        if !outcome.build.diagnostics.diagnostics.is_empty() {
            println!("{}", render_diagnostics(&outcome.build.diagnostics));
        }
    }
    if outcome.success {
        0
    } else {
        1
    }
}

pub fn patch(args: &[String]) -> u8 {
    let Some(artifact) = positional(args, 0) else {
        eprintln!("usage: ravum patch <artifact> [--json]");
        return 2;
    };
    let abi = ExpectedAbi {
        abi_version: flag_u64(args, "--abi", ravum_protocol::PATCH_ABI_VERSION as u64) as u32,
        ..ExpectedAbi::default()
    };
    match inspect_artifact(Path::new(&artifact), &abi) {
        Ok(facts) => {
            if has(args, "--json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "accepted",
                        "artifact_id": facts.artifact_id,
                        "abi_version": facts.abi_version,
                        "manifest": facts.manifest,
                    }))
                    .unwrap()
                );
            } else {
                println!("status        accepted");
                println!("artifact_id   {}", facts.artifact_id);
                println!("abi_version   {}", facts.abi_version);
                println!("patch_id      {}", facts.manifest.patch_id);
                println!("export        {}", facts.manifest.export);
                println!("signature     {}", facts.manifest.signature);
            }
            0
        }
        Err(reason) => {
            if has(args, "--json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "refused",
                        "reason": reason,
                    }))
                    .unwrap()
                );
            } else {
                println!("status        refused");
                println!("reason        {reason:?}");
            }
            1
        }
    }
}

pub fn run(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!("usage: ravum run <file.rs> [--tick-ms N] [--max-ticks N]");
        return 2;
    };
    let tick_ms = flag_u64(args, "--tick-ms", 100);
    let max_ticks = flag_u64(args, "--max-ticks", u64::MAX);
    let snapshot = match load_snapshot(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let pipeline = pipeline();
    let report = pipeline.prepare_candidate(&snapshot, &|| snapshot.id().clone());
    if !matches!(report.outcome, ravum_coordinator::LoopOutcome::CandidateReady) {
        eprintln!("cannot start the demo host: {:?}", report.outcome);
        return 1;
    }
    let Some(artifact) = report.artifact_path() else {
        eprintln!("internal error: no artifact path");
        return 2;
    };
    let mut host = match HostProc::spawn(Path::new(artifact), tick_ms) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("cannot spawn demo host: {e}");
            return 2;
        }
    };
    eprintln!(
        "demo host pid={} artifact={} (Ctrl+C to stop)",
        host.pid(),
        report.artifact_id().map(|a| a.short().to_owned()).unwrap_or_default()
    );
    let mut ticks = 0u64;
    loop {
        let Some(msg) = host.recv_timeout(Duration::from_secs(1)) else {
            if !host.is_running() {
                break;
            }
            continue;
        };
        if msg.get("event").and_then(|v| v.as_str()) == Some("tick") {
            ticks += 1;
            println!(
                "{} {} -> {}",
                msg["version"].as_str().unwrap_or("?"),
                msg["tick"],
                msg["value"]
            );
            if ticks >= max_ticks {
                break;
            }
        }
    }
    host.shutdown();
    0
}

/// `ravum prove` — check one obligation on a freshly built artifact and record
/// the verdict in the append-only ledger.
pub fn prove(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!(
            "usage: ravum prove <file.rs> [--claim finite|deterministic|non-negative|monotonic|equals] [--equals V] [--inputs a,b,c] [--timeout-ms N] [--ledger PATH] [--json]"
        );
        return 2;
    };
    let Some(claim) = parse_claim(args) else {
        eprintln!("invalid --claim or missing --equals value");
        return 2;
    };
    let inputs = match parse_inputs(args) {
        Some(v) if !v.is_empty() => v,
        _ => {
            eprintln!("invalid --inputs: expected a comma-separated list of finite numbers");
            return 2;
        }
    };
    let timeout = Duration::from_millis(flag_u64(args, "--timeout-ms", 30_000));
    let ledger_path = flag(args, "--ledger")
        .map(PathBuf::from)
        .unwrap_or_else(|| work_dir().join("evidence-ledger.jsonl"));

    let snapshot = match load_snapshot(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let pipeline = pipeline();
    match pipeline.analyze(&snapshot) {
        Ok(check) if check.diagnostics.has_errors() => {
            eprintln!("{}", render_diagnostics(&check.diagnostics));
            eprintln!("refused: analysis reported errors, nothing was built");
            return 1;
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("adapter error: {e}");
            return 2;
        }
    }
    let built = match pipeline.build(&snapshot) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("adapter error: {e}");
            return 2;
        }
    };
    if !built.success {
        eprintln!("refused: build failed");
        return 1;
    }

    let ledger = match EvidenceLedger::open(&ledger_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            return 2;
        }
    };
    let mut broker = ProofBroker::new(Box::new(ExhaustiveVerifier::with_timeout(timeout)), ledger);
    let ctx = broker.context_for(&built.build);
    let obligation = Obligation::new("cli.obligation", claim, inputs);
    let result = match broker.prove(Path::new(&built.build.artifact_path), &obligation, &ctx) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", e);
            return 2;
        }
    };
    let record = &result.record;

    if has(args, "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": record.status.as_str(),
                "passed": record.passed,
                "authorizes_activation": record.authorizes_activation(),
                "from_cache": result.from_cache,
                "verification_key": result.key,
                "property": record.property,
                "counterexample": record.counterexample,
                "assumptions": record.assumptions,
                "artifact_id": record.artifact_id,
                "ledger": ledger_path,
            }))
            .unwrap()
        );
    } else {
        println!("property      {}", record.property);
        println!("status        {}", record.status.as_str());
        println!("passed        {}", record.passed);
        println!("from_cache    {}", result.from_cache);
        println!("artifact_id   {}", record.artifact_id.short());
        println!("ledger        {}", ledger_path.display());
        if let Some(ce) = &record.counterexample {
            println!("counterexample {ce}");
        }
        println!(
            "note          a bounded domain proves only that domain; this is not a general proof"
        );
    }
    if record.authorizes_activation() {
        0
    } else {
        1
    }
}

fn parse_claim(args: &[String]) -> Option<Claim> {
    match flag(args, "--claim").as_deref().unwrap_or("finite") {
        "finite" => Some(Claim::Finite),
        "deterministic" => Some(Claim::Deterministic),
        "non-negative" | "non_negative" => Some(Claim::NonNegative),
        "monotonic" | "monotonic-non-decreasing" => Some(Claim::MonotonicNonDecreasing),
        "equals" | "equal" => flag(args, "--equals")
            .and_then(|v| v.parse::<f64>().ok())
            .map(|expected| Claim::Equals { expected }),
        _ => None,
    }
}

fn parse_inputs(args: &[String]) -> Option<Vec<f64>> {
    match flag(args, "--inputs") {
        None => Some(vec![0.0, 1.0, -1.0, 2.5]),
        Some(raw) => raw
            .split(',')
            .map(|part| part.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
            .collect(),
    }
}

/// `ravum artifacts` — list or prune built artifacts. Legacy files are removed,
/// the newest survive; the caller can name the active artifact to protect it.
pub fn artifacts(args: &[String]) -> u8 {
    let dir = work_dir().join("artifacts");
    if !dir.is_dir() {
        eprintln!("no artifacts directory at {}", dir.display());
        return 1;
    }
    let keep = flag_u64(args, "--keep", 5) as usize;
    let active = flag(args, "--active").map(PathBuf::from);

    if has(args, "--prune") {
        match ravum_coordinator::prune_artifacts(&dir, keep, active.as_deref()) {
            Ok(report) => {
                if has(args, "--json") {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "kept": report.kept,
                            "removed": report.removed,
                            "cache_entries_removed": report.cache_entries_removed,
                        }))
                        .unwrap()
                    );
                } else {
                    println!("kept    {}", report.kept.len());
                    println!("removed {}", report.removed.len());
                    println!("cache entries removed {}", report.cache_entries_removed);
                }
                0
            }
            Err(e) => {
                eprintln!("prune failed: {e}");
                2
            }
        }
    } else {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        for path in entries {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("{:>10}  {}", size, path.display());
        }
        0
    }
}

pub fn status(args: &[String]) -> u8 {
    let path = state_path();    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("no recorded state at {}", path.display());
        return 1;
    };
    if has(args, "--json") {
        println!("{text}");
        return 0;
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or(text));
            0
        }
        Err(e) => {
            eprintln!("corrupt state file: {e}");
            1
        }
    }
}
