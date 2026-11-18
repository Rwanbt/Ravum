# Changelog

All notable changes to Ravum are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Conventional Commits](https://www.conventionalcommits.org/). There is no
released version yet: every entry below is part of the DRAFT Core 1.0 work.

## [Unreleased]

### Added

- Compatibility contract (T2, `compatibility.md`): FeatureIds F-META-019 to F-META-023, 6 conformance cases, `tests/compat_contract.rs` (digest follows a dependency path, an invalid lockfile is left untouched) and append-only snapshots of rule ids and feature ids. COMPAT-3.6 now names `W-DEPRECATED` and COMPAT-4.2 `E-MOD-DUPLICATE`, as the registry does.
- `run --json` (DIAG-8.1, D-18-5): one JSON object on stdout with the program's exit code, `summary.executed` and `summary.program_exit_code`; the program's own stdout goes to stderr.

- Diagnostics contract (T2, `diagnostics.md`): FeatureIds F-META-009 to F-META-017, 13 conformance cases, `tests/diagnostics_contract.rs` (byte-identical runs, per-phase order, no colour, no absolute path, one-line JSON) and an append-only snapshot of published diagnostic codes (`tools/diagnostic_codes.snapshot`).

- Core 1.0 DRAFT compiler and toolchain under `crates/ravum-lang-*`:
  `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`, `doc`, `cache`,
  `doctor` and an LSP 3.17 server.
- Declarative conformance corpus with a blocking `check => build` contract
  (`ravum-conformance run --verify-build`).
- Native Windows/Linux and `wasm32-wasip1` targets; a documented scalar C FFI.
- Experimental Rust platform crates (`ravum-*`), explicitly outside the Core 1.0
  promise.
- Supply-chain and governance guard-rails: blocking Clippy, `cargo-deny`,
  weekly Dependabot updates, `CODEOWNERS` and this changelog.
- `W-DEPRECATED` (DIAG-1.10): a use of a deprecated standard-library path is
  reported with its version and replacement and stays valid. The register is
  empty in Core 1.0.
- DIAG-1.9: a binding whose name starts with `_` is never reported unused.

### Fixed

- An operand already in error no longer produces a second `E-TYP-MISMATCH` through a binary operator (`y + 1` with `y` unknown reported `<error>`; DIAG-4.2).

- Lexer conformance to `lexical.md` (found by the T2 lexical cases): a lone
  `CR`, VT, FF, NEL, U+2028/U+2029 and C0 controls were accepted as blanks
  (now `E-UNSUPPORTED-TOKEN`); `"\xFF"` was accepted (now `E-LEX-ESCAPE`); a
  `\u{D800}` surrogate is `E-LEX-CHAR`; `b'x'`, `r#"…"#` and `1e_10` produced
  parse cascades or the wrong code; an unterminated `'a` was read as a
  lifetime; a malformed escape no longer adds a second diagnostic.
- Parser conformance to `grammar.md` (found by the T2 grammar cases): `pub
  impl` and `pub extern` were accepted (now `E-PARSE-EXPECTED`); a token that
  cannot start an item is `E-PARSE-EXPECTED`; a match guard is
  `E-UNSUPPORTED-TOKEN` and an empty arm `E-PARSE-EXPECTED`; an item nested in
  a function body, `-> {` and a refused `?` no longer produce cascades.
- Diagnostic columns counted UTF-8 bytes; they now count UTF-16 code units
  (DIAG-5.2, DIAG-5.6, LEX-9.3), in JSON and in `file:line:col`.

### Changed

- Array-element moves are lowered at each access instead of being hoisted by a
  planner. `MovePlanner` is removed; `rustc` now checks every access, so a
  checker/backend disagreement becomes a compile error rather than a stale
  value.
- `rust-version` now names the toolchain actually tested (`1.95.0`).
- The backend no longer re-derives ownership decisions (RFC-OWN-FACTS-2, steps
  C2–C5). The checker publishes `owned_position`, `consumed` and
  `binding_identity`; the emitter reads them, and its own copy/move classifier
  (`is_copy_type`, `is_owned_position`, `consumed_by_checker`) is removed.
  Generated Rust is byte-identical on the examples and the conformance corpus.

### Fixed

- `run/power-array-move-write-region-nested`: `check` accepted the program but
  `build` emitted a plain `a[0]`/`a[1]` and `rustc` rejected it with `E0508`.
  The case remains in the corpus and now passes.
- Two array parameters of one function are distinct bindings for the
  array-element lowering: moving `a[0]` no longer lowers reads of an unrelated
  parameter `b` as if `b` were partially moved.
- `OWN-5.11`/`OWN-5.12`: element re-initialisation and non-literal indexes are
  now expressed with rules the backend can preserve exactly.

### Security

- `cargo-deny` rejects unknown registries, unknown Git sources, duplicate
  dependency versions and licenses outside the approved set.
- The generated Rust keeps `#![forbid(unsafe_code)]`; the only audited `unsafe`
  is the scalar FFI wrapper.
