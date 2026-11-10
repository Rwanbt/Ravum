"""Regression tests for the validation gate contracts (P0, 2026-11-10).

These tests reproduce the two historical false-PASS vectors and prove the tools
now fail on them:

* `tools/features_coverage.py` used to exit 1 for `F-META-006` while the gate
  matrix claimed T2 PASS on the strength of a prose exemption. The tool now
  accepts only a machine-readable, justified and reviewed exemption, and rejects
  any attempt to exempt a rule the specification did not mark `NOT_COVERED`.
* `tools/registry_audit.py` now fails both when a source emits a code absent
  from the registry and when the registry promises a code no source emits.

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

import features_coverage  # noqa: E402
import registry_audit  # noqa: E402

EXEMPTION = {
    "rule": "F-META-006",
    "reason": (
        "La specification declare elle-meme la regle non testable : sa colonne "
        "Pass/Fail vaut « aucun (oracle absent) » et MACH-1.4 interdit de citer "
        "un resultat machine comme preuve."
    ),
    "normative_reference": "spec/language/core-machine.md MACH-1.4",
    "review_condition": (
        "Revoquer des qu'une machine abstraite executable existe et que la "
        "couverture stricte peut repasser a 188/188."
    ),
    "approved_by": "gate T2 definition",
    "date": "2026-11-10",
}


def features_document(*rows: str) -> str:
    header = (
        "| FeatureId | Fonctionnalite | Pass / Fail | Statut | Conformance |\n"
        "|---|---|---|---|---|\n"
    )
    return header + "".join(row + "\n" for row in rows)


def write_exemptions(root: Path, *entries: dict) -> None:
    path = root / features_coverage.EXEMPTIONS_REL
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {"schema_version": 1, "exemptions": list(entries)}
    path.write_text(json.dumps(payload), encoding="utf-8")


def run_coverage(root: Path) -> tuple[int, str]:
    module = features_coverage
    saved_min = module.MIN_RULES
    module.MIN_RULES = 1
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = module.main(["--strict"], repo_root=root)
    finally:
        module.MIN_RULES = saved_min
    return code, out.getvalue()


def coverage_root(tmp: Path, document: str) -> Path:
    (tmp / "spec" / "language").mkdir(parents=True, exist_ok=True)
    (tmp / "spec" / "language" / "features.md").write_text(document, encoding="utf-8")
    (tmp / "conformance" / "cases").mkdir(parents=True, exist_ok=True)
    return tmp


class CoverageExemptionContract(unittest.TestCase):
    def test_uncovered_rule_without_exemption_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = coverage_root(Path(tmp), features_document(
                "| `F-META-006` | oracle | pass : aucun (oracle absent) | experimental | NOT_COVERED |"
            ))
            code, out = run_coverage(root)
            self.assertEqual(code, 1, out)
            self.assertIn("uncovered", out)
            self.assertIn("unsat F-META-006", out)

    def test_justified_exemption_passes_and_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = coverage_root(Path(tmp), features_document(
                "| `F-META-006` | oracle | pass : aucun (oracle absent) | experimental | NOT_COVERED |"
            ))
            write_exemptions(root, EXEMPTION)
            code, out = run_coverage(root)
            self.assertEqual(code, 0, out)
            self.assertIn("exempt           : 1", out)
            self.assertIn("1 justified exemption", out)

    def test_exemption_cannot_hide_an_uncited_rule(self) -> None:
        """An empty Conformance cell is a missing test, never an exemption."""
        with tempfile.TemporaryDirectory() as tmp:
            root = coverage_root(Path(tmp), features_document(
                "| `F-META-006` | oracle | pass : aucun (oracle absent) | experimental |  |"
            ))
            write_exemptions(root, EXEMPTION)
            code, out = run_coverage(root)
            self.assertEqual(code, 1, out)
            self.assertIn("must say NOT_COVERED", out)

    def test_stale_exemption_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = coverage_root(Path(tmp), features_document(
                "| `F-TYP-001` | types | pass : ok ; fail : `E-TYP-MISMATCH` | candidate-stable | cases/types/ok |"
            ))
            case = root / "conformance" / "cases" / "types" / "ok"
            case.mkdir(parents=True)
            (case / "case.json").write_text(
                json.dumps({"id": "types/ok", "mode": "compile-pass"}), encoding="utf-8"
            )
            write_exemptions(root, dict(EXEMPTION, rule="F-TYP-001"))
            code, out = run_coverage(root)
            self.assertEqual(code, 1, out)
            self.assertIn("stale exemption", out)

    def test_malformed_or_unknown_exemption_fails(self) -> None:
        cases = [
            (dict(EXEMPTION, reason="TODO"), "at least 40"),
            (dict(EXEMPTION, rule="F-NOPE-999"), "unknown rule"),
            (dict(EXEMPTION, extra="typo"), "unknown field"),
            (dict(EXEMPTION, date="2026-13-40"), "ISO-8601"),
        ]
        for bad, expected in cases:
            with self.subTest(expected=expected), tempfile.TemporaryDirectory() as tmp:
                root = coverage_root(Path(tmp), features_document(
                    "| `F-META-006` | oracle | pass : aucun (oracle absent) | experimental | NOT_COVERED |"
                ))
                write_exemptions(root, bad)
                code, out = run_coverage(root)
                self.assertEqual(code, 1, out)
                self.assertIn(expected, out)


def registry_root(tmp: Path, codes: list[str], source: str) -> Path:
    (tmp / "spec" / "language").mkdir(parents=True, exist_ok=True)
    table = "| Code | Phase | Sens |\n|---|---|---|\n" + "".join(
        f"| `{code}` | types | sens |\n" for code in codes
    )
    (tmp / "spec" / "language" / "diagnostics.md").write_text(table, encoding="utf-8")
    crate = tmp / "crates" / "demo" / "src"
    crate.mkdir(parents=True)
    (crate / "lib.rs").write_text(textwrap.dedent(source), encoding="utf-8")
    return tmp


def run_registry(root: Path) -> tuple[int, str]:
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
        code = registry_audit.main([], repo_root=root)
    return code, out.getvalue()


class RegistryContract(unittest.TestCase):
    def test_unregistered_emission_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = registry_root(
                Path(tmp),
                ["E-TYP-MISMATCH"],
                'fn f() { diags.error("E-FAKE-CODE", span, ""); }',
            )
            code, out = run_registry(root)
            self.assertEqual(code, 1, out)
            self.assertIn("E-FAKE-CODE", out)
            self.assertIn("unregistered   : 1", out)

    def test_registered_emission_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = registry_root(
                Path(tmp),
                ["E-TYP-MISMATCH"],
                'fn f() { diags.error("E-TYP-MISMATCH", span, ""); }',
            )
            code, out = run_registry(root)
            self.assertEqual(code, 0, out)

    def test_registered_but_never_emitted_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = registry_root(
                Path(tmp),
                ["E-TYP-MISMATCH", "E-TYP-PROMISED"],
                'fn f() { diags.error("E-TYP-MISMATCH", span, ""); }',
            )
            code, out = run_registry(root)
            self.assertEqual(code, 1, out)
            self.assertIn("E-TYP-PROMISED", out)
            self.assertIn("never emitted  : 1", out)


if __name__ == "__main__":
    unittest.main()
