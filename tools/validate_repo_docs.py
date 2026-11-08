#!/usr/bin/env python3
"""Documentation-only sanity checks. Not a compiler/test/proof validation."""
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
readme = root / "README.md"
adr_dir = root / "docs" / "adr"
adrs = sorted(adr_dir.glob("ADR-[0-9][0-9][0-9]-*.md"))
errors = []
if len(adrs) != 42:
    errors.append(f"Expected 42 original ADR files, found {len(adrs)}")
nums = [int(p.name.split("-")[1]) for p in adrs]
if nums != list(range(42)):
    errors.append("ADR numbering is not exactly 000..041")
for p in adrs:
    data = p.read_text(encoding="utf-8")
    if "PROPOSED" not in data:
        errors.append(f"Missing PROPOSED marker: {p}")
    if re.search(r"\*\*Statut\*\*\s*:\s*`ACCEPTED`", data):
        errors.append(f"Unexpected automatically accepted ADR: {p}")
for path in [
    "README.md","CONTRIBUTING.md","docs/roadmap/MVP.md",
    "docs/architecture/PROJECT_INVARIANTS.md",
    "docs/reviews/2026-11-08-consolidation.md",
]:
    if not (root / path).is_file():
        errors.append(f"Required document missing: {path}")
# Only root README links, not future/external/docs across old archives.
for link in re.findall(r"\]\(([^)#]+)(?:#[^)]+)?\)", readme.read_text(encoding="utf-8")):
    if "://" in link or link.startswith(("mailto:","#","../../")):
        continue
    if not (root / link).exists():
        errors.append(f"Broken root README link: {link}")
if errors:
    print("Documentation check FAILED:")
    for e in errors:
        print(" -",e)
    sys.exit(1)
print("Documentation-only check PASS: 42 proposed ADRs and required files present.")
print("NOT A COMPILER, PROOF, HOT-SWAP, AUDIO OR E2E QUALIFICATION.")
