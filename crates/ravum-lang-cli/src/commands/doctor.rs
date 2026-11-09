//! `ravum-lang doctor` — honest diagnosis of the local toolchain.
//!
//! Besides rustc/paths/cache, `doctor` probes the three frontend stages so a
//! developer can see whether a stage is still a stub in this build. It never
//! reports a stage as ready by assumption.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, CheckedProgram, Facts};
use ravum_lang_sema::{check, Profile};
use ravum_lang_syntax::ast::{Module, Program};
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use serde_json::json;
use std::path::PathBuf;

use crate::json;
use crate::pipeline;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Ready,
    Pending,
    Error,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Ready => "ready",
            Stage::Pending => "pending",
            Stage::Error => "error",
        }
    }
}

pub fn run(json_output: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let package_root = util::find_package_root(&cwd);
    let cache = package_root.join(util::CACHE_DIR);
    let cache_present = std::fs::symlink_metadata(&cache)
        .map(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        .unwrap_or(false);
    let cache_bytes = if cache_present {
        util::dir_stats(&cache).map(|stats| stats.bytes).unwrap_or(0)
    } else {
        0
    };
    let rustc_command = pipeline::rustc_command();
    let rustc_version = pipeline::rustc_version();
    let rustc_present = rustc_version.is_some();
    let parser = probe_parser();
    let (checker, checked) = probe_checker();
    let codegen = probe_codegen(checked);

    let all_ready =
        parser == Stage::Ready && checker == Stage::Ready && codegen == Stage::Ready;
    let any_error =
        parser == Stage::Error || checker == Stage::Error || codegen == Stage::Error;
    let verdict = if !rustc_present {
        "blocked"
    } else if any_error {
        "error"
    } else if all_ready {
        "ready"
    } else {
        "partial"
    };

    let success = rustc_present;
    let exit = if rustc_present {
        util::EXIT_OK
    } else {
        util::EXIT_TOOL
    };

    if json_output {
        let summary = json!({
            "environment": {
                "ravum_lang": env!("CARGO_PKG_VERSION"),
                "toolchain_edition": ravum_lang_syntax::LANGUAGE_EDITION,
                "manifest_edition": ravum_lang_sema::loader::MANIFEST_EDITION,
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "cwd": util::pretty_path(&cwd).to_string(),
                "executable": std::env::current_exe()
                    .map(|path| util::pretty_path(&path))
                    .unwrap_or_default(),
                "package_root": util::pretty_path(&package_root).to_string(),
                "cache_dir": util::pretty_path(&cache).to_string(),
                "cache_present": cache_present,
                "cache_bytes": cache_bytes,
                "rustc": {
                    "command": rustc_command,
                    "present": rustc_present,
                    "version": rustc_version,
                },
            },
            "frontend": {
                "parser": parser.name(),
                "checker": checker.name(),
                "codegen": codegen.name(),
            },
            "checks": [
                {"name": "rustc", "status": if rustc_present { "ok" } else { "missing" }},
                {"name": "parser", "status": parser.name()},
                {"name": "checker", "status": checker.name()},
                {"name": "codegen", "status": codegen.name()},
                {"name": "cache", "status": if cache_present { "present" } else { "absent" }},
            ],
            "verdict": verdict,
        });
        let meta = json::ReportMeta {
            command: "doctor",
            success,
            exit_code: exit,
            target_kind: "package",
            target_path: util::target_display(&package_root),
            profile: None,
            dependencies_digest: None,
            lockfile_stale: None,
        };
        let envelope = json::envelope(meta, Vec::new(), summary);
        json::print(&envelope);
    } else {
        println!(
            "ravum-lang {} (toolchain edition {}, manifest edition {})",
            env!("CARGO_PKG_VERSION"),
            ravum_lang_syntax::LANGUAGE_EDITION,
            ravum_lang_sema::loader::MANIFEST_EDITION
        );
        println!(
            "os: {} / {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        println!("cwd: {}", util::pretty_path(&cwd));
        println!("package root: {}", util::pretty_path(&package_root));
        println!(
            "cache: {} ({}, {} bytes)",
            util::pretty_path(&cache),
            if cache_present { "present" } else { "absent" },
            cache_bytes
        );
        match &rustc_version {
            Some(version) => println!("rustc: {version} ({rustc_command})"),
            None => println!(
                "rustc: NOT FOUND ({rustc_command}); `build`, `run` and `test` cannot work"
            ),
        }
        println!(
            "frontend: parser={}, checker={}, codegen={}",
            parser.name(),
            checker.name(),
            codegen.name()
        );
        match verdict {
            "ready" => println!("verdict: ready"),
            "partial" => println!(
                "verdict: partial — at least one frontend stage is still a stub in this build; \
                 commands report its diagnostic and exit 1"
            ),
            "error" => println!("verdict: error — a frontend stage returned an unexpected error"),
            _ => println!(
                "verdict: blocked — no usable rustc; install a Rust toolchain (rustc 1.74+ \
                 recommended)"
            ),
        }
    }

    exit
}

fn empty_program() -> Program {
    Program {
        file: 0,
        root: Module {
            id: 1,
            name: None,
            items: Vec::new(),
            span: Span::dummy(),
        },
    }
}

fn probe_parser() -> Stage {
    let result = std::panic::catch_unwind(|| ravum_lang_syntax::parser::parse_file(0, ""));
    match result {
        Ok(result) => {
            if result
                .diags
                .iter()
                .any(|diag| diag.code == "E-INTERNAL-PARSER")
            {
                Stage::Pending
            } else if result.diags.has_errors() {
                Stage::Error
            } else {
                Stage::Ready
            }
        }
        Err(_) => Stage::Error,
    }
}

fn probe_checker() -> (Stage, Option<CheckedProgram>) {
    let program = empty_program();
    let sources = SourceMap::new();
    let options = CheckOptions::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check(&program, &sources, &options)
    }));
    let Ok(result) = result else {
        return (Stage::Error, None);
    };
    let pending = result
        .diags
        .iter()
        .any(|diag| diag.code == "E-INTERNAL-CHECKER");
    // A real checker may legitimately reject the empty probe program (for
    // example `E-RES-MAIN-MISSING`); only its explicit stub marker means the
    // stage is not implemented.
    let stage = if pending { Stage::Pending } else { Stage::Ready };
    (stage, result.checked)
}

fn probe_codegen(checked: Option<CheckedProgram>) -> Stage {
    let program = empty_program();
    let checked = checked.unwrap_or_else(|| CheckedProgram {
        program,
        facts: Facts::default(),
        files: Vec::new(),
        profile: Profile::Core,
    });
    let options = GenOptions::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        generate(&checked, &options)
    }));
    match result {
        Ok(Ok(_)) => Stage::Ready,
        Ok(Err(error)) => {
            if error.message.contains("not implemented") {
                Stage::Pending
            } else {
                // The generator rejected the synthetic program but is present.
                Stage::Ready
            }
        }
        Err(_) => Stage::Error,
    }
}
