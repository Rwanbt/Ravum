//! `ravum-conformance` — honest, declarative conformance harness for Ravum
//! Core 1.0.
//!
//! Commands:
//!   run  [--cases DIR] [--filter SUBSTR] [--json OUT] [--toolchain PATH]
//!        [--timeout-ms N] [--keep-temp]
//!   list [--cases DIR] [--filter SUBSTR]
//!   --version | --help
//!
//! Exit codes: 0 = no failure and no un-run case; 1 = at least one FAIL;
//! 2 = usage/configuration error or unwritable report; 3 = at least one
//! NOT_RUN (environment unavailable), with no FAIL.

mod case;
mod json;
mod report;
mod runner;
mod util;

use crate::util::{find_repo_root, format_utc, Sanitizer};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;

fn main() -> ExitCode {
    match execute(std::env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("run `ravum-conformance --help` for usage");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Clone)]
struct Options {
    cases: Option<String>,
    filter: Option<String>,
    json: Option<String>,
    toolchain: Option<String>,
    timeout_ms: u64,
    keep_temp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Run,
    List,
}

fn execute(args: Vec<String>) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None => {
            print_help();
            Err("missing command (expected `run`, `list`, `--version` or `--help`)".to_string())
        }
        Some("--version") | Some("-V") => {
            println!(
                "ravum-conformance {VERSION} (report schema {})",
                report::REPORT_SCHEMA_VERSION
            );
            Ok(0)
        }
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(0)
        }
        Some("run") => match parse_options(Command::Run, &args[1..])? {
            Some(options) => execute_run(options),
            None => Ok(0),
        },
        Some("list") => match parse_options(Command::List, &args[1..])? {
            Some(options) => execute_list(options),
            None => Ok(0),
        },
        Some(other) => Err(format!(
            "unknown command `{other}`; expected `run`, `list`, `--version` or `--help`"
        )),
    }
}

fn print_help() {
    println!("ravum-conformance {VERSION} — Ravum Core 1.0 conformance harness");
    println!();
    println!("USAGE:");
    println!("  ravum-conformance run  [OPTIONS]");
    println!("  ravum-conformance list [OPTIONS]");
    println!("  ravum-conformance --version");
    println!();
    println!("RUN OPTIONS:");
    println!("  --cases DIR        Corpus root (default: <repo>/conformance/cases)");
    println!("  --filter SUBSTR    Only cases whose id or path contains SUBSTR");
    println!("  --json OUT         Write the schema_version 1 report to OUT");
    println!("  --toolchain PATH   ravum-lang binary (default: $RAVUM_LANG, then");
    println!("                     <repo>/target/debug/ravum-lang[.exe])");
    println!("  --timeout-ms N     Per-case timeout, 1..={MAX_TIMEOUT_MS} (default {DEFAULT_TIMEOUT_MS})");
    println!("  --keep-temp        Keep the isolated temporary copies for inspection");
    println!();
    println!("LIST OPTIONS:");
    println!("  --cases DIR, --filter SUBSTR");
    println!();
    println!("Exit codes: 0 ok, 1 FAIL, 2 usage/config error, 3 NOT_RUN (no toolchain).");
    println!("Statuses: PASS, FAIL, SKIP (platform), NOT_RUN (environment).");
}

fn parse_options(command: Command, args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options {
        cases: None,
        filter: None,
        json: None,
        toolchain: None,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        keep_temp: false,
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        match name {
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--cases" => {
                let value = take_value(args, &mut index, name, inline_value)?;
                if options.cases.replace(value).is_some() {
                    return Err("--cases was given more than once".to_string());
                }
            }
            "--filter" => {
                let value = take_value(args, &mut index, name, inline_value)?;
                if options.filter.replace(value).is_some() {
                    return Err("--filter was given more than once".to_string());
                }
            }
            "--json" => {
                if command != Command::Run {
                    return Err("--json is only valid for `run`".to_string());
                }
                let value = take_value(args, &mut index, name, inline_value)?;
                if options.json.replace(value).is_some() {
                    return Err("--json was given more than once".to_string());
                }
            }
            "--toolchain" => {
                if command != Command::Run {
                    return Err("--toolchain is only valid for `run`".to_string());
                }
                let value = take_value(args, &mut index, name, inline_value)?;
                if options.toolchain.replace(value).is_some() {
                    return Err("--toolchain was given more than once".to_string());
                }
            }
            "--timeout-ms" => {
                if command != Command::Run {
                    return Err("--timeout-ms is only valid for `run`".to_string());
                }
                let value = take_value(args, &mut index, name, inline_value)?;
                let timeout = value
                    .parse::<u64>()
                    .map_err(|_| format!("--timeout-ms expects an integer, got `{value}`"))?;
                if !(1..=MAX_TIMEOUT_MS).contains(&timeout) {
                    return Err(format!(
                        "--timeout-ms must be in 1..={MAX_TIMEOUT_MS}, got {timeout}"
                    ));
                }
                options.timeout_ms = timeout;
            }
            "--keep-temp" => {
                if command != Command::Run {
                    return Err("--keep-temp is only valid for `run`".to_string());
                }
                if inline_value.is_some() {
                    return Err("--keep-temp does not take a value".to_string());
                }
                options.keep_temp = true;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }
    Ok(Some(options))
}

fn take_value(
    args: &[String],
    index: &mut usize,
    name: &str,
    inline: Option<String>,
) -> Result<String, String> {
    if let Some(value) = inline {
        if value.is_empty() {
            return Err(format!("{name} requires a non-empty value"));
        }
        return Ok(value);
    }
    *index += 1;
    match args.get(*index) {
        Some(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err(format!("{name} requires a value")),
    }
}

fn cases_root(options: &Options, repo: &Path, cwd: &Path) -> PathBuf {
    match &options.cases {
        Some(value) => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        }
        None => repo.join("conformance").join("cases"),
    }
}

fn execute_list(options: Options) -> Result<u8, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot determine the current directory: {error}"))?;
    let repo = find_repo_root(&cwd);
    let root = cases_root(&options, &repo, &cwd);
    let entries = case::discover(&root, options.filter.as_deref())?;
    let display = sanitize_display(&root, &repo);
    report::print_list(&entries, &display, VERSION);
    Ok(0)
}

