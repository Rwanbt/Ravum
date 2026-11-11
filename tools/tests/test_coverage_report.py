"""Contrats de `tools/coverage_report.py`.

Le 2026-11-10, la CI de couverture (run 42881587154) a échoué parce que le
normalisateur de chemins ne retirait que le préfixe `/Ravum/` : llvm-cov produait
`Ravum/crates/...` sur Linux, donc chaque seuil déclaré semblait « absent du
rapport ». Ces tests figent la normalisation pour les trois formes observées.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
TOOL = REPO / "tools" / "coverage_report.py"
sys.path.insert(0, str(REPO / "tools"))

import coverage_report  # noqa: E402


def entry(filename: str, covered: int, count: int) -> dict:
    return {
        "filename": filename,
        "segments": [],
        "summary": {
            "lines": {"percent": 100.0 * covered / count, "covered": covered, "count": count},
            "regions": {"percent": 100.0 * covered / count, "covered": covered, "count": count},
            "functions": {"percent": 100.0, "covered": 1, "count": 1},
        },
    }


class RelpathTest(unittest.TestCase):
    def test_windows_absolute(self):
        self.assertEqual(
            coverage_report.relpath("D:\\App\\Ravum\\crates\\ravum-lang-sema\\src\\typeck.rs"),
            "crates/ravum-lang-sema/src/typeck.rs",
        )

    def test_linux_absolute(self):
        self.assertEqual(
            coverage_report.relpath("/home/runner/work/Ravum/Ravum/crates/x/src/lib.rs"),
            "crates/x/src/lib.rs",
        )

    def test_ci_repository_relative_with_prefix(self):
        # This is the shape that broke the gate.
        self.assertEqual(
            coverage_report.relpath("Ravum/crates/ravum-lang-sema/src/builtins.rs"),
            "crates/ravum-lang-sema/src/builtins.rs",
        )

    def test_already_relative(self):
        self.assertEqual(
            coverage_report.relpath("crates/ravum-lang-sema/src/own.rs"),
            "crates/ravum-lang-sema/src/own.rs",
        )

    def test_tools_path_is_preserved(self):
        self.assertEqual(
            coverage_report.relpath("Ravum/tools/conformance/src/runner.rs"),
            "tools/conformance/src/runner.rs",
        )


class CheckTest(unittest.TestCase):
    def run_tool(self, files: list[dict], thresholds: dict) -> subprocess.CompletedProcess:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            report = tmp_path / "cov.json"
            report.write_text(json.dumps({"data": [{"files": files, "totals": {
                "lines": {"percent": 90.0, "covered": 9, "count": 10},
                "regions": {"percent": 90.0, "covered": 9, "count": 10},
            }}]}), encoding="utf-8")
            rules = tmp_path / "thresholds.json"
            rules.write_text(json.dumps(thresholds), encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(TOOL), "--input", str(report),
                 "--thresholds", str(rules), "--check"],
                capture_output=True, text=True,
            )

    def test_prefixed_linux_path_matches_its_threshold(self):
        result = self.run_tool(
            [entry("Ravum/crates/ravum-lang-sema/src/typeck.rs", 93, 100)],
            {"files": {"crates/ravum-lang-sema/src/typeck.rs": {"lines": 90, "regions": 85}}},
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_below_floor_fails(self):
        result = self.run_tool(
            [entry("crates/ravum-lang-sema/src/typeck.rs", 80, 100)],
            {"files": {"crates/ravum-lang-sema/src/typeck.rs": {"lines": 90, "regions": 85}}},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("THRESHOLD VIOLATIONS", result.stderr)

    def test_declared_but_absent_fails(self):
        result = self.run_tool(
            [entry("crates/ravum-lang-sema/src/types.rs", 100, 100)],
            {"files": {"crates/ravum-lang-sema/src/typeck.rs": {"lines": 90, "regions": 85}}},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("absent from the report", result.stderr)


if __name__ == "__main__":
    unittest.main()
