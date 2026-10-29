#!/usr/bin/env python3
import copy
import unittest
from realm_manifest_normalize import normalize


class NormalizeTests(unittest.TestCase):
    def setUp(self):
        self.manifest = {"version": 1, "declarations": [
            {"name": "Atomics.add", "source": "atomics.rs", "line": 825,
             "length": 3, "births": ["Function.prototype"],
             "properties": [{"key": "source", "value": {"kind": "string", "value": "line"}}]}
        ]}

    def test_only_declaration_provenance_changes_are_equal(self):
        moved = copy.deepcopy(self.manifest)
        moved["declarations"][0].update(source="install_static.rs", line=1000)
        self.assertEqual(normalize(self.manifest), normalize(moved))

    def test_every_other_fact_and_array_order_remain_significant(self):
        for field, value in [
            ("name", "Atomics.sub"), ("length", 2),
            ("births", ["Array", "Function.prototype"]), ("source_symbol", "changed"),
            ("properties", [{"key": "line", "value": {"kind": "string", "value": "line"}}]),
        ]:
            with self.subTest(field=field):
                changed = copy.deepcopy(self.manifest)
                changed["declarations"][0][field] = value
                self.assertNotEqual(normalize(self.manifest), normalize(changed))
        changed = copy.deepcopy(self.manifest)
        changed["declarations"] *= 2
        self.assertNotEqual(normalize(self.manifest), normalize(changed))

    def test_nested_source_and_line_are_not_removed(self):
        changed = copy.deepcopy(self.manifest)
        changed["declarations"][0]["properties"][0]["line"] = 1
        self.assertNotEqual(normalize(self.manifest), normalize(changed))

    def test_unknown_top_level_fields_are_preserved(self):
        changed = {**self.manifest, "future_fact": 1}
        self.assertNotEqual(normalize(self.manifest), normalize(changed))


if __name__ == "__main__":
    unittest.main()
