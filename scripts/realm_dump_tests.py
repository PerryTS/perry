#!/usr/bin/env python3
"""The realm gate must reject descriptor, identity and topology drift."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("realm_dump", Path(__file__).with_name("realm_dump.py"))
realm_dump = importlib.util.module_from_spec(spec)
spec.loader.exec_module(realm_dump)


def fixture():
    return {
        "$global": {"path": "$global", "type": "object", "prototype": None,
                    "ownKeyOrder": ["A", "B"], "ownSymbolOrder": [], "intrinsics": [],
                    "properties": [
                        {"key": "A", "descriptor": {"value": {"ref": "a"}, "writable": True,
                                                       "configurable": True, "enumerable": False}},
                        {"key": "B", "descriptor": {"value": {"ref": "b"}, "writable": True,
                                                       "configurable": True, "enumerable": False}}]},
        "a": {"path": "a", "type": "function", "name": "A", "length": 2,
              "prototype": {"ref": "a"}, "ownKeyOrder": [], "ownSymbolOrder": [], "properties": []},
        "b": {"path": "b", "type": "object", "prototype": None,
              "ownKeyOrder": [], "ownSymbolOrder": [], "properties": []},
    }


class RealmGateTests(unittest.TestCase):
    def test_every_observable_field_can_fail_the_gate(self):
        base = fixture()
        mutations = [
            lambda x: x["$global"]["properties"][0]["descriptor"].update(writable=False),
            lambda x: x["a"].update(length=3),
            lambda x: x["a"].update(name="wrong"),
            lambda x: x["a"].update(prototype={"ref": "b"}),
            lambda x: x["a"].update(ownSymbolOrder=["Symbol(Symbol.toStringTag)"]),
            lambda x: x["a"]["properties"].append({"symbol": "Symbol(Symbol.toStringTag)",
                "descriptor": {"value": "wrong", "writable": False, "enumerable": False, "configurable": True}}),
            lambda x: x["$global"].update(ownKeyOrder=["B", "A"]),
            lambda x: x["$global"]["properties"][1]["descriptor"].update(value={"ref": "a"}),
        ]
        for mutate in mutations:
            candidate = copy.deepcopy(base)
            mutate(candidate)
            self.assertNotEqual(realm_dump.canonical(base), realm_dump.canonical(candidate))

    def test_restricted_gate_ignores_only_unreachable_cells(self):
        base = fixture()
        candidate = copy.deepcopy(base)
        candidate["b"]["type"] = "changed"
        self.assertEqual(realm_dump.canonical(base, {"A"}), realm_dump.canonical(candidate, {"A"}))
        candidate["a"]["length"] = 4
        self.assertNotEqual(realm_dump.canonical(base, {"A"}), realm_dump.canonical(candidate, {"A"}))

    def test_cycle_identity_is_path_independent(self):
        base = fixture()
        candidate = copy.deepcopy(base)
        candidate["renamed"] = candidate.pop("a")
        candidate["renamed"]["path"] = "renamed"
        candidate["renamed"]["prototype"]["ref"] = "renamed"
        candidate["$global"]["properties"][0]["descriptor"]["value"]["ref"] = "renamed"
        self.assertEqual(realm_dump.canonical(base), realm_dump.canonical(candidate))
        with self.assertRaisesRegex(ValueError, "absent from global"):
            realm_dump.canonical(base, {"Missing"})
        candidate["renamed"]["prototype"]["ref"] = "dangling"
        with self.assertRaisesRegex(ValueError, "dangling"):
            realm_dump.canonical(candidate)


if __name__ == "__main__":
    unittest.main()
