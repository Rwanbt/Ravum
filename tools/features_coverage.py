#!/usr/bin/env python3
"""Measure rule -> conformance-case traceability for `spec/language/features.md`.

The gate requires every normative rule to name at least one *existing* case, so
the tool does not stop at "the cell is non-empty": it resolves every
`cases/<taxonomy>/<name>` reference against `conformance/cases/` and reports
missing directories as `dangling`.

A rule the specification itself declares non-testable may be exempted, but only
through `tools/coverage_exemptions.json`: a machine-readable record with a
reason, a normative reference, an approver and a review condition. The tool
rejects an exemption that is malformed, unknown, stale (the rule is covered
again) or that tries to hide an uncited rule (`Conformance` cell must literally
say `NOT_COVERED`). A prose exemption in a report is not enough.

Usage:
    python tools/features_coverage.py            # human summary + exit code
    python tools/features_coverage.py --json f   # machine report

Exit codes: 0 = every rule is covered-and-strict or covered by a justified
            exemption, and no table/exemption problem was found,
            1 = at least one rule is uncovered or dangling, a rule fails the
                strict criterion, or an exemption is invalid,
            2 = the table could not be parsed.
"""

from __future__ import annotations

import json
import re
import sys
from collections import Counter, defaultdict
from datetime import date
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parent
FEATURES_REL = Path("spec") / "language" / "features.md"
CASES_REL = Path("conformance") / "cases"
EXEMPTIONS_REL = Path("tools") / "coverage_exemptions.json"

RULE_ID = re.compile(r"^F-[A-Z0-9]+-\d{3}$")
CASE_REF = re.compile(r"cases/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")

# A table that parses to fewer rows means the document format changed under the
# tool's feet; refusing to publish a reassuring number is the safe default.
MIN_RULES = 100

EXEMPTION_FIELDS = {
    "rule",
    "reason",
    "normative_reference",
    "review_condition",
    "approved_by",
    "date",
}
# Minimum lengths keep the justification from degenerating into "TODO".
EXEMPTION_TEXT_MIN = {
    "reason": 40,
    "normative_reference": 10,
    "review_condition": 20,
    "approved_by": 8,
}


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


def classify(rows: list[dict], cases_root: Path) -> list[dict]:
    for row in rows:
        refs = CASE_REF.findall(row["conformance"])
        row["refs"] = refs
        row["missing"] = [
            ref for ref in refs if not (cases_root / ref[len("cases/") :]).is_dir()
        ]
        if not refs:
            row["state"] = "uncovered"
        elif row["missing"]:
            row["state"] = "dangling"
        else:
            row["state"] = "covered"
    return rows


