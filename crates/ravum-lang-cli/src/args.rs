//! Command-line parsing (manual, dependency-free, deterministic).

use ravum_lang_sema::Profile;
use std::path::PathBuf;

use crate::pipeline::BuildTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheAction {
    Status,
    Clean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Init {
        dir: PathBuf,
        lib: bool,
        force: bool,
    },
    Check {
        path: PathBuf,
        json: bool,
        profile: Profile,
        locked: bool,
    },
    Build {
        path: PathBuf,
        release: bool,
        emit_rust: Option<PathBuf>,
        emit_ir: bool,
        target: BuildTarget,
    },
    Run {
        path: PathBuf,
        args: Vec<String>,
        target: BuildTarget,
    },
    Watch {
        path: PathBuf,
        run: bool,
        release: bool,
        /// Internal test hook: stop after N change-triggered rebuilds.
        max_rebuilds: Option<u64>,
    },
    Test {
        path: PathBuf,
        filter: Option<String>,
        json: bool,
    },
    Fmt {
        path: PathBuf,
        check: bool,
    },
    Lint {
        path: PathBuf,
        deny: Vec<String>,
    },
    Doc {
        path: PathBuf,
        out: Option<PathBuf>,
    },
    Cache {
        action: CacheAction,
        path: PathBuf,
    },
    Doctor {
        json: bool,
    },
    Lsp {
        json: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Help(Option<String>),
    Version,
    Command(Command),
}

pub fn parse(raw: &[String]) -> Result<Invocation, String> {
    let Some(first) = raw.first() else {
        return Ok(Invocation::Help(None));
    };
    match first.as_str() {
        "--version" | "-V" | "version" => {
            if raw.len() > 1 {
                return Err("`--version` does not take arguments".to_string());
            }
            Ok(Invocation::Version)
        }
        "--help" | "-h" | "help" => {
            let topic = raw.get(1).cloned();
            if raw.len() > 2 {
                return Err("`help` takes at most one command name".to_string());
            }
            Ok(Invocation::Help(topic))
        }
        command => parse_command(command, &raw[1..]).map(Invocation::Command),
    }
}

fn parse_command(name: &str, rest: &[String]) -> Result<Command, String> {
    match name {
        "init" => parse_init(rest),
        "check" => parse_check(rest),
        "build" => parse_build(rest),
        "run" => parse_run(rest),
        "watch" | "dev" => parse_watch(rest),
        "test" => parse_test(rest),
        "fmt" => parse_fmt(rest),
        "lint" => parse_lint(rest),
        "doc" => parse_doc(rest),
        "cache" => parse_cache(rest),
        "doctor" => parse_doctor(rest),
        "lsp" => parse_lsp(rest),
        other => Err(format!(
            "unknown command `{other}`; run `ravum-lang --help` for the supported commands"
        )),
    }
}

fn option_value(rest: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    rest.get(*index)
        .cloned()
        .ok_or_else(|| format!("`{flag}` requires a value"))
}

fn default_path() -> PathBuf {
    PathBuf::from(".")
}

fn parse_path_argument(arg: &str, path: &mut Option<PathBuf>) -> Result<(), String> {
    if path.is_some() {
        return Err(format!(
            "unexpected argument `{arg}` (only one PATH is accepted)"
        ));
    }
    *path = Some(PathBuf::from(arg));
    Ok(())
}

fn parse_init(rest: &[String]) -> Result<Command, String> {
    let mut dir: Option<PathBuf> = None;
    let mut lib = false;
    let mut force = false;
    for arg in rest {
        match arg.as_str() {
            "--lib" => lib = true,
            "--force" => force = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `init`"))
            }
            other => parse_path_argument(other, &mut dir)?,
        }
    }
    let dir = dir.ok_or_else(|| "`init` requires a target directory".to_string())?;
    Ok(Command::Init { dir, lib, force })
}

fn parse_check(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut json = false;
    let mut profile = Profile::Core;
    let mut locked = false;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--json" => json = true,
            "--locked" => locked = true,
            "--profile" => {
                let value = option_value(rest, &mut i, "--profile")?;
                profile = parse_profile(&value)?;
            }
            other if other.starts_with("--profile=") => {
                let value = other.trim_start_matches("--profile=");
                profile = parse_profile(value)?;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `check`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Check {
        path: path.unwrap_or_else(default_path),
        json,
        profile,
        locked,
    })
}

