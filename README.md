<!-- Ravum repository banner -->
![Ravum — Universal language for continuous development](https://raw.githubusercontent.com/Rwanbt/Ravum/main/assets/ravum-banner.jpg)

# Ravum

<p align="center">
  <a href="README_fr.md"><img src="https://img.shields.io/badge/Fran%C3%A7ais-2f81f7?style=for-the-badge" alt="Lire en français"></a>
</p>

**Ravum** is an experimental universal programming language and continuous
development environment. It is designed to shorten the path from a source edit —
human or AI-generated — to a verified, running program through incremental
analysis, targeted compilation and fail-closed preview.

> **Project status — Core 1.0 DRAFT.** A compiler and language server exist and
> support a documented subset. There is no stable release, no external audit and
> no production-readiness claim. The ADRs remain proposals.

## Overview

Ravum is built around nine working invariants: fluidity, performance, memory
safety, honest verification, native AI feedback, real-time profiles,
universality, resource sobriety and end-to-end productivity. They are engineering
requirements, not marketing claims. The full set is maintained in
[`docs/architecture/PROJECT_INVARIANTS.md`](docs/architecture/PROJECT_INVARIANTS.md).

The repository currently contains an experimental Core 1.0 language
implementation and a separate Rust platform. The language is the long-term
target; the platform is an experimental strategy for applying the same
continuous-development ideas to existing Rust code.

## Status

| Component | Current state |
|---|---|
| Language | Core 1.0 **DRAFT** subset; compiler and CLI implemented under `crates/ravum-lang-*` |
| Toolchain | `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`, `doc`, `cache`, `lsp`, `doctor` |
| Targets | Native Windows/Linux; `wasm32-wasip1` through Node.js WASI |
| Conformance | **503 cases: 502 PASS / 0 FAIL / 1 SKIP** on the reference machine; `build_verified: true` |
| Workspace tests | **1,740 passed / 0 failed / 8 ignored** |
| Coverage | **91.65 % lines / 91.32 % regions** (sema + codegen); **90.37 % product code** (1,305 uncovered / 13,548); the 100 % target is **not met** |
| Performance | `own` phase exponent **1.9803 → 0.8372**; `check` at 50,110 lines **602.6 ms** (debug); LSP p95 **19.551 ms** at 1,025 lines and **80.283 ms** at 5,110 lines (release, clean machine) |
| Release | No tag, no release, no signed artifact and no external audit |

> Reference environment: Windows 11 x64, `rustc 1.95.0`, code state of
> 2026-11-15 (production-readiness step E1). These figures are tied to the cited
> commands and revision; they are not portable guarantees. Reproduce them with
> the commands in [Development](#development); the detailed protocol is in
> [`T2-FINDINGS.md` §29](docs/qualification/T2-FINDINGS.md#295-vérification-e1-2026-11-15).

## Quick start

Prerequisites: Rust `1.95.0` (pinned by `rust-toolchain.toml`), `rustc` on
`PATH`, and Node.js only for the WASI target or the editor/browser harnesses.

```bash
cargo build -p ravum-lang-cli --bin ravum-lang
./target/debug/ravum-lang doctor
./target/debug/ravum-lang run examples/hello
```

On Windows the binary is `target\debug\ravum-lang.exe`. A successful run prints
`hello, ravum`.

## Toolchain

### Compiler pipeline

```text
.rv → loader/parser → name resolution → type checking → effects
    → ownership and borrow checking → trap checks → CheckedProgram
    → emit.rs → Rust → rustc 1.95.0
```

The compiler emits Rust and delegates object generation to `rustc`. There is no
hidden interpreter and no fallback that reports success when a stage is
unavailable.

### Command-line interface

```text
ravum-lang init <dir> [--lib] [--force]
ravum-lang check [PATH] [--json] [--profile core|pure|rt] [--rt-entry FN]
ravum-lang build [PATH] [--release] [--target TRIPLE] [--emit-rust FILE]
ravum-lang run   [PATH] [--target TRIPLE] [-- ARGS...]
ravum-lang watch [PATH] [--run] [--release]
ravum-lang test  [PATH] [--filter TEXT] [--json]
ravum-lang fmt   [PATH] [--check]
ravum-lang lint  [PATH] [--deny CODE]...
ravum-lang doc   [PATH] [--out DIR]
ravum-lang cache status|clean [PATH]
ravum-lang lsp
ravum-lang doctor [--json]
```

Paths default to the current directory. A directory must contain `ravum.toml`;
a single `.rv` file is checked as a standalone program. JSON output is available
for `check`, `test` and `doctor` with a versioned schema.

### Language server

`ravum-lang lsp` implements LSP 3.17 over stdio: diagnostics, hover, definition,
references, rename, completion, document symbols and formatting. The client is
covered by integration tests and by the VS Code extension in
[`tools/vscode-ravum`](tools/vscode-ravum).

### Targets and FFI

Native binaries are the default; `wasm32-wasip1` builds run through Node.js WASI.
The C FFI is deliberately small: only a documented scalar ABI is accepted.
`i128` and `u128` are rejected by both the verifier and the backend. Functions
that unwind, call `longjmp`, raise a signal or terminate the process are outside
the contract and remain the caller's responsibility.

## Conformance status

`conformance/cases/run/power-array-move-write-region-nested` was a real
`check → build` divergence: `check` accepted the program, but the backend emitted
plain `a[0]`/`a[1]` accesses and `rustc` rejected them with
`error[E0508]: cannot move out of type [String; 2]`. It is now fixed in the
compiler: array-element moves are lowered at the access instead of being hoisted
through a planner, and the case remains in the regression corpus. The reference
run reports **503 cases: 502 PASS / 0 FAIL / 1 SKIP** and
`build_verified: true`.

`build_verified` covers the build dimension only. The corpus is bounded and does
not prove that no unknown form exists; it is not a formal proof. The
`--verify-build` command, its perimeter and its limits are documented in
[`CONFORMANCE-BUILD-CONTRACT.md`](docs/qualification/CONFORMANCE-BUILD-CONTRACT.md).

## Current limitations

- **Coverage.** Product code is at 90.37 % against a 100 % objective; 1,305 lines
  remain uncovered.
- **No release-grade guarantees.** There is no stable CLI, JSON or LSP contract,
  no signed artifact, no external audit and no Go/No-Go decision.
- **Verification is bounded.** The bounded proof ledger covers explicitly
  enumerated finite domains; there is no general Ravum proof engine and no
  general `PROVED`.
- **Runtime capabilities are partial.** There is no state-preserving hot reload
  and no stable patch ABI. MVP-0d (incrementality and collaboration) is partial.
- **Test-power coverage is partial.** The T2 campaign exercised 9 of 13
  historical forms; 4 inverse forms were not run.
- **Performance is machine-specific.** The published timings come from one
  Windows reference machine. No hard real-time guarantee is claimed.

## Repository layout

| Path | Contents |
|---|---|
| `crates/ravum-lang-*` | Core 1.0 compiler: syntax, semantic analysis, effects, ownership, codegen, LSP and CLI |
| `crates/ravum-*` | Rust platform: daemon, coordinator, patch host, snapshots, DSP harness, proof ledger, full-stack core |
| `conformance/` | Positive and negative `.rv` cases, including the known failing case |
| `spec/language/` | Normative Core 1.0 DRAFT: grammar, ABI, features, core machine |
| `tools/` | Coverage, mutation, registry, MUST, timing and evidence gates |
| `docs/` | Architecture, gate matrix, production readiness, RFCs, qualification reports |
| `examples/` | Compilable `.rv` programs and packages |
| `scripts/` | Smoke, demo, timing and evidence scripts |
| `apps/ravum-todo` | Full-stack reference application used by the Rust platform |
| `.github/workflows/` | Linux/macOS qualification, MVP, coverage, performance, reproducibility, VS Code and conformance CI |

## Architecture

Ravum has two separate implementation lines:

- **Core 1.0 language** (`crates/ravum-lang-*`): parser, resolver, type checker,
  effect profiles, ownership analysis, trap checks, Rust code generation, CLI and
  LSP. This is the path toward a native Ravum toolchain.
- **Rust platform** (`crates/ravum-*`): snapshots, incremental analysis, a
  persistent daemon, transactional Git publication, patch ABI and DSP block
  handling for Rust code. It is useful on its own and serves as a baseline for
  the language work.

The platform is the **F** experimental strategy. A native Ravum backend is the
conditional **D** target; the decision belongs to gate M5 and depends on
reproducible measurements of edit-to-preview time, resource use, enforcement of
guarantees and maintenance cost.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets -- --test-threads=2
cargo run -p ravum-conformance -- run --verify-build
```

The conformance command exits 0 when the corpus and the `check → build`
dimension are both green; a `SKIP` is reported separately and is never counted
as a pass. Focused checks are also available per crate, for example:

```bash
cargo test -p ravum-lang-sema
cargo test -p ravum-lang-codegen
```

The `mvp` CI job builds the workspace, runs the test suite, executes the M1–M4
demos and runs the browser E2E suite. The conformance job runs the
`check → build` corpus.

## Contributing

Development follows a short-lived branch workflow:

1. Branch from `dev`.
2. Keep the change focused and include tests for new behavior and refusals.
3. Run formatting, the relevant tests and, when applicable, the conformance
   corpus.
4. Open a pull request against `dev`. The required `mvp` check must pass.

`main` is the default branch and the documentation reference. `dev` is the
integration branch. Long-lived branches are `main` and `dev`; work branches are
deleted after merge.

Tests, conformance cases, assertions and thresholds are part of the contract.
Do not delete, skip or weaken them to obtain a green result. Report failures with
the command, SHA, toolchain and platform that produced them. Automated
contributions are welcome under the same review rules.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full process and
[AGENTS.md](AGENTS.md) for the rules that apply to AI agents.

## Quality gates

| Status | Gates |
|---|---|
| `PASS` | T0, T3, T5, T7 |
| `PARTIAL` | T1, T2, T6, T8, T9, T10, T11 |
| `BLOCKED` | T4 (external audit), T12 (Go/No-Go) |

Readiness criteria R1–R14 are tracked separately. R4 and R13 are blocked by the
external audit, R12 is deferred, and the other criteria are `PARTIAL` or
`TESTED`. No gate is promoted without a command, a SHA and a recorded context.
The canonical register is the
[gate matrix](docs/roadmap/CORE-1.0-GATE-MATRIX.md); the gap plan is
[production readiness](docs/roadmap/PRODUCTION-READINESS.md).

## Roadmap

| Track | Next milestone |
|---|---|
| Core 1.0 language | Fix the array-move divergence; close T4 (external audit) and T12 (Go/No-Go) |
| Rust platform | Complete the MVP-0d external corpus; add state migration and a stable patch ABI for DSP |
| Quality | Raise product coverage toward 100 %; complete the `emit.rs` mutation campaign |
| Distribution | Produce signed artifacts and verify provenance (R12) |

No delivery date is promised. The target is a documented, fail-closed Core 1.0
subset, not a claim that arbitrary programs are safe or verified.

## Documentation

- [Getting started with Core 1.0 DRAFT](docs/guide/GETTING-STARTED.md)
- [Project invariants](docs/architecture/PROJECT_INVARIANTS.md)
- [Core 1.0 gate matrix](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness and gap plan](docs/roadmap/PRODUCTION-READINESS.md)
- [Latest qualification report](docs/reports/GOAL-WAVE-3-REPORT.md)
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
