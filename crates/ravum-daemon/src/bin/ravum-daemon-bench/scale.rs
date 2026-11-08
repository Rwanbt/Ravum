//! Scale experiment: at which unit size does persistent `-C incremental` start
//! to pay for itself, if ever, on this machine?
//!
//! The campaign fixture is small on purpose (a realistic single library unit),
//! so a single size cannot answer the "no gain" question. This suite generates
//! deterministic synthetic units at three sizes and compares, for each size:
//!
//! * `identical` — the same content submitted again (no source change);
//! * `body_edit` — one literal changed inside one function body.
//!
//! Both are measured cold vs warm persistent incremental, interleaved, using the
//! same public `rustc` CLI the daemon drives.

use crate::harness::{
    base_files, load_json, machine_json, run_cold, run_incremental, stats, toolchain_json,
    write_evidence, Fixture, ModeRun,
};
use ravum_daemon::{BuildContext, SourceFile};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct SizeSpec {
    name: &'static str,
    functions: usize,
    structs: usize,
}

const SIZES: &[SizeSpec] = &[
    SizeSpec {
        name: "small",
        functions: 60,
        structs: 40,
    },
    SizeSpec {
        name: "medium",
        functions: 180,
        structs: 120,
    },
    SizeSpec {
        name: "large",
        functions: 400,
        structs: 300,
    },
];

pub struct ScaleOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub reps: usize,
}

pub fn run(options: &ScaleOptions) -> Result<Value, String> {
    if options.reps == 0 {
        return Err("--scale-reps must be >= 1".to_owned());
    }
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = crate::harness::rustc_path();
    let start_load = load_json(&[], &rustc);
    let mut sizes = Vec::new();
    let mut summary = Vec::new();
    for spec in SIZES {
        let fixture = generate(spec, &options.work_root)?;
        let base = base_files(&fixture);
        let line_count: usize = base.iter().map(|file| file.content.lines().count()).sum();
        let cold_dir = options.work_root.join("scale").join(spec.name).join("cold");
        let _ = std::fs::remove_dir_all(&cold_dir);

        let identical = run_case(
            &options.work_root,
            &rustc,
            &fixture,
            &base,
            spec.name,
            "identical",
            options.reps,
            false,
        )?;
        let body_edit = run_case(
            &options.work_root,
            &rustc,
            &fixture,
            &base,
            spec.name,
            "body_edit",
            options.reps,
            true,
        )?;
        let incremental_dir_bytes = dir_size(
            &options
                .work_root
                .join("incremental-mode")
                .join(format!("{}-body_edit", spec.name))
                .join("incremental"),
        );
        summary.push(json!({
            "size": spec.name,
            "line_count": line_count,
            "files": base.len(),
            "identical": {
                "cold_p50_ms": identical["cold"]["p50"],
                "incremental_p50_ms": identical["incremental"]["p50"],
            },
            "body_edit": {
                "cold_p50_ms": body_edit["cold"]["p50"],
                "incremental_p50_ms": body_edit["incremental"]["p50"],
            },
            "incremental_dir_bytes": incremental_dir_bytes,
        }));
        sizes.push(json!({
            "size": spec.name,
            "generator": {
                "functions": spec.functions,
                "structs": spec.structs,
                "note": "deterministic synthetic unit; generation cost is not measured",
            },
            "line_count": line_count,
            "bytes": base.iter().map(|file| file.content.len()).sum::<usize>(),
            "identical": identical,
            "body_edit": body_edit,
            "incremental_dir_bytes": incremental_dir_bytes,
        }));
    }
    let end_load = load_json(&[], &rustc);
    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-incremental-scale",
        "generated_unix": ravum_daemon::cache::now_unix(),
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": crate::harness::git_json(&options.repo_root),
        "measurement": {
            "cold_ms": "one `rustc --emit=metadata --error-format=json` process, fresh --out-dir, no -C incremental",
            "incremental_ms": "same process with persistent `-C incremental=<dir>`, same source paths",
            "order": "cold and incremental interleaved per repetition",
            "reps": options.reps,
            "percentiles": "nearest-rank; with 5 reps p50 is the 3rd value, a coarse estimate",
        },
        "command": format!(
            "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite scale --scale-reps {}",
            options.reps
        ),
        "load_start": start_load,
        "load_end": end_load,
        "sizes": sizes,
        "summary": summary,
        "limits": [
            "synthetic unit generated by the harness, not a real-world crate: it isolates front-end/query work, it does not model trait-heavy or macro-heavy code",
            "the shared machine was loaded by other agents during the run (see load_start/load_end)",
            "absolute milliseconds are machine-specific; the cold/incremental ratio at each size is the usable observation",
            "with 5 repetitions the percentiles are coarse; the per-repetition samples are included so the distribution is visible",
            "codegen and linking are not measured; `-C incremental` benefits codegen more than the metadata-only front-end measured here",
        ],
    }))
}

