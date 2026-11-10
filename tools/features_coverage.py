#!/usr/bin/env python3
"""Measure rule -> conformance-case traceability for `spec/language/features.md`.

The gate requires every normative rule to name at least one *existing* case, so
the tool does not stop at "the cell is non-empty": it resolves every
`cases/<taxonomy>/<name>` reference against `conformance/cases/` and reports
missing directories as `dangling`.

Usage:
    python tools/features_coverage.py            # human summary + exit code
    python tools/features_coverage.py --json f   # machine report

Exit codes: 0 = every rule traces to at least one existing case,
            1 = at least one rule is uncovered or dangling,
            2 = the table could not be parsed.
"""

from __future__ import annotations

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FEATURES = REPO / "spec" / "language" / "features.md"
CASES = REPO / "conformance" / "cases"

RULE_ID = re.compile(r"^F-[A-Z0-9]+-\d{3}$")
CASE_REF = re.compile(r"cases/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")


def parse_rows(text: str) -> tuple[list[dict], list[str]]:
    """Parse every markdown table that has a `FeatureId` and a `Conformance` column.

    `features.md` uses two table shapes (the nine-column detail tables and the
    six-column `F-UNS` table), so the parser keys off the header instead of
    hard-coding column positions.
    """
    rows: list[dict] = []
    problems: list[str] = []
    columns: list[str] | None = None
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if not stripped.startswith("|"):
            columns = None
            continue
        cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", stripped)[1:-1]]
        cells = [cell.replace("\\|", "|") for cell in cells]
        if any(cell == "FeatureId" for cell in cells):
            columns = cells
            continue
        if columns is None or not cells:
            continue
        rule_id = cells[0].strip("`")
        if not RULE_ID.match(rule_id):
            continue
        if "Conformance" not in columns:
            problems.append(f"line {number}: {rule_id} table has no Conformance column")
            continue
        if len(cells) != len(columns):
            problems.append(
                f"line {number}: {rule_id} has {len(cells)} cells, header has {len(columns)}"
            )
            continue
        record = dict(zip(columns, cells))
        rows.append(
            {
                "line": number,
                "id": rule_id,
                "group": rule_id.split("-")[1],
                "label": record.get("Fonctionnalité", ""),
                "pass_fail": record.get("Pass / Fail", record.get("Fonctionnalité refusée", "")),
                "status": record.get("Statut", ""),
                "conformance": record["Conformance"],
                # The `F-UNS` table has no `Pass / Fail` column: every row is a
                # pure refusal, so no accepting case can exist.
                "refusal_table": "Pass / Fail" not in record,
            }
        )
    return rows, problems


def classify(rows: list[dict]) -> list[dict]:
    for row in rows:
        refs = CASE_REF.findall(row["conformance"])
        row["refs"] = refs
        row["missing"] = [
            ref
            for ref in refs
            if not (CASES / ref[len("cases/") :]).is_dir()
        ]
        if not refs:
            row["state"] = "uncovered"
        elif row["missing"]:
            row["state"] = "dangling"
        else:
            row["state"] = "covered"
    return rows


