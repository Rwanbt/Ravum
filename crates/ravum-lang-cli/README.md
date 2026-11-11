# `ravum-lang` — Ravum Core 1.0 toolchain (CLI worker scope)

This crate owns the `ravum-lang` binary. The package/module loader lives in
`crates/ravum-lang-sema/src/loader.rs` (owned by the same worker). The syntax,
checker, ownership and codegen workers own the other files.

## Build

```text
cargo build -p ravum-lang-cli
cargo test  -p ravum-lang-cli
```

The binary is `target/debug/ravum-lang` (`ravum-lang.exe` on Windows). It uses
`serde`/`serde_json` at runtime and shells out to `rustc` for `build`, `run` and
`test` (`RAVUM_RUSTC` overrides the executable name).

## Commands and honest status

| Command | Status |
|---|---|
| `--version`, `--help`, `help [CMD]` | implemented |
| `doctor [--json]` | implemented; probes parser/checker/codegen and prints ready/pending/error |
| `init <dir> [--lib] [--force]` | implemented |
| `check [PATH] [--json] [--profile core\|pure\|rt] [--locked]` | implemented |
| `build [PATH] [--release] [--target TRIPLE] [--emit-rust FILE] [--emit-ir]` | implemented (needs `rustc`) |
| `run [PATH] [--target TRIPLE] [-- ARGS...]` | implemented (needs `rustc`; `wasm32-wasip1` needs Node.js) |
| `watch [PATH] [--run] [--release]` (alias `dev`) | implemented; polling restart loop (see `docs/CLI-WATCH-WASM.md`) |
| `test [PATH] [--filter S] [--json]` | implemented (needs `rustc`) |
| `fmt [PATH] [--check]` | implemented |
| `lint [PATH] [--deny CODE]...` | implemented; reports checker warnings (`W-*`) only |
| `doc [PATH] [--out DIR]` | implemented from the AST |
| `cache status\|clean [PATH]` | implemented |
| `lsp` | **UNSUPPORTED** (exit 3); listed under UNSUPPORTED in `--help` |

If a frontend stage were still a stub, it would report its own internal
diagnostic (`E-INTERNAL-*`) and the command would exit 1: the CLI never
fabricates success. `doctor` makes the current stage status visible
(`frontend.parser/checker/codegen = ready|pending|error`).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | success |
| `1` | compilation/diagnostic/test failure, or a safe operational refusal (non-empty `init` target, lockfile drift under `--locked`, build failure) |
| `2` | invalid command line (unknown command/option, missing argument, invalid `--profile`, invalid path) |
| `3` | external tool missing (`rustc`) or explicitly UNSUPPORTED command (`lsp`) |

`run` forwards a normal child exit code (`0..=255`). Where the parent observes
the full 32-bit status (Windows), a value above `255` is truncated to the low 8
bits as POSIX shells do, except that a non-zero status which would truncate to
`0` (e.g. `256`) is reported as `1` — a failure is never turned into a success by
truncation, and stderr names the original status.

On POSIX the kernel truncates *before* the parent looks: `waitpid` reports
`_exit(300)` as `44` and `_exit(256)` as `0`. The CLI therefore forwards exactly
what the OS reports and prints no truncation note — naming the original value
would be a guess, not an observation. A program exiting `256` is consequently
indistinguishable from success for every parent process on Linux and macOS; that
is a platform limitation, not a Ravum one, and the two halves of the contract are
pinned separately in `tests/exit_codes.rs` (`cfg(windows)` / `cfg(unix)`).

On Windows, `ExitStatus::code()` returns exception codes
as negative `i32` (an access violation is `-1073741819` = `0xC0000005`); such a
child exits `1` and stderr names the exact exception code. A killed child is
reported as 1. `--json` never changes the exit code.

Manual reproduction note for a genuine exception: a native dependency reached
through `extern` that dereferences a NULL pointer terminates the host process
with `STATUS_ACCESS_VIOLATION` (`0xC0000005`); `run` then exits `1` and prints
`exception 0xC0000005 (access violation)`. No such program is shipped because
the safe Core tests cannot produce a real crash; the mapping itself is covered by
unit tests plus an end-to-end run of a program that chooses the same status with
`std::process::exit(-1073741819)`.

## JSON output (`schema_version: 1`)

`check`, `test` and `doctor` accept `--json`. Exactly one JSON object is written
to stdout, on one line, with no colour and no other text. Usage errors (exit 2)
still go to stderr.

