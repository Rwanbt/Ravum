//! `rustc` CLI driver: one child process per cache miss, cancellable and
//! bounded.
//!
//! Public interface only: `--emit=metadata`, `--error-format=json`,
//! `-C incremental=<persistent dir>`. No `rustc_private`, no
//! `RUSTC_BOOTSTRAP`, no `RUSTFLAGS`/wrapper inheritance (the child environment
//! is scrubbed so a verdict is attributable to the declared context).
//!
//! Cancellation is real: a superseding revision sets a flag, the driver polls
//! it and kills the child process instead of ignoring the answer afterwards.

use crate::config::{validate_extern_name, BuildContext};
use crate::error::{DaemonError, Result};
use crate::revision::CancelToken;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Inputs of one compiler invocation.
pub struct DriverOptions<'a> {
    /// Absolute path of the crate root inside the stable compile unit.
    pub root_path: &'a Path,
    /// Directory receiving `std::rc` metadata output.
    pub out_dir: &'a Path,
    /// Persistent `-C incremental` directory for this build context.
    pub incremental_dir: &'a Path,
    /// Declared build context (edition, crate identity, extra flags).
    pub context: &'a BuildContext,
    /// Poll interval for cancellation/stale checks.
    pub poll_interval_ms: u64,
    /// Hard wall-clock cap; `0` disables the cap (not recommended).
    pub timeout_ms: u64,
}

/// Result of one compiler invocation.
#[derive(Debug, Clone)]
pub struct DriverOutput {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub analysis_ms: u64,
}

/// One already-compiled direct dependency, addressed by the exact `.rmeta` bytes
/// that rustc must consume for the dependent (`--extern NAME=PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledExtern {
    pub name: String,
    pub rmeta: PathBuf,
}

pub struct RustcDriver {
    rustc: PathBuf,
}

impl RustcDriver {
    pub fn new(rustc: impl Into<PathBuf>) -> Self {
        Self {
            rustc: rustc.into(),
        }
    }

    pub fn rustc(&self) -> &Path {
        &self.rustc
    }

    /// Arguments shared by both modes: everything except `--extern` and the root.
    fn base_args(&self, options: &DriverOptions<'_>) -> Vec<String> {
        let mut args = vec![
            format!("--edition={}", options.context.edition),
            format!("--crate-name={}", options.context.crate_name),
            format!("--crate-type={}", options.context.crate_type),
            "--emit=metadata".to_owned(),
            "--error-format=json".to_owned(),
            "--diagnostic-width=120".to_owned(),
            format!("--out-dir={}", options.out_dir.display()),
            "-C".to_owned(),
            format!("incremental={}", options.incremental_dir.display()),
        ];
        args.extend(options.context.flags.iter().cloned());
        args
    }

    /// The exact argument vector used for one analysis, minus the source path.
    /// Kept public so evidence and documentation can quote the real flags.
    pub fn args(&self, options: &DriverOptions<'_>) -> Vec<String> {
        let mut args = self.base_args(options);
        args.push(options.root_path.display().to_string());
        args
    }

    /// Validates and builds the argument vector of a dependent compilation with
    /// fresh `--extern NAME=<.rmeta>` artifacts. Fail-closed: an invalid extern
    /// name, a missing/non-file artifact or a duplicate name is an error before
    /// any process spawn.
    pub fn args_with_externs(
        &self,
        options: &DriverOptions<'_>,
        externs: &[CompiledExtern],
    ) -> Result<Vec<String>> {
        let mut names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut dependency_dirs: Vec<PathBuf> = Vec::new();
        for extern_input in externs {
            validate_extern_name(&extern_input.name)?;
            if !extern_input.rmeta.is_file() {
                return Err(DaemonError::invalid(format!(
                    "extern {:?} references a missing .rmeta: {}",
                    extern_input.name,
                    extern_input.rmeta.display()
                )));
            }
            if !names.insert(extern_input.name.as_str()) {
                return Err(DaemonError::invalid(format!(
                    "duplicate extern name {:?}",
                    extern_input.name
                )));
            }
            if let Some(parent) = extern_input.rmeta.parent() {
                if !dependency_dirs.iter().any(|dir| dir == parent) {
                    dependency_dirs.push(parent.to_path_buf());
                }
            }
        }
        let mut args = self.base_args(options);
        for extern_input in externs {
            args.push("--extern".to_owned());
            args.push(format!(
                "{}={}",
                extern_input.name,
                extern_input.rmeta.display()
            ));
        }
        for directory in dependency_dirs {
            args.push("-L".to_owned());
            args.push(format!("dependency={}", directory.display()));
        }
        args.push(options.root_path.display().to_string());
        Ok(args)
    }

