#!/usr/bin/env python3
"""Measure individual MUST-statement traceability in `spec/language/`.

`tools/features_coverage.py --strict` answers one question: does every
**FeatureId** in `features.md` name an existing conformance case that satisfies
the strict criterion? It counts 188 ids. It says nothing about how many
individual normative statements the specification contains, and 188 FeatureIds
are not 188 rules: one FeatureId can cite a whole rule range
(`G-4.1<->4.2`), and a whole document (ABI, COMPAT, DIAG, MACH, ...) can have
dozens of MUST rows that no FeatureId ever cites.

This tool measures the second thing, with its own denominator and oracle:

* **FeatureId metric** (existing oracle, reused, not reimplemented): the
  188/188 result of `tools/features_coverage.py --strict`, including the
  `F-META-006` exemption. Denominator: FeatureIds parsed from `features.md`.
* **MUST-statement metric** (this tool): every row of a
  `| ID | <text> | Niveau |` table in `spec/language/*.md` whose level cell is
  `MUST` is one normative statement. Denominator: that count. Each statement is
  then classified by how (if at all) it reaches a FeatureId:
    - `rule`    : a FeatureId's `Syntaxe`/`Typage`/`Runtime` cells cite the
                  exact rule id (`G-3.1`);
    - `range`   : they cite a range that contains it (`G-4.1<->4.2`);
    - `group`   : they cite the rule's minor group (`TYP-11` -> `TYP-11.5`);
    - `section` : they cite a section of the same document (`types §8`);
    - `unmapped`: none of the above.
  For mapped statements the tool also reports whether the mapped FeatureId has
  at least one **existing** conformance case (`case_linked`) and whether it
  passes the strict criterion of `features_coverage.py` (`strict_linked`).

What the numbers mean, and what they do not
-------------------------------------------
* A mapping is **declared by the specification**, not discovered by this tool:
  `features.md` names the rule ids, the tool verifies the name resolves. It
  cannot prove that the cited case truly exercises the statement.
* `case_linked` means "a FeatureId that cites this rule cites at least one case
  directory that exists". `strict_linked` additionally requires the strict
  criterion of `features_coverage.py`. Neither is a `PROVED`/`PASS` verdict.
* An exempted FeatureId (`F-META-006`) has no case by definition; it is counted
  as a valid exemption by the FeatureId oracle and as `case_linked = false`
  here, which is the honest reading.

The tool is **report-only by default** (exit 0) so it changes no existing gate.
`--strict` makes it exit 1 if any MUST statement is unmapped or any FeatureId
fails its own strict oracle; that mode is proposed for a future gate and was
**not** wired into CI by the session that wrote this file.

Usage:
    python tools/must_audit.py
    python tools/must_audit.py --json target/must-audit.json
    python tools/must_audit.py --strict
    python -m unittest discover -s tools/tests -v

Exit codes: 0 = report produced, 1 = `--strict` found an unmapped MUST statement
or a FeatureId that fails the feature-coverage oracle, 2 = the specification
tables could not be parsed.
"""

from __future__ import annotations

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parent

import features_coverage  # noqa: E402  (same directory, imported by design)

SPEC_REL = Path("spec") / "language"
FEATURES_REL = SPEC_REL / "features.md"

# Rule ids are `<GROUP>-<section>.<number>`; the group vocabulary is closed by
# the documents that exist, but the regex stays permissive so a new document is
# measured instead of silently skipped.
RULE_ID = re.compile(r"^([A-Z]+)-(\d+)\.(\d+)$")
# A reference token in `features.md`: an exact rule id or a minor-group id.
REF_TOKEN = re.compile(r"\b([A-Z]{1,7})-(\d+)(?:\.(\d+))?\b")
# A range such as `G-4.1<->4.2`, `TYP-1.4<->1.5` or `MOD-2.3<->2.5`. The
# separator is the en dash used by the documents; a plain hyphen is accepted
# because it is the ASCII fallback a future edit may use.
REF_RANGE = re.compile(
    r"\b([A-Z]{1,7})-(\d+)\.(\d+)\s*[\u2013\u2014-]\s*(?:([A-Z]{1,7})-)?(\d+)(?:\.(\d+))?\b"
)
# A section citation such as `lexical §8.1` or `types §13`. The document stem
# maps to the group prefix used by its rule ids.
SECTION_REF = re.compile(
    r"\b(lexical|grammar|types|expressions|modules|ownership|effects|stdlib|"
    r"unsupported|diagnostics|concurrency|abi|compatibility|core-machine|features)"
    r"\s*\u00a7\s*(\d+)(?:\.(\d+))?"
)
MUST_NOT = re.compile(r"MUST\s+\*\*NOT\*\*|MUST NOT")