```json
{
  "schema_version": 1,
  "tool": "ravum-lang",
  "tool_version": "0.1.0",
  "spec_version": "core-1.0-draft.1",
  "command": "check",
  "status": "failure",
  "exit_code": 1,
  "success": false,
  "target": {"kind": "package", "path": "hello"},
  "profile": "core",
  "dependencies_digest": "sha256:…",
  "lockfile_stale": false,
  "summary": {},
  "diagnostics": [
    {
      "code": "E-TYP-MISMATCH",
      "severity": "error",
      "phase": "types",
      "message": "…",
      "file": "src/main.rv",
      "line": 4,
      "column": 18,
      "end_line": 4,
      "end_column": 22,
      "primary": {
        "file": "src/main.rv",
        "start_byte": 40, "end_byte": 44,
        "start_line": 4, "start_col": 18, "end_line": 4, "end_col": 22,
        "start_utf16": 40, "end_utf16": 44
      },
      "labels": [],
      "notes": []
    }
  ]
}
```

* The envelope follows `spec/language/diagnostics.md` §7 (`tool`,
  `tool_version`, `spec_version`, `status`, `exit_code`, `target`, optional
  `profile`/`dependencies_digest`/`lockfile_stale`) and keeps the compatibility
  fields `success` and the flat `file/line/column/end_line/end_column` on each
  diagnostic. Unknown fields must be ignored by consumers; removing a required
  field would bump `schema_version`.
* `status` is `success`, `failure` or `unsupported` (the latter for `lsp`).
* `file` is `null` and positions are `0` when a diagnostic has no source span
  (manifest/lockfile/toolchain messages). Paths are package-relative where
  possible and always use `/`.
* `target.path` is relative to the invocation directory when possible and falls
  back to the final component; absolute machine paths are never emitted.
* Diagnostics are deterministically ordered (file id, start, end, code, message)
  and de-duplicated.
* `check`/`lint` summary keys: `package`, `crate`, `profile`, `files` (`.rv`
  files loaded, including dependency sources), `items`, `functions`, `tests`,
  `errors`, `warnings`, `effects` (`[{name, path, effects, declared, is_test}]`),
  `dependencies` (`[{name, version, digest}]`), `dependencies_digest`,
  `lockfile` (`created|current|stale|missing|invalid|not_applicable`).
* `test` summary keys: `total` (tests executed), `declared` (tests the checked
  program declares), `passed`, `failed`, `ignored`, `filtered`, `filter`,
  `exit_code` (raw runner status, may be negative/`null`), `validation_errors`
  (empty on a consistent run), `tests` (`[{name, status}]`), and `error` on
  toolchain failure. `test` refuses to report a partial run as complete: it
  validates the runner's counts and final summary against the declared tests and
  the process exit code, and rejects unparsable result lines.
* `doctor` summary keys: `environment`, `frontend`, `checks`, `verdict`.

Warnings do not fail `check`. `lint --deny CODE` (or `--deny all`) turns a
matching warning into a failure. Only the canonical warning list is reported
(`W-RES-UNUSED-IMPORT`, `W-OWN-UNUSED-VARIABLE`); no heuristic rule is invented.

## Loader policy (`ravum-lang-sema/src/loader.rs`)

### `ravum.toml` (strict TOML subset, no external `toml` crate)

```toml
[package]
name = "app"            # [a-z][a-z0-9_-]*
version = "0.1.0"       # MAJOR.MINOR.PATCH, decimal, no leading zero, no pre-release
edition = "core-1.0"    # the only accepted edition

[dependencies]
util = { path = "../util" }   # only form accepted (local path, one `path` key)
```

* Only `[package]`, `[dependencies]` and `key = "string"` / one-line
  `key = { inner = "string" }` are parsed. Arrays, multiline values, booleans,
  numbers and dotted keys are rejected.
* Unknown section/key: `E-MOD-MANIFEST`. `[build]`/`[scripts]`:
  `E-MOD-BUILD-SCRIPT`. `[dependencies.x]` table form: `E-MOD-DEPENDENCY`.
* Missing/malformed `name`, `version`, `edition` or unknown edition:
  `E-MOD-MANIFEST` (missing/malformed fields, unknown keys) or
  `E-UNSUPPORTED-EDITION` (edition other than `core-1.0`).
* There is no registry, no version range, no build script, no code execution
  while resolving imports.

### Dependencies and paths

* `path` is relative to the package directory, has at most 8 components,
  and must resolve **inside the parent directory** of the package. `../util` is
  legitimate (sibling dependency, `spec/language/modules.md` Q-MOD-2);
  `../../outside`, absolute paths and Windows prefixes are
  `E-MOD-PATH-ESCAPE`.
* Every traversed component is inspected with `symlink_metadata`; symbolic links
  are refused (`E-MOD-PATH`), never followed.
