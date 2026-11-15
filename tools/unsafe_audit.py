#!/usr/bin/env python3
"""D7 confinement check: the audited `unsafe` inventory must match the code.

`docs/ffi/SAFETY.md` (D7, PROPOSED) requires that `unsafe` never appears outside
an audited module, and `docs/ffi/UNSAFE-INVENTORY.md` records the only such
module. The compiler is the real enforcement: four language crates carry
`#![forbid(unsafe_code)]`, and the CLI carries `#![deny(unsafe_code)]` with one
local `#[allow(unsafe_code)]` on the signal-handler module.

This tool checks the *shape* of that claim, which the compiler cannot check for
us:

* the five crate roots still carry the expected attribute;
* no crate uses `#![allow(unsafe_code)]` (a blanket opt-out would silently
  defeat the confinement);
* the only `#[allow(unsafe_code)]` attributes in the language crates are the two
  `mod signal` variants of `crates/ravum-lang-cli/src/commands/watch.rs` — the
  exact sites listed in the inventory.

It does not parse Rust: it is a guard against an accidental widening, not a
substitute for the compiler lint or for the human audit (D4/R13).

Exit codes: 0 inventory matches, 1 mismatch, 2 usage error.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# repo-relative path -> expected crate-level attribute
LOCKED_ROOTS: dict[str, str] = {
    "crates/ravum-lang-syntax/src/lib.rs": "#![forbid(unsafe_code)]",
    "crates/ravum-lang-sema/src/lib.rs": "#![forbid(unsafe_code)]",
    "crates/ravum-lang-codegen/src/lib.rs": "#![forbid(unsafe_code)]",
    "crates/ravum-lang-lsp/src/lib.rs": "#![forbid(unsafe_code)]",
    "crates/ravum-lang-cli/src/main.rs": "#![deny(unsafe_code)]",
}

# repo-relative path -> number of local `#[allow(unsafe_code)]` attributes
ALLOWED_LOCAL_ALLOWS: dict[str, int] = {
    "crates/ravum-lang-cli/src/commands/watch.rs": 2,
}

LANGUAGE_CRATES = (
    "crates/ravum-lang-syntax",
    "crates/ravum-lang-sema",
    "crates/ravum-lang-codegen",
    "crates/ravum-lang-cli",
    "crates/ravum-lang-lsp",
)


def audit(sources: dict[str, str]) -> list[str]:
    """Apply the inventory rules to {repo-relative path: content}."""
    problems: list[str] = []

    for path, attribute in LOCKED_ROOTS.items():
        content = sources.get(path)
        if content is None:
            problems.append(f"{path}: missing (cannot verify {attribute})")
            continue
        if attribute not in content:
            problems.append(f"{path}: expected crate attribute {attribute}")

    for path, content in sources.items():
        if any(line.strip().startswith("#![allow(unsafe_code)]") for line in content.splitlines()):
            problems.append(f"{path}: crate-level #![allow(unsafe_code)] defeats the D7 confinement")

    for path, content in sources.items():
        # Count the attribute as a real item annotation, not a mention inside a
        # string, a doc comment or an explanatory comment: the no-unsafe-context
        # lint is driven by the attribute line, not by the word.
        local = sum(1 for line in content.splitlines() if line.strip() == "#[allow(unsafe_code)]")
        allowed = ALLOWED_LOCAL_ALLOWS.get(path, 0)
        if local != allowed:
            problems.append(
                f"{path}: found {local} local #[allow(unsafe_code)] attribute(s), "
                f"expected {allowed}; every new site must be audited and listed in "
                "docs/ffi/UNSAFE-INVENTORY.md"
            )
        if local and path in ALLOWED_LOCAL_ALLOWS:
            lines = content.splitlines()
            for index, line in enumerate(lines):
                if line.strip() != "#[allow(unsafe_code)]":
                    continue
                following = next((candidate.strip() for candidate in lines[index + 1 :] if candidate.strip()), "")
                if not following.startswith("mod signal"):
                    problems.append(
                        f"{path}:{index + 1}: #[allow(unsafe_code)] must annotate the signal module, "
                        f"found {following!r}"
                    )

    return problems


def gather(root: Path) -> dict[str, str]:
    """Read the roots plus every source file that carries an allow attribute."""
    sources: dict[str, str] = {}
    for path in LOCKED_ROOTS:
        absolute = root / path
        if absolute.is_file():
            sources[path] = absolute.read_text(encoding="utf-8")
    for crate in LANGUAGE_CRATES:
        for absolute in sorted((root / crate / "src").rglob("*.rs")):
            relative = absolute.relative_to(root).as_posix()
            content = absolute.read_text(encoding="utf-8")
            if "#[allow(unsafe_code)]" in content or "#![allow(unsafe_code)]" in content:
                sources[relative] = content
    return sources


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Audit the D7 unsafe inventory against the language crates.")
    parser.add_argument("--root", type=Path, default=ROOT, help="repository root (default: detected from the tool location)")
    args = parser.parse_args(argv)

    root = args.root.resolve()
    sources = gather(root)

    print("unsafe inventory (D7):")
    for path, attribute in LOCKED_ROOTS.items():
        print(f"  {path}: {attribute}")
    for path, count in ALLOWED_LOCAL_ALLOWS.items():
        print(f"  {path}: {count} local #[allow(unsafe_code)] on mod signal")

    problems = audit(sources)
    if problems:
        for problem in problems:
            print(f"::error::unsafe inventory: {problem}")
        print(f"unsafe_audit: FAIL — {len(problems)} problem(s)")
        return 1

    print("unsafe_audit: PASS — the code matches the audited inventory")
    print("unsafe_audit: reminder: the compiler lint enforces the rest; this is not an external audit (D4/R13).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
