#!/usr/bin/env python3
"""Performance regression alarm for `ravum-lang check`.

What this tool is
-----------------
It compares a fresh `scripts/lang-timings.ps1 -CheckOnly` export with the
committed reference `docs/qualification/timings-baseline.json`. For every
example and every check mode ("cold", "edit") it compares the total p50 and p95
against a *ceiling*:

    ceiling = max(total_floor_ms, baseline_value * factor)

The floor absorbs the difference between the reference machine and a shared
CI runner (and ordinary noise); the factor absorbs a slower CPU. The gate is
therefore a **coarse alarm**: it catches an order-of-magnitude regression, not
a 20 % drift. The full JSON export is uploaded by the workflow so that the
trend can be inspected, and the baseline can be refreshed deliberately.

This is not a performance budget: `docs/qualification/PERFORMANCE.md` publishes
the measured figures and their conditions; a green gate says "no gross
regression on this runner", never "the budget is met".

Exit codes: 0 all ceilings hold, 1 at least one ceiling is exceeded (or the
baseline/measurement is incomplete), 2 invalid input.

The module is importable: `evaluate()` returns rows, violations and warnings so
that `tools/tests/test_timings_gate.py` can exercise the rules without running
the compiler.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

MODES = ("cold", "edit")
STATS = ("p50", "p95")


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
    total_floor_ms: float = 200.0,
) -> tuple[list[dict], list[str], list[str]]:
    """Apply the ceilings; return (rows, violations, warnings)."""
    if factor <= 0:
        raise ValueError("factor must be > 0")
    if total_floor_ms < 0:
        raise ValueError("total_floor_ms must be >= 0")

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
                ceiling = max(total_floor_ms, base_value * factor)
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
                        f"{ceiling:.3f} ms (baseline {base_value:.3f} ms, floor {total_floor_ms:.3f} ms, factor {factor:g})"
                    )

    for name in sorted(set(actual) - set(expected)):
        warnings.append(f"{name}: measured but absent from the baseline (add it deliberately, then regenerate the baseline)")

    if measured.get("check_only") is False:
        violations.append("measurement is not a --CheckOnly export; the baseline covers check only")

    return rows, violations, warnings


def _report(rows: list[dict], violations: list[str], warnings: list[str], factor: float, floor: float) -> None:
    width = max((len(row["example"]) for row in rows), default=7)
    print(f"{'example':<{width}}  mode  stat   baseline     measured      ceiling  verdict")
    for row in rows:
        verdict = "PASS" if row["ok"] else "FAIL"
        print(
            f"{row['example']:<{width}}  {row['mode']:<4}  {row['stat']:<4} "
            f"{row['baseline_ms']:>10.3f}  {row['measured_ms']:>10.3f}  {row['ceiling_ms']:>12.3f}  {verdict}"
        )
    print()
    print(f"ceiling = max({floor:g} ms, baseline * {factor:g}); {len(rows)} comparisons")
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
    parser.add_argument("--total-floor-ms", type=float, default=200.0, help="absolute ceiling floor in ms (default 200)")
    args = parser.parse_args(argv)

    baseline = _read(args.baseline)
    measured = _read(args.input)
    rows, violations, warnings = evaluate(
        baseline,
        measured,
        factor=args.factor,
        total_floor_ms=args.total_floor_ms,
    )
    _report(rows, violations, warnings, args.factor, args.total_floor_ms)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