fn parse_profile(value: &str) -> Result<Profile, String> {
    Profile::parse(value)
        .ok_or_else(|| format!("invalid profile `{value}`; expected `core`, `pure` or `rt`"))
}

fn parse_target(value: &str, command: &str) -> Result<BuildTarget, String> {
    BuildTarget::parse(value).ok_or_else(|| {
        format!(
            "unknown target `{value}` for `{command}`; supported target: `wasm32-wasip1` \
             (native is the default)"
        )
    })
}

fn parse_build(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut release = false;
    let mut emit_rust: Option<PathBuf> = None;
    let mut emit_ir = false;
    let mut target = BuildTarget::Native;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--release" => release = true,
            "--emit-ir" => emit_ir = true,
            "--emit-rust" => {
                let value = option_value(rest, &mut i, "--emit-rust")?;
                emit_rust = Some(PathBuf::from(value));
            }
            "--target" => {
                let value = option_value(rest, &mut i, "--target")?;
                target = parse_target(&value, "build")?;
            }
            other if other.starts_with("--emit-rust=") => {
                let value = other.trim_start_matches("--emit-rust=");
                emit_rust = Some(PathBuf::from(value));
            }
            other if other.starts_with("--target=") => {
                target = parse_target(other.trim_start_matches("--target="), "build")?;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `build`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Build {
        path: path.unwrap_or_else(default_path),
        release,
        emit_rust,
        emit_ir,
        target,
    })
}

fn parse_run(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut child: Vec<String> = Vec::new();
    let mut target = BuildTarget::Native;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--" => {
                child.extend(rest[i + 1..].iter().cloned());
                break;
            }
            "--target" => {
                let value = option_value(rest, &mut i, "--target")?;
                target = parse_target(&value, "run")?;
            }
            other if other.starts_with("--target=") => {
                target = parse_target(other.trim_start_matches("--target="), "run")?;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!(
                    "unknown option `{other}` for `run`; child arguments go after `--`"
                ));
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Run {
        path: path.unwrap_or_else(default_path),
        args: child,
        target,
    })
}

fn parse_watch(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut run = false;
    let mut release = false;
    let mut max_rebuilds: Option<u64> = None;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--run" => run = true,
            "--release" => release = true,
            "--max-rebuilds" => {
                let value = option_value(rest, &mut i, "--max-rebuilds")?;
                max_rebuilds = Some(parse_max_rebuilds(&value)?);
            }
            other if other.starts_with("--max-rebuilds=") => {
                max_rebuilds = Some(parse_max_rebuilds(
                    other.trim_start_matches("--max-rebuilds="),
                )?);
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `watch`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Watch {
        path: path.unwrap_or_else(default_path),
        run,
        release,
        max_rebuilds,
    })
}

fn parse_max_rebuilds(value: &str) -> Result<u64, String> {
    value.parse::<u64>().map_err(|_| {
        format!("invalid `--max-rebuilds` value `{value}`; expected a non-negative integer")
    })
}

fn parse_test(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut filter: Option<String> = None;
    let mut json = false;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--json" => json = true,
            "--filter" => filter = Some(option_value(rest, &mut i, "--filter")?),
            other if other.starts_with("--filter=") => {
                filter = Some(other.trim_start_matches("--filter=").to_string());
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `test`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Test {
        path: path.unwrap_or_else(default_path),
        filter,
        json,
    })
}

