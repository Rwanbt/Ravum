//! Ravum runtime prelude, embedded verbatim in every generated program.
//!
//! OWNER: worker `codegen`. The prelude must compile with `rustc --edition 2021`
//! without external crates. It defines: panic hook and exit convention, print
//! helpers, argument/environment access, file helpers, `Error`, assertions and
//! the test runner support. It never exposes `unsafe` to generated user code —
//! and contains none at all.
//!
//! Semantics fixed by `docs/architecture/RAVUM-CORE-DESIGN.md` §3.8:
//! * run mode panic: `panic: <message>` on stderr, exit code `1`;
//! * `main -> Result<(), Error>`: `Err` prints `error: <message>`, exit code `1`;
//! * assertions report the Ravum `file:line` decided by the generator;
//! * every I/O error keeps the operating-system message (`std::io::Error`
//!   `Display`), never a bare code.
//!
//! The runtime performs no effect checking and no overflow instrumentation: the
//! checker decides effects and the CLI compiles with `-C overflow-checks=on`.

/// Runtime prelude source, inserted verbatim after the crate-level attributes.
pub const PRELUDE: &str = r##"// ---------------------------------------------------------------------------
// Ravum Core 1.0 runtime prelude (generated; do not edit by hand).
//
// Safe Rust only: no `unsafe`, no raw pointers, no `transmute`. Generated
// programs are compiled with `-C overflow-checks=on`; this runtime never
// weakens a check.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: &str) -> Error {
        Error {
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Human-readable text of a panic payload (safe: no `unsafe`, no raw pointers).
fn rv_panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic".to_string()
    }
}

/// Run-mode panic hook: `panic: <message>` on stderr, then exit code 1.
pub fn rv_install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = rv_panic_text(info.payload());
        eprintln!("panic: {}", text);
        std::process::exit(1);
    }));
}

thread_local! {
    static RV_TEST_PANIC: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
}

/// Test-mode panic hook: capture the message and let unwinding continue so the
/// generated runner can report the failure. Never exits the process.
pub fn rv_install_test_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = rv_panic_text(info.payload());
        RV_TEST_PANIC.with(|slot| {
            *slot.borrow_mut() = Some(text);
        });
    }));
}

fn rv_take_test_panic() -> Option<String> {
    RV_TEST_PANIC.with(|slot| slot.borrow_mut().take())
}

/// Run one generated test function; print `test <name> ... ok|FAILED`.
pub fn rv_test(name: &str, test: fn()) -> bool {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
    match outcome {
        Ok(()) => {
            println!("test {} ... ok", name);
            true
        }
        Err(payload) => {
            let text = rv_take_test_panic().unwrap_or_else(|| rv_panic_text(&*payload));
            println!("test {} ... FAILED", name);
            println!("    panicked: {}", text);
            false
        }
    }
}

/// Run one generated `test fn` that returns `Result<(), Error>`:
/// `Ok(())` succeeds, `Err(error)` fails with the error message, a panic fails.
pub fn rv_test_result(name: &str, test: fn() -> Result<(), Error>) -> bool {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
    match outcome {
        Ok(Ok(())) => {
            println!("test {} ... ok", name);
            true
        }
        Ok(Err(error)) => {
            println!("test {} ... FAILED", name);
            println!("    error: {}", error.message);
            false
        }
        Err(payload) => {
            let text = rv_take_test_panic().unwrap_or_else(|| rv_panic_text(&*payload));
            println!("test {} ... FAILED", name);
            println!("    panicked: {}", text);
            false
        }
    }
}

pub fn rv_println(text: &str) {
    println!("{}", text);
}

pub fn rv_print(text: &str) {
    print!("{}", text);
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

pub fn rv_eprintln(text: &str) {
    eprintln!("{}", text);
}

pub fn rv_eprint(text: &str) {
    eprint!("{}", text);
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

pub fn rv_read_line() -> Result<String, Error> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(line)
        }
        Err(error) => Err(Error::new(&format!("cannot read standard input: {}", error))),
    }
}

pub fn rv_args() -> Vec<String> {
    std::env::args().collect()
}

pub fn rv_env_var(name: &str) -> Result<String, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(error) => Err(Error::new(&format!(
            "cannot read environment variable `{}`: {}",
            name, error
        ))),
    }
}

pub fn rv_fs_read_to_string(path: &str) -> Result<String, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) => Err(Error::new(&format!(
            "cannot read file `{}`: {}",
            path, error
        ))),
    }
}

pub fn rv_fs_write_string(path: &str, contents: &str) -> Result<(), Error> {
    match std::fs::write(path, contents) {
        Ok(()) => Ok(()),
        Err(error) => Err(Error::new(&format!(
            "cannot write file `{}`: {}",
            path, error
        ))),
    }
}

pub fn rv_fs_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

pub fn rv_exit(code: i32) -> ! {
    std::process::exit(code)
}

pub fn rv_panic(message: &str) -> ! {
    panic!("{}", message)
}

pub fn rv_assert(condition: bool, file: &str, line: u32) {
    if !condition {
        panic!("assertion failed at {}:{}", file, line);
    }
}

pub fn rv_assert_msg(condition: bool, message: &str, file: &str, line: u32) {
    if !condition {
        panic!("assertion failed at {}:{}: {}", file, line, message);
    }
}

pub fn rv_assert_eq<T: std::fmt::Debug + PartialEq>(left: &T, right: &T, file: &str, line: u32) {
    if left != right {
        panic!(
            "assertion failed: left == right at {}:{}\n  left: {:?}\n right: {:?}",
            file, line, left, right
        );
    }
}

/// `assert_eq` for Ravum text comparisons between two different Rust types
/// (`String` versus `&str`): Rust's `PartialEq` cross-implementations apply.
pub fn rv_assert_eq_cross<A, B>(left: &A, right: &B, file: &str, line: u32)
where
    A: std::fmt::Debug + PartialEq<B> + ?Sized,
    B: std::fmt::Debug + ?Sized,
{
    if left != right {
        panic!(
            "assertion failed: left == right at {}:{}\n  left: {:?}\n right: {:?}",
            file, line, left, right
        );
    }
}
"##;

#[cfg(test)]
mod tests {
    use super::PRELUDE;

    #[test]
    fn prelude_is_embedded_and_safe() {
        assert!(PRELUDE.contains("pub struct Error"));
        assert!(PRELUDE.contains("pub fn rv_assert("));
        assert!(PRELUDE.contains("pub fn rv_assert_eq<"));
        assert!(PRELUDE.contains("pub fn rv_test("));
        assert!(PRELUDE.contains("pub fn rv_test_result("));
        for forbidden in ["unsafe fn", "unsafe {", "transmute(", "*const ", "*mut "] {
            assert!(
                !PRELUDE.contains(forbidden),
                "prelude must not use `{forbidden}`"
            );
        }
        assert!(PRELUDE.ends_with('\n'));
    }

    #[test]
    fn prelude_does_not_weaken_overflow_checks() {
        for forbidden in [
            "wrapping_add",
            "wrapping_sub",
            "wrapping_mul",
            "overflow-checks=off",
            "overflow_checks=off",
        ] {
            assert!(
                !PRELUDE.contains(forbidden),
                "prelude must not weaken overflow checks: {forbidden}"
            );
        }
    }
}
