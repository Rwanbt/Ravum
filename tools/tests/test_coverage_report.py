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

    def test_module_aggregate_holds_when_every_part_passes(self):
        # Splitting a file re-baselines its per-file floors; the aggregate keeps
        # the floor the whole file had before the split.
        result = self.run_tool(
            [
                entry("crates/ravum-lang-codegen/src/emit/mod.rs", 92, 100),
                entry("crates/ravum-lang-codegen/src/emit/emitter_types.rs", 56, 100),
            ],
            {
                "files": {
                    "crates/ravum-lang-codegen/src/emit/mod.rs": {"lines": 90, "regions": 90},
                    "crates/ravum-lang-codegen/src/emit/emitter_types.rs": {"lines": 56, "regions": 56},
                },
                "modules": {"crates/ravum-lang-codegen/src/emit/": {"lines": 74, "regions": 74}},
            },
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_module_aggregate_fails_even_when_each_part_passes(self):
        # 60 + 100 -> aggregate 80 < 90 even though both parts meet (or beat)
        # their own floors. This is the regression a per-file gate would hide.
        result = self.run_tool(
            [
                entry("crates/ravum-lang-codegen/src/emit/mod.rs", 100, 100),
                entry("crates/ravum-lang-codegen/src/emit/emitter_calls.rs", 60, 100),
            ],
            {
                "files": {
                    "crates/ravum-lang-codegen/src/emit/mod.rs": {"lines": 90, "regions": 90},
                    "crates/ravum-lang-codegen/src/emit/emitter_calls.rs": {"lines": 60, "regions": 60},
                },
                "modules": {"crates/ravum-lang-codegen/src/emit/": {"lines": 90, "regions": 90}},
            },
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("module lines", result.stderr)

    def test_module_without_measured_files_fails(self):
        result = self.run_tool(
            [entry("crates/ravum-lang-codegen/src/lib.rs", 100, 100)],
            {"modules": {"crates/ravum-lang-codegen/src/emit/": {"lines": 90, "regions": 90}}},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("no measured file matches", result.stderr)


if __name__ == "__main__":
    unittest.main()