fn parse_fmt(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut check = false;
    for arg in rest {
        match arg.as_str() {
            "--check" => check = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `fmt`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
    }
    Ok(Command::Fmt {
        path: path.unwrap_or_else(default_path),
        check,
    })
}

fn parse_lint(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut deny: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--deny" => deny.push(option_value(rest, &mut i, "--deny")?),
            other if other.starts_with("--deny=") => {
                deny.push(other.trim_start_matches("--deny=").to_string());
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `lint`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Lint {
        path: path.unwrap_or_else(default_path),
        deny,
    })
}

fn parse_doc(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--out" => out = Some(PathBuf::from(option_value(rest, &mut i, "--out")?)),
            other if other.starts_with("--out=") => {
                out = Some(PathBuf::from(other.trim_start_matches("--out=")));
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `doc`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
        i += 1;
    }
    Ok(Command::Doc {
        path: path.unwrap_or_else(default_path),
        out,
    })
}

fn parse_cache(rest: &[String]) -> Result<Command, String> {
    let mut action: Option<CacheAction> = None;
    let mut path: Option<PathBuf> = None;
    for arg in rest {
        match arg.as_str() {
            "status" if action.is_none() => action = Some(CacheAction::Status),
            "clean" if action.is_none() => action = Some(CacheAction::Clean),
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}` for `cache`"))
            }
            other => parse_path_argument(other, &mut path)?,
        }
    }
    let action = action.ok_or_else(|| {
        "`cache` requires an action: `ravum-lang cache status` or `ravum-lang cache clean`"
            .to_string()
    })?;
    Ok(Command::Cache {
        action,
        path: path.unwrap_or_else(default_path),
    })
}

fn parse_doctor(rest: &[String]) -> Result<Command, String> {
    let mut json = false;
    for arg in rest {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(format!("unknown option `{other}` for `doctor`")),
        }
    }
    Ok(Command::Doctor { json })
}

fn parse_lsp(rest: &[String]) -> Result<Command, String> {
    let mut json = false;
    for arg in rest {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(format!("unknown option `{other}` for `lsp`")),
        }
    }
    Ok(Command::Lsp { json })
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

pub fn help_text() -> String {
    format!(
        "ravum-lang {} — Ravum Core 1.0 toolchain

USAGE:
    ravum-lang <COMMAND> [OPTIONS]

COMMANDS:
    init <dir> [--lib] [--force]        Create a package (`ravum.toml` + `src/`)
    check [PATH] [--json] [--profile core|pure|rt] [--locked]
                                        Load, resolve and check a package or a `.rv` file
    build [PATH] [--release] [--target TRIPLE] [--emit-rust FILE] [--emit-ir]
                                        Check, generate Rust and compile it with `rustc`
    run [PATH] [--target TRIPLE] [-- ARGS...]
                                        Build, then run the binary (args/stdin/stdout/stderr forwarded)
    watch [PATH] [--run] [--release]    Poll sources and rebuild on change (alias: `dev`)
    test [PATH] [--filter TEXT] [--json]
                                        Build the test runner, run it, parse `test ... ok/FAILED`
    fmt [PATH] [--check]                Format every `.rv` file, or verify with `--check`
    lint [PATH] [--deny CODE]...        Report checker warnings; `--deny` turns one into an error
    doc [PATH] [--out DIR]              Generate Markdown API documentation for public items
    cache status|clean [PATH]           Inspect or remove the package `.ravum/` cache
    doctor [--json]                     Diagnose rustc, paths, cache and frontend stages
    help [COMMAND]                      Show this help or one command
    --version, -V                       Print the toolchain and language edition

UNSUPPORTED (explicit, never a fake success):
    lsp                                 No language server is implemented in this build (exit 3)

PATHS:
    PATH defaults to the current directory. A directory must contain `ravum.toml`;
    a single `.rv` file is loaded as a standalone program (no manifest, no lockfile).

TARGETS (build/run):
    native (default)        host executable
    wasm32-wasip1           `.wasm` via `rustc --target`, run through Node.js WASI

EXIT CODES:
    0  success
    1  compilation/diagnostic/test failure or a safe operational refusal
    2  invalid command line
    3  external tool missing (`rustc`) or UNSUPPORTED command

JSON (`check`, `test`, `doctor` with --json), schema_version 1, stdout only:
    {{schema_version, command, success, diagnostics, summary}}
    diagnostics: [{{code, severity, message, file, line, column, end_line,
                   end_column, notes}}], package-relative paths where possible.

NOTE:
    check/build/run/test/fmt/lint/doc run the real pipeline. When a frontend
    stage is still a stub in this build, the command reports that internal
    diagnostic and exits 1; it never reports a false success.
",
        env!("CARGO_PKG_VERSION")
    )
}

pub fn help_for(topic: &str) -> String {
    let body = match topic {
        "init" => "USAGE: ravum-lang init <dir> [--lib] [--force]\n\n\
                   Creates `ravum.toml`, `src/main.rv` (or `src/lib.rv` with --lib) and a\n\
                   `.gitignore` that ignores `.ravum/`. Refuses a non-empty directory\n\
                   unless --force is given; --force overwrites only the generated files.\n",
        "check" => "USAGE: ravum-lang check [PATH] [--json] [--profile core|pure|rt] [--locked]\n\n\
                    Loads the package (manifest, dependency graph, modules), validates the\n\
                    lockfile and runs the checker. Exit 0 when there is no error; warnings do\n\
                    not fail unless a later lint --deny turns them into errors.\n",
        "build" => "USAGE: ravum-lang build [PATH] [--release] [--target TRIPLE] \
                    [--emit-rust FILE] [--emit-ir]\n\n\
                    Checks the package, generates safe Rust and compiles it with\n\
                    `rustc --edition 2021 -C overflow-checks=on -C debuginfo=2`.\n\
                    Artifacts: `<package>/.ravum/gen/main.rs`, `<package>/.ravum/bin/<name>`\n\
                    (library packages: `.ravum/lib/<name>.rlib`).\n\
                    `--target wasm32-wasip1` writes `.ravum/bin/<name>.wasm`; it requires the\n\
                    `wasm32-wasip1` rustc target. --emit-ir dumps the checked facts; it is not\n\
                    a stable IR.\n",
        "run" => "USAGE: ravum-lang run [PATH] [--target TRIPLE] [-- ARGS...]\n\n\
                  Builds the binary crate, then executes it from the package root with stdin,\n\
                  stdout, stderr and the exit code forwarded. Child arguments must follow `--`.\n\
                  `--target wasm32-wasip1` runs the `.wasm` artifact through Node.js WASI\n\
                  (Node.js 22+; `RAVUM_NODE` overrides the executable).\n",
        "watch" => "USAGE: ravum-lang watch [PATH] [--run] [--release]   (alias: dev)\n\n\
                    Polls `.rv`, `ravum.toml` and `ravum.lock` (250 ms, no external dependency),\n\
                    ignoring `.ravum/`, `target/` and `*.ravum-tmp`. Rebuilds on change; with\n\
                    `--run` the previous child is killed before the new binary is launched.\n\
                    A build failure is reported and polling continues. Ctrl+C stops cleanly.\n\
                    Internal test hook: `--max-rebuilds N` stops after N rebuilds.\n",
        "test" => "USAGE: ravum-lang test [PATH] [--filter TEXT] [--json]\n\n\
                   Builds the generated test runner and executes it (`test <name> ... ok/FAILED`).\n\
                   --filter is forwarded to the runner as `--filter <TEXT>`. Exit 1 if any test\n\
                   fails or the runner exits non-zero.\n",
        "fmt" => "USAGE: ravum-lang fmt [PATH] [--check]\n\n\
                  Formats every `.rv` file under `src/` (or the given file) with the Ravum\n\
                  formatter. --check writes nothing and exits 1 if a file would change.\n",
        "lint" => "USAGE: ravum-lang lint [PATH] [--deny CODE]...\n\n\
                   Runs the checker and reports its warnings (W-*). --deny CODE turns a matching\n\
                   warning into a failure; --deny all denies every warning. Only documented\n\
                   checker warnings are reported; no extra heuristic rules are invented.\n",
        "doc" => "USAGE: ravum-lang doc [PATH] [--out DIR]\n\n\
                  Generates `index.md` documenting public functions, types, constants, modules\n\
                  and effects from the AST. Default output: `<package>/.ravum/doc/`. No HTML.\n",
        "cache" => "USAGE: ravum-lang cache status|clean [PATH]\n\n\
                    status: size, entries and GC policy of `<package>/.ravum/`.\n\
                    clean: safely removes only `<package>/.ravum/` (never a symlink).\n",
        "doctor" => "USAGE: ravum-lang doctor [--json]\n\n\
                     Reports rustc presence/version, edition, paths, cache and whether the\n\
                     parser/checker/codegen stages are ready or still stubs. Exit 3 if rustc\n\
                     is missing.\n",
        "lsp" => "USAGE: ravum-lang lsp\n\n\
                  UNSUPPORTED: no language server is implemented in this build. Exits 3 and\n\
                  is listed under UNSUPPORTED in `--help`; it is not advertised as available.\n",
        other => {
            return format!(
                "ravum-lang: unknown help topic `{other}`\n\n{}",
                help_text()
            )
        }
    };
    format!("ravum-lang {}\n\n{body}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse(&[]).unwrap(), Invocation::Help(None));
    }

    #[test]
    fn version_is_recognized() {
        assert_eq!(parse(&args(&["--version"])).unwrap(), Invocation::Version);
    }

    #[test]
    fn unknown_command_is_usage_error() {
        assert!(parse(&args(&["nope"])).is_err());
    }

    #[test]
    fn check_defaults_to_current_directory() {
        let parsed = parse(&args(&["check", "--json"])).unwrap();
        match parsed {
            Invocation::Command(Command::Check {
                path, json, locked, ..
            }) => {
                assert_eq!(path, PathBuf::from("."));
                assert!(json);
                assert!(!locked);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn invalid_profile_is_usage_error() {
        assert!(parse(&args(&["check", "--profile", "fast"])).is_err());
    }

    #[test]
    fn run_separates_child_arguments() {
        let parsed = parse(&args(&["run", "pkg", "--", "--flag", "x"])).unwrap();
        match parsed {
            Invocation::Command(Command::Run { path, args, .. }) => {
                assert_eq!(path, PathBuf::from("pkg"));
                assert_eq!(args, vec!["--flag".to_string(), "x".to_string()]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn watch_and_dev_parse_flags() {
        for command in ["watch", "dev"] {
            let parsed = parse(&args(&[
                command,
                "pkg",
                "--run",
                "--release",
                "--max-rebuilds",
                "2",
            ]))
            .unwrap();
            match parsed {
                Invocation::Command(Command::Watch {
                    path,
                    run,
                    release,
                    max_rebuilds,
                }) => {
                    assert_eq!(path, PathBuf::from("pkg"));
                    assert!(run);
                    assert!(release);
                    assert_eq!(max_rebuilds, Some(2));
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn watch_rejects_invalid_max_rebuilds() {
        assert!(parse(&args(&["watch", "--max-rebuilds", "nope"])).is_err());
        assert!(parse(&args(&["watch", "--max-rebuilds"])).is_err());
    }

    #[test]
    fn build_and_run_parse_wasm_target() {
        match parse(&args(&["build", "pkg", "--target", "wasm32-wasip1"])).unwrap() {
            Invocation::Command(Command::Build { target, path, .. }) => {
                assert_eq!(target, BuildTarget::Wasm32WasiP1);
                assert_eq!(path, PathBuf::from("pkg"));
            }
            other => panic!("unexpected {other:?}"),
        }
        match parse(&args(&["run", "--target=wasm32-wasip1", "pkg", "--", "a"])).unwrap() {
            Invocation::Command(Command::Run { target, args, path }) => {
                assert_eq!(target, BuildTarget::Wasm32WasiP1);
                assert_eq!(path, PathBuf::from("pkg"));
                assert_eq!(args, vec!["a".to_string()]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse(&args(&["build", "--target", "x86_64-unknown-none"])).is_err());
        assert!(parse(&args(&["run", "--target", "wasm32-wasi"])).is_err());
    }

    #[test]
    fn cache_requires_action() {
        assert!(parse(&args(&["cache"])).is_err());
        assert!(parse(&args(&["cache", "status"])).is_ok());
        assert!(parse(&args(&["cache", "clean"])).is_ok());
    }

    #[test]
    fn help_is_honest_about_lsp() {
        let text = help_text();
        assert!(text.contains("UNSUPPORTED"));
        assert!(text.contains("lsp"));
        assert!(text.contains("exit 3"));
    }
}
