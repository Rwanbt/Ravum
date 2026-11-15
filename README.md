<!-- Ravum repository banner -->
![Ravum — Universal language for continuous development](https://raw.githubusercontent.com/Rwanbt/Ravum/main/assets/ravum-banner.jpg)

# Ravum

<p align="center">
  <a href="README_fr.md"><img src="https://img.shields.io/badge/Language-Fran%C3%A7ais-2f81f7?style=for-the-badge" alt="Lire en français"></a>
</p>

> **Research and engineering project — Core 1.0 DRAFT, not published.**
> An experimental Ravum compiler exists and runs a documented Core 1.0 subset:
> lexer/parser, name resolution, type checking, effect profiles, ownership and
> borrow checking, trap checks, Rust code generation, CLI, LSP, conformance
> corpus, native Windows/Linux and `wasm32-wasip1` targets. It is **not stable,
> not externally audited and not production-ready**. The ADRs remain
> **PROPOSED**, and there is no tag, release or signature. The canonical gate
> register is [`docs/roadmap/CORE-1.0-GATE-MATRIX.md`](docs/roadmap/CORE-1.0-GATE-MATRIX.md).

**Ravum** is an exploratory **universal programming language** and **continuous
development environment**. The long-term goal is to shorten the time from an
edit — human or AI-generated — to a verified, running deliverable, across
frontend, backend, real-time DSP, native applications and embedded systems.

The project explores a fail-closed loop: an edit triggers targeted analysis,
verification, focused compilation and a refreshed preview. If a change is
incomplete or incorrect, the last valid version stays active. Claims must be
backed by a command, a SHA, a toolchain and a platform; they are never inferred
from a green demo.

## Founding principles (P0)

| ID | Non-negotiable objective |
|---|---|
| **P0-A** | **Fluidity** — fast feedback while typing or generating with AI; incremental compilation and preview. |
| **P0-B** | **Performance** — native, low overhead, compact code, SIMD, multi-core, no mandatory GC. |
| **P0-C** | **Memory safety** — ownership and borrows, explicit `unsafe` boundaries. |
| **P0-D** | **Honest verification** — contracts, targeted proofs, deterministic tests and explicit statuses. |
| **P0-E** | **Native AI** — structured diagnostics, counterexamples, contract preservation and automatable repair. |
| **P0-F** | **Real time** — verifiable effect/resource profiles, especially for audio DSP and VST3/CLAP plugins. |
| **P0-G** | **Universality** — web, server, database, desktop, mobile, embedded, systems tooling, compute/GPU. |
| **P0-H** | **Sobriety** — a permanent compiler with CPU/RAM budgets; initial 16 GB reference machine. |
| **P0-I** | **Global productivity** — measure time to a **verified deliverable**, not only compilation microbenchmarks. |

“Universal” does **not** mean automatically proving every program, guaranteeing
zero bugs or crashes, migrating arbitrary structures across a hot swap, or
promising hard real-time deadlines on every OS/CPU. Guarantees must state their
assumptions, versions, platforms and trust boundaries. The statuses `TESTED`,
`ASSUMED`, `UNKNOWN`, `PENDING`, `STALE`, `TIMEOUT` and `UNSUPPORTED` are never
presented as `PROVED`.

## Strategy: learn before freezing

**F — experimental strategy:** build a small usable platform on top of existing
Rust: immutable snapshots, diagnostics, orchestration, controlled ABI patching
and metrology. It must deliver standalone value.

**D — conditional target architecture:** if experiments justify it, develop
Ravum's own semantics, verifiable IRs and native backends. The instrumented Rust
project remains the baseline and a legitimate alternative.

The decision to build an autonomous frontend belongs to the **M5** gate, based on
reproducible results: incremental definition compilation, edit-to-preview time,
CPU/RAM costs, AI correction on new syntax, guarantees actually enforced and
maintenance cost.

## Status at a glance

As of **2026-11-15**, the repository keeps only two branches: `main` and `dev`.
They are synchronized after the wave-3 merge (PRs #111–#113), and no work pull
request is left open.

| Area | Current state |
|---|---|
| Language | Core 1.0 **DRAFT** subset; compiler and CLI implemented under `crates/ravum-lang-*` |
| Pipeline | `.rv` → loader/parser → resolution → type check → effects → ownership → traps → `CheckedProgram` → `emit.rs` → Rust → `rustc 1.95.0` |
| CLI | `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`, `doc`, `cache`, `lsp`, `doctor` |
| Targets | Native Windows/Linux; `wasm32-wasip1` through Node.js WASI |
| Conformance | **487 cases: 485 PASS / 1 FAIL / 1 SKIP** on the reference machine; the failure is one known, intentionally red case; `build_verified: false` |
| Workspace tests | **1,732 passed / 0 failed / 8 ignored** |
| Coverage | **91.65 % lines / 91.32 % regions** (sema + codegen); **90.37 % product code** (1,305 uncovered / 13,548); the 100 % S5 target is **not met** |
| Performance | `own` phase exponent **1.9803 → 0.8372**; `check` at 50,110 lines **602.6 ms** (debug); LSP p95 **19.551 ms** at 1,025 lines and **80.283 ms** at 5,110 lines (release, clean machine) |
| Gates | `PASS`: T0, T3, T5, T7 — `PARTIAL`: T1, T2, T6, T8, T9, T10, T11 — `BLOCKED`: T4, T12 |
| Release | No tag, release, signed artifact, external audit or Go/No-Go decision |

> Reference figures: Windows 11 x64, `rustc 1.95.0`, code commit `5fd54b8`
> (2026-11-14), merged as `400336b` into `dev` and `20e65d2` into `main` on
> 2026-11-15. The first Linux CI run after the wave-3 merge measured
> 484 PASS / 1 FAIL / 2 SKIP; the FAIL is the same known case and the extra
> skip is environment-dependent. These figures are local and CI-specific; they
> are not portable guarantees. See the
> [wave-3 report](docs/reports/GOAL-WAVE-3-REPORT.md).

## What actually works

- **Compile and run real programs.** `.rv` packages with `ravum.toml`,
  multi-file examples, `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`,
  `doc`, `cache` and `doctor` all execute the real pipeline. A frontend stage
  that is still incomplete reports a stable diagnostic and exits non-zero; it
  never reports a false success. The CLI documents this explicitly.
- **A real language server.** LSP 3.17 over stdio: diagnostics, hover,
  definition, references, rename, completion, symbols and formatting. It is
  exercised by an integrated client test and by a real VS Code session through
  the `tools/vscode-ravum` extension.
- **A constrained FFI boundary.** Only a documented scalar C ABI is accepted;
  `i128` and `u128` are refused by **both** the verifier and the backend.
  Functions that unwind, `longjmp`, raise a signal or terminate the process are
  outside the contract (C-FFI-4). `unsafe` is confined and audited.
- **Reproducible evidence tooling.** The conformance runner checks the
  `check → build` contract, the smoke suite exercises examples, and the
  `tools/` scripts audit coverage, mutation outcomes, registry codes, feature
  traceability (`188/188`), MUST coverage (`809 / 465 / 344`) and `unsafe`.
- **The Rust platform line.** The separate `ravum-*` crates provide the
  experimental platform for Rust code: incremental analysis, a persistent
  daemon, transactional Git publication, hot patch ABI, DSP block handling and
  the full-stack `ravum-todo` reference app. MVP-0a/0b/0c are integrated;
  MVP-0d is partial.

## Known red conformance case

> **The conformance gate is not green, and this is deliberate.**
>
> `conformance/cases/run/power-array-move-write-region-nested` records a real
> `check → build` divergence. `check` exits 0; `build` exits 1 with
> `error[E0508]: cannot move out of type [String; 2]`. The case is kept red
> until the array move planner is fixed. It is a **result**, not a gate
> incident, and it is neither skipped nor deleted.
>
> On the wave-3 merge tree (`400336b`), the required `mvp` check, coverage,
> gate contracts, documentation and VS Code jobs pass. The Linux, Windows and
> macOS conformance jobs fail on this single case, and `Qualification evidence`
> fails as a consequence. No assertion, test case or threshold was weakened to
> obtain green.

## Quick start

Prerequisites: Rust `1.95.0` (pinned by `rust-toolchain.toml`), `rustc` on
`PATH`, and Node.js only for the WASI target or the VS Code/browser harnesses.

```bash
cargo build -p ravum-lang-cli --bin ravum-lang
./target/debug/ravum-lang doctor
./target/debug/ravum-lang run examples/hello
```

On Windows the binary is `target\debug\ravum-lang.exe`.

Other useful commands:

```bash
./target/debug/ravum-lang check examples/calculator
./target/debug/ravum-lang test examples/tests-demo
./target/debug/ravum-lang lsp

cargo test --workspace --all-targets -- --test-threads=2
cargo run -p ravum-conformance -- run --verify-build
```

The conformance command exits 1 while the known red case exists: the current
reference result is 487 cases, 485 PASS / 1 FAIL / 1 SKIP, with
`build_verified: false`. The FAIL is expected and documented above.

## Repository layout

| Path | Contents |
|---|---|
| `crates/ravum-lang-*` | Core 1.0 compiler: syntax, semantic analysis, effects, ownership, codegen, LSP and CLI |
| `crates/ravum-*` | Rust platform line: daemon, coordinator, patch host, snapshots, DSP harness, proof ledger, full-stack core |
| `conformance/` | Positive and negative `.rv` cases, including the intentionally red divergence |
| `spec/language/` | Normative Core 1.0 DRAFT: grammar, ABI, features, core machine |
| `tools/` | Coverage, mutation, registry, MUST, timings and evidence gates |
| `docs/` | Decisions, gate matrix, production readiness, RFCs, architecture, qualification reports |
| `examples/` | Compilable `.rv` programs and packages |
| `scripts/` | Smoke, demo, timing and evidence scripts |
| `apps/ravum-todo` | Full-stack reference application used by MVP-0a/0b/0c |
| `.github/workflows/` | Linux/macOS qualification, MVP, coverage, performance, reproducibility, VS Code and conformance CI |

## Gates, readiness and evidence

The gate matrix is the single source of truth for T0–T12. The readiness document
tracks R1–R14 against the decided D2 target (“production ready”). A `PASS` is
never written without an executed command, a SHA and a reproducible context;
`SKIP` and `NOT_RUN` are never converted into `PASS`, and no general `PROVED`
exists.

| Status | Gates |
|---|---|
| `PASS` | T0, T3, T5, T7 |
| `PARTIAL` | T1, T2, T6, T8, T9, T10, T11 |
| `BLOCKED` | T4 (external audit), T12 (Go/No-Go) |

Wave 3 changed no gate verdict. R2 is `PARTIAL` because the divergence above is
a known open case; R4 and R13 are `BLOCKED` by the external audit; R12 is
`DEFERRED` by the signing-key decision; the other readiness criteria are
`PARTIAL` or `TESTED`.

Key evidence:

- [Gate matrix T0–T12](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness R1–R14](docs/roadmap/PRODUCTION-READINESS.md)
- [Wave-3 campaign report and figures](docs/reports/GOAL-WAVE-3-REPORT.md)
- [Owner decisions of 2026-11-14](docs/decision/DECISIONS-2026-11-14.md)
- [Human decisions still required](docs/reports/HUMAN-DECISIONS-REQUIRED.md)
- [T2 power-test findings](docs/qualification/T2-FINDINGS.md)
- [LSP qualification](docs/qualification/LSP.md)
- [Coverage objective and gaps](docs/qualification/COVERAGE-100-OBJECTIVE.md)

## Roadmap

| Track | Now | Next decision or proof |
|---|---|---|
| Core 1.0 language | DRAFT compiler, LSP, conformance and Rust backend in `main`/`dev` | Close T4 (independent audit) and T12 (Go/No-Go); fix the known array-move divergence |
| Rust platform line | MVP-0a/0b/0c integrated; MVP-0d partial | External project corpus for MVP-0d; state migration and a formal patch ABI for DSP |
| Quality | Coverage 90.37 % product; partial mutation campaign | Ratify or reject 14 coverage exemptions; complete the `emit.rs` mutation campaign |
| Trust | Reproducible builds on two runners; Linux installer contract | Signed artefacts (D9); external audit (D4) |

No delivery date is promised. The language target is a documented, fail-closed
Core 1.0 subset — not a claim that arbitrary programs are safe or verified.

## Still waiting on humans

Open decisions: **D2** (ratify the normative base and RFC-D2), **D4**
(independent audit), **D9** (signing key), **T12** (Go/No-Go),
**D-2026-11-14-6** and **D-2026-11-14-7**.

Wave 3 also leaves these decisions: keep the known red case in the gate or hold
it outside until fixed; ratify or reject the 14 coverage exemptions (140 lines);
authorize an `OwnFacts` schema extension for the five blocked families; decide
the full `emit.rs` mutation campaign in CI; decide the LSP runner capacity; and
update the stale current-figure documents. The pre-existing MUST debt is 344
unmapped statements (two unescaped `|` lines remain in the spec).

## Branches and contributions

- **`main`** is the default branch and documentation reference, not a maturity
  claim. **`dev`** is the integration branch. New work starts from `dev`, goes
  through a PR to `dev`, and short-lived branches are deleted after merge.
- `mvp` is a required check on both branches. Force pushes and branch deletion
  are disabled on `main` and `dev`.
- Do not delete, skip or weaken tests, conformance cases, assertions or
  thresholds to obtain green. Report failures honestly.
- AI proposals are welcome, but no agent accepts an ADR, changes a normative
  contract or weakens a gate without an explicit human decision.
- See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

## Documentation

Status pages are dated. Where a figure is duplicated, the gate matrix and the
wave-3 report prevail over older status pages.

- [Getting started with Core 1.0 DRAFT](docs/guide/GETTING-STARTED.md)
- [Architecture status — two separate lines](docs/architecture/STATUS.md)
- [Core 1.0 gate matrix](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness and gap plan](docs/roadmap/PRODUCTION-READINESS.md)
- [Wave-3 campaign report](docs/reports/GOAL-WAVE-3-REPORT.md)
- [Owner decisions of 2026-11-14](docs/decision/DECISIONS-2026-11-14.md)
- [Human decisions required](docs/reports/HUMAN-DECISIONS-REQUIRED.md)
- [RFC index](docs/rfc/README.md)
- [Original ADR register](docs/adr/README.md) — all proposed
- [MVP-0a/0b/0c integration](docs/mvp/INTEGRATION-0abc.md)
- [MVP-0d report](docs/mvp/MVP-0d-REPORT.md)
- [Contributing](CONTRIBUTING.md)
- [AI agent mandate](AGENTS.md)

## License

**MIT OR Apache-2.0**, at the recipient's choice. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). This dual
license covers original contributions whose authors hold the rights; third-party
components, texts and assets remain under their own licenses. Programs written
with Ravum do not automatically inherit the compiler's license merely by using
the language or the toolchain.
