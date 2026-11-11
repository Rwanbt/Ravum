"""Regression tests for `tools/must_audit.py` (P1-D, 2026-11-11).

The tool exists because "188/188 FeatureIds covered" is not the same claim as
"every MUST statement is traceable". These tests pin the second metric's
mechanics on synthetic repositories, then smoke-test the real one:

* only rule-table rows with level MUST enter the denominator (SHOULD does not);
* a rule is reached through an exact id, a range, a minor group or a section,
  and the four kinds are reported separately instead of being merged;
* an unmapped statement is reported as unmapped — never silently counted as
  covered by the exemption machinery;
* a case link requires an *existing* case directory and the strict verdict of
  `features_coverage.py`; a dangling citation does not count;
* `--strict` is opt-in and exits 1 on a real gap, while the default is
  report-only (the tool changes no existing gate);
* a row containing an unescaped `|` is still counted and the escaping defect is
  reported.

Run with: python -m unittest discover -s tools/tests -v
"""

from __future__ import annotations

import contextlib
import io
import json
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tools"))

import must_audit  # noqa: E402

FEATURES_HEADER = (
    "| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Conformance |\n"
    "|---|---|---|---|---|---|---|\n"
)


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(textwrap.dedent(text).lstrip("\n"), encoding="utf-8")


def case(root: Path, tree: str, name: str, payload: dict) -> None:
    write(root / "conformance" / "cases" / tree / name / "case.json", json.dumps(payload))


def features_doc(rows: str) -> str:
    """A `features.md` table with contiguous rows (a blank line ends a table)."""
    return FEATURES_HEADER + textwrap.dedent(rows).lstrip("\n")


def make_repo(root: Path, rows: str, feature_rows: str) -> None:
    write(
        root / "spec" / "language" / "types.md",
        "## 1. Section one\n\n| ID | Règle | Niveau |\n|---|---|---|\n"
        + textwrap.dedent(rows).lstrip("\n"),
    )
    write(root / "spec" / "language" / "features.md", features_doc(feature_rows))


def run(argv: list[str], root: Path) -> tuple[int, str]:
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        code = must_audit.main(argv, root)
    return code, buffer.getvalue()


class MustCountTests(unittest.TestCase):
    def test_only_must_rows_enter_the_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                """
                | TYP-1.1 | first | MUST |
                | TYP-1.2 | second | SHOULD |
                | TYP-1.3 | third | MUST |
                """,
                """
                | F-TYP-001 | first | TYP-1.1 | — | — | pass : ok | cases/types/ok |
                | F-TYP-002 | third | TYP-1.3 | — | — | pass : ok | cases/types/ok |
                """,
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            report = must_audit.build_report(root)
            self.assertEqual(report["must_metric"]["statements"], 2)
            self.assertEqual(report["must_metric"]["should_statements"], 1)
            self.assertEqual(report["denominators"]["must_statements"], 2)

    def test_non_rule_tables_are_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(root, "| TYP-1.1 | first | MUST |\n", "")
            write(
                root / "spec" / "language" / "grammar.md",
                """
                | Niveau | Opérateurs | Associativité |
                |---|---|---|
                | 1 | `*` | gauche |
                """,
            )
            report = must_audit.build_report(root)
            self.assertEqual(report["must_metric"]["statements"], 1)

    def test_unescaped_pipe_is_counted_and_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(root, "| TYP-1.1 | `a || b` | MUST |\n", "")
            report = must_audit.build_report(root)
            self.assertEqual(report["must_metric"]["statements"], 1)
            self.assertTrue(
                any("unescaped `|`" in problem for problem in report["problems"]),
                report["problems"],
            )


