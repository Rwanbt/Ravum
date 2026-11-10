#!/usr/bin/env python3
"""Fill the `Conformance` column of `spec/language/features.md` from coverage maps.

Each coverage map is a JSON file written by a case-authoring worker:

    {"agent": "...", "rules": {"F-SYN-001": ["parser/a-pass", "parser/a-fail"]}}

The tool rewrites only the last cell of the matching table row, keeps the rest of
the line byte-identical, refuses to invent a rule id, and refuses to point at a
case directory that does not exist. `--dry-run` prints the changes without
writing.

Usage:
    python tools/apply_coverage_map.py target/coverage-maps/*.json [--dry-run]
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FEATURES = REPO / "spec" / "language" / "features.md"
CASES = REPO / "conformance" / "cases"

RULE_ID = re.compile(r"^F-[A-Z0-9]+-\d{3}$")
LAST_CELL = re.compile(r"\|([^|]*)\|(\s*)$")


def main(argv: list[str]) -> int:
    dry_run = "--dry-run" in argv
    paths = [Path(arg) for arg in argv if not arg.startswith("--")]
    if not paths:
        print("usage: apply_coverage_map.py <map.json> [...] [--dry-run]", file=sys.stderr)
        return 2

    mapping: dict[str, list[str]] = {}
    for path in paths:
        payload = json.loads(path.read_text(encoding="utf-8"))
        for rule_id, entry in (payload.get("rules") or {}).items():
            if not RULE_ID.match(rule_id):
                print(f"{path.name}: ignoring malformed rule id {rule_id!r}", file=sys.stderr)
                continue
            # Two accepted shapes: a bare list of case ids, or
            # {"cases": [...], "why": "..."} from the strict-traceability pass.
            case_ids = entry.get("cases") if isinstance(entry, dict) else entry
            if not case_ids:
                print(f"{path.name}: {rule_id} has no case list; skipped", file=sys.stderr)
                continue
            missing = [case_id for case_id in case_ids if not (CASES / case_id).is_dir()]
            if missing:
                print(
                    f"{path.name}: {rule_id} cites missing case(s) {missing}; skipped",
                    file=sys.stderr,
                )
                continue
            mapping.setdefault(rule_id, [])
            for case_id in case_ids:
                reference = f"cases/{case_id}"
                if reference not in mapping[rule_id]:
                    mapping[rule_id].append(reference)

    lines = FEATURES.read_text(encoding="utf-8").splitlines(keepends=True)
    changed = 0
    unknown = set(mapping)
    for index, line in enumerate(lines):
        if not line.lstrip().startswith("|"):
            continue
        first = line.lstrip().split("|")[1].strip().strip("`")
        if first not in mapping:
            continue
        unknown.discard(first)
        refs = mapping[first]
        cell = ", ".join(refs)
        body = line.rstrip("\r\n")
        ending = line[len(body):] or "\n"
        match = LAST_CELL.search(body)
        if not match:
            print(f"line {index + 1}: cannot locate the Conformance cell of {first}", file=sys.stderr)
            continue
        old = match.group(1).strip()
        if old == cell:
            continue
        newline = body[: match.start()] + f"| {cell} |" + ending
        lines[index] = newline
        changed += 1
        print(f"{first}: {old} -> {cell}")

    for rule_id in sorted(unknown):
        print(f"{rule_id}: not present as a row in features.md", file=sys.stderr)

    if dry_run:
        print(f"(dry run) {changed} row(s) would change")
        return 0
    FEATURES.write_text("".join(lines), encoding="utf-8")
    print(f"{changed} row(s) updated in spec/language/features.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
