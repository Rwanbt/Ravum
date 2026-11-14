"""Contrats de `tools/coverage_product_split.py`.

Le 2026-11-14, le script classait `loader.rs` à « 0 ligne produit / 100 % » :
la fonction cherchait le *premier* `#[cfg(test)]`, or `loader.rs` déclare deux
helpers de test avant son module de tests (ligne 93). Toute la fin du fichier
était donc comptée comme échafaudage. Ces tests figent la distinction entre
« attribut de test » et « module de tests ».
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "tools"))

import coverage_product_split  # noqa: E402


class TestModuleStartTest(unittest.TestCase):
    def test_a_file_without_tests_has_no_boundary(self):
        self.assertIsNone(coverage_product_split.test_module_start(["fn main() {}", "}"]))

    def test_a_plain_test_module(self):
        lines = ["fn product() {}", "", "#[cfg(test)]", "mod tests {", "    #[test]", "    fn t() {}", "}"]
        self.assertEqual(coverage_product_split.test_module_start(lines), 3)

    def test_a_test_module_behind_another_attribute(self):
        lines = ["#[cfg(test)]", '#[allow(clippy::all)]', 'mod tests {', '}']
        self.assertEqual(coverage_product_split.test_module_start(lines), 1)

    def test_early_test_helpers_do_not_start_the_boundary(self):
        lines = [
            "fn product() {}",
            "#[cfg(test)]",
            "fn helper_only_in_tests() {}",
            "",
            "fn more_product() {}",
            "#[cfg(test)]",
            "mod tests {",
            "}",
        ]
        self.assertEqual(coverage_product_split.test_module_start(lines), 6)

    def test_a_test_only_function_alone_is_not_a_module(self):
        lines = ["#[cfg(test)]", "fn helper() {}", "fn product() {}"]
        self.assertIsNone(coverage_product_split.test_module_start(lines))


if __name__ == "__main__":
    unittest.main()
