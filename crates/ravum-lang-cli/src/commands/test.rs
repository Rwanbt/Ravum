//! `ravum-lang test` — build the generated test runner, execute it and parse
//! its `test <name> ... ok/FAILED` lines.

use ravum_lang_sema::Profile;
use serde_json::json;
use std::path::Path;
use std::process::Command;

use crate::json;
use crate::pipeline::{self, BuildOptions};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestStatus {
    Passed,
    Failed,
    Ignored,
}

impl TestStatus {
    fn name(self) -> &'static str {
        match self {
            TestStatus::Passed => "ok",
            TestStatus::Failed => "FAILED",
            TestStatus::Ignored => "ignored",
        }
    }
}

#[derive(Debug, Clone)]
struct TestCase {
    name: String,
    status: TestStatus,
}

pub fn run(path: &Path, filter: Option<&str>, json_output: bool) -> i32 {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    if !frontend.success() {
        let diagnostics = json::diagnostics(frontend.diags.as_slice(), &frontend.pkg.sources);
        if json_output {
            let summary = test_summary(&[], filter, None, Some("analysis failed"));
            json::print(&report(
                &frontend,
                path,
                false,
                util::EXIT_FAIL,
                diagnostics,
                summary,
            ));
        } else {
            eprint!("{}", pipeline::render_diags(&frontend));
            util::err(format!(
                "test: {} error(s); the test runner was not built",
                frontend.error_count()
            ));
        }
        return util::EXIT_FAIL;
    }

    let options = BuildOptions {
        release: false,
        test_mode: true,
        emit_rust: None,
    };
    let artifacts = match pipeline::compile_program(&frontend, &options) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            error.print();
            if json_output {
                let diagnostic = json::synthetic("E-BUILD", "error", error.message());
                let summary = test_summary(&[], filter, None, Some(&error.message()));
                json::print(&report(
                    &frontend,
                    path,
                    false,
                    error.exit_code(),
                    vec![diagnostic],
                    summary,
                ));
            }
            return error.exit_code();
        }
    };

    let mut command = Command::new(&artifacts.binary);
    if let Some(filter) = filter {
        command.arg("--filter").arg(filter);
    }
    command.current_dir(frontend.root());
    let output = match command.output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let message = format!(
                "cannot execute the generated test runner `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            );
            util::err(&message);
            if json_output {
                let diagnostic = json::synthetic("E-TOOL", "error", message.clone());
                let summary = test_summary(&[], filter, None, Some(&message));
                json::print(&report(
                    &frontend,
                    path,
                    false,
                    util::EXIT_TOOL,
                    vec![diagnostic],
                    summary,
                ));
            }
            return util::EXIT_TOOL;
        }
        Err(error) => {
            let message = format!(
                "cannot execute the generated test runner `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            );
            util::err(&message);
            if json_output {
                let diagnostic = json::synthetic("E-TOOL", "error", message.clone());
                let summary = test_summary(&[], filter, None, Some(&message));
                json::print(&report(
                    &frontend,
                    path,
                    false,
                    util::EXIT_FAIL,
                    vec![diagnostic],
                    summary,
                ));
            }
            return util::EXIT_FAIL;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut combined = stdout.clone();
    if !stderr.is_empty() {
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    let cases = parse_test_output(&combined);
    let failed = cases
        .iter()
        .filter(|case| case.status == TestStatus::Failed)
        .count();
    let exit_code = output.status.code();
    let success = output.status.success() && failed == 0;

    if json_output {
        let summary = test_summary(&cases, filter, exit_code, None);
        let exit = if success {
            util::EXIT_OK
        } else {
            util::EXIT_FAIL
        };
        json::print(&report(&frontend, path, success, exit, Vec::new(), summary));
    } else {
        print!("{stdout}");
        eprint!("{stderr}");
        let passed = cases
            .iter()
            .filter(|case| case.status == TestStatus::Passed)
            .count();
        let ignored = cases
            .iter()
            .filter(|case| case.status == TestStatus::Ignored)
            .count();
        println!(
            "test result: {}; {} passed; {} failed; {} ignored{}",
            if success { "ok" } else { "FAILED" },
            passed,
            failed,
            ignored,
            match filter {
                Some(filter) => format!(" (filter `{filter}`)"),
                None => String::new(),
            }
        );
    }

    if success {
        util::EXIT_OK
    } else {
        util::EXIT_FAIL
    }
}

fn report(
    frontend: &pipeline::Frontend,
    path: &Path,
    success: bool,
    exit_code: i32,
    diagnostics: Vec<json::JsonDiagnostic>,
    summary: serde_json::Value,
) -> json::JsonEnvelope {
    use ravum_lang_sema::loader::{CrateKind, LockfileState};
    let target_kind = if frontend.pkg.crate_kind == Some(CrateKind::File) {
        "file"
    } else {
        "package"
    };
    let lockfile_stale = match frontend.pkg.lockfile_state {
        LockfileState::Stale => Some(true),
        LockfileState::Created | LockfileState::Current => Some(false),
        _ => None,
    };
    json::envelope(
        json::ReportMeta {
            command: "test",
            success,
            exit_code,
            target_kind,
            target_path: util::target_display(path),
            profile: Some("core".to_string()),
            dependencies_digest: Some(frontend.pkg.dependencies_digest.clone()),
            lockfile_stale,
        },
        diagnostics,
        summary,
    )
}

fn parse_test_output(text: &str) -> Vec<TestCase> {
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, verdict)) = rest.rsplit_once(" ... ") else {
            continue;
        };
        let status = match verdict.trim() {
            "ok" => TestStatus::Passed,
            "FAILED" => TestStatus::Failed,
            "ignored" => TestStatus::Ignored,
            _ => continue,
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        cases.push(TestCase {
            name: name.to_string(),
            status,
        });
    }
    cases
}

fn test_summary(
    cases: &[TestCase],
    filter: Option<&str>,
    exit_code: Option<i32>,
    error: Option<&str>,
) -> serde_json::Value {
    let passed = cases
        .iter()
        .filter(|case| case.status == TestStatus::Passed)
        .count();
    let failed = cases
        .iter()
        .filter(|case| case.status == TestStatus::Failed)
        .count();
    let ignored = cases
        .iter()
        .filter(|case| case.status == TestStatus::Ignored)
        .count();
    let entries = cases
        .iter()
        .map(|case| {
            json!({
                "name": case.name,
                "status": case.status.name(),
            })
        })
        .collect::<Vec<_>>();
    let mut summary = json!({
        "total": cases.len(),
        "passed": passed,
        "failed": failed,
        "ignored": ignored,
        "filter": filter,
        "exit_code": exit_code,
        "tests": entries,
    });
    if let Some(error) = error {
        summary["error"] = json!(error);
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rust_style_test_lines() {
        let text = "\nrunning 3 tests\ntest alpha ... ok\ntest beta ... FAILED\ntest gamma ... ignored\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored\n";
        let cases = parse_test_output(text);
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0].name, "alpha");
        assert_eq!(cases[0].status, TestStatus::Passed);
        assert_eq!(cases[1].status, TestStatus::Failed);
        assert_eq!(cases[2].status, TestStatus::Ignored);
    }

    #[test]
    fn ignores_unrelated_lines() {
        let cases = parse_test_output("note: something\ntest ... ok\ntest x ...\n");
        assert!(cases.is_empty());
    }
}
