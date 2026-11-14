"""Contrats de `tools/timings_gate.py`.

Le gate compare un export `lang-timings -CheckOnly` à la référence versionnée.
Ces tests figent les règles qui pourraient être « ajustées » pour obtenir du
vert : plancher, facteur, exemple manquant, export non-check. Ils n'exécutent
aucun compilateur.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "tools"))

import timings_gate  # noqa: E402


def example(name: str, cold_ms: float, edit_ms: float = None, *, phases: bool = False) -> dict:
    edit_ms = cold_ms if edit_ms is None else edit_ms
    mode = {
        "total": {"p50": cold_ms, "p95": cold_ms},
    }
    if phases:
        mode["typeck"] = {"p50": cold_ms / 3.0, "p95": cold_ms / 2.0}
    return {
        "name": name,
        "check": {
            "cold": mode,
            "edit": {
                "total": {"p50": edit_ms, "p95": edit_ms},
                **({"typeck": {"p50": edit_ms / 3.0, "p95": edit_ms / 2.0}} if phases else {}),
            },
        },
    }


def document(*examples: dict) -> dict:
    return {"schema_version": 1, "check_only": True, "examples": list(examples)}


class EvaluateTest(unittest.TestCase):
    def test_identical_measurements_pass(self):
        rows, violations, warnings = timings_gate.evaluate(document(example("hello", 5.0)), document(example("hello", 5.0)))
        self.assertEqual(len(rows), 4)  # cold/edit x p50/p95
        self.assertEqual(violations, [])
        self.assertEqual(warnings, [])

    def test_a_coarse_regression_fails(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)), document(example("hello", 500.0)), total_floor_ms=200.0
        )
        self.assertEqual(len(violations), 4)
        self.assertTrue(any("hello/cold/p95" in violation for violation in violations))

    def test_the_floor_absorbs_a_slow_runner(self):
        # 5 ms * 3 = 15 ms, below the 200 ms floor: a 30 ms runner passes.
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)), document(example("hello", 30.0)), total_floor_ms=200.0
        )
        self.assertEqual(violations, [])

    def test_the_floor_cannot_be_used_to_hide_a_large_regression(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)), document(example("hello", 201.0)), total_floor_ms=200.0
        )
        self.assertTrue(violations)

    def test_a_missing_example_fails(self):
        _rows, violations, _warnings = timings_gate.evaluate(document(example("hello", 5.0)), document(example("other", 5.0)))
        self.assertEqual(len(violations), 1)
        self.assertIn("missing", violations[0])

    def test_an_unknown_example_is_a_warning_not_a_failure(self):
        _rows, violations, warnings = timings_gate.evaluate(
            document(example("hello", 5.0)), document(example("hello", 5.0), example("new", 5.0))
        )
        self.assertEqual(violations, [])
        self.assertEqual(len(warnings), 1)
        self.assertIn("new", warnings[0])

    def test_a_non_check_export_fails(self):
        measured = document(example("hello", 5.0))
        measured["check_only"] = False
        _rows, violations, _warnings = timings_gate.evaluate(document(example("hello", 5.0)), measured)
        self.assertTrue(violations)

    def test_missing_measurement_field_fails_closed(self):
        broken = document(example("hello", 5.0))
        del broken["examples"][0]["check"]["cold"]["total"]
        with self.assertRaises(SystemExit):
            timings_gate.evaluate(document(example("hello", 5.0)), broken)

    def test_no_baseline_example_fails_closed(self):
        with self.assertRaises(SystemExit):
            timings_gate.evaluate(document(), document(example("hello", 5.0)))

    def test_invalid_factor_rejected(self):
        with self.assertRaises(ValueError):
            timings_gate.evaluate(document(example("hello", 5.0)), document(example("hello", 5.0)), factor=0.0)


if __name__ == "__main__":
    unittest.main()
