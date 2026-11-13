#!/usr/bin/env python3
"""Split uncovered lines into product code and `#[cfg(test)]` scaffolding.

The raw line-coverage number mixes two very different things: source lines a
user can reach, and the asserts, panic paths and helper arms of the in-crate
test modules. A green test run can never cover the second group (an `assert!`
message only runs when a test fails), so counting it as a coverage gap makes
the objective look further away than it is -- and, worse, points the next
batch of tests at test code.

This script reads the LLVM coverage JSON produced by
`cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen --json --output-path`
and reports, per file:

    prod_unc   uncovered lines before the first `#[cfg(test)]`
    test_unc   uncovered lines inside the test block
    prod_lin   lines with an executable region before `#[cfg(test)]`
    prod%      line coverage of product code only

Regions are rebuilt from the export's segment table: every segment with
`is_region_entry` starts a region that runs to the next entry, and a line is
covered when at least one non-gap region on it has a non-zero count.

Usage:
    python tools/coverage_product_split.py --input target/cov-now.json
    python tools/coverage_product_split.py --input target/cov-now.json --file loader.rs

Exit codes: 0 = OK, 1 = bad input.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOP_LEVEL_DIRS = ("crates/", "tools/", "apps/", "examples/", "benchmarks/", "tests/", "src/")
TEST_ATTRIBUTE = "#[cfg(test)]"


def relpath(filename: str) -> str:
    """Normalise an llvm-cov filename to a repo-relative POSIX path."""
    posix = filename.replace("\\", "/")
    for marker in TOP_LEVEL_DIRS:
        index = posix.find("/" + marker)
        if index >= 0:
            return posix[index + 1 :]
        if posix.startswith(marker):
            return posix
    return posix


def file_lines(filename: str, cache: dict[str, list[str]]) -> list[str] | None:
    if filename not in cache:
        path = ROOT / filename
        try:
            cache[filename] = path.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            cache[filename] = None  # type: ignore[assignment]
    return cache[filename]


def test_module_start(lines: list[str]) -> int | None:
    """1-based line of the first `#[cfg(test)]`, or None for a product-only file."""
    for index, line in enumerate(lines, start=1):
        if line.strip().startswith(TEST_ATTRIBUTE):
            return index
    return None


def line_sets(segments: list[list[int]]) -> tuple[set[int], set[int]]:
    """Rebuild covered and uncovered lines from an llvm-cov segment table."""
    entries = []
    for segment in segments:
        line, _col, count, has_count, is_entry, is_gap = segment[:6]
        entries.append((line, count if has_count else None, bool(is_entry), bool(is_gap)))

    covered: set[int] = set()
    uncovered: set[int] = set()
    for index, (line, count, is_entry, is_gap) in enumerate(entries):
        if not is_entry or is_gap or count is None:
            continue
        end = entries[index + 1][0] if index + 1 < len(entries) else line
        for current in range(line, max(line, end) + 1):
            if count > 0:
                covered.add(current)
            else:
                uncovered.add(current)
    uncovered -= covered
    return covered, uncovered


def analyse(document: dict, only: str | None) -> tuple[list[tuple], dict]:
    cache: dict[str, list[str]] = {}
    rows = []
    merge: dict[str, dict] = {}
    for export in document.get("data", []):
        for item in export.get("files", []):
            name = relpath(item["filename"])
            if only and only not in name:
                continue
            lines = file_lines(name, cache)
            if lines is None:
                continue
            start = test_module_start(lines)
            covered, uncovered = line_sets(item["segments"])

            if start is None:
                product = covered | uncovered
                product_uncovered = len(uncovered)
                test_uncovered = 0
            else:
                product = {line for line in covered | uncovered if line < start}
                product_uncovered = len([line for line in uncovered if line < start])
                test_uncovered = len([line for line in uncovered if line >= start])

            # A file may appear once per test binary; keep the best
            # instantiation so duplicated compilation units do not double count.
            current = merge.get(name)
            if current is None or product_uncovered < current["prod_unc"]:
                merge[name] = {
                    "prod_unc": product_uncovered,
                    "test_unc": test_uncovered,
                    "prod_lin": len(product),
                }

    for name, values in merge.items():
        rows.append((name, values["prod_unc"], values["test_unc"], values["prod_lin"]))
    totals = {
        "prod_unc": sum(row[1] for row in rows),
        "test_unc": sum(row[2] for row in rows),
        "prod_lin": sum(row[3] for row in rows),
    }
    return rows, totals


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--file", default=None, help="restrict to paths containing this string")
    parser.add_argument("--check", action="store_true", help="exit 1 if any product line is uncovered")
    args = parser.parse_args()

    try:
        document = json.loads(args.input.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print("cannot read %s: %s" % (args.input, error), file=sys.stderr)
        return 1

    rows, totals = analyse(document, args.file)
    if not rows:
        print("no measured file matched", file=sys.stderr)
        return 1

    print("%-58s %8s %8s %8s %7s" % ("file", "prod_unc", "test_unc", "prod_lin", "prod%"))
    for name, product_uncovered, test_uncovered, product_lines in sorted(
        rows, key=lambda row: (-row[1], row[0])
    ):
        percent = 100.0 * (product_lines - product_uncovered) / product_lines if product_lines else 100.0
        print(
            "%-58s %8d %8d %8d %6.2f%%"
            % (name, product_uncovered, test_uncovered, product_lines, percent)
        )

    total_lines = totals["prod_lin"]
    percent = (
        100.0 * (total_lines - totals["prod_unc"]) / total_lines if total_lines else 100.0
    )
    print(
        "%-58s %8d %8d %8d %6.2f%%"
        % ("TOTAL", totals["prod_unc"], totals["test_unc"], total_lines, percent)
    )
    print(
        "\nproduct line coverage %.2f%%: %d uncovered product line(s), %d uncovered line(s)"
        " inside #[cfg(test)] modules (assert messages, panic paths, helper arms)."
        % (percent, totals["prod_unc"], totals["test_unc"])
    )

    if args.check and totals["prod_unc"]:
        print("\n--check: %d product line(s) still uncovered" % totals["prod_unc"], file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
