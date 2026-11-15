"""Repository hygiene guard: no drive-prefixed literal path at the repo root.

Historical bug (found 2026-11-13): a Windows absolute temp path such as
``C:\\Users\\<user>\\AppData\\Local\\Temp\\ravum-sh-test-XXXX`` was handled as a
*relative* path somewhere in the tooling. The result was a directory named
``C`` + U+F03A at the repository root, containing only empty directories
(``Users\\<user>\\AppData\\Local\\Temp\\ravum-sh-test-XXXX``). The writer no
longer exists in the tree or its history; the leftover was removed after proving
it contained no file (only empty temporary directories).

This guard fails if such an entry reappears at the repository root. A colon
cannot appear in a Windows file name, so the historical name used U+F03A (a
private-use colon glyph); U+FF1A (fullwidth colon) and U+003A are also covered.

Run with: python -m unittest discover -s tools/tests -v
"""

from __future__ import annotations

import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

#: Characters that have been observed (or could be observed) where a Windows
#: drive separator ``:`` was expected, after a path was mangled into a name.
COLON_LIKE = {":", "\uf03a", "\uff1a"}


def is_drive_prefix_name(name: str) -> bool:
    """True when ``name`` looks like a mangled Windows drive prefix (``C:``).

    Only two-character names whose first character is an ASCII letter and whose
    second character is a colon-like glyph match. Legitimate repository entries
    (``crates``, ``Cargo.toml``, ``conformance``) never match.
    """
    return (
        len(name) == 2
        and name[0].isascii()
        and name[0].isalpha()
        and name[1] in COLON_LIKE
    )


class DrivePrefixDetectorTests(unittest.TestCase):
    def test_detects_the_historical_private_use_colon(self) -> None:
        self.assertTrue(is_drive_prefix_name("C\uf03a"))
        self.assertTrue(is_drive_prefix_name("D\uf03a"))

    def test_detects_ascii_and_fullwidth_colons(self) -> None:
        self.assertTrue(is_drive_prefix_name("C:"))
        self.assertTrue(is_drive_prefix_name("c\uff1a"))

    def test_rejects_legitimate_names(self) -> None:
        for name in ["Cargo.toml", "crates", "C", "conformance", "CC\uf03a", "C\u0301"]:
            self.assertFalse(
                is_drive_prefix_name(name), f"{name!r} must not match the guard"
            )


class RepositoryRootTests(unittest.TestCase):
    def test_repository_root_is_the_expected_one(self) -> None:
        # Non-vacuity: a moved checkout must not turn this guard into a no-op.
        self.assertTrue((REPO / "Cargo.toml").is_file(), f"no Cargo.toml in {REPO}")
        self.assertTrue((REPO / "crates").is_dir(), f"no crates/ in {REPO}")
        self.assertTrue((REPO / "tools").is_dir(), f"no tools/ in {REPO}")

    def test_no_drive_prefixed_entry_at_the_repository_root(self) -> None:
        offenders = sorted(
            entry.name
            for entry in REPO.iterdir()
            if is_drive_prefix_name(entry.name)
        )
        self.assertEqual(
            offenders,
            [],
            "a Windows absolute path was written as a repository-relative "
            f"directory: {offenders!r} (see docs/operations/sessions/GOAL-JOURNAL.md)",
        )


if __name__ == "__main__":
    unittest.main()
