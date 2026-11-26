"""Fail-closed tests of run_round3.py: a run counts only with positive evidence."""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_round3 as r3  # noqa: E402

GOOD = {"schema": r3.SCHEMA, "stack": "egui", "rows": 10, "frames": 5, "first_frame_ms": 1.0,
        "interval_ms": {"p50": 1, "p95": 1, "p99": 1, "max": 1}, "os": "linux", "arch": "x86_64"}
PROC = {"exit": 0, "timeout": False, "wall_s": 1.0, "cpu_user_s": 0.5, "cpu_sys_s": 0.1, "peak_rss_kib": 1000}


class Judge(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.rp = Path(self.dir.name) / "r.json"

    def tearDown(self):
        self.dir.cleanup()

    def write(self, rep):
        self.rp.write_text(json.dumps(rep))

    def test_valid_run_passes(self):
        self.write(GOOD)
        run = r3.judge(PROC, self.rp, 5, "egui")
        self.assertEqual(run["verdict"], "PASS")
        self.assertAlmostEqual(run["cpu_ms_per_frame"], 120.0)

    def test_nonzero_exit_fails_even_with_a_report(self):
        self.write(GOOD)
        self.assertEqual(r3.judge(dict(PROC, exit=1), self.rp, 5, "egui")["verdict"], "FAIL")

    def test_timeout_fails(self):
        self.assertEqual(r3.judge({"exit": None, "timeout": True}, self.rp, 5, "egui")["verdict"], "FAIL")

    def test_missing_report_fails(self):
        self.assertEqual(r3.judge(PROC, self.rp, 5, "egui")["verdict"], "FAIL")

    def test_wrong_frame_count_schema_or_stack_fails(self):
        for rep in (dict(GOOD, frames=4), dict(GOOD, schema="other"), dict(GOOD, stack="slint")):
            self.write(rep)
            self.assertEqual(r3.judge(PROC, self.rp, 5, "egui")["verdict"], "FAIL", rep)

    def test_missing_external_measure_fails(self):
        self.write(GOOD)
        p = dict(PROC)
        del p["peak_rss_kib"]
        self.assertEqual(r3.judge(p, self.rp, 5, "egui")["verdict"], "FAIL")


class Measure(unittest.TestCase):
    def test_child_exit_code_and_resources_are_measured(self):
        res = r3.run_measured([sys.executable, "-c", "import sys; sum(range(10**6)); sys.exit(3)"], {}, 60)
        self.assertEqual(res["exit"], 3)
        self.assertGreater(res["cpu_user_s"] + res["cpu_sys_s"], 0)
        self.assertGreater(res["peak_rss_kib"], 0)

    def test_timeout_kills_the_child(self):
        res = r3.run_measured([sys.executable, "-c", "import time; time.sleep(30)"], {}, 0.5)
        self.assertTrue(res["timeout"])
        self.assertIsNone(res["exit"])


if __name__ == "__main__":
    unittest.main()