DOC_GROUP = {
    "abi": "ABI",
    "compatibility": "COMPAT",
    "concurrency": "CONC",
    "core-machine": "MACH",
    "diagnostics": "DIAG",
    "effects": "EFF",
    "expressions": "EXPR",
    "features": "FEAT",
    "grammar": "G",
    "lexical": "LEX",
    "modules": "MOD",
    "ownership": "OWN",
    "stdlib": "STD",
    "types": "TYP",
    "unsupported": "UNS",
}

MAPPING_KINDS = ("rule", "range", "group", "section")


def split_row(line: str) -> list[str]:
    """Split a markdown table row, honouring `\\|` and stripping the outer pipes."""
    cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip())[1:-1]]
    return [cell.replace("\\|", "|") for cell in cells]


def parse_rule_tables(repo_root: Path) -> tuple[list[dict], list[str]]:
    """Collect every row of an `| ID | ... | Niveau |` table in the spec.

    Returns `(rows, problems)`. A row is kept only when its first cell is a
    well-formed rule id and the row has exactly as many cells as its header, so
    a malformed table is reported instead of silently shrinking the denominator.
    """
    rows: list[dict] = []
    problems: list[str] = []
    spec_dir = repo_root / SPEC_REL
    if not spec_dir.is_dir():
        return [], [f"missing directory {spec_dir}"]
    for path in sorted(spec_dir.glob("*.md")):
        columns: list[str] | None = None
        seen_rows = 0
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            stripped = line.strip()
            if not stripped.startswith("|"):
                columns = None
                continue
            cells = split_row(stripped)
            if "ID" in cells and "Niveau" in cells:
                columns = cells
                continue
            if columns is None or not cells:
                continue
            # A rule text legitimately contains `|` inside a code span (`||`,
            # `|=`, `|x|`) and a few rows do not escape it. Such a row splits
            # into more cells than the header, but its last cell is still the
            # level, so it is counted and the escaping defect is reported rather
            # than silently shrinking the denominator.
            unescaped_pipes = len(cells) > len(columns)
            if len(cells) < len(columns):
                problems.append(
                    f"{path.name}:{number}: {len(cells)} cells, header has {len(columns)}"
                )
                continue
            match = RULE_ID.match(cells[0].strip("`"))
            if not match:
                continue
            if unescaped_pipes:
                problems.append(
                    f"{path.name}:{number}: rule row has an unescaped `|` "
                    f"({len(cells)} cells); counted, text may be truncated"
                )
            level = cells[-1].strip().upper()
            rows.append(
                {
                    "doc": path.name,
                    "stem": path.stem,
                    "line": number,
                    "id": cells[0].strip("`"),
                    "group": match.group(1),
                    "section": int(match.group(2)),
                    "number": int(match.group(3)),
                    "level": level,
                    "explicit_must_not": bool(MUST_NOT.search(cells[1])),
                    "pipes_unescaped": unescaped_pipes,
                }
            )
            seen_rows += 1
        if seen_rows == 0:
            # A document with no rule table is legitimate only if it declares no
            # MUST; flag the absence so a truncated file cannot hide rules.
            text = path.read_text(encoding="utf-8")
            if "MUST" in text:
                problems.append(f"{path.name}: no rule table parsed but the file mentions MUST")
    return rows, problems


