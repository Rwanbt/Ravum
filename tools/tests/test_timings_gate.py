"""Contrats de `tools/timings_gate.py`.

Le gate compare un export `lang-timings -CheckOnly` à la référence versionnée.
Ces tests figent les règles qui pourraient être « ajustées » pour obtenir du
vert : planchers p50/p95, facteur, exemple manquant, export non-check. Ils
n'exécutent aucun compilateur.

Depuis la vague 3 (S8), le gate porte aussi le contrôle de linéarité : entre les
corpus générés de 5 110 et 20 110 lignes, l'exposant de `check` doit rester
≤ 1,3. Les tests ci-dessous vérifient qu'une croissance linéaire passe, qu'une
croissance quadratique échoue, et que toute donnée incomplète échoue fermée.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "tools"))

import timings_gate  # noqa: E402


def example(name: str, cold_p50: float, cold_p95: float = None, edit_p50: float = None, edit_p95: float = None) -> dict:
    cold_p95 = cold_p50 if cold_p95 is None else cold_p95
    edit_p50 = cold_p50 if edit_p50 is None else edit_p50
    edit_p95 = cold_p95 if edit_p95 is None else edit_p95
    return {
        "name": name,
        "check": {
            "cold": {"total": {"p50": cold_p50, "p95": cold_p95}},
            "edit": {"total": {"p50": edit_p50, "p95": edit_p95}},
        },
    }


def document(*examples: dict) -> dict:
    return {"schema_version": 1, "check_only": True, "examples": list(examples)}


def scale_document(points: dict[int, float]) -> dict:
    """A `scale_corpus` measurement, one entry per generated line count."""
    return {
        "schema_version": 1,
        "scale_corpus": [
            {
                "name": f"scale-{lines}",
                "source_lines": lines,
                "check": {"phases": {"total": {"p50": value}}},
            }
            for lines, value in sorted(points.items())
        ],
    }


class EvaluateTest(unittest.TestCase):
    def test_identical_measurements_pass(self):
        rows, violations, warnings = timings_gate.evaluate(document(example("hello", 5.0)), document(example("hello", 5.0)))
        self.assertEqual(len(rows), 4)  # cold/edit x p50/p95
        self.assertEqual(violations, [])
        self.assertEqual(warnings, [])

    def test_a_coarse_regression_fails_both_statistics(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)),
            document(example("hello", 500.0)),
            p50_floor_ms=100.0,
            p95_floor_ms=500.0,
        )
        # p50 fails; p95 = 500 is exactly at the loose ceiling.
        self.assertEqual(len(violations), 2)
        self.assertTrue(any("hello/cold/p50" in violation for violation in violations))

    def test_the_p50_floor_absorbs_a_slow_runner(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)),
            document(example("hello", 30.0)),
            p50_floor_ms=100.0,
            p95_floor_ms=500.0,
        )
        self.assertEqual(violations, [])

    def test_the_p50_floor_cannot_be_used_to_hide_a_large_regression(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("hello", 5.0)),
            document(example("hello", 150.0)),
            p50_floor_ms=100.0,
            p95_floor_ms=500.0,
        )
        self.assertTrue(violations)

    def test_a_single_p95_outlier_is_tolerated_because_p95_is_the_max(self):
        # Real shape of a shared runner: p50 stable at 2 ms, one 238 ms outlier.
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("fib", 2.0, 6.0)),
            document(example("fib", 2.3, 238.0)),
            p50_floor_ms=100.0,
            p95_floor_ms=500.0,
        )
        self.assertEqual(violations, [])

    def test_a_catastrophic_outlier_fails(self):
        _rows, violations, _warnings = timings_gate.evaluate(
            document(example("fib", 2.0, 6.0)),
            document(example("fib", 2.3, 900.0)),
            p50_floor_ms=100.0,
            p95_floor_ms=500.0,
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


class LinearityTest(unittest.TestCase):
    """S8: the `check` exponent between the 5 110- and 20 110-line corpora."""

    RATIO = 20110 / 5110

    def test_the_exponent_formula_is_applied_to_the_p50_totals(self):
        # t = base * n^0.5: the measured exponent must be 0.5.
        scale = scale_document({5110: 100.0, 20110: 100.0 * self.RATIO**0.5})
        self.assertAlmostEqual(timings_gate.check_exponent(scale), 0.5, places=9)

    def test_linear_growth_is_accepted(self):
        scale = scale_document({5110: 300.0, 20110: 300.0 * self.RATIO})
        row, violations = timings_gate.evaluate_linearity(scale)
        self.assertAlmostEqual(row["exponent"], 1.0, places=9)
        self.assertTrue(row["ok"])
        self.assertEqual(violations, [])

    def test_quadratic_growth_is_refused(self):
        # The pre-fix shape: ×19.3 of time for ×3.93 of lines is e = 2.
        scale = scale_document({5110: 365.0, 20110: 365.0 * self.RATIO**2})
        row, violations = timings_gate.evaluate_linearity(scale)
        self.assertAlmostEqual(row["exponent"], 2.0, places=9)
        self.assertFalse(row["ok"])
        self.assertEqual(len(violations), 1)
        self.assertIn("linearity", violations[0])
        self.assertIn("exponent 2.000", violations[0])

    def test_the_ceiling_is_the_one_that_decides(self):
        linear = scale_document({5110: 300.0, 20110: 300.0 * self.RATIO})
        _row, violations = timings_gate.evaluate_linearity(linear, ceiling=1.3)
        self.assertEqual(violations, [])
        _row, violations = timings_gate.evaluate_linearity(linear, ceiling=0.9)
        self.assertEqual(len(violations), 1)

    def test_a_high_ceiling_cannot_be_used_to_accept_quadratic_growth(self):
        quadratic = scale_document({5110: 300.0, 20110: 300.0 * self.RATIO**2})
        _row, violations = timings_gate.evaluate_linearity(quadratic, ceiling=1.3)
        self.assertEqual(len(violations), 1)

    def test_a_missing_corpus_size_fails_closed(self):
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent(scale_document({5110: 300.0}))
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent(scale_document({20110: 3000.0}))

    def test_an_empty_scale_section_fails_closed(self):
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent({"scale_corpus": []})
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent({})

    def test_a_non_positive_time_fails_closed(self):
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent(scale_document({5110: 0.0, 20110: 3000.0}))

    def test_a_missing_p50_fails_closed(self):
        broken = scale_document({5110: 300.0, 20110: 3000.0})
        del broken["scale_corpus"][0]["check"]["phases"]["total"]["p50"]
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent(broken)

    def test_duplicate_corpus_sizes_fail_closed(self):
        duplicated = scale_document({5110: 300.0, 20110: 3000.0})
        duplicated["scale_corpus"].append(dict(duplicated["scale_corpus"][0]))
        with self.assertRaises(SystemExit):
            timings_gate.check_exponent(duplicated)

    def test_an_invalid_ceiling_or_order_is_rejected(self):
        linear = scale_document({5110: 300.0, 20110: 1200.0})
        with self.assertRaises(ValueError):
            timings_gate.evaluate_linearity(linear, ceiling=0.0)
        with self.assertRaises(ValueError):
            timings_gate.check_exponent(linear, small_lines=20110, large_lines=5110)


if __name__ == "__main__":
    unittest.main()
