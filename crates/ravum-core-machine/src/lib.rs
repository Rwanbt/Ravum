//! Reference interpreter of the pure core of Ravum Core 1.0.
//!
//! It implements `spec/language/core-machine.md`: values (§2), configurations
//! (§3), evaluation rules (§4), arithmetic and traps (§5), statements (§6) and
//! the step budget (§7). It is written **independently** of the compiler: it
//! has its own lexer, parser and evaluator, and depends on no `ravum-lang-*`
//! crate (MACH-1.3, MACH-8.1). It has no dependency at all, so it needs no
//! network (MACH-9.3).
//!
//! Its results are `TESTED` on a finite corpus, never `PROVED` (MACH-8.5); it
//! is an oracle of results, not of typing or ownership (MACH-8.4). The program
//! is assumed to be accepted by the checker: a construct the machine cannot
//! evaluate is reported as a `fault`, never given a default value (MACH-3.1).

mod ast;
mod eval;
mod lexer;
mod parser;
mod value;

pub use value::Value;

/// Version of the specification this machine implements (MACH-9.2).
pub const SPEC_VERSION: &str = "core-1.0-draft.1";

/// Default step budget (MACH-7.1). Not part of the language (MACH-3.3).
pub const DEFAULT_BUDGET: u64 = 1_000_000;

/// A comparable result (MACH-8.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A final value, with its type and its `to_string` text (STD-12.1,
    /// STD-12.2).
    Value { ty: String, text: String },
    /// `⟨panic: msg⟩`, a terminal state (MACH-5.1, MACH-5.2).
    Panic(String),
    /// `⟨diverge: n⟩`: the budget ran out. Inconclusive, never a proof of
    /// non-termination (MACH-7.2).
    Diverge(u64),
    /// `⟨fault: …⟩`: the machine refuses to produce a result (unbound name,
    /// no matching arm, stack limit, libm, unknown version, outside the pure
    /// core).
    Fault(String),
}

impl Outcome {
    pub fn render(&self) -> String {
        match self {
            Outcome::Value { ty, text } => format!("value {ty} {text}"),
            Outcome::Panic(m) => format!("panic: {m}"),
            Outcome::Diverge(n) => format!("diverge: {n}"),
            Outcome::Fault(m) => format!("fault: {m}"),
        }
    }
}

/// Evaluate `entry()` in `source` under the specification `spec_version`, with
/// at most `budget` steps. `entry` takes no argument and must be declared
/// `effects()` (MACH-1.1).
pub fn run(source: &str, entry: &str, spec_version: &str, budget: u64) -> Outcome {
    if spec_version != SPEC_VERSION {
        // MACH-9.2: an unknown specification version is refused.
        return Outcome::Fault(format!("unknown specification version {spec_version}"));
    }
    let source = source.to_string();
    let entry = entry.to_string();
    // The evaluator recurses on the syntax tree; a dedicated thread with a
    // large stack lets MAX_CALL_DEPTH, not the host stack, decide
    // `fault: stack-limit` (MACH-7.3).
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || run_here(&source, &entry, budget))
        .expect("spawn the machine thread")
        .join()
        .unwrap_or_else(|_| Outcome::Fault("internal error".into()))
}

fn run_here(source: &str, entry: &str, budget: u64) -> Outcome {
    let toks = match lexer::lex(source) {
        Ok(t) => t,
        Err(e) => return Outcome::Fault(format!("outside the pure core: {e}")),
    };
    let items = match parser::Parser::new(toks).program() {
        Ok(i) => i,
        Err(e) => return Outcome::Fault(format!("outside the pure core: {e}")),
    };
    for item in &items {
        if let ast::Item::Func(f) = item {
            // The only effect admitted is `panic`, the terminal state of §3
            // (`⟨panic: msg⟩`): it is how the corpus observes evaluation
            // order (§4 example, O1, O2). Any other effect is outside the
            // pure core.
            if f.effects
                .as_ref()
                .is_some_and(|e| e.iter().any(|x| x != "panic"))
            {
                return Outcome::Fault(format!(
                    "outside the pure core: `{}` declares effects",
                    f.name
                ));
            }
        }
    }
    let mut machine = eval::Machine::new(items, budget);
    match machine.function(entry) {
        None => return Outcome::Fault(format!("unbound {entry}")),
        Some(f) if f.effects.is_none() => {
            return Outcome::Fault(format!(
                "outside the pure core: `{entry}` is not declared `effects()`"
            ))
        }
        Some(_) => {}
    }
    match machine.call(entry, vec![]) {
        Ok(v) => Outcome::Value {
            ty: value::type_name(&v),
            text: value::display(&v),
        },
        Err(value::Stop::Panic(m)) => Outcome::Panic(m),
        Err(value::Stop::Fault(m)) => Outcome::Fault(m),
        Err(value::Stop::Diverge) => Outcome::Diverge(budget),
        Err(_) => Outcome::Fault("control flow escaped a function".into()),
    }
}