def rule_refs(cell_text: str) -> tuple[set[tuple[str, int, int]], set[tuple[str, int, int]], set[tuple[str, int]], list[tuple[str, int, int]]]:
    """Extract `(exact, ranged, groups, sections)` references from a cell.

    `exact` and `ranged` hold `(group, section, number)`; `groups` holds
    `(group, section)`; `sections` holds `(document_stem, section, number)` with
    `number = 0` when the citation names a whole section.
    """
    exact: set[tuple[str, int, int]] = set()
    ranged: set[tuple[str, int, int]] = set()
    groups: set[tuple[str, int]] = set()
    sections: list[tuple[str, int, int]] = []

    for match in REF_RANGE.finditer(cell_text):
        group, section, first = match.group(1), int(match.group(2)), int(match.group(3))
        second_group = match.group(4) or group
        second_section = int(match.group(5))
        second_number = int(match.group(6)) if match.group(6) is not None else 0
        if match.group(6) is None:
            # `G-4.1-4.2` style: the right side repeats the section.
            second_section = section
            second_number = int(match.group(5))
        start = (section, first)
        end = (second_section, second_number)
        if second_group != group:
            continue  # cross-document ranges do not exist in this spec
        if (end[0], end[1]) < (start[0], start[1]):
            start, end = end, start
        if (end[0] - start[0]) * 1000 + (end[1] - start[1]) > 200:
            continue  # refuse an absurd expansion instead of inventing coverage
        for sec in range(start[0], end[0] + 1):
            lo = start[1] if sec == start[0] else 1
            hi = end[1] if sec == end[0] else 99
            for num in range(lo, hi + 1):
                ranged.add((group, sec, num))

    for match in REF_TOKEN.finditer(cell_text):
        group, section = match.group(1), int(match.group(2))
        if match.group(3) is None:
            groups.add((group, section))
        else:
            exact.add((group, section, int(match.group(3))))

    for match in SECTION_REF.finditer(cell_text):
        stem = match.group(1)
        section = int(match.group(2))
        number = int(match.group(3)) if match.group(3) else 0
        sections.append((stem, section, number))

    return exact, ranged, groups, sections


def build_feature_links(repo_root: Path) -> tuple[list[dict], dict]:
    """Parse `features.md`, resolve its case references, and index its rule refs.

    Returns `(feature_rows, index)` where `index` holds, for every rule key, the
    FeatureIds that cite it, plus the mapping kind. The case resolution and the
    strict verdict are delegated to `features_coverage` so both tools share one
    oracle for the 188/188 claim.
    """
    features = repo_root / FEATURES_REL
    cases_root = repo_root / features_coverage.CASES_REL
    exemptions_path = repo_root / features_coverage.EXEMPTIONS_REL
    text = features.read_text(encoding="utf-8")
    parsed, problems = features_coverage.parse_rows(text)
    parsed = features_coverage.classify(parsed, cases_root)

    # Reuse the exemption oracle: a rule the specification declares
    # non-testable is only satisfied through a validated machine-readable
    # exemption, never through silence.
    exemptions, exemption_problems = features_coverage.load_exemptions(exemptions_path)
    by_rule, validation_problems = features_coverage.validate_exemptions(exemptions, parsed)
    problems.extend(exemption_problems)
    problems.extend(validation_problems)
    for row in parsed:
        if row["id"] in by_rule:
            row["state"] = "exempt"

    index: dict[str, dict] = {
        "rule": defaultdict(set),
        "range": defaultdict(set),
        "group": defaultdict(set),
        "section": defaultdict(set),
    }
    rows: list[dict] = list(parsed)
    # `parse_rows` keeps only a few columns; the rule references live in
    # `Syntaxe`/`Typage`/`Runtime`, so re-parse the table to keep every column.
    columns: list[str] | None = None
    full_rows: list[dict] = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("|"):
            columns = None
            continue
        cells = split_row(stripped)
        if any(cell == "FeatureId" for cell in cells):
            columns = cells
            continue
        if columns is None or not cells or len(cells) != len(columns):
            continue
        rule_id = cells[0].strip("`")
        if not re.match(r"^F-[A-Z0-9]+-\d{3}$", rule_id):
            continue
        record = dict(zip(columns, cells))
        full_rows.append({"id": rule_id, "record": record})

    by_id = {row["id"]: row for row in rows}
    for entry in full_rows:
        feature_id = entry["id"]
        reference_text = " | ".join(
            entry["record"].get(key, "") for key in ("Syntaxe", "Typage", "Runtime")
        )
        exact, ranged, groups, sections = rule_refs(reference_text)
        for key in exact:
            index["rule"][key].add(feature_id)
        for key in ranged:
            index["range"][key].add(feature_id)
        for key in groups:
            index["group"][key].add(feature_id)
        for stem, section, number in sections:
            index["section"][(stem, section, number)].add(feature_id)

    for row in rows:
        row["strict"] = (
            features_coverage.strict_check(row, cases_root) if row["state"] == "covered" else None
        )
    return rows, {
        "index": index,
        "problems": problems,
        "by_id": by_id,
        "cases_root": cases_root,
    }