def load_case(case_id: str, cases_root: Path) -> dict | None:
    """`case_id` is a repository-relative reference such as `cases/run/hello`."""
    payload = cases_root.parent / case_id / "case.json"
    if not payload.is_file():
        return None
    try:
        return json.loads(payload.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None


PASS_MODES = {"compile-pass", "run-pass", "cli", "fmt-idempotent"}
FAIL_MODES = {"compile-fail", "run-fail"}
CODE = re.compile(r"E-[A-Z0-9-]+")


def strict_check(row: dict, cases_root: Path) -> dict:
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
    cases = {case_id: load_case(case_id, cases_root) for case_id in row["refs"]}
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


def load_exemptions(path: Path) -> tuple[list[dict], list[str]]:
    """Read and structurally validate `coverage_exemptions.json`.

    Returns `(exemptions, problems)`. A missing file is not itself a problem:
    a repository with no non-testable rule legitimately has no exemption list,
    and any uncovered rule will still fail the gate.
    """
    problems: list[str] = []
    if not path.is_file():
        return [], problems
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [], [f"{path}: unreadable exemption file ({error})"]
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        return [], [f"{path}: expected an object with `schema_version: 1`"]
    raw = payload.get("exemptions")
    if not isinstance(raw, list):
        return [], [f"{path}: `exemptions` must be an array"]
    exemptions: list[dict] = []
    seen: set[str] = set()
    for index, entry in enumerate(raw):
        where = f"{path}: exemptions[{index}]"
        if not isinstance(entry, dict):
            problems.append(f"{where}: entry must be an object")
            continue
        unknown = sorted(set(entry) - EXEMPTION_FIELDS)
        if unknown:
            problems.append(f"{where}: unknown field(s) {', '.join(unknown)}")
            continue
        rule = entry.get("rule")
        if not isinstance(rule, str) or not RULE_ID.match(rule):
            problems.append(f"{where}: `rule` must be a FeatureId such as F-META-006")
            continue
        if rule in seen:
            problems.append(f"{where}: duplicate exemption for {rule}")
            continue
        seen.add(rule)
        invalid = False
        for field, minimum in EXEMPTION_TEXT_MIN.items():
            value = entry.get(field)
            if not isinstance(value, str) or len(value.strip()) < minimum:
                problems.append(
                    f"{where}: `{field}` must be a string of at least {minimum} characters"
                )
                invalid = True
        if "date" in entry:
            try:
                date.fromisoformat(str(entry["date"]))
            except ValueError:
                problems.append(f"{where}: `date` must be ISO-8601 (YYYY-MM-DD)")
                invalid = True
        if not invalid:
            exemptions.append(entry)
    return exemptions, problems


def validate_exemptions(
    exemptions: list[dict], rows: list[dict]
) -> tuple[dict[str, dict], list[str]]:
    """Reject exemptions that are unknown, stale, or hide a missing citation."""
    problems: list[str] = []
    by_rule: dict[str, dict] = {}
    known = {row["id"]: row for row in rows}
    for entry in exemptions:
        rule = entry["rule"]
        row = known.get(rule)
        if row is None:
            problems.append(f"exemption for unknown rule {rule}")
            continue
        if row["state"] == "covered":
            problems.append(
                f"stale exemption for {rule}: the rule now cites an existing case; remove it"
            )
            continue
        if row["state"] == "dangling":
            problems.append(
                f"exemption for {rule} is invalid: a cited case is dangling; fix the case instead"
            )
            continue
        # This is the anti-fake-PASS check: an exemption may only document a
        # rule the specification itself declares non-testable. An empty or
        # typo'd Conformance cell must fail, never be silently exempted.
        if row["conformance"].strip() != "NOT_COVERED":
            problems.append(
                f"exemption for {rule} is invalid: its Conformance cell must say "
                f"NOT_COVERED (got {row['conformance']!r})"
            )
            continue
        by_rule[rule] = entry
    return by_rule, problems


def main(argv: list[str], repo_root: Path | None = None) -> int:
    root = Path(repo_root) if repo_root is not None else REPO
    features = root / FEATURES_REL
    cases_root = root / CASES_REL
    exemptions_path = root / EXEMPTIONS_REL

    if not features.is_file():
        print(f"missing {features}", file=sys.stderr)
        return 2
    text = features.read_text(encoding="utf-8")
    rows, problems = parse_rows(text)
    if len(rows) < MIN_RULES:
        print(
            f"only {len(rows)} rules parsed; the table format changed",
            file=sys.stderr,
        )
        return 2
    rows = classify(rows, cases_root)

    exemptions, exemption_problems = load_exemptions(exemptions_path)
    problems.extend(exemption_problems)
    by_rule, validation_problems = validate_exemptions(exemptions, rows)
    problems.extend(validation_problems)
    for row in rows:
        if row["id"] in by_rule:
            row["state"] = "exempt"

    # Strict criterion: covered rules must satisfy the corpus check; exempted
    # rules are satisfied by a *validated* exemption, not by silence.
    strict: dict[str, dict] = {}
    for row in rows:
        if row["state"] == "exempt":
            strict[row["id"]] = {
                "ok": True,
                "why": f"exempt ({by_rule[row['id']]['approved_by']})",
            }
        elif row["state"] == "covered":
            strict[row["id"]] = strict_check(row, cases_root)
        else:
            strict[row["id"]] = {"ok": False, "why": row["state"]}

    by_state = Counter(row["state"] for row in rows)
    by_group: dict[str, Counter] = defaultdict(Counter)
    for row in rows:
        by_group[row["group"]][row["state"]] += 1

    bad_strict = [rule_id for rule_id, verdict in strict.items() if not verdict["ok"]]
    failing = [row for row in rows if row["state"] in ("uncovered", "dangling")]

    print(f"rules            : {len(rows)}")
    for state in ("covered", "exempt", "uncovered", "dangling"):
        print(f"{state:16} : {by_state.get(state, 0)}")
    print()
    print(f"{'group':10} {'total':>5} {'covered':>7} {'exempt':>7} {'uncovered':>9} {'dangling':>8}")
    for group in sorted(by_group):
        counter = by_group[group]
        total = sum(counter.values())
        print(
            f"{group:10} {total:5} {counter.get('covered', 0):7} "
            f"{counter.get('exempt', 0):7} {counter.get('uncovered', 0):9} "
            f"{counter.get('dangling', 0):8}"
        )
    if problems:
        print()
        for problem in problems:
            print(f"problem: {problem}")

    if failing:
        print()
        for row in failing:
            detail = row["conformance"] if row["state"] == "dangling" else "-"
            print(f"{row['state']:10} {row['id']:12} line {row['line']:<5} {detail}")

    if by_rule:
        print()
        for rule in sorted(by_rule):
            entry = by_rule[rule]
            print(f"exempt      {rule:12} review: {entry['review_condition']}")

    if "--strict" in argv:
        print()
        satisfied = len(rows) - len(bad_strict)
        suffix = ""
        if by_rule:
            suffix = f" ({len(by_rule)} justified exemption{'s' if len(by_rule) != 1 else ''})"
        print(f"strict traceability: {satisfied}/{len(rows)} rules satisfied{suffix}")
        for rule_id in bad_strict:
            print(f"  unsat {rule_id:12} {strict[rule_id]['why']}")

    if "--json" in argv:
        target = Path(argv[argv.index("--json") + 1])
        payload = {
            "schema_version": 2,
            "source": FEATURES_REL.as_posix(),
            "total": len(rows),
            "states": dict(by_state),
            "problems": problems,
            "strict_satisfied": len(rows) - len(bad_strict),
            "exemptions": {
                rule: {
                    "reason": entry["reason"],
                    "normative_reference": entry["normative_reference"],
                    "review_condition": entry["review_condition"],
                    "approved_by": entry["approved_by"],
                    "date": entry.get("date"),
                }
                for rule, entry in sorted(by_rule.items())
            },
            "strict": strict,
            "rules": rows,
        }
        target.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {target}", file=sys.stderr)

    if problems or failing or bad_strict:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