fn execute_run(options: Options) -> Result<u8, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot determine the current directory: {error}"))?;
    let repo = find_repo_root(&cwd);
    let root = cases_root(&options, &repo, &cwd);
    let entries = case::discover(&root, options.filter.as_deref())?;
    if entries.is_empty() && options.filter.is_none() {
        return Err(format!(
            "no cases discovered under {} (an empty corpus is not a passing gate)",
            root.display()
        ));
    }

    let temp_base = temp_base();
    std::fs::create_dir_all(&temp_base).map_err(|error| {
        format!(
            "cannot create the temp directory {}: {error}",
            temp_base.display()
        )
    })?;
    let sanitizer = Sanitizer::new(&repo, &temp_base, Some(&temp_base.join("cases")));
    let root_display = sanitizer.apply(&root.display().to_string());

    let (toolchain, toolchain_error) =
        match runner::locate_toolchain(options.toolchain.as_deref(), &repo) {
            Ok(path) => {
                let version = runner::probe_version(&path, &repo, &temp_base);
                let display = sanitizer.apply(&path.display().to_string());
                (
                    Some(runner::Toolchain {
                        path,
                        display,
                        version,
                    }),
                    String::new(),
                )
            }
            Err(message) => (None, sanitizer.apply(&message)),
        };

    report::print_header(
        VERSION,
        &root_display,
        toolchain.as_ref(),
        &toolchain_error,
        options.filter.as_deref(),
    );

    let started_at = format_utc(SystemTime::now());
    let context = runner::RunContext {
        toolchain,
        toolchain_error,
        timeout_ms: options.timeout_ms,
        work_dir: temp_base.clone(),
        sanitizer: &sanitizer,
    };

    let width = entries
        .iter()
        .map(|entry| entry.id.chars().count())
        .max()
        .unwrap_or(20)
        .clamp(20, 64);
    let mut results = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let result = runner::run_case(entry, &context, index);
        report::print_case_line(&result, width);
        results.push(result);
    }

    report::print_failures(&results);
    let summary = report::summarize(&results);
    report::print_summary(summary);

    if let Some(path) = &options.json {
        let report = report::build_report(context.toolchain.as_ref(), &started_at, &results);
        report::write_report(Path::new(path), &report)?;
        println!("report written: {path}");
    }

    if options.keep_temp {
        println!("temp dir kept: {}", temp_base.display());
    } else {
        let _ = std::fs::remove_dir_all(&temp_base);
    }

    if summary.fail > 0 {
        Ok(1)
    } else if summary.not_run > 0 {
        Ok(3)
    } else {
        Ok(0)
    }
}

fn sanitize_display(path: &Path, repo: &Path) -> String {
    let temp = temp_base();
    let sanitizer = Sanitizer::new(repo, &temp, None);
    sanitizer.apply(&path.display().to_string())
}

fn temp_base() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("ravum-conformance-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options_for(command: Command, args: &[&str]) -> Result<Option<Options>, String> {
        let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        parse_options(command, &owned)
    }

    fn unwrap_options(options: Option<Options>) -> Options {
        options.expect("expected parsed options, got --help")
    }

    #[test]
    fn parses_run_options() {
        let options = unwrap_options(
            options_for(
                Command::Run,
                &[
                    "--cases",
                    "corpus",
                    "--filter=run/",
                    "--json",
                    "out.json",
                    "--timeout-ms",
                    "500",
                    "--keep-temp",
                ],
            )
            .unwrap(),
        );
        assert_eq!(options.cases.as_deref(), Some("corpus"));
        assert_eq!(options.filter.as_deref(), Some("run/"));
        assert_eq!(options.json.as_deref(), Some("out.json"));
        assert_eq!(options.timeout_ms, 500);
        assert!(options.keep_temp);
    }

    #[test]
    fn rejects_bad_or_duplicate_options() {
        assert!(options_for(Command::Run, &["--timeout-ms", "0"]).is_err());
        assert!(options_for(Command::Run, &["--timeout-ms", "abc"]).is_err());
        assert!(options_for(Command::Run, &["--json", "a", "--json", "b"]).is_err());
        assert!(options_for(Command::List, &["--json", "a"]).is_err());
        assert!(options_for(Command::List, &["--toolchain", "x"]).is_err());
        assert!(options_for(Command::Run, &["--unknown"]).is_err());
        assert!(options_for(Command::Run, &["--cases"]).is_err());
    }

    #[test]
    fn help_option_is_not_a_usage_error() {
        assert!(options_for(Command::Run, &["--help"]).unwrap().is_none());
    }
}
