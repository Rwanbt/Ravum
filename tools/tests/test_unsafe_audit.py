"""Contrats de `tools/unsafe_audit.py` (confinement D7).

Ces tests figent la propriété qui pourrait être « élargie » pour faire passer un
build : les quatre crates du langage restent en `forbid(unsafe_code)`, le CLI
garde son `deny` et un seul module audité peut porter `allow(unsafe_code)`.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "tools"))

import unsafe_audit  # noqa: E402

ROOTS = {
    "crates/ravum-lang-syntax/src/lib.rs": "#![forbid(unsafe_code)]\npub mod ast;\n",
    "crates/ravum-lang-sema/src/lib.rs": "#![forbid(unsafe_code)]\npub mod check;\n",
    "crates/ravum-lang-codegen/src/lib.rs": "#![forbid(unsafe_code)]\nuse x;\n",
    "crates/ravum-lang-lsp/src/lib.rs": "#![forbid(unsafe_code)]\npub mod server;\n",
    "crates/ravum-lang-cli/src/main.rs": "#![deny(unsafe_code)]\nfn main() {}\n",
}

WATCH = (
    '#[cfg(windows)]\n#[allow(unsafe_code)]\nmod signal {\n'
    '    unsafe extern "system" { fn f(); }\n}\n'
    '#[cfg(not(windows))]\n#[allow(unsafe_code)]\nmod signal {\n'
    '    unsafe extern "C" { fn g(); }\n}\n'
)


def sources(**overrides: str) -> dict[str, str]:
    document = dict(ROOTS)
    document["crates/ravum-lang-cli/src/commands/watch.rs"] = WATCH
    document.update(overrides)
    return document


class AuditTest(unittest.TestCase):
    def test_the_current_inventory_passes(self):
        self.assertEqual(unsafe_audit.audit(sources()), [])

    def test_a_missing_crate_attribute_fails(self):
        broken = sources()
        broken["crates/ravum-lang-sema/src/lib.rs"] = "pub mod check;\n"
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("forbid(unsafe_code)" in problem for problem in problems))

    def test_a_blanket_allow_fails(self):
        broken = sources()
        broken["crates/ravum-lang-sema/src/lib.rs"] = "#![forbid(unsafe_code)]\n#![allow(unsafe_code)]\n"
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("crate-level" in problem for problem in problems))

    def test_an_unlisted_local_allow_fails(self):
        broken = sources()
        broken["crates/ravum-lang-cli/src/main.rs"] = (
            "#![deny(unsafe_code)]\n#[allow(unsafe_code)]\nfn helper() {}\nfn main() {}\n"
        )
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("main.rs" in problem and "expected 0" in problem for problem in problems))

    def test_a_second_allow_in_watch_is_refused(self):
        broken = sources()
        broken["crates/ravum-lang-cli/src/commands/watch.rs"] = WATCH + '#[allow(unsafe_code)]\nmod other {}\n'
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("found 3" in problem for problem in problems))

    def test_an_allow_on_the_wrong_item_fails(self):
        broken = sources()
        broken["crates/ravum-lang-cli/src/commands/watch.rs"] = (
            '#[cfg(windows)]\n#[allow(unsafe_code)]\nmod not_a_signal {\n    unsafe extern "system" { fn f(); }\n}\n'
            '#[cfg(not(windows))]\n#[allow(unsafe_code)]\nmod signal {\n    unsafe extern "C" { fn g(); }\n}\n'
        )
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("must annotate the signal module" in problem for problem in problems))

    def test_a_missing_root_file_fails(self):
        broken = sources()
        del broken["crates/ravum-lang-lsp/src/lib.rs"]
        problems = unsafe_audit.audit(broken)
        self.assertTrue(any("lsp/src/lib.rs" in problem for problem in problems))


if __name__ == "__main__":
    unittest.main()
