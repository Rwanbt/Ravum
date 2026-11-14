#!/usr/bin/env python3
"""Performance regression alarm for `ravum-lang check`.

What this tool is
-----------------
It compares a fresh `scripts/lang-timings.ps1 -CheckOnly` export with the
committed reference `docs/qualification/timings-baseline.json`. For every
example and every check mode ("cold", "edit") it compares the total p50 and p95
against a *ceiling*:

    ceiling = max(floor(stat), baseline_value * factor)

Two floors, because the two statistics do not carry the same noise:

* **p50** — `max(100 ms, baseline * 3)`: the stable signal. A sustained
  regression raises it.
* **p95** — `max(500 ms, baseline * 3)`: with 10 samples the nearest-rank p95
  is the maximum, so it is fail-on-one-outlier. Observed on a shared runner:
  p50 = 2.3 ms, p95 = 238 ms for the same example. This ceiling only catches a
  catastrophic stall.

The gate is therefore a **coarse alarm**: it catches an order-of-magnitude
regression, not a 20 % drift. The full JSON export is uploaded by the workflow
so that the trend can be inspected, and the baseline can be refreshed
deliberately.

This is not a performance budget: `docs/qualification/PERFORMANCE.md` publishes
the measured figures and their conditions; a green gate says "no gross
regression on this runner", never "the budget is met".

Linearity gate (wave 3, S8)
----------------------------
Ceilings on the examples cannot catch a superlinear algorithm: the examples are
too small. The gate therefore also accepts `--scale <json>`, the
`scale_corpus` section of `scripts/lang-timings.ps1 -Scale` (or an equivalent
measurement with the same schema), and checks the growth of the `check` total
between the two generated corpora of **5 110** and **20 110 lines**:

    e = ln(total_p50(20 110) / total_p50(5 110)) / ln(20 110 / 5 110)

`e <= 1.3` is required. The ratio is machine-independent, so the check runs on
the same shared runner as the alarm; it is an **algorithmic-growth alarm**
(quadratic accumulation, accidental O(n²) copies), not a latency budget. The
measured exponent is published in `docs/reports/LANG-TIMINGS.md`.

Exit codes: 0 all ceilings hold, 1 at least one ceiling is exceeded (or the
baseline/measurement is incomplete), 2 invalid input.

The module is importable: `evaluate()` returns rows, violations and warnings so
that `tools/tests/test_timings_gate.py` can exercise the rules without running
the compiler.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

MODES = ("cold", "edit")
STATS = ("p50", "p95")

# S8 (owner decision, 2026-11-14): the correctness of the `check` growth is
# pinned on two generated corpora and an exponent ceiling.
LINEARITY_SMALL_LINES = 5110
LINEARITY_LARGE_LINES = 20110
LINEARITY_CEILING = 1.3


def _read(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise SystemExit(f"timings_gate: {path} does not exist") from None
    except json.JSONDecodeError as error:
        raise SystemExit(f"timings_gate: {path} is not valid JSON: {error}") from None


def _examples(document: dict, label: str) -> dict[str, dict]:
    examples = document.get("examples")
    if not isinstance(examples, list) or not examples:
        raise SystemExit(f"timings_gate: {label} has no non-empty 'examples' list")
    by_name: dict[str, dict] = {}
    for entry in examples:
        name = entry.get("name")
        if not isinstance(name, str) or not name:
            raise SystemExit(f"timings_gate: {label} contains an example without a name")
        if name in by_name:
            raise SystemExit(f"timings_gate: {label} contains duplicate example {name!r}")
        by_name[name] = entry
    return by_name


def _check_mode(example: dict, name: str, mode: str, label: str) -> dict:
    try:
        return example["check"][mode]
    except (KeyError, TypeError):
        raise SystemExit(f"timings_gate: {label} example {name!r} has no check.{mode} measurement") from None


def _total(measurement: dict, name: str, mode: str, stat: str, label: str) -> float:
    try:
        value = measurement["total"][stat]
    except (KeyError, TypeError):
        raise SystemExit(f"timings_gate: {label} example {name!r} check.{mode} has no total.{stat}") from None
    try:
        return float(value)
    except (TypeError, ValueError):
        raise SystemExit(f"timings_gate: {label} example {name!r} check.{mode} total.{stat} is not a number") from None


def evaluate(
    baseline: dict,
    measured: dict,
    *,
    factor: float = 3.0,
    p50_floor_ms: float = 100.0,
    p95_floor_ms: float = 500.0,
) -> tuple[list[dict], list[str], list[str]]:
    """Apply the ceilings; return (rows, violations, warnings).

    The two statistics do not carry the same noise. With 10 samples, the
    nearest-rank p95 is the **maximum**: one scheduler or filesystem outlier
    fails it (observed on a shared runner: 238 ms while the p50 was 2.3 ms).
    The gate therefore uses a tight ceiling on p50, the stable signal of a
    sustained regression, and a deliberately loose ceiling on p95 that only
    catches a catastrophic stall.
    """
    if factor <= 0:
        raise ValueError("factor must be > 0")
    if p50_floor_ms < 0 or p95_floor_ms < 0:
        raise ValueError("the floors must be >= 0")

    floors = {"p50": p50_floor_ms, "p95": p95_floor_ms}
    expected = _examples(baseline, "baseline")
    actual = _examples(measured, "measurement")

    warnings: list[str] = []
    violations: list[str] = []
    rows: list[dict] = []

    for name in sorted(expected):
        if name not in actual:
            violations.append(f"{name}: missing from the measurement (the example exists in the baseline)")
            continue
        for mode in MODES:
            reference = _check_mode(expected[name], name, mode, "baseline")
            candidate = _check_mode(actual[name], name, mode, "measurement")
            for stat in STATS:
                base_value = _total(reference, name, mode, stat, "baseline")
                measured_value = _total(candidate, name, mode, stat, "measurement")
                floor = floors[stat]
                ceiling = max(floor, base_value * factor)
                rows.append(
                    {
                        "example": name,
                        "mode": mode,
                        "stat": stat,
                        "baseline_ms": base_value,
                        "measured_ms": measured_value,
                        "ceiling_ms": ceiling,
                        "ok": measured_value <= ceiling,
                    }
                )
                if measured_value > ceiling:
                    violations.append(
                        f"{name}/{mode}/{stat}: measured {measured_value:.3f} ms > ceiling "
                        f"{ceiling:.3f} ms (baseline {base_value:.3f} ms, floor {floor:.3f} ms, factor {factor:g})"
                    )

    for name in sorted(set(actual) - set(expected)):
        warnings.append(f"{name}: measured but absent from the baseline (add it deliberately, then regenerate the baseline)")

    if measured.get("check_only") is False:
        violations.append("measurement is not a --CheckOnly export; the baseline covers check only")

    return rows, violations, warnings


def _scale_points(scale: dict) -> dict[int, dict]:
    """The `scale_corpus` section, keyed by exact source line count.

    Fail-closed on anything that would silently weaken the exponent: a missing
    or empty section, a non-numeric/absent p50, a non-positive time or line
    count, and duplicate sizes are all rejected.
    """
    entries = scale.get("scale_corpus")
    if not isinstance(entries, list) or not entries:
        raise SystemExit("timings_gate: the scale measurement has no non-empty 'scale_corpus' list")
    points: dict[int, dict] = {}
    for entry in entries:
        try:
            lines = int(entry["source_lines"])
            p50 = float(entry["check"]["phases"]["total"]["p50"])
        except (KeyError, TypeError, ValueError):
            raise SystemExit(
                "timings_gate: a scale_corpus entry has no numeric "
                "'source_lines' / 'check.phases.total.p50'"
            ) from None
        name = entry.get("name")
        if lines <= 0 or p50 <= 0:
            raise SystemExit(
                f"timings_gate: scale_corpus entry {name!r} has a non-positive line count or p50"
            )
        if lines in points:
            raise SystemExit(f"timings_gate: duplicate scale_corpus entry for {lines} lines")
        points[lines] = {"name": name, "p50_ms": p50}
    return points


def linearity_row(
    scale: dict,
    *,
    small_lines: int = LINEARITY_SMALL_LINES,
    large_lines: int = LINEARITY_LARGE_LINES,
) -> dict:
    """Measured exponent `e` of `total_p50 ∝ lines^e` between two corpora."""
    if small_lines <= 0 or large_lines <= 0:
        raise ValueError("the corpus sizes must be > 0")
    if large_lines <= small_lines:
        raise ValueError("large_lines must be greater than small_lines")
    points = _scale_points(scale)
    small = points.get(small_lines)
    large = points.get(large_lines)
    if small is None or large is None:
        present = ", ".join(str(lines) for lines in sorted(points)) or "none"
        raise SystemExit(
            f"timings_gate: the linearity check needs the {small_lines}-line and "
            f"{large_lines}-line corpora (found: {present})"
        )
    exponent = math.log(large["p50_ms"] / small["p50_ms"]) / math.log(
        large_lines / small_lines
    )
    return {
        "small_lines": small_lines,
        "small_p50_ms": small["p50_ms"],
        "large_lines": large_lines,
        "large_p50_ms": large["p50_ms"],
        "exponent": exponent,
    }


def check_exponent(
    scale: dict,
    *,
    small_lines: int = LINEARITY_SMALL_LINES,
    large_lines: int = LINEARITY_LARGE_LINES,
) -> float:
    """The `check` growth exponent between the two generated corpora."""
    return linearity_row(
        scale, small_lines=small_lines, large_lines=large_lines
    )["exponent"]


def evaluate_linearity(
    scale: dict,
    *,
    ceiling: float = LINEARITY_CEILING,
    small_lines: int = LINEARITY_SMALL_LINES,
    large_lines: int = LINEARITY_LARGE_LINES,
) -> tuple[dict, list[str]]:
    """Apply the exponent ceiling; return (row, violations).

    A ceiling of 1.3 admits the log-shaped overhead of ordered maps and the
    constant factors of a shared accumulator, and refuses quadratic growth: the
    median case N ×19.3 of time between 5 110 and 20 110 lines would be e=2.
    """
    if ceiling <= 0:
        raise ValueError("the linearity ceiling must be > 0")
    row = linearity_row(scale, small_lines=small_lines, large_lines=large_lines)
    row["ceiling"] = ceiling
    row["ok"] = row["exponent"] <= ceiling
    violations: list[str] = []
    if not row["ok"]:
        violations.append(
            f"linearity: check exponent {row['exponent']:.3f} > ceiling {ceiling:g} "
            f"between {small_lines} and {large_lines} lines "
            f"({row['small_p50_ms']:.3f} ms -> {row['large_p50_ms']:.3f} ms)"
        )
    return row, violations


def _report(
    rows: list[dict],
    violations: list[str],
    warnings: list[str],
    factor: float,
    p50_floor_ms: float,
    p95_floor_ms: float,
    linearity: dict | None = None,
) -> None:
    width = max((len(row["example"]) for row in rows), default=7)
    print(f"{'example':<{width}}  mode  stat   baseline     measured      ceiling  verdict")
    for row in rows:
        verdict = "PASS" if row["ok"] else "FAIL"
        print(
            f"{row['example']:<{width}}  {row['mode']:<4}  {row['stat']:<4} "
            f"{row['baseline_ms']:>10.3f}  {row['measured_ms']:>10.3f}  {row['ceiling_ms']:>12.3f}  {verdict}"
        )
    print()
    print(
        f"ceiling = max(floor, baseline * {factor:g}), floor p50={p50_floor_ms:g} ms "
        f"p95={p95_floor_ms:g} ms; {len(rows)} comparisons"
    )
    if linearity is not None:
        verdict = "PASS" if linearity["ok"] else "FAIL"
        print(
            f"linearity: check p50 {linearity['small_p50_ms']:.3f} ms at "
            f"{linearity['small_lines']} lines -> {linearity['large_p50_ms']:.3f} ms at "
            f"{linearity['large_lines']} lines; exponent {linearity['exponent']:.3f} "
            f"(ceiling {linearity['ceiling']:g})  {verdict}"
        )
    for warning in warnings:
        print(f"timings_gate: warning: {warning}")
    if violations:
        for violation in violations:
            print(f"::error::timings gate: {violation}")
        print(f"timings_gate: FAIL — {len(violations)} ceiling(s) exceeded or incomplete")
    else:
        print("timings_gate: PASS — every check total is under its coarse ceiling")
        print("timings_gate: reminder: this is a regression alarm, not a performance budget.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Coarse performance regression alarm for ravum-lang check.")
    parser.add_argument("--baseline", required=True, type=Path, help="reference JSON (docs/qualification/timings-baseline.json)")
    parser.add_argument("--input", required=True, type=Path, help="measurement JSON from scripts/lang-timings.ps1 -CheckOnly")
    parser.add_argument("--factor", type=float, default=3.0, help="multiplier applied to the baseline (default 3.0)")
    parser.add_argument("--p50-floor-ms", type=float, default=100.0, help="absolute p50 ceiling floor in ms (default 100)")
    parser.add_argument("--p95-floor-ms", type=float, default=500.0, help="absolute p95 ceiling floor in ms (default 500)")
    parser.add_argument(
        "--scale",
        type=Path,
        help="scale measurement JSON (scale_corpus with 5 110 and 20 110 lines); adds the linearity gate",
    )
    parser.add_argument(
        "--exponent-ceiling",
        type=float,
        default=LINEARITY_CEILING,
        help=f"maximum check growth exponent (default {LINEARITY_CEILING:g})",
    )
    parser.add_argument(
        "--small-lines",
        type=int,
        default=LINEARITY_SMALL_LINES,
        help=f"small generated corpus size in lines (default {LINEARITY_SMALL_LINES})",
    )
    parser.add_argument(
        "--large-lines",
        type=int,
        default=LINEARITY_LARGE_LINES,
        help=f"large generated corpus size in lines (default {LINEARITY_LARGE_LINES})",
    )
    args = parser.parse_args(argv)

    baseline = _read(args.baseline)
    measured = _read(args.input)
    rows, violations, warnings = evaluate(
        baseline,
        measured,
        factor=args.factor,
        p50_floor_ms=args.p50_floor_ms,
        p95_floor_ms=args.p95_floor_ms,
    )
    linearity = None
    if args.scale is not None:
        scale = _read(args.scale)
        linearity, linearity_violations = evaluate_linearity(
            scale,
            ceiling=args.exponent_ceiling,
            small_lines=args.small_lines,
            large_lines=args.large_lines,
        )
        violations.extend(linearity_violations)
    _report(
        rows,
        violations,
        warnings,
        args.factor,
        args.p50_floor_ms,
        args.p95_floor_ms,
        linearity=linearity,
    )
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
