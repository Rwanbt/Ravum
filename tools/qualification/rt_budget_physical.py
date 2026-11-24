#!/usr/bin/env python3
"""V2-G10: temporal qualification of T16 (`rt_budget`) on a physical machine.

Fail-closed: a run is PASS only with positive evidence, never because a word
such as FAILED is absent.

For each of N runs (N > 0), the test binary is run with Cargo and its full log
is kept. A run counts as evidence only if **all** of the following hold:
  * Cargo's exit code is 0 (a compilation error, a panic or a failed test
    gives a non-zero code);
  * the deadline test was actually executed and reported `ok`;
  * the log has exactly one `T16 temporal:` verdict line with its statistics.

Verdicts, overall:
  PASS          every run PASS, and no regression against the baseline of the
                declared profile (when one exists);
  FAIL          a run failed, gave no or incomplete evidence, or regressed;
  INCONCLUSIVE  every run ran, but at least one host was identified as virtual
                (its temporal verdict is INCONCLUSIVE, never PASS);
  NOT_RUN       zero runs requested, or the campaign was interrupted.

Exit codes: PASS 0, FAIL 1, INCONCLUSIVE 2, NOT_RUN 3. The report (JSON) and
the logs go to the output directory.

The command can be replaced for the script's own tests by the environment
variable RAVUM_RT_CARGO (a program taking the same arguments as `cargo`).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path

DEADLINE_TEST = "block_processing_meets_its_deadline_across_hot_swaps"
VERDICT_RE = re.compile(
    r"^T16 temporal: (PASS|INCONCLUSIVE) \((.*?)\) cpu_ns p50=(\d+) p95=(\d+) p99=(\d+) "
    r"p999=(\d+) max=(\d+) cpu_misses=(\d+) wall_misses=(\d+) preempted=(\d+)$"
)
TEST_ARGS = [
    "test", "--release", "-p", "ravum-lang-jit", "--test", "rt_budget",
    "--", "--nocapture", "--test-threads=1",
]
EXIT = {"PASS": 0, "FAIL": 1, "INCONCLUSIVE": 2, "NOT_RUN": 3}
# A regression is a statistic above this multiple of its baseline.
REGRESSION_FACTOR = 2.0


def git(*args: str) -> str:
    try:
        return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def host_facts() -> dict:
    cpu = platform.processor() or "unknown"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    try:
        rustc = subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip()
    except OSError:
        rustc = "unknown"
    return {
        "os": f"{platform.system()} {platform.release()} {platform.machine()}",
        "cpu": cpu,
        "rustc": rustc or "unknown",
        "profile": "release",
    }


def judge_run(code: int, log: str) -> dict:
    """Verdict of one run from its exit code and full log (fail-closed)."""
    run = {"exit_code": code, "verdict": "FAIL", "reason": "", "stats": None}
    if code in (130, 143, -2, -15):
        run.update(verdict="NOT_RUN", reason="interrupted")
        return run
    if code != 0:
        run["reason"] = f"cargo exited with {code}"
        return run
    if not re.search(rf"^test {DEADLINE_TEST} \.\.\. ok$", log, re.M):
        run["reason"] = "the deadline test did not run or did not pass"
        return run
    verdicts = [m for m in (VERDICT_RE.match(l) for l in log.splitlines()) if m]
    if len(verdicts) != 1:
        run["reason"] = f"expected exactly one temporal verdict line, found {len(verdicts)}"
        return run
    m = verdicts[0]
    keys = ["p50", "p95", "p99", "p999", "max", "cpu_misses", "wall_misses", "preempted"]
    run["stats"] = dict(zip(keys, (int(x) for x in m.groups()[2:])))
    run["host"] = m.group(2)
    run["verdict"] = m.group(1)
    return run


def regressions(stats: dict, baseline: dict | None) -> list[str]:
    if not baseline:
        return []
    out = []
    for k in ("p50", "p99", "p999"):
        b = baseline.get(k)
        if b and stats[k] > REGRESSION_FACTOR * b:
            out.append(f"{k}={stats[k]}ns > {REGRESSION_FACTOR}x baseline {b}ns")
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--out", type=Path, default=None, help="output directory (default: dated)")
    ap.add_argument("--profile", default=os.environ.get("RAVUM_RT_PROFILE", ""),
                    help="hardware/software profile name, for the baseline comparison")
    ap.add_argument("--baselines", type=Path,
                    default=Path(__file__).with_name("rt_budget_baselines.json"))
    args = ap.parse_args(argv)

    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = args.out or Path(f"rt-budget-{stamp}")
    out.mkdir(parents=True, exist_ok=True)
    report = {
        "schema": "ravum.rt_budget.physical/v1",
        "date": stamp,
        "commit": git("rev-parse", "HEAD"),
        "dirty": git("status", "--porcelain") not in ("", "unknown"),
        "host": host_facts(),
        "profile": args.profile or None,
        "runs_requested": args.runs,
        "runs": [],
    }
    baseline = None
    if args.profile and args.baselines.exists():
        baseline = json.loads(args.baselines.read_text()).get("profiles", {}).get(args.profile)
    report["baseline"] = baseline

    def finish(verdict: str, reason: str) -> int:
        report["verdict"], report["reason"] = verdict, reason
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"rt_budget physique : {verdict} ({reason}) — rapport : {out / 'report.json'}")
        return EXIT[verdict]

    if args.runs <= 0:
        return finish("NOT_RUN", "zero runs requested")
    cargo = os.environ.get("RAVUM_RT_CARGO", "cargo")
    try:
        for i in range(1, args.runs + 1):
            p = subprocess.run([cargo, *TEST_ARGS], capture_output=True, text=True)
            log = p.stdout + p.stderr
            (out / f"run-{i}.log").write_text(log)
            run = judge_run(p.returncode, log)
            if run["stats"] and run["verdict"] == "PASS":
                reg = regressions(run["stats"], baseline)
                if reg:
                    run.update(verdict="FAIL", reason="regression: " + "; ".join(reg))
            report["runs"].append(run)
            if run["verdict"] == "NOT_RUN":
                return finish("NOT_RUN", f"run {i} interrupted")
    except KeyboardInterrupt:
        return finish("NOT_RUN", "campaign interrupted")
    verdicts = [r["verdict"] for r in report["runs"]]
    if "FAIL" in verdicts:
        bad = next(r for r in report["runs"] if r["verdict"] == "FAIL")
        return finish("FAIL", f"{verdicts.count('FAIL')} run(s) failed; first: {bad['reason']}")
    if "INCONCLUSIVE" in verdicts:
        return finish("INCONCLUSIVE", "a host was identified as virtual: temporal qualification not possible")
    note = "no baseline for this profile" if baseline is None else "no regression against the baseline"
    return finish("PASS", f"{len(verdicts)} run(s), {note}")


if __name__ == "__main__":
    sys.exit(main())
