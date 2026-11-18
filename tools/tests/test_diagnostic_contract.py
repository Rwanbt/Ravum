"""Evolution rules of `spec/language/diagnostics.md` §9 and DIAG-1.2 / 6.6.

* DIAG-9.1, DIAG-9.2, DIAG-9.5: a published code is never removed, reused or
  given another severity. `tools/diagnostic_codes.snapshot` lists every code
  ever published; the registry must keep each one, with the same `E`/`W`
  prefix (the prefix is the severity). New codes are allowed and must be
  appended to the snapshot in the same change.
* DIAG-1.2, DIAG-6.6, DIAG-9.4: messages are not contractual, so no
  conformance case may assert the text of a diagnostic message. A case may
  assert codes, severities, phases, spans and the `error[CODE]:` prefix.
"""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tools"))

import registry_audit  # noqa: E402

SNAPSHOT = REPO / "tools" / "diagnostic_codes.snapshot"
REGISTRY = REPO / "spec" / "language" / "diagnostics.md"
CASES = REPO / "conformance" / "cases"


def snapshot_codes() -> list[str]:
    lines = SNAPSHOT.read_text(encoding="utf-8").splitlines()
    return [line.strip() for line in lines if line.strip() and not line.startswith("#")]


class PublishedCodesAreImmutable(unittest.TestCase):
    def test_every_published_code_is_still_registered(self) -> None:
        registry = registry_audit.registry_codes(REGISTRY)
        missing = [code for code in snapshot_codes() if code not in registry]
        self.assertEqual(missing, [], "a published code was removed or renamed")

    def test_every_registered_code_is_in_the_snapshot(self) -> None:
        published = set(snapshot_codes())
        new = sorted(registry_audit.registry_codes(REGISTRY) - published)
        self.assertEqual(new, [], "append new codes to tools/diagnostic_codes.snapshot")

    def test_the_snapshot_has_no_duplicate(self) -> None:
        codes = snapshot_codes()
        self.assertEqual(len(codes), len(set(codes)))

    def test_codes_keep_their_severity_prefix(self) -> None:
        for code in snapshot_codes():
            self.assertRegex(code, r"^[EW]-[A-Z0-9-]+$")


class NoCaseAssertsAMessage(unittest.TestCase):
    def test_no_case_asserts_a_diagnostic_message(self) -> None:
        offenders = []
        for spec in sorted(CASES.glob("*/*/case.json")):
            case = json.loads(spec.read_text(encoding="utf-8"))
            for key in ("stdout_contains", "stderr_contains"):
                value = case.get(key) or ""
                if '"message"' in value:
                    offenders.append(f"{case['id']}: {key}")
        self.assertEqual(offenders, [])



def snapshot(name: str) -> list[str]:
    lines = (REPO / "tools" / name).read_text(encoding="utf-8").splitlines()
    return [line.strip() for line in lines if line.strip() and not line.startswith("#")]


class PublishedIdentifiersAreImmutable(unittest.TestCase):
    """DIAG-9.6, COMPAT-2.9: a `rule_id` or `feature_id` is never renamed."""

    def current_rule_ids(self) -> set[str]:
        import must_audit

        rows, _ = must_audit.parse_rule_tables(REPO)
        return {row["id"] for row in rows}

    def current_feature_ids(self) -> set[str]:
        import re

        text = (REPO / "spec" / "language" / "features.md").read_text(encoding="utf-8")
        return set(re.findall(r"^\| (F-[A-Z]+-\d{3}) \|", text, re.M))

    def test_no_published_rule_id_disappears(self) -> None:
        missing = sorted(set(snapshot("rule_ids.snapshot")) - self.current_rule_ids())
        self.assertEqual(missing, [], "a published rule id was renamed or removed")

    def test_new_rule_ids_are_appended_to_the_snapshot(self) -> None:
        new = sorted(self.current_rule_ids() - set(snapshot("rule_ids.snapshot")))
        self.assertEqual(new, [], "append new rule ids to tools/rule_ids.snapshot")

    def test_no_published_feature_id_disappears(self) -> None:
        missing = sorted(set(snapshot("feature_ids.snapshot")) - self.current_feature_ids())
        self.assertEqual(missing, [], "a published feature id was renamed or removed")

    def test_new_feature_ids_are_appended_to_the_snapshot(self) -> None:
        new = sorted(self.current_feature_ids() - set(snapshot("feature_ids.snapshot")))
        self.assertEqual(new, [], "append new feature ids to tools/feature_ids.snapshot")


if __name__ == "__main__":
    unittest.main()
