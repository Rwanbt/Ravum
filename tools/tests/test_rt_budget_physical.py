"""Tests of tools/qualification/rt_budget_physical.py (V2-G10, fail-closed)."""

from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "qualification" / "rt_budget_physical.py"
OK_LOG = (
    "test block_processing_meets_its_deadline_across_hot_swaps ... ok\n"
    "T16 temporal: {verdict} ({host}) cpu_ns p50=12000 p95=15000 p99=28000 p999=42000 "
    "max=100000 cpu_misses=0 wall_misses=0 preempted=0\n"
    "test result: ok. 11 passed; 0 failed\n"
)


class Campaign(unittest.TestCase):
    def run_with(self, body: str, runs: int = 2, profile: str = "", baselines: dict | None = None):
        tmp = Path(tempfile.mkdtemp())
        fake = tmp / "fake-cargo"
        fake.write_text("#!/bin/sh\n" + textwrap.dedent(body))
        fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
        args = [sys.executable, str(SCRIPT), "--runs", str(runs), "--out", str(tmp / "out")]
        if profile:
            args += ["--profile", profile]
        if baselines is not None:
            b = tmp / "b.json"
            b.write_text(json.dumps({"profiles": baselines}))
            args += ["--baselines", str(b)]
        env = dict(os.environ, RAVUM_RT_CARGO=str(fake))
        p = subprocess.run(args, capture_output=True, text=True, env=env)
        report = json.loads((tmp / "out" / "report.json").read_text())
        return p.returncode, report

    def emit(self, verdict: str, host: str = "host not identified as virtual") -> str:
        return f"cat <<'EOF'\n{OK_LOG.format(verdict=verdict, host=host)}EOF\nexit 0\n"

    def test_all_runs_pass(self):
        code, r = self.run_with(self.emit("PASS"))
        self.assertEqual((code, r["verdict"]), (0, "PASS"))
        self.assertEqual(len(r["runs"]), 2)
        self.assertIn("commit", r)
        self.assertIn("dirty", r)

    def test_compilation_error_is_fail(self):
        code, r = self.run_with("echo 'error[E0425]: cannot find value' >&2\nexit 101\n")
        self.assertEqual((code, r["verdict"]), (1, "FAIL"))

    def test_panic_is_fail(self):
        code, r = self.run_with(
            "echo 'test block_processing_meets_its_deadline_across_hot_swaps ... FAILED'\nexit 101\n"
        )
        self.assertEqual((code, r["verdict"]), (1, "FAIL"))

    def test_exit_zero_without_evidence_is_fail(self):
        # No temporal verdict line: success of Cargo is not a qualification.
        code, r = self.run_with("echo 'test result: ok. 0 passed'\nexit 0\n")
        self.assertEqual((code, r["verdict"]), (1, "FAIL"))

    def test_interruption_is_not_run(self):
        code, r = self.run_with("exit 130\n")
        self.assertEqual((code, r["verdict"]), (3, "NOT_RUN"))

    def test_virtual_host_is_inconclusive_never_pass(self):
        code, r = self.run_with(self.emit("INCONCLUSIVE", "virtualized runner"))
        self.assertEqual((code, r["verdict"]), (2, "INCONCLUSIVE"))

    def test_zero_runs_is_not_run(self):
        code, r = self.run_with(self.emit("PASS"), runs=0)
        self.assertEqual((code, r["verdict"]), (3, "NOT_RUN"))

    def test_regression_against_baseline_is_fail(self):
        code, r = self.run_with(
            self.emit("PASS"), profile="ref", baselines={"ref": {"p50": 5000, "p99": 28000, "p999": 42000}}
        )
        self.assertEqual((code, r["verdict"]), (1, "FAIL"))
        self.assertIn("regression", r["reason"])

    def test_within_baseline_is_pass(self):
        code, r = self.run_with(
            self.emit("PASS"), profile="ref", baselines={"ref": {"p50": 12000, "p99": 28000, "p999": 42000}}
        )
        self.assertEqual((code, r["verdict"]), (0, "PASS"))


if __name__ == "__main__":
    unittest.main()