def map_statement(row: dict, link_index: dict) -> tuple[list[str], list[str]]:
    """Return `(kinds, feature_ids)` for one MUST statement.

    A statement can be reached through several kinds (e.g. one FeatureId cites
    the exact rule and another cites the whole section); all of them are
    reported, and the strongest kind is the first in `MAPPING_KINDS` order.
    """
    kinds: list[str] = []
    features: set[str] = set()
    key_rule = (row["group"], row["section"], row["number"])
    key_group = (row["group"], row["section"])

    for kind, keys in (
        ("rule", [key_rule]),
        ("range", [key_rule]),
        ("group", [key_group]),
    ):
        found: set[str] = set()
        for key in keys:
            found |= link_index["index"][kind].get(key, set())
        if found:
            kinds.append(kind)
            features |= found

    section_hits: set[str] = set()
    for (stem, section, number), ids in link_index["index"]["section"].items():
        if stem != row["stem"] or section != row["section"]:
            continue
        if number and number != row["number"]:
            continue
        section_hits |= ids
    if section_hits:
        kinds.append("section")
        features |= section_hits

    return kinds, sorted(features)


def build_report(repo_root: Path) -> dict:
    rows, problems = parse_rule_tables(repo_root)
    feature_rows, links = build_feature_links(repo_root)
    problems.extend(links["problems"])

    strict = {
        row["id"]: row["strict"] for row in feature_rows if row["strict"] is not None
    }
    covered = [row for row in feature_rows if row["state"] == "covered"]
    exempt = [row["id"] for row in feature_rows if row["state"] == "exempt"]

    must = [row for row in rows if row["level"] == "MUST"]
    should = [row for row in rows if row["level"] == "SHOULD"]

    kind_counter: Counter = Counter()
    statements: list[dict] = []
    by_doc: dict[str, Counter] = defaultdict(Counter)
    for row in must:
        kinds, features = map_statement(row, links)
        kind = kinds[0] if kinds else "unmapped"
        if kinds:
            kind_counter.update(kinds)
        else:
            kind_counter["unmapped"] += 1
        case_linked = False
        strict_linked = False
        for feature_id in features:
            feature = links["by_id"].get(feature_id)
            if feature is None:
                continue
            refs = feature.get("refs") or []
            if feature["state"] == "covered" and refs:
                case_linked = True
                verdict = strict.get(feature_id)
                if verdict and verdict.get("ok"):
                    strict_linked = True
        by_doc[row["doc"]][kind] += 1
        statements.append(
            {
                "id": row["id"],
                "doc": row["doc"],
                "line": row["line"],
                "level": row["level"],
                "explicit_must_not": row["explicit_must_not"],
                "mapping": kinds,
                "strongest": kind,
                "features": features,
                "case_linked": case_linked,
                "strict_linked": strict_linked,
            }
        )

    featureid_metric = {
        "total": len(feature_rows),
        "states": dict(Counter(row["state"] for row in feature_rows)),
        "covered": len(covered),
        "exempt": exempt,
        "strict_satisfied": sum(
            1
            for row in feature_rows
            if (row["state"] == "covered" and (row["strict"] or {}).get("ok"))
            or row["state"] == "exempt"
        ),
        "oracle": "tools/features_coverage.py --strict",
    }

    must_metric = {
        "statements": len(must),
        "should_statements": len(should),
        "explicit_must_not": sum(1 for row in must if row["explicit_must_not"]),
        "by_doc": {doc: dict(counter) for doc, counter in sorted(by_doc.items())},
        "mapping": {
            "rule": kind_counter.get("rule", 0),
            "range": kind_counter.get("range", 0),
            "group": kind_counter.get("group", 0),
            "section": kind_counter.get("section", 0),
            "unmapped": kind_counter.get("unmapped", 0),
        },
        "strongest_only": dict(
            Counter(record["strongest"] for record in statements)
        ),
        "case_linked": sum(1 for record in statements if record["case_linked"]),
        "strict_linked": sum(1 for record in statements if record["strict_linked"]),
    }

    return {
        "schema_version": 1,
        "sources": {
            "rules": "spec/language/*.md",
            "features": "spec/language/features.md",
        },
        "denominators": {
            "featureids": len(feature_rows),
            "must_statements": len(must),
        },
        "featureid_metric": featureid_metric,
        "must_metric": must_metric,
        "problems": problems,
        "statements": statements,
    }