    /// Human-readable command template, for evidence files and docs.
    pub fn command_template(&self, options: &DriverOptions<'_>) -> String {
        let mut parts = vec![format!("{}", self.rustc.display())];
        parts.extend(self.args(options));
        parts
            .iter()
            .map(|part| {
                if part.contains(' ') {
                    format!("\"{part}\"")
                } else {
                    part.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Runs one analysis. Returns a driver error only when the child could not
    /// be spawned or reaped; a compiler failure is a successful invocation with
    /// `success == false` (the diagnostics are the result).
    pub fn run(&self, options: &DriverOptions<'_>, token: &CancelToken) -> Result<DriverOutput> {
        self.execute(self.args(options), options, token)
    }

    /// Runs one dependent compilation with fresh extern interface artifacts.
    pub fn run_with_externs(
        &self,
        options: &DriverOptions<'_>,
        externs: &[CompiledExtern],
        token: &CancelToken,
    ) -> Result<DriverOutput> {
        let args = self.args_with_externs(options, externs)?;
        self.execute(args, options, token)
    }

    fn execute(
        &self,
        args: Vec<String>,
        options: &DriverOptions<'_>,
        token: &CancelToken,
    ) -> Result<DriverOutput> {
        std::fs::create_dir_all(options.out_dir)?;
        if let Some(parent) = options.incremental_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut command = Command::new(&self.rustc);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("RUSTC_BOOTSTRAP");
        let started = Instant::now();
        let mut child = command.spawn().map_err(|e| {
            DaemonError::driver(format!("cannot spawn {}: {e}", self.rustc.display()))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DaemonError::driver("compiler stdout pipe missing"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DaemonError::driver("compiler stderr pipe missing"))?;
        // Reader threads prevent a full pipe buffer from deadlocking the child.
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = stdout_tx.send(read_all(stdout));
        });
        std::thread::spawn(move || {
            let _ = stderr_tx.send(read_all(stderr));
        });

        let deadline = if options.timeout_ms == 0 {
            None
        } else {
            Some(started + Duration::from_millis(options.timeout_ms))
        };
        let poll = Duration::from_millis(options.poll_interval_ms.max(1));
        let mut cancelled = false;
        let mut timed_out = false;
        let status = loop {
            if token.is_cancelled() {
                cancelled = true;
                let _ = child.kill();
                let status = child.wait().ok();
                break status;
            }
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    let status = child.wait().ok();
                    break status;
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(poll),
                Err(error) => {
                    return Err(DaemonError::driver(format!(
                        "cannot poll compiler child: {error}"
                    )))
                }
            }
        };
        let analysis_ms = started.elapsed().as_millis() as u64;
        let stdout = stdout_rx.recv().unwrap_or_default();
        let stderr = stderr_rx.recv().unwrap_or_default();
        let exit_code = status.and_then(|status| status.code());
        let success = !cancelled && !timed_out && exit_code == Some(0);
        Ok(DriverOutput {
            exit_code,
            success,
            cancelled,
            timed_out,
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            analysis_ms,
        })
    }
}

fn read_all(mut reader: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = reader.read_to_end(&mut buffer);
    buffer
}

/// Computes the persistent incremental directory for one build context. One
/// directory per context (identity includes flags and toolchain), so switching
/// the context cannot silently reuse another context's state.
pub fn incremental_dir(work_dir: &Path, context_id: &str) -> PathBuf {
    let short = &context_id[..12.min(context_id.len())];
    work_dir.join("incremental").join(short)
}

/// Computes the stable compile-unit directory for one build context.
pub fn unit_dir(work_dir: &Path, context_id: &str) -> PathBuf {
    let short = &context_id[..12.min(context_id.len())];
    work_dir.join("units").join(short)
}

/// Computes the output directory for one build context.
pub fn out_dir(work_dir: &Path, context_id: &str) -> PathBuf {
    let short = &context_id[..12.min(context_id.len())];
    work_dir.join("out").join(short)
}
#[cfg(test)]
mod extern_tests {
    use super::*;
    use crate::revision::CancelToken;
    use std::path::PathBuf;

    fn context(crate_name: &str) -> BuildContext {
        BuildContext {
            edition: "2021".to_owned(),
            crate_name: crate_name.to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        }
    }

    fn options<'a>(
        root: &'a Path,
        out_dir: &'a Path,
        incremental: &'a Path,
        context: &'a BuildContext,
    ) -> DriverOptions<'a> {
        DriverOptions {
            root_path: root,
            out_dir,
            incremental_dir: incremental,
            context,
            poll_interval_ms: 2,
            timeout_ms: 60_000,
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "ravum-daemon-extern-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn malformed_externs_are_rejected_before_spawning() {
        let root = temp_root("invalid");
        let root_path = root.join("src").join("lib.rs");
        std::fs::create_dir_all(root_path.parent().unwrap()).unwrap();
        std::fs::write(&root_path, "pub fn a() {}").unwrap();
        let out_dir = root.join("out");
        let incremental = root.join("inc");
        let context = context("main_unit");
        let options = options(&root_path, &out_dir, &incremental, &context);
        let driver = RustcDriver::new("rustc");
        let valid_file = root.join("dep.rmeta");
        std::fs::write(&valid_file, b"not-real").unwrap();
        let mut invalid_name = CompiledExtern {
            name: "../core".to_owned(),
            rmeta: valid_file.clone(),
        };
        assert!(driver
            .args_with_externs(&options, &[invalid_name.clone()])
            .is_err());
        invalid_name.name = "core".to_owned();
        assert!(driver
            .args_with_externs(&options, &[invalid_name.clone(), invalid_name.clone()])
            .is_err());
        let missing = CompiledExtern {
            name: "core".to_owned(),
            rmeta: root.join("missing.rmeta"),
        };
        assert!(driver.args_with_externs(&options, &[missing]).is_err());
        let args = driver
            .args_with_externs(&options, &[invalid_name])
            .expect("valid extern args");
        assert!(args.windows(2).any(|pair| pair[0] == "--extern"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compiles_a_real_dependent_against_a_fresh_rmeta() {
        let root = temp_root("chain");
        let dep_source = root.join("dep.rs");
        std::fs::write(&dep_source, "pub fn twice(x: f64) -> f64 { x * 2.0 }\n").unwrap();
        let dep_rmeta = root.join("libdep.rmeta");
        let status = Command::new("rustc")
            .args([
                "--crate-name",
                "dep",
                "--crate-type",
                "lib",
                "--emit=metadata",
            ])
            .arg("-o")
            .arg(&dep_rmeta)
            .arg(&dep_source)
            .status()
            .expect("compile dependency");
        assert!(status.success(), "dependency compilation must succeed");
        let main_source = root.join("main.rs");
        std::fs::write(
            &main_source,
            "pub fn use_dep(x: f64) -> f64 { dep::twice(x) + 1.0 }\n",
        )
        .unwrap();
        let out_dir = root.join("out");
        let incremental = root.join("inc");
        let context = context("main_unit");
        let options = options(&main_source, &out_dir, &incremental, &context);
        let driver = RustcDriver::new("rustc");
        let token = CancelToken::new();
        let externs = vec![CompiledExtern {
            name: "dep".to_owned(),
            rmeta: dep_rmeta,
        }];
        let output = driver
            .run_with_externs(&options, &externs, &token)
            .expect("driver invocation");
        assert!(
            output.success,
            "dependent compilation must succeed: {}",
            output.stderr
        );
        assert!(output.analysis_ms > 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
