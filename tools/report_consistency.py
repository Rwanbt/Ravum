#!/usr/bin/env python3
"""Check that the published figures in the consolidated report match the register.

A report drifts silently: one round updates the matrix, the next forgets the
report, and a plausible-looking PDF ends up contradicting the source of truth.
This tool extracts the few figures that matter from
`docs/roadmap/CORE-1.0-GATE-MATRIX.md` and requires the same strings in
`docs/reports/CORE-1.0-FINAL-REPORT.md`.

It checks *consistency between documents*, not the truth of the measurements:
run the commands in the matrix to obtain those.

Usage: python tools/report_consistency.py
Exit code 0 when every checked figure agrees, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
MATRIX = REPO / "docs" / "roadmap" / "CORE-1.0-GATE-MATRIX.md"
REPORT = REPO / "docs" / "reports" / "CORE-1.0-FINAL-REPORT.md"

CONFORMANCE = re.compile(r"(\d+) cas : (\d+) PASS / (\d+) FAIL / (\d+) SKIP / (\d+) NOT_RUN")
TESTS = re.compile(r"(\d+) suites, (\d+) passés / (\d+) échec / (\d+) ignorés")
STRICT = re.compile(r"\*\*(\d+)/(\d+)\*\* règles tracées, \*\*(\d+)/(\d+)\*\*")
STRICT_TESTABLE = re.compile(
    r"\*\*(\d+)/(\d+)\*\* règles tracées, \*\*(\d+)/(\d+) testables\*\*"
)


def figures() -> list[tuple[str, str]]:
    matrix = MATRIX.read_text(encoding="utf-8")
    out: list[tuple[str, str]] = []

    match = CONFORMANCE.search(matrix)
    if match:
        total, passed, failed, skip = match.group(1), match.group(2), match.group(3), match.group(4)
        out.append(("conformance", f"{total} cas"))
        out.append(("conformance", f"{passed} PASS / {failed} FAIL / {skip} SKIP"))

    match = TESTS.search(matrix)
    if match:
        out.append(("tests", f"{match.group(1)} suites"))
        out.append(("tests", f"{match.group(2)} passés"))

    match = STRICT_TESTABLE.search(matrix) or STRICT.search(matrix)
    if match:
        out.append(("strict", f"{match.group(3)}/{match.group(4)}"))
    else:
        out.append(("strict", "<figure introuvable dans le registre>"))

    out.append(("registry", "112 codes"))
    return out


def main() -> int:
    report = REPORT.read_text(encoding="utf-8")
    checked = figures()
    missing = [f"{kind}: {value}" for kind, value in checked if value not in report]
    print(f"figures checked : {len(checked)}")
    for kind, value in checked:
        state = "ok    " if value in report else "ABSENT"
        print(f"  {state} {kind:12} {value}")
    if missing:
        print()
        print("the consolidated report does not carry these register figures:")
        for item in missing:
            print(f"  - {item}")
        return 1
    print()
    print("report-consistency: PASS — the report carries the register figures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