* Reserved Windows path components are refused even if the local filesystem
  accepts them: `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or
  without extension, and any component with a trailing dot or space
  (`E-MOD-RESERVED-NAME`).
* The target must exist, be a directory, contain `ravum.toml`, be named like the
  dependency key, and expose `src/lib.rv`; malformed dependencies (including
  name mismatch and missing library) are `E-MOD-DEPENDENCY`. A binary
  dependency is not importable.
* Graph depth is bounded by `MAX_PACKAGE_DEPTH = 8` and dependency `path` by
  `MAX_DEPENDENCY_PATH_DEPTH = 8` (`E-MOD-PATH-ESCAPE`); cycles including
  self-dependencies are refused
  (`E-MOD-CYCLE`); two different packages with the same name are
  `E-MOD-DUPLICATE`.
* Dependency package names must be usable as module identifiers; a name with
  `-` is refused because `use my-util::x` cannot be lexed as an identifier in
  Core 1.0. Names that are reserved Windows components are
  `E-MOD-RESERVED-NAME`.
* Loaded dependency sources are merged into the crate root as public modules
  named after the dependency, because the frozen `sema::check(program, sources,
  options)` interface accepts one `Program`. Cross-package *visibility*
  (importing an undeclared dependency is `E-RES-DEPENDENCY-UNDECLARED`) remains
  the checker's responsibility; the loader provides the definitions.

### Crate roots, single files and modules

* `src/main.rv` (bin) **or** `src/lib.rv` (lib): a package with both is refused
  (`E-MOD-BOTH-ROOTS`) — no silent choice, and `main.rv` importing the package
  library is UNSUPPORTED (`E-UNSUPPORTED-SELF-LIB`, checker-side).
* A single `.rv` file is a standalone program (`crate = "file"`, name = file
  stem, no manifest, no lockfile, dependencies empty). A directory without
  `ravum.toml` containing exactly one top-level `.rv` file is treated the same
  way (convenience for `check --json` on one script); two or more `.rv` files
  without a manifest are `E-MOD-MANIFEST`.
* `mod foo;` resolves `dir(F)/foo.rv` then `dir(F)/foo/mod.rv`, where `dir(F)`
  is the directory of the file containing the declaration. For an inline module
  `mod m { mod x; }` in file `F`, `x` resolves under `dir(F)/m/`.
  Both files present: `E-MOD-AMBIGUOUS`; neither: `E-MOD-MISSING`.
* Module files must stay under the package `src/` (`E-MOD-PATH-ESCAPE`);
  symlinks are refused (`E-MOD-PATH`).
* The AST is a tree: a file included twice in one crate is `E-MOD-DUPLICATE`
  (this is also how a module self-cycle surfaces). `mod` declarations are
  expanded into inline `ast::Module` values; the checker never sees an
  `ItemKind::ModDecl`.
* `LoadedPackage.program` is `Some` only when no loader/syntax error was
  emitted; the CLI still reports all diagnostics. `lib_program` is always
  `None` now that both roots are refused (kept for interface evolution).

### `ravum.lock`

* Deterministic, sorted by `(name, version)`, one `[[package]]` entry per
  resolved package **including the root package**; `lockfile_version = 1`.
* A package digest is the SHA-256 of `path\0len\0bytes` for every `.rv` file
  under `src/` (sorted by relative path) plus `ravum.toml`, with `\r\n`
  normalised to `\n`. The digest is printed as `sha256:<hex>`.
* `dependencies_digest` is the SHA-256 aggregate of the dependency entries
  (name, version, digest) sorted by name; it excludes the root package and is
  `sha256:e3b0c44…` (SHA-256 of the empty input) when there is no dependency.
* `check`/`build` create the lockfile when absent and rewrite it when stale.
  Staleness is reported through the command's `lockfile`/`lockfile_stale` state,
  not as a warning (the canonical warning list is closed). `--locked` refuses a
  missing lockfile (`E-MOD-MISSING`) or a different one (`E-MOD-LOCK`). An
  unreadable lockfile or unknown `lockfile_version` is `E-MOD-LOCK`
  (fail-closed).
* No URL, no registry checksum, no signature: local content only.

### Limits

`MAX_FILE_SIZE = 8 MiB`, `MAX_SOURCE_FILES = 200` per load (the parser reserves
the high byte of `NodeId` for the `FileId`), `MAX_PACKAGE_FILES = 4096` for the
digest, `MAX_LOCKFILE_SIZE = 1 MiB`. Non-UTF-8 sources are `E-IO-INVALID-UTF8`.

## Build, test and cache

* Generated Rust: `<package>/.ravum/gen/main.rs` (tests: `gen/tests.rs`);
  `--emit-rust FILE` writes the generated file at the explicit path instead.
* Binaries: `<package>/.ravum/bin/<name>[.exe]`; library packages build with
  `--crate-type lib` into `<package>/.ravum/lib/<name>.rlib`. Test runners are
  `<package>/.ravum/bin/<name>-tests[.exe]`.
* Invocation: `rustc --edition 2021 -C overflow-checks=on -C debuginfo=2
  -C opt-level=0|3 [--crate-type lib] -o <out> <generated>`; raw `rustc` errors
  are printed unchanged and exit 1. A missing `rustc` exits 3.
* `--emit-ir` prints a readable dump of the checked facts (functions, types,
  effects, constants, dependencies). It is explicitly **not** a stable IR.
* `--target wasm32-wasip1` (build/run) writes `.ravum/bin/<name>.wasm` and runs
  it through a generated Node.js WASI preview1 launcher; see
  `docs/CLI-WATCH-WASM.md` for prerequisites and limits (no preopened host
  directory, no threads/FFI, Node.js 22+).
* `run` executes from the package root and forwards args (after `--`), stdin,
  stdout, stderr and the exit code.
* `test` builds the generated runner, parses `test <name> ... ok|FAILED|ignored`,
  prints the runner output and then **one** accounting line: the emitted runner's
  own `test result: ok|FAILED; P passed; F failed; I ignored; K filtered out` is
  the primary output, followed by `test cases: D declared; K filtered out; E
  executed`. (With a runner that prints no `test result:` line, the CLI prints
  `test result: ...` itself, so there is never a duplicate.) Before claiming
  success the run is validated: declared tests must equal executed + filtered,
  the runner's final summary must agree with the parsed verdicts and the exit
  code, and result-looking lines that cannot be parsed are a failure. A non-zero
  runner status, a partial run or any `FAILED` line exits 1;
  `test fn -> Result<(), Error>` returning `Err` is a runner failure.
  `--filter <S>` is forwarded to the runner (substring match on the test name);
  the CLI also verifies that no executed test contradicts the filter, and it
  fails closed with an explicit message if the runner reports no filter
  accounting (`test result: ... K filtered out`).
* `fmt` writes atomically; `--check` writes nothing and exits 1 on any
  difference. The `init` templates are already in canonical form.
* `doc` writes `index.md` (default `<package>/.ravum/doc/`), public items only,
  no HTML. Doc comments are not in the Core 1.0 AST, so prose documentation is
  not generated.
* `cache clean` removes only a real `<package>/.ravum/` directory (refuses
  symlinks and any other path); no automatic GC exists in Core 1.0.
* Human-readable output strips the Windows `\\?\` verbatim prefix; internal
  filesystem operations may still use canonical verbatim paths.

## Diagnostic codes owned by this crate

Manifests, dependencies, modules, lockfile (canonical registry +
Lead decisions): `E-MOD-MISSING`, `E-MOD-AMBIGUOUS`, `E-MOD-CYCLE`,
`E-MOD-DUPLICATE`, `E-MOD-PATH-ESCAPE`, `E-MOD-RESERVED-NAME`,
`E-MOD-BOTH-ROOTS`, `E-MOD-MANIFEST`, `E-MOD-DEPENDENCY`, `E-MOD-BUILD-SCRIPT`,
`E-MOD-LOCK`, `E-MOD-PATH` (symlinks/unreadable), `E-UNSUPPORTED-EDITION`.
I/O/limits: `E-IO`, `E-IO-INVALID-UTF8`, `E-LIMIT-FILE-SIZE`, `E-LIMIT-FILES`.
Warnings are not emitted by the loader (the warning list is closed by
`spec/language/diagnostics.md` DIAG-1.6).

## Tests

```text
cargo test -p ravum-lang-cli
cargo test -p ravum-lang-sema loader::tests   # loader unit tests
pwsh -File scripts/lang-smoke.ps1 -SkipBuild  # external smoke over examples/
```

`tests/cli.rs` runs the real binary (`CARGO_BIN_EXE_ravum-lang`) in real
temporary directories. Covered end to end: `--version`/`--help`, unknown
command/option exit 2, `init` (bin/lib/refusal/`--force`), valid hello `check`
(exit 0, JSON), invalid program `check` (exit 1 + stable JSON), single-file
mode (check/build/run), local path dependency (check/build/run + lockfile
determinism and change detection), `--locked` missing/current, hostile/absolute
dependency paths, both crate roots, reserved Windows module names, `build`/`run`
of hello, `test` runner pass/fail parsing, `fmt --check` idempotence, `doc`,
`cache status|clean`, `doctor --json`, and `lsp` UNSUPPORTED (exit 3).

No test is ignored: every test exercises the real binary. `check_warnings_do_not_fail`
uses the documented `W-RES-UNUSED-IMPORT` warning (exit 0) and
`lint_deny_turns_warning_into_error` verifies `--deny CODE`/`--deny all` exit 1.
Watch tests read stdout through bounded channels/timeouts (rebuild, failure
recovery, child reaping); WASM tests skip explicitly when the `wasm32-wasip1`
rustc target or Node.js is missing. Current total: 14 unit + 43 integration.
