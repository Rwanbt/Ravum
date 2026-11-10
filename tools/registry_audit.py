#!/usr/bin/env python3
"""Audit every diagnostic code emitted by the toolchain against the registry.

`spec/language/diagnostics.md` §3 declares its list canonical and states that
"tout code absent est interdit"; DIAG-1.1 requires every diagnostic to carry a
code from that list. This tool is the only reliable way to check that, because a
code can be emitted on a path that no conformance case exercises.

It scans the Rust sources for string literals of the form `E-…`/`W-…` (the shape
used by every `diags.error(...)` call) and reports the ones that are not in the
registry. It also checks the opposite direction: a code the registry *promises*
but no source emits is a divergence too (the gate matrix claims
"0 déclaré-jamais-émis"), so it fails the audit instead of printing a warning
nobody reads.

Usage: python tools/registry_audit.py [--json PATH]
Exit code 0 when every emitted code is registered and every registered code is
emitted, 1 otherwise.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
REGISTRY_REL = pathlib.Path("spec") / "language" / "diagnostics.md"
CRATES_REL = pathlib.Path("crates")
CODE_LITERAL = re.compile(r'"((?:E|W)-[A-Z0-9-]+)"')

# `crates/ravum-lang-cli/src/json.rs` maps a *prefix* to a phase ("E-TYP" ->
# "types"); those literals are not emitted codes. Same for the `E-OWN-` fragment
# used to build messages.
PREFIX_LITERALS = {"E-LEX", "E-PARSE", "E-RES", "E-TYP", "E-OWN", "E-OWN-", "E-EFF",
                   "E-MOD", "E-UNSUPPORTED", "E-IO", "E-CODEGEN", "W-OWN", "W-RES"}
# Codes produced by the toolchain itself (build, doctor, wasm runner) rather than
# by the language; the registry covers language diagnostics.
TOOLING_CODES = {"E-BUILD", "E-TOOL", "E-CLI-WASM-RUNTIME", "E-INTERNAL-PARSER",
                 "E-INTERNAL-CHECKER"}

# DIAG-1.6 declares the warning list *closed*: only these two `W-*` codes may be
# registered or emitted. Without this table, re-adding `W-TYP-UNREACHABLE` to
# §3 and emitting it would pass the audit (found by independent review).
WARNING_WHITELIST = {"W-RES-UNUSED-IMPORT", "W-OWN-UNUSED-VARIABLE"}


def strip_comment(line: str) -> str:
    """Drop a `//` comment so a code named in prose is not counted as emitted.

    The scan is lexical by nature; ignoring comments removes the easiest way to
    make a registered-but-never-emitted code look emitted.
    """
    index = line.find("//")
    return line if index < 0 else line[:index]


def registry_codes(registry: pathlib.Path) -> set[str]:
    text = registry.read_text(encoding="utf-8")
    return set(re.findall(r"^\| `((?:E|W)-[A-Z0-9-]+)`", text, flags=re.M))


def scan(crates: pathlib.Path, repo: pathlib.Path) -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    for path in sorted(crates.rglob("*.rs")):
        if "target" in path.parts:
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for code in CODE_LITERAL.findall(strip_comment(line)):
                where = f"{path.relative_to(repo).as_posix()}:{number}"
                found.setdefault(code, []).append(where)
    return found


def main(argv: list[str], repo_root: pathlib.Path | None = None) -> int:
    repo = pathlib.Path(repo_root) if repo_root is not None else REPO
    registry = repo / REGISTRY_REL
    if not registry.is_file():
        print(f"missing {registry}", file=sys.stderr)
        return 2
    registered = registry_codes(registry)
    emitted = scan(repo / CRATES_REL, repo)
    unregistered = {
        code: places
        for code, places in emitted.items()
        if code not in registered and code not in PREFIX_LITERALS and code not in TOOLING_CODES
    }
    ignored = sorted(
        code for code in emitted if code in PREFIX_LITERALS or code in TOOLING_CODES
    )

    # The opposite direction: a code the registry *promises* that no source ever
    # emits. `W-OWN-UNUSED-VARIABLE` is the case that motivated this check.
    never_emitted = sorted(code for code in registered if code not in emitted)

    # DIAG-1.6 closure: the warning list is closed, so any other `W-*` code is a
    # specification violation whether it is registered, emitted, or both.
    extra_warnings = sorted(
        {
            code
            for code in registered | set(emitted)
            if code.startswith("W-")
            and code not in PREFIX_LITERALS
            and code not in TOOLING_CODES
        }
        - WARNING_WHITELIST
    )

    print(f"registry codes : {len(registered)}")
    print(f"codes emitted  : {len(emitted)}")
    print(f"unregistered   : {len(unregistered)} (language codes emitted but absent from §3)")
    print(f"ignored        : {len(ignored)} prefix/tooling literals")
    print(f"never emitted  : {len(never_emitted)} (declared in §3, produced by no source literal)")
    print(f"closed W list  : {len(extra_warnings)} extra warning code(s) outside DIAG-1.6")
    for code in extra_warnings:
        print(f"  {code}")
    for code in never_emitted:
        print(f"  {code}")
    for code in sorted(unregistered):
        places = unregistered[code]
        print(f"  {code}  ({len(places)} site(s))")
        for place in places[:4]:
            print(f"      {place}")

    if "--json" in argv:
        target = pathlib.Path(argv[argv.index("--json") + 1])
        target.write_text(
            json.dumps(
                {
                    "schema_version": 2,
                    "registered": sorted(registered),
                    "emitted": {code: places for code, places in sorted(emitted.items())},
                    "unregistered": {code: places for code, places in sorted(unregistered.items())},
                    "never_emitted": never_emitted,
                    "extra_warnings": extra_warnings,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"wrote {target}", file=sys.stderr)

    return 0 if not unregistered and not never_emitted and not extra_warnings else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