class MappingKindTests(unittest.TestCase):
    def test_exact_range_group_and_section_are_distinguished(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(
                root / "spec" / "language" / "types.md",
                """
                ## 1. One
                | ID | Règle | Niveau |
                |---|---|---|
                | TYP-1.1 | exact | MUST |
                | TYP-1.2 | range | MUST |
                | TYP-1.3 | range | MUST |
                ## 2. Two
                | ID | Règle | Niveau |
                |---|---|---|
                | TYP-2.1 | group | MUST |
                | TYP-2.2 | section | MUST |
                """,
            )
            write(
                root / "spec" / "language" / "features.md",
                features_doc(
                    """
                    | F-TYP-001 | exact | TYP-1.1 | — | — | pass : ok | cases/types/ok |
                    | F-TYP-002 | range | TYP-1.2–1.3 | — | — | pass : ok | cases/types/ok |
                    | F-TYP-003 | group | TYP-2 | — | — | pass : ok | cases/types/ok |
                    | F-TYP-004 | section | types §2 | — | — | pass : ok | cases/types/ok |
                    """
                ),
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            report = must_audit.build_report(root)
            by_id = {entry["id"]: entry for entry in report["statements"]}
            self.assertEqual(by_id["TYP-1.1"]["mapping"], ["rule"])
            # The left edge of a range is also an exact token, so the statement
            # is reached both ways; the kind list keeps every route, and the
            # strongest kind (`rule`) is what `strongest_only` counts.
            self.assertEqual(by_id["TYP-1.2"]["mapping"], ["rule", "range"])
            self.assertEqual(by_id["TYP-1.3"]["mapping"], ["range"])
            self.assertEqual(by_id["TYP-1.2"]["strongest"], "rule")
            self.assertIn("group", by_id["TYP-2.1"]["mapping"])
            self.assertIn("section", by_id["TYP-2.2"]["mapping"])
            self.assertEqual(report["must_metric"]["mapping"]["unmapped"], 0)

    def test_unmapped_statement_is_not_counted_as_covered(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                """
                | TYP-1.1 | mapped | MUST |
                | TYP-9.9 | orphan | MUST |
                """,
                "| F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok | cases/types/ok |\n",
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            report = must_audit.build_report(root)
            self.assertEqual(report["must_metric"]["mapping"]["unmapped"], 1)
            self.assertEqual(report["must_metric"]["case_linked"], 1)
            orphan = [entry for entry in report["statements"] if entry["id"] == "TYP-9.9"][0]
            self.assertEqual(orphan["mapping"], [])
            self.assertFalse(orphan["case_linked"])
            self.assertFalse(orphan["strict_linked"])


class CaseLinkTests(unittest.TestCase):
    def test_dangling_case_is_not_case_linked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                "| TYP-1.1 | mapped | MUST |\n",
                "| F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok | cases/types/missing |\n",
            )
            report = must_audit.build_report(root)
            self.assertEqual(report["featureid_metric"]["states"].get("dangling"), 1)
            self.assertEqual(report["must_metric"]["case_linked"], 0)

    def test_failing_case_is_not_strict_linked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                "| TYP-1.1 | mapped | MUST |\n",
                "| F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok ; fail : E-TYP-MISMATCH | "
                "cases/types/ok |\n",
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            report = must_audit.build_report(root)
            self.assertEqual(report["must_metric"]["case_linked"], 1)
            # The strict criterion needs a failing case asserting the mandated
            # code; a single accepting case is not enough.
            self.assertEqual(report["must_metric"]["strict_linked"], 0)

    def test_exemption_is_counted_as_satisfied_but_has_no_case(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                "| TYP-1.1 | mapped | MUST |\n",
                """
                | F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok | cases/types/ok |
                | F-META-999 | none | — | — | — | pass : aucun (oracle absent) | NOT_COVERED |
                """,
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            write(
                root / "tools" / "coverage_exemptions.json",
                json.dumps(
                    {
                        "schema_version": 1,
                        "exemptions": [
                            {
                                "rule": "F-META-999",
                                "reason": (
                                    "La spécification déclare elle-même la règle non "
                                    "testable et aucun oracle n'existe pour elle."
                                ),
                                "normative_reference": "spec/language/core-machine.md MACH-1.4",
                                "review_condition": (
                                    "Révoquer dès qu'un oracle exécutable existe pour la règle."
                                ),
                                "approved_by": "gate T2 definition",
                                "date": "2026-11-10",
                            }
                        ],
                    }
                ),
            )
            report = must_audit.build_report(root)
            self.assertEqual(report["featureid_metric"]["exempt"], ["F-META-999"])
            self.assertEqual(report["featureid_metric"]["strict_satisfied"], 2)
            self.assertEqual(report["must_metric"]["case_linked"], 1)


class CliTests(unittest.TestCase):
    def test_default_is_report_only_and_strict_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                """
                | TYP-1.1 | mapped | MUST |
                | TYP-9.9 | orphan | MUST |
                """,
                "| F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok | cases/types/ok |\n",
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            code, _ = run([], root)
            self.assertEqual(code, 0)
            code, output = run(["--strict"], root)
            self.assertEqual(code, 1)
            self.assertIn("unmapped", output)

    def test_json_report_is_machine_readable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_repo(
                root,
                "| TYP-1.1 | mapped | MUST |\n",
                "| F-TYP-001 | mapped | TYP-1.1 | — | — | pass : ok | cases/types/ok |\n",
            )
            case(root, "types", "ok", {"id": "types/ok", "mode": "compile-pass"})
            target = root / "out" / "audit.json"
            code, _ = run(["--json", str(target)], root)
            self.assertEqual(code, 0)
            payload = json.loads(target.read_text(encoding="utf-8"))
            self.assertEqual(payload["schema_version"], 1)
            self.assertEqual(payload["denominators"]["featureids"], 1)
            self.assertEqual(payload["denominators"]["must_statements"], 1)
            self.assertEqual(payload["statements"][0]["id"], "TYP-1.1")


class RealRepositoryTests(unittest.TestCase):
    """Pin the published denominator on the real specification.

    A large drop means the table parser stopped seeing normative rows; a large
    rise means rules were added. Either way the MUST-TRACEABILITY report and the
    gate matrix have to be updated deliberately, not by accident.
    """

    def test_real_spec_is_parsed_and_report_only_passes(self) -> None:
        report = must_audit.build_report(REPO)
        self.assertEqual(report["denominators"]["featureids"], 188)
        self.assertGreaterEqual(report["denominators"]["must_statements"], 800)
        self.assertLessEqual(report["denominators"]["must_statements"], 900)
        self.assertEqual(report["featureid_metric"]["strict_satisfied"], 188)
        code, _ = run([], REPO)
        self.assertEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
