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

import json
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
MATRIX = REPO / "docs" / "roadmap" / "CORE-1.0-GATE-MATRIX.md"
REPORT = REPO / "docs" / "reports" / "CORE-1.0-FINAL-REPORT.md"

CONFORMANCE = re.compile(r"(\d+) cas : (\d+) PASS / (\d+) FAIL / (\d+) SKIP / (\d+) NOT_RUN")
TESTS = re.compile(r"(\d+) suites, (\d+) passés / (\d+) échec / (\d+) ignorés")
STRICT = re.compile(r"(\d+)/(\d+)\*\* règles tracées, \*\*(\d+)/(\d+)")
STRICT_TESTABLE = re.compile(
    r"\*\*(\d+)/(\d+)\*\* règles tracées, \*\*(\d+)/(\d+) testables\*\*"
)
STRICT_SATISFIED = re.compile(
    r"\*\*(\d+)/(\d+)\*\* règles tracées, \*\*(\d+)/(\d+) satisfaites\*\*"
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

    match = STRICT_SATISFIED.search(matrix) or STRICT_TESTABLE.search(matrix) or STRICT.search(matrix)
    if match:
        out.append(("strict", f"{match.group(3)}/{match.group(4)}"))
    else:
        out.append(("strict", "<figure introuvable dans le registre>"))

    out.append(("registry", "108 codes"))
    return out


EVIDENCE = REPO / "evidence" / "current.json"

# T9: current-state declarations. Each anchor is (document, regex, metric keys
# in capture order). The regexes target the *current figures* declarations only;
# dated/historical sections keep their own numbers and are not rewritten.
ANCHORS: list[tuple[str, str, list[str]]] = [
    (
        "docs/reports/LANGUAGE-CORE-STATUS.md",
        r"\*\*(\d+) cas : (\d+) PASS / (\d+) FAIL / (\d+) SKIP / (\d+) NOT_RUN\*\* ; "
        r"`build: perimeter=(\d+) covered=(\d+) verified=(\d+) implicit=(\d+) failed=(\d+)`",
        [
            "conformance.cases",
            "conformance.pass",
            "conformance.fail",
            "conformance.skip",
            "conformance.not_run",
            "build.perimeter",
            "build.covered",
            "build.verified",
            "build.implicit",
            "build.failed",
        ],
    ),
    (
        "docs/roadmap/CORE-1.0-GATE-MATRIX.md",
        r"\*\*(\d+) cas : (\d+) PASS / (\d+) FAIL / (\d+) SKIP\*\*,\s*> "
        r"`perimeter=(\d+)/(\d+)`, `verified=(\d+)`",
        [
            "conformance.cases",
            "conformance.pass",
            "conformance.fail",
            "conformance.skip",
            "build.perimeter",
            "build.perimeter",
            "build.verified",
        ],
    ),
    (
        "docs/roadmap/PRODUCTION-READINESS.md",
        r"conformance \*\*(\d+) \((\d+) PASS\)\*\*",
        ["conformance.cases", "conformance.pass"],
    ),
]


def check_evidence() -> int:
    """Compare current-state declarations with evidence/current.json."""
    if not EVIDENCE.exists():
        print(f"evidence: {EVIDENCE.relative_to(REPO)} is absent — run scripts/evidence.ps1")
        return 1
    data = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    metrics = data.get("metrics", {})
    problems: list[str] = []
    checked = 0
    for relative, pattern, keys in ANCHORS:
        path = REPO / relative
        if not path.exists():
            problems.append(f"{relative}: missing")
            continue
        matches = list(re.finditer(pattern, path.read_text(encoding="utf-8")))
        if not matches:
            problems.append(f"{relative}: current-state anchor not found")
            continue
        for match in matches:
            for index, key in enumerate(keys):
                actual = int(match.group(index + 1))
                expected = metrics.get(key)
                checked += 1
                if expected is None:
                    problems.append(f"{relative}: metric {key} absent from evidence/current.json")
                elif actual != expected:
                    problems.append(f"{relative}: {key} = {actual}, evidence says {expected}")
    print(
        f"evidence: {data.get('git_sha_short', '?')} from {data.get('generated_at', '?')}"
    )
    print(f"current-state declarations checked: {checked}")
    if problems:
        print()
        print("documents disagree with evidence/current.json:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print()
    print("report-consistency --evidence: PASS — current figures match evidence/current.json")
    return 0


def main() -> int:
    if "--evidence" in sys.argv:
        return check_evidence()
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
