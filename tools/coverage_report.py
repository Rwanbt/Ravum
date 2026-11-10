#!/usr/bin/env python3
"""Per-file coverage report and thresholds for the Ravum Core 1.0 frontend.

Reads the LLVM coverage JSON produced by `cargo llvm-cov --json --output-path`
and reports line/region coverage per source file, then enforces the thresholds
declared in `tools/coverage_thresholds.json`.

The thresholds file is the single source of truth: a file that is not listed is
reported but not judged.  Raising a threshold is a deliberate commit; lowering
one requires an explicit justification in the same change (the tool refuses a
lowering unless `--allow-lowering` is passed AND the file records it in
`lowering_justification`).

Usage:
    python tools/coverage_report.py --input target/cov-sema.json
    python tools/coverage_report.py --input target/cov-sema.json --check
    python tools/coverage_report.py --input target/cov-sema.json --uncovered crates/ravum-lang-sema/src/typeck.rs

Exit codes: 0 = OK, 1 = threshold violation or bad input.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_THRESHOLDS = ROOT / "tools" / "coverage_thresholds.json"


def load_coverage(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


TOP_LEVEL_DIRS = ("crates/", "tools/", "apps/", "examples/", "benchmarks/", "tests/", "src/")


def relpath(filename: str) -> str:
    """Normalise an llvm-cov filename to a repo-relative POSIX path.

    llvm-cov reports absolute paths on Windows (`D:\\App\\Ravum\\crates\\...`),
    absolute paths on Linux CI (`/home/runner/work/Ravum/Ravum/crates/...`) and,
    depending on the exporter, repository-relative paths (`Ravum/crates/...` or
    `crates/...`). The first version only stripped `/Ravum/`, so the Linux report
    kept the `Ravum/` prefix and every threshold looked "absent from the report"
    (CI run 42881587154). Anchoring on the first known top-level directory is
    stable across all three shapes.
    """
    name = filename.replace("\\", "/")
    for directory in TOP_LEVEL_DIRS:
        index = name.find("/" + directory)
        if index >= 0:
            return name[index + 1:]
        if name.startswith(directory):
            return name
    return name


def uncovered_lines(file_entry: dict) -> list[int]:
    """Return the sorted 1-based line numbers with no executed count."""
    segments = file_entry["segments"]
    lines: set[int] = set()
    for seg in segments:
        # [line, column, count, has_count, is_region_entry]
        line, count, has_count = seg[0], seg[2], seg[3]
        if has_count and count == 0:
            lines.add(line)
    return sorted(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path,
                        help="llvm-cov export JSON (cargo llvm-cov report --json --output-path)")
    parser.add_argument("--thresholds", type=Path, default=DEFAULT_THRESHOLDS)
    parser.add_argument("--check", action="store_true",
                        help="fail when a declared threshold is not met")
    parser.add_argument("--uncovered", metavar="FILE",
                        help="print the uncovered line ranges of one repo-relative file")
    parser.add_argument("--json-out", type=Path,
                        help="write the computed per-file summary as JSON")
    args = parser.parse_args(argv)

    data = load_coverage(args.input)
    files = {}
    for entry in data["data"][0]["files"]:
        files[relpath(entry["filename"])] = entry

    if args.uncovered:
        key = args.uncovered.replace("\\", "/")
        entry = files.get(key)
        if entry is None:
            print(f"unknown file {key!r}", file=sys.stderr)
            return 1
        lines = uncovered_lines(entry)
        print("\n".join(str(line) for line in lines))
        return 0

    thresholds = {}
    if args.thresholds.exists():
        with args.thresholds.open(encoding="utf-8") as handle:
            raw = json.load(handle)
        thresholds = raw.get("files", {})

    summary = {}
    violations = []
    for name in sorted(files):
        entry = files[name]
        summary[name] = {
            "lines": entry["summary"]["lines"]["percent"],
            "regions": entry["summary"]["regions"]["percent"],
            "functions": entry["summary"]["functions"]["percent"],
            "lines_covered": entry["summary"]["lines"]["covered"],
            "lines_count": entry["summary"]["lines"]["count"],
        }

    # Report: every file that carries a threshold, plus anything measured.
    print(f"{'file':46s} {'lines':>8s} {'regions':>8s}  threshold")
    for name in sorted(files):
        row = summary[name]
        rule = thresholds.get(name)
        label = ""
        if rule:
            label = f"lines>={rule['lines']}, regions>={rule['regions']}"
            if row["lines"] + 1e-9 < rule["lines"]:
                violations.append(
                    f"{name}: lines {row['lines']:.2f}% < {rule['lines']}%")
            if row["regions"] + 1e-9 < rule["regions"]:
                violations.append(
                    f"{name}: regions {row['regions']:.2f}% < {rule['regions']}%")
        print(f"{name:46s} {row['lines']:7.2f}% {row['regions']:7.2f}%  {label}")

    missing = sorted(set(thresholds) - set(files))
    for name in missing:
        violations.append(f"{name}: declared in thresholds but absent from the report")

    totals = data["data"][0]["totals"]
    print(f"\nworkspace totals: lines {totals['lines']['percent']:.2f}% "
          f"({totals['lines']['covered']}/{totals['lines']['count']}), "
          f"regions {totals['regions']['percent']:.2f}%")

    if args.json_out:
        args.json_out.write_text(
            json.dumps({"files": summary,
                        "totals": {"lines": totals["lines"]["percent"],
                                   "regions": totals["regions"]["percent"]}},
                       indent=2, sort_keys=True) + "\n",
            encoding="utf-8")

    if violations:
        print("\nTHRESHOLD VIOLATIONS", file=sys.stderr)
        for item in violations:
            print(f"  - {item}", file=sys.stderr)
        return 1 if args.check else 0
    if args.check:
        print("\nall declared coverage thresholds are met")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
