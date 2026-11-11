"""Triage of the failing conformance cases against the documented code registry.

For every failing case, compare the codes the specification demands with the
codes the implementation emits, and flag any emitted code that no
`spec/language/*.md` file documents. The registry check is what decides whether a
divergence is a specification defect (the code is documented elsewhere) or an
implementation defect (the compiler invents an undocumented code).

Usage: python tools/triage_conformance.py target/conformance-after-t2.json
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

CODE = re.compile(r"E-[A-Z0-9-]+")


def main(argv: list[str]) -> int:
    report_path = pathlib.Path(argv[0] if argv else "target/conformance-after-t2.json")
    report = json.loads(report_path.read_text(encoding="utf-8"))
    spec = pathlib.Path("spec/language")
    documented = set()
    for path in spec.glob("*.md"):
        documented.update(CODE.findall(path.read_text(encoding="utf-8")))

    rows = []
    for case in report["cases"]:
        if case["status"] != "fail":
            continue
        expected_text = case.get("expected", "")
        actual_text = case.get("actual", "")
        match = re.search(r"codes\s*=\s*\{([^}]*)\}", actual_text)
        actual = set(CODE.findall(match.group(1))) if match else set()
        match = re.search(r"exactly\s*\{([^}]*)\}", expected_text)
        expected = set(CODE.findall(match.group(1))) if match else set()
        rows.append((case["id"], expected, actual))

    print(f"failing cases: {len(rows)}")
    undocumented = set()
    for case_id, expected, actual in rows:
        missing = actual - documented
        undocumented |= missing
        print(
            f"{case_id:44} exp={','.join(sorted(expected)) or '-':38} "
            f"act={','.join(sorted(actual)) or '-':38} "
            f"undocumented={','.join(sorted(missing)) or '-'}"
        )
    print()
    print(f"codes emitted but absent from spec/language/*.md: {', '.join(sorted(undocumented)) or 'none'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