fn run_case(
    work_root: &Path,
    rustc: &Path,
    fixture: &Fixture,
    base: &[SourceFile],
    size: &str,
    case: &str,
    reps: usize,
    body_edit: bool,
) -> Result<Value, String> {
    let category = format!("{size}-{case}");
    let mut cold_samples = Vec::new();
    let mut incremental_samples = Vec::new();
    let mut cold_runs: Vec<ModeRun> = Vec::new();
    let mut incremental_runs: Vec<ModeRun> = Vec::new();
    let mut sequence: u64 = 0;
    // Warm-up for the incremental directory (unrecorded).
    let _ = run_incremental(work_root, rustc, fixture, &category, base)?;
    let mut state = base.to_vec();
    for rep in 1..=reps {
        if body_edit {
            state = apply_body_edit(&state, rep)?;
        }
        // Interleave: cold first on even reps, incremental first on odd reps.
        if rep % 2 == 0 {
            let incremental = run_incremental(work_root, rustc, fixture, &category, &state)?;
            incremental_samples.push(incremental.ms);
            incremental_runs.push(incremental);
            let cold = run_cold(work_root, rustc, fixture, &category, sequence, &state)?;
            sequence += 1;
            cold_samples.push(cold.ms);
            cold_runs.push(cold);
        } else {
            let cold = run_cold(work_root, rustc, fixture, &category, sequence, &state)?;
            sequence += 1;
            cold_samples.push(cold.ms);
            cold_runs.push(cold);
            let incremental = run_incremental(work_root, rustc, fixture, &category, &state)?;
            incremental_samples.push(incremental.ms);
            incremental_runs.push(incremental);
        }
    }
    let consistent = cold_runs
        .iter()
        .zip(incremental_runs.iter())
        .all(|(cold, incremental)| cold.signatures() == incremental.signatures());
    Ok(json!({
        "cold": stats(&cold_samples),
        "incremental": stats(&incremental_samples),
        "cold_samples_ms": cold_samples,
        "incremental_samples_ms": incremental_samples,
        "diagnostics_consistent": consistent,
        "reps": reps,
    }))
}

/// Changes the first function body literal: `let a = x * SCALE + 1.0;` becomes
/// `1.1`, then `1.2`, ... Each repetition is a real content change.
fn apply_body_edit(state: &[SourceFile], rep: usize) -> Result<Vec<SourceFile>, String> {
    let mut next = state.to_vec();
    let math = next
        .iter_mut()
        .find(|file| file.path == "src/math.rs")
        .ok_or_else(|| "synthetic unit has no src/math.rs".to_owned())?;
    let find = format!(
        "pub fn op_1(x: f64) -> f64 {{\n    let a = x * SCALE + 1.{};",
        rep - 1
    );
    let replace = format!("pub fn op_1(x: f64) -> f64 {{\n    let a = x * SCALE + 1.{rep};");
    if math.content.matches(&find).count() != 1 {
        return Err(format!("body edit anchor {find:?} not found exactly once"));
    }
    math.content = math.content.replacen(&find, &replace, 1);
    Ok(next)
}

fn generate(spec: &SizeSpec, work_root: &Path) -> Result<Fixture, String> {
    let unit_dir = work_root
        .join("scale")
        .join(spec.name)
        .join("generated-unit");
    let _ = std::fs::remove_dir_all(&unit_dir);
    let src = unit_dir.join("src");
    std::fs::create_dir_all(&src)
        .map_err(|error| format!("cannot create synthetic unit: {error}"))?;

    let mut math = String::from("//! Synthetic math module.\n\npub const SCALE: f64 = 2.5;\n\n");
    for index in 1..=spec.functions {
        math.push_str(&format!(
            "/// Applies operation {index} to its input.\npub fn op_{index}(x: f64) -> f64 {{\n    let a = x * SCALE + 1.0;\n    let b = if a > 0.0 {{ a.sqrt() }} else {{ -a }};\n    b * 1.5 + {index}.25\n}}\n\n"
        ));
    }
    let mut shapes = String::from("//! Synthetic shapes module.\n\n");
    for index in 1..=spec.structs {
        shapes.push_str(&format!(
            "#[derive(Debug, Clone, Copy)]\npub struct Point{index} {{\n    pub x: f64,\n    pub y: f64,\n}}\n\nimpl Point{index} {{\n    pub fn norm(&self) -> f64 {{\n        (self.x * self.x + self.y * self.y + {index}.0).sqrt()\n    }}\n\n    pub fn scaled(&self, k: f64) -> Self {{\n        Self {{ x: self.x * k, y: self.y * k }}\n    }}\n}}\n\n"
        ));
    }
    let pipeline = "//! Synthetic pipeline module.\n\nuse crate::math::*;\nuse crate::shapes::*;\n\npub fn score(values: &[f64]) -> f64 {\n    let mut total = 0.0;\n    for value in values {\n        total += op_1(*value) + op_2(*value);\n    }\n    let point = Point1 { x: 1.0, y: 2.0 };\n    total + point.norm()\n}\n";
    let lib = "//! Synthetic generated unit.\n#![forbid(unsafe_code)]\n\nmod math;\nmod pipeline;\nmod shapes;\n\npub fn evaluate(x: f64) -> f64 {\n    pipeline::score(&[x])\n}\n";

    let mut files = Vec::new();
    for (path, content) in [
        ("src/lib.rs", lib.to_owned()),
        ("src/math.rs", math),
        ("src/pipeline.rs", pipeline.to_owned()),
        ("src/shapes.rs", shapes),
    ] {
        std::fs::write(src.join(path.trim_start_matches("src/")), &content)
            .map_err(|error| format!("cannot write synthetic file {path}: {error}"))?;
        files.push(SourceFile {
            path: path.to_owned(),
            content,
        });
    }
    Ok(Fixture {
        root: "src/lib.rs".to_owned(),
        files,
        context: BuildContext {
            edition: "2021".to_owned(),
            crate_name: "synthetic_unit".to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        },
        dir: unit_dir,
    })
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}
