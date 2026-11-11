//! Ravum CLI — continuous-development loop for the M1 MVP.
//!
//! Only the commands implemented here are advertised in the README:
//! `check`, `build`, `patch`, `run`, `watch`, `status`, `bench`.

mod batch_check;
mod bench;
mod collab_cycle;
mod commands;
mod cycle_bench;
mod dsp;
mod hostproc;
mod metrics;
mod unit_check;
mod watch;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        usage();
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let code = match command {
        "check" => commands::check(rest),
        "build" => commands::build(rest),
        "patch" => commands::patch(rest),
        "run" => commands::run(rest),
        "status" => commands::status(rest),
        "watch" => watch::watch(rest),
        "bench" => bench::bench(rest),
        "cycle-bench" => cycle_bench::cycle_bench(rest),
        "prove" => commands::prove(rest),
        "artifacts" => commands::artifacts(rest),
        "dsp" => dsp::dsp(rest),
        "collab-cycle" => collab_cycle::collab_cycle(rest),
        "unit-check" => unit_check::unit_check(rest),
        "batch-check" => batch_check::batch_check(rest),
        "--help" | "-h" | "help" => {
            usage();
            0
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            2
        }
    };
    ExitCode::from(code)
}

fn usage() {
    eprintln!(
        "ravum {version} — MVP-0a continuous development loop\n\
         \n\
         USAGE:\n\
         \x20 ravum check  <file.rs> [--json]              analysis only (rustc --emit=metadata)\n\
         \x20 ravum build  <file.rs> [--json]              build a patchable cdylib after a clean check\n\
         \x20 ravum patch  <artifact> [--json]             validate an existing artifact (ABI, manifest, symbols)\n\
         \x20 ravum run    <file.rs> [--tick-ms N]         build then run the demo host (info only; --concurrent overlaps analysis and build)\n\
         \x20 ravum watch  <file.rs> [options]              full loop: edit -> check -> build -> verify -> hot activate (--concurrent overlaps check and build)\n\
         \x20 ravum status [--json]                        last recorded state (target/ravum/state.json)\n\
         \x20 ravum bench  <file-a.rs> <file-b.rs> [options] measure every stage, p50/p95/p99\n\
         \x20 ravum cycle-bench <file.rs> [options]     edit -> diagnostic -> build -> verify -> load -> activate, MVP-0a vs MVP-0b\n\
         \x20 ravum prove  <file.rs> [options]          check one obligation, record it in the evidence ledger\n\
         \x20 ravum artifacts [--prune --keep N]          list or prune built artifacts (never the active one when known)\n\
         \x20 ravum dsp   <file.rs> [--next <file.rs>]     measured DSP session, optional hot swap of the processing\n\
         \x20 ravum collab-cycle <workspace> [options]   targeted daemon analysis of accepted collaborative changes\n\
         \x20   options: [--protocol v1|v2] [--oracle unit-graph --oracle-state <dir>] [--proposals <dir>]\n\
         \x20 ravum unit-check <workspace> [options]      real unit graph + chained rustc campaign (no file writes)\n\
         \x20 ravum batch-check <workspace> --proposals <file>  group proposals, real cargo test per batch\n\
         \x20   options: [--changed <path>]... [--state <dir>] [--json] [--out <file>]\n\
         \n\
         ENV: RAVUM_HOST_BIN overrides the demo-host binary path.\n\
         NOTE: no command here is a proof, a release or a real-time qualification.",
        version = env!("CARGO_PKG_VERSION")
    );
}