def print_summary(report: dict) -> None:
    featureid = report["featureid_metric"]
    must = report["must_metric"]
    print("Metric 1 - FeatureId coverage (oracle: features_coverage.py --strict)")
    print(f"  FeatureIds      : {featureid['total']}")
    print(f"  covered         : {featureid['covered']}")
    print(f"  exempt          : {', '.join(featureid['exempt']) or '-'}")
    print(f"  strict satisfied: {featureid['strict_satisfied']}/{featureid['total']}")
    print()
    print("Metric 2 - individual MUST-statement traceability (this tool)")
    print(f"  MUST statements : {must['statements']} (denominator: rule-table rows with level MUST)")
    print(f"  SHOULD rows     : {must['should_statements']} (excluded from the denominator)")
    print(f"  explicit MUST NOT wording: {must['explicit_must_not']}")
    print()
    mapping = must["mapping"]
    print(f"  reached through a rule id    : {mapping['rule']}")
    print(f"  reached through a rule range : {mapping['range']}")
    print(f"  reached through a minor group: {mapping['group']}")
    print(f"  reached through a section    : {mapping['section']}")
    print(f"  UNMAPPED (no FeatureId)      : {mapping['unmapped']}")
    print()
    print(f"  with an existing case        : {must['case_linked']}/{must['statements']}")
    print(f"  strict-satisfied through one : {must['strict_linked']}/{must['statements']}")
    print()
    print(f"{'document':16} {'MUST':>5} {'rule':>5} {'range':>6} {'group':>6} {'section':>8} {'unmapped':>9}")
    for doc, counter in must["by_doc"].items():
        total = sum(counter.values())
        print(
            f"{doc:16} {total:5} {counter.get('rule', 0):5} {counter.get('range', 0):6} "
            f"{counter.get('group', 0):6} {counter.get('section', 0):8} {counter.get('unmapped', 0):9}"
        )
    unmapped = [record for record in report["statements"] if not record["mapping"]]
    if unmapped:
        print()
        print(f"unmapped MUST statements ({len(unmapped)}):")
        for record in unmapped:
            print(f"  {record['doc']}:{record['line']:<5} {record['id']}")
    if report["problems"]:
        print()
        for problem in report["problems"]:
            print(f"problem: {problem}")


def main(argv: list[str], repo_root: Path | None = None) -> int:
    root = Path(repo_root) if repo_root is not None else REPO
    try:
        report = build_report(root)
    except (OSError, ValueError) as error:
        print(f"cannot parse the specification: {error}", file=sys.stderr)
        return 2

    print_summary(report)

    if "--json" in argv:
        index = argv.index("--json")
        if index + 1 >= len(argv):
            print("--json needs a path", file=sys.stderr)
            return 2
        target = Path(argv[index + 1])
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"\nwrote {target}", file=sys.stderr)

    if "--strict" in argv:
        featureid = report["featureid_metric"]
        must = report["must_metric"]
        unsatisfied = [
            record for record in report["statements"] if not record["mapping"]
        ]
        featureid_bad = featureid["strict_satisfied"] != featureid["total"]
        if unsatisfied or featureid_bad:
            print()
            if unsatisfied:
                print(f"strict: {len(unsatisfied)} MUST statement(s) unmapped to any FeatureId")
            if featureid_bad:
                print(
                    f"strict: FeatureId coverage {featureid['strict_satisfied']}/"
                    f"{featureid['total']} (oracle: features_coverage.py --strict)"
                )
            return 1
        print()
        print(f"strict: every MUST statement is mapped, {must['statements']}/{must['statements']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
