"""Rules of `spec/language/features.md` about the table itself (FEAT-1, FEAT-12).

* FEAT-1.3, FEAT-12.3: an `unsupported` row cites a code that `unsupported.md`
  lists, and at least one cited case is a negative case that expects it.
* FEAT-12.4: every diagnostic code cited in a FeatureId row exists in the
  registry (or is a toolchain code, `registry_audit.TOOLING_CODES`).
* FEAT-12.5: the rows marked `experimental` are exactly the ones FEAT-12.5 names.
* FEAT-1.6: the registry has no `reserved` code, and no case expects a code
  outside the registry, so no case can pass on a code nothing emits.
* FEAT-1.4 is held by `test_diagnostic_contract.py` (`feature_ids.snapshot`).
"""
from __future__ import annotations

import json
import re
import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tools"))

import features_coverage  # noqa: E402
import registry_audit  # noqa: E402

FEATURES = REPO / "spec" / "language" / "features.md"
UNSUPPORTED = REPO / "spec" / "language" / "unsupported.md"
REGISTRY = REPO / "spec" / "language" / "diagnostics.md"
CASES = REPO / "conformance" / "cases"
CODE = re.compile(r"\b[EW]-[A-Z0-9]+(?:-[A-Z0-9]+)*\b")
CASE_REF = re.compile(r"cases/([a-z0-9_-]+/[a-z0-9_.-]+)")


def rows() -> list[dict]:
    parsed, problems = features_coverage.parse_rows(FEATURES.read_text(encoding="utf-8"))
    assert not problems, problems
    return parsed


def row_text(row: dict) -> str:
    lines = FEATURES.read_text(encoding="utf-8").splitlines()
    return lines[row["line"] - 1]


def case(case_id: str) -> dict | None:
    path = CASES / case_id / "case.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


class FeatureTable(unittest.TestCase):
    def test_unsupported_rows_have_a_listed_code_and_a_negative_case(self) -> None:
        listed = set(CODE.findall(UNSUPPORTED.read_text(encoding="utf-8")))
        problems = []
        unsupported = [row for row in rows() if row["status"] == "unsupported"]
        self.assertGreater(len(unsupported), 0)
        for row in unsupported:
            codes = set(CODE.findall(row_text(row)))
            if not codes & listed:
                problems.append(f"{row['id']}: no code listed in unsupported.md ({sorted(codes)})")
            negative = False
            for case_id in CASE_REF.findall(row["conformance"]):
                spec = case(case_id)
                if spec and spec.get("mode") == "compile-fail" and set(spec.get("expect_codes", [])) & codes:
                    negative = True
            if not negative:
                problems.append(f"{row['id']}: no compile-fail case expects {sorted(codes)}")
        self.assertEqual(problems, [])

    def test_every_cited_code_is_registered(self) -> None:
        # Only the FeatureId rows: the table of renamed codes (section 13)
        # lists old corpus names on purpose. A bare family such as `E-MOD`
        # (`E-MOD-*`) names a prefix, not a code.
        registered = registry_audit.registry_codes(REGISTRY) | registry_audit.TOOLING_CODES
        cited = set()
        for row in rows():
            cited |= set(CODE.findall(row_text(row)))
        unknown = cited - registered - registry_audit.PREFIX_LITERALS
        self.assertEqual(sorted(unknown), [])

    def test_experimental_rows_are_the_ones_feat_12_5_names(self) -> None:
        text = FEATURES.read_text(encoding="utf-8")
        rule = next(line for line in text.splitlines() if line.startswith("| FEAT-12.5 |"))
        named = set(re.findall(r"F-[A-Z]+-\d{3}", rule))
        marked = {row["id"] for row in rows() if row["status"] == "experimental"}
        self.assertEqual(marked, named)

    def test_no_case_expects_a_reserved_or_unknown_code(self) -> None:
        registry = REGISTRY.read_text(encoding="utf-8")
        self.assertIn("aucun code `reserved` ne subsiste", registry)
        # Tooling codes (`E-TOOL`, `E-BUILD`, ...) come from the toolchain, not
        # the language registry (`registry_audit.TOOLING_CODES`).
        registered = registry_audit.registry_codes(REGISTRY) | registry_audit.TOOLING_CODES
        unknown = []
        for path in sorted(CASES.glob("*/*/case.json")):
            spec = json.loads(path.read_text(encoding="utf-8"))
            for code in spec.get("expect_codes", []):
                if code not in registered:
                    unknown.append(f"{spec['id']}: {code}")
        self.assertEqual(unknown, [])


if __name__ == "__main__":
    unittest.main()