def load_case(case_id: str) -> dict | None:
    """`case_id` is a repository-relative reference such as `cases/run/hello`."""
    payload = CASES.parent / case_id / "case.json"
    if not payload.is_file():
        return None
    try:
        return json.loads(payload.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


PASS_MODES = {"compile-pass", "run-pass", "cli", "fmt-idempotent"}
FAIL_MODES = {"compile-fail", "run-fail"}
CODE = re.compile(r"E-[A-Z0-9-]+")


def strict_check(row: dict) -> dict:
    """Require a positive case and, when the rule mandates a code, a negative one.

    The criterion is deliberately stated in terms of what the corpus can prove:

    * the rule cites at least one case that exists;
    * the rule cites at least one case whose mode exercises acceptance;
    * every `E-*` code the rule's `Pass / Fail` column mandates appears in the
      `expect_codes` of at least one cited failing case.

    Traceability is *declared* by the specification and verified here for
    existence, mode and code agreement. No case can declare which rule it
    covers (the harness rejects unknown `case.json` fields), so the link is
    one-directional by design.
    """
    if row["state"] != "covered":
        return {"ok": False, "why": row["state"]}
    cases = {case_id: load_case(case_id) for case_id in row["refs"]}
    if any(payload is None for payload in cases.values()):
        return {"ok": False, "why": "unreadable case.json"}
    modes = {case_id: payload.get("mode") for case_id, payload in cases.items()}
    has_pass = any(mode in PASS_MODES for mode in modes.values())
    fail_cases = {
        case_id: payload for case_id, payload in cases.items() if payload.get("mode") in FAIL_MODES
    }
    produced = set()
    for payload in fail_cases.values():
        produced.update(payload.get("expect_codes") or [])
    text = row["pass_fail"]
    # The pass side may legitimately be empty: a pure-refusal rule says
    # "pass : aucune" (F-UNS-*, F-OWN-015, ...). Requiring an accepting case
    # there would demand a program the specification says does not exist.
    pass_part = text.split("fail", 1)[0] if "fail" in text else text
    refusal_only = row.get("refusal_table", False) or (
        "aucun" in pass_part.lower()
        or not pass_part.replace("pass", "").replace(":", "").strip(" .—-")
    )
    fail_part = text.split("fail", 1)[1] if "fail" in text else ""
    mandated = set(CODE.findall(fail_part))
    missing_codes = sorted(mandated - produced)
    problems = []
    if not has_pass and not refusal_only:
        problems.append("no accepting case")
    if mandated and not fail_cases:
        problems.append("no failing case")
    if missing_codes:
        problems.append("codes not asserted: " + ",".join(missing_codes))
    return {"ok": not problems, "why": "; ".join(problems)}


def main(argv: list[str]) -> int:
    if not FEATURES.is_file():
        print(f"missing {FEATURES}", file=sys.stderr)
        return 2
    text = FEATURES.read_text(encoding="utf-8")
    rows, problems = parse_rows(text)
    if len(rows) < 100:
        print(f"only {len(rows)} rules parsed; the table format changed", file=sys.stderr)
        return 2
    rows = classify(rows)

    by_state = Counter(row["state"] for row in rows)
    by_group: dict[str, Counter] = defaultdict(Counter)
    for row in rows:
        by_group[row["group"]][row["state"]] += 1

    if "--json" in argv:
        target = argv[argv.index("--json") + 1]
        payload = {
            "schema_version": 1,
            "source": "spec/language/features.md",
            "total": len(rows),
            "states": dict(by_state),
            "problems": problems,
            "rules": rows,
        }
        Path(target).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {target}", file=sys.stderr)

    print(f"rules            : {len(rows)}")
    for state in ("covered", "uncovered", "dangling"):
        print(f"{state:16} : {by_state.get(state, 0)}")
    print()
    print(f"{'group':10} {'total':>5} {'covered':>7} {'uncovered':>9} {'dangling':>8}")
    for group in sorted(by_group):
        counter = by_group[group]
        total = sum(counter.values())
        print(
            f"{group:10} {total:5} {counter.get('covered', 0):7} "
            f"{counter.get('uncovered', 0):9} {counter.get('dangling', 0):8}"
        )
    if problems:
        print()
        for problem in problems:
            print(f"problem: {problem}")

    failing = [row for row in rows if row["state"] != "covered"]
    if failing:
        print()
        for row in failing:
            detail = row["conformance"] if row["state"] == "dangling" else "-"
            print(f"{row['state']:10} {row['id']:12} line {row['line']:<5} {detail}")

    if "--strict" in argv:
        strict = {row["id"]: strict_check(row) for row in rows}
        bad = [rule_id for rule_id, verdict in strict.items() if not verdict["ok"]]
        print()
        print(f"strict traceability: {len(rows) - len(bad)}/{len(rows)} rules satisfied")
        for rule_id in bad:
            print(f"  unsat {rule_id:12} {strict[rule_id]['why']}")
        if "--json" in argv:
            target = Path(argv[argv.index("--json") + 1])
            payload = json.loads(target.read_text(encoding="utf-8"))
            payload["strict"] = strict
            payload["strict_satisfied"] = len(rows) - len(bad)
            target.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        if bad:
            return 1

    if failing:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
