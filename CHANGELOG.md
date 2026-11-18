# Changelog

All notable changes to Ravum are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Conventional Commits](https://www.conventionalcommits.org/). There is no
released version yet: every entry below is part of the DRAFT Core 1.0 work.

## [Unreleased]

### Added

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

### Fixed

- Lexer conformance to `lexical.md` (found by the T2 lexical cases): a lone
  `CR`, VT, FF, NEL, U+2028/U+2029 and C0 controls were accepted as blanks
  (now `E-UNSUPPORTED-TOKEN`); `"\xFF"` was accepted (now `E-LEX-ESCAPE`); a
  `\u{D800}` surrogate is `E-LEX-CHAR`; `b'x'`, `r#"…"#` and `1e_10` produced
  parse cascades or the wrong code; an unterminated `'a` was read as a
  lifetime; a malformed escape no longer adds a second diagnostic.
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
