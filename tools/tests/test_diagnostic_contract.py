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


if __name__ == "__main__":
    unittest.main()
