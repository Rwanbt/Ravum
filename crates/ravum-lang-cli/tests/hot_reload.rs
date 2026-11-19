//! RFC-E11-HOT-RELOAD §4 — "0 perte" for `ravum-lang watch --reload`.
//!
//! Each test drives a real host: events on its standard input, edits of
//! `src/main.rv` on disk, and the host's messages read on its standard error.
//! The final state is printed by the host when its input ends, so every
//! assertion is on the exact state, never on timing.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(240);

const COUNTER: &str = r#"struct State { count: i64 }

fn init() -> State effects(alloc) {
    State { count: 0 }
}

fn step(state: State, line: String) -> State effects(alloc) {
    drop(line);
    State { count: state.count + 1 }
}

fn main() -> () effects(alloc) {
    drop(step(init(), String::from("x")));
}
"#;

struct Session {
    root: PathBuf,
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<String>,
    log: Vec<String>,
    edits: usize,
}

impl Session {
    fn start(tag: &str, source: &str) -> Session {
        let root =
            std::env::temp_dir().join(format!("ravum-hot-reload-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("ravum.toml"),
            "[package]\nname = \"counter\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rv"), source).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .args(["watch", "--reload", "."])
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start ravum-lang watch --reload");
        let input = child.stdin.take();
        let stderr = child.stderr.take().unwrap();
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        let mut session = Session {
            root,
            child,
            input,
            messages,
            log: Vec::new(),
            edits: 0,
        };
        session.wait_for("reload: started");
        session
    }

    fn wait_for(&mut self, needle: &str) -> String {
        loop {
            match self.messages.recv_timeout(WAIT) {
                Ok(line) => {
                    self.log.push(line.clone());
                    if line.contains(needle) {
                        return line;
                    }
                }
                Err(_) => panic!("`{needle}` never came; host said:\n{}", self.log.join("\n")),
            }
        }
    }

    fn events(&mut self, count: usize, text: &str) {
        let input = self.input.as_mut().unwrap();
        for _ in 0..count {
            writeln!(input, "{text}").unwrap();
        }
        input.flush().unwrap();
    }

    /// Replace the source. A trailing comment that grows with each edit makes
    /// the size differ, so the poller sees the change even within one mtime tick.
    fn edit(&mut self, source: &str) {
        self.edits += 1;
        let text = format!("{source}// edit {}\n", "#".repeat(self.edits));
        fs::write(self.root.join("src/main.rv"), text).unwrap();
    }

    fn finish(mut self) -> String {
        drop(self.input.take());
        let line = self.wait_for("reload: final state");
        let status = self.child.wait().unwrap();
        assert!(
            status.success(),
            "host exit {status:?}; log:\n{}",
            self.log.join("\n")
        );
        let _ = fs::remove_dir_all(&self.root);
        line.split("final state ").nth(1).unwrap().to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 10 000 events, 20 compatible edits applied while they flow: the count is exact.
#[test]
fn twenty_compatible_edits_lose_no_event() {
    let mut session = Session::start("zero-loss", COUNTER);
    for _ in 0..20 {
        session.events(250, "tick");
        session.edit(COUNTER);
        session.events(250, "tick");
        session.wait_for("reload: applied (state kept)");
    }
    assert_eq!(session.finish(), "{10000;}");
}

/// A changed `State` without `migrate` is refused; the old module keeps counting.
#[test]
fn a_state_schema_change_is_refused() {
    let mut session = Session::start("schema", COUNTER);
    session.events(10, "tick");
    session.edit(
        &COUNTER
            .replace(
                "struct State { count: i64 }",
                "struct State { count: i64, extra: bool }",
            )
            .replace("State { count: 0 }", "State { count: 0, extra: false }")
            .replace(
                "State { count: state.count + 1 }",
                "State { count: state.count + 1, extra: true }",
            ),
    );
    let refusal = session.wait_for("reload: refused");
    assert!(refusal.contains("state schema changed"), "{refusal}");
    session.events(5, "tick");
    assert_eq!(session.finish(), "{15;}");
}

/// `migrate` takes exactly the current schema: the state is converted (HR-1.7).
#[test]
fn migrate_converts_the_current_state() {
    let mut session = Session::start("migrate", COUNTER);
    session.events(7, "tick");
    let migrated = r#"struct Before { count: i64 }
struct State { total: i64, last: String }

fn init() -> State effects(alloc) {
    State { total: 0, last: String::from("") }
}

fn migrate(old: Before) -> State effects(alloc) {
    State { total: old.count * 10, last: String::from("migrated") }
}

fn step(state: State, line: String) -> State effects(alloc) {
    State { total: state.total + 1, last: line }
}

fn main() -> () effects(alloc) {
    drop(step(init(), String::from("x")));
}
"#;
    session.edit(migrated);
    session.wait_for("reload: applied (state migrated)");
    session.events(2, "after \"quoted\"");
    assert_eq!(session.finish(), "{72;\"after \\\"quoted\\\"\";}");
}

/// A panic in the new `step` restores the state and replays the event on the
/// previous module: no event is lost (HR-1.5).
#[test]
fn a_panicking_step_reverts_without_losing_the_event() {
    let mut session = Session::start("panic", COUNTER);
    session.events(3, "tick");
    session.edit(
        &COUNTER
            .replace(
                "    drop(line);\n",
                "    if line == String::from(\"boom\") {\n        panic(\"boom\");\n    }\n",
            )
            .replace("effects(alloc)", "effects(alloc, panic)"),
    );
    session.wait_for("reload: applied (state kept)");
    session.events(2, "tick");
    session.events(1, "boom");
    session.wait_for("reverted to the previous module");
    session.events(4, "tick");
    assert_eq!(session.finish(), "{10;}");
}

/// An edit that does not compile changes nothing (HR-1.4).
#[test]
fn an_edit_that_does_not_compile_changes_nothing() {
    let mut session = Session::start("broken", COUNTER);
    session.events(4, "tick");
    session.edit(&COUNTER.replace("state.count + 1", "state.count + "));
    session.wait_for("reload: refused: the program has errors");
    session.events(4, "tick");
    assert_eq!(session.finish(), "{8;}");
}

/// Every canonical form (enum variants, `Vec`, `Option`, `f64`, `char`, tuple,
/// array, escaped text) crosses a compatible swap unchanged (HR-2.1).
#[test]
fn a_rich_state_survives_a_compatible_edit() {
    let source =
        include_str!("../../../conformance/cases/types/reload-contract-accepted/src/main.rv");
    let mut session = Session::start("rich", source);
    session.events(1, "a \"b\"");
    session.events(1, "back\\slash");
    session.edit(source);
    session.wait_for("reload: applied (state kept)");
    session.events(1, "c");
    assert_eq!(
        session.finish(),
        "{3;2(\"ok\";);[\"a \\\"b\\\"\";\"back\\\\slash\";\"c\";];x3fe0000000000000;N;(1;t;);[0;0;0;];}"
    );
}
