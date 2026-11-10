#!/usr/bin/env python3
"""Audit every diagnostic code emitted by the toolchain against the registry.

`spec/language/diagnostics.md` §3 declares its list canonical and states that
"tout code absent est interdit"; DIAG-1.1 requires every diagnostic to carry a
code from that list. This tool is the only reliable way to check that, because a
code can be emitted on a path that no conformance case exercises.

It scans the Rust sources for string literals of the form `E-…`/`W-…` (the shape
used by every `diags.error(...)` call) and reports the ones that are not in the
registry.

Usage: python tools/registry_audit.py [--json PATH]
Exit code 0 when every emitted code is registered, 1 otherwise.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
REGISTRY = REPO / "spec" / "language" / "diagnostics.md"
CRATES = REPO / "crates"
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


def registry_codes() -> set[str]:
    text = REGISTRY.read_text(encoding="utf-8")
    return set(re.findall(r"^\| `((?:E|W)-[A-Z0-9-]+)`", text, flags=re.M))


def scan() -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    for path in sorted(CRATES.rglob("*.rs")):
        if "target" in path.parts:
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for code in CODE_LITERAL.findall(line):
                where = f"{path.relative_to(REPO).as_posix()}:{number}"
                found.setdefault(code, []).append(where)
    return found


def main(argv: list[str]) -> int:
    registered = registry_codes()
    emitted = scan()
    unregistered = {
        code: places
        for code, places in emitted.items()
        if code not in registered and code not in PREFIX_LITERALS and code not in TOOLING_CODES
    }
    ignored = sorted(
        code for code in emitted if code in PREFIX_LITERALS or code in TOOLING_CODES
    )

    print(f"registry codes : {len(registered)}")
    print(f"codes emitted  : {len(emitted)}")
    print(f"unregistered   : {len(unregistered)} (language codes emitted but absent from §3)")
    print(f"ignored        : {len(ignored)} prefix/tooling literals")
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
                    "schema_version": 1,
                    "registered": sorted(registered),
                    "emitted": {code: places for code, places in sorted(emitted.items())},
                    "unregistered": {code: places for code, places in sorted(unregistered.items())},
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"wrote {target}", file=sys.stderr)

    return 0 if not unregistered else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
