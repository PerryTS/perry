#!/usr/bin/env python3
"""Sampling order round-trip fixtures, including mangled runtime symbols."""
import unittest
from record_function_order import select_symbols, alias_groups


class RecorderTests(unittest.TestCase):
    def test_runtime_and_generated_samples_share_one_order(self):
        rust = "_ZN13perry_runtime6string3foo17h0123456789abcdefE"
        defined = {"main": 32, rust: 400, "js_string_concat": 160, "never": 100}
        names, covered, total = select_symbols(
            [(rust, 100), ("main", 1), ("libc_only", 999), (rust, 50),
             ("js_string_concat", 20)], defined, .99,
        )
        self.assertEqual(names, [rust, "js_string_concat"])
        self.assertEqual((covered, total), (170, 171))
        # The emitted format parses without changing or demangling any symbol.
        self.assertEqual("\n".join(names).splitlines(), names)

    def test_aliases_are_one_sampled_body_and_emit_the_canonical_section_name(self):
        aliases, sizes = alias_groups([(16, 40, "canonical"), (16, 40, "alias"),
                                      (80, 8, "other")])
        names, covered, total = select_symbols(
            [("alias", 100), ("canonical", 50), ("other", 10)],
            {"canonical": 40, "alias": 40, "other": 8}, .9, aliases)
        self.assertEqual(names, ["alias", "canonical"])
        self.assertEqual((covered, total), (150, 160))
        self.assertEqual(sizes[aliases["alias"]], 40)

    def test_repeated_local_names_join_all_of_their_physical_aliases(self):
        aliases, sizes = alias_groups([(16, 4, "local"), (16, 4, "first"),
                                      (32, 8, "local"), (32, 8, "second")])
        self.assertEqual(aliases["local"], ("first", "local", "second"))
        self.assertEqual(sizes[aliases["local"]], 12)

    def test_ties_are_deterministic_and_zero_samples_are_empty(self):
        self.assertEqual(select_symbols([("b", 1), ("a", 1)], {"a": 4, "b": 4}, 1)[0],
                         ["a", "b"])
        self.assertEqual(select_symbols([], {"main": 4}, 1), ([], 0, 0))


if __name__ == "__main__":
    unittest.main()
