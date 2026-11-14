#!/usr/bin/env python3
"""The realm gate must reject descriptor, identity and topology drift."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("realm_dump", Path(__file__).parents[4] / 'scripts/realm_dump.py')
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

    def test_chain_held_name_is_not_demanded_on_global(self):
        base = fixture()
        base["a"]["prototype"] = {"ref": "b"}
        projection = {
            "%Global%": {"properties": [{"key": "A", "value": {"kind": "reference", "value": "A"}}]},
            "A": {"prototype": "ChainOnly", "properties": []},
            "ChainOnly": {"properties": []},
        }
        actual = realm_dump.canonical(base, {"A", "ChainOnly"}, projection)
        self.assertIn('"path":"$global.properties[', actual)
        self.assertNotIn('"key":"B"', actual)
        self.assertEqual(len(actual.splitlines()), 3)
        candidate = copy.deepcopy(base)
        candidate["b"]["prototype"] = {"ref": "a"}
        self.assertNotEqual(actual, realm_dump.canonical(candidate, {"A", "ChainOnly"}, projection))
        candidate = copy.deepcopy(base)
        candidate["a"]["prototype"] = None
        with self.assertRaisesRegex(ValueError, "missing kept prototype on A"):
            realm_dump.canonical(candidate, {"A", "ChainOnly"}, projection)

    def test_dropped_kept_slot_is_red(self):
        base = fixture()
        projection = {"%Global%": {"properties": [{"key": "A", "value": {"kind": "reference", "value": "A"}}]},
                      "A": {"properties": [{"key": "length"}]}}
        base["a"]["properties"] = [{"key": "length", "descriptor": {"value": 2}}]
        base["a"]["ownKeyOrder"] = ["length"]
        realm_dump.canonical(base, {"A"}, projection)
        candidate = copy.deepcopy(base)
        candidate["a"]["properties"] = []
        with self.assertRaisesRegex(ValueError, "missing kept slots on A"):
            realm_dump.canonical(candidate, {"A"}, projection)

    def test_whole_uses_the_same_kept_slots_and_missing_slot_control(self):
        base = fixture()
        base["a"]["properties"] = [{"key": "length", "descriptor": {"value": 2}},
                                   {"key": "unkept", "descriptor": {"value": 3}}]
        base["a"]["ownKeyOrder"] = ["length", "unkept"]
        projection = {"%Global%": {"properties": [{"key": "A", "value": {"kind": "reference", "value": "A"}}]},
                      "A": {"properties": [{"key": "length"}]}}
        expected = realm_dump.canonical(copy.deepcopy(base), {"A"}, projection, {"A"})
        candidate = copy.deepcopy(base)
        candidate["a"]["properties"] = candidate["a"]["properties"][:1]
        candidate["a"]["ownKeyOrder"] = ["length"]
        self.assertEqual(expected, realm_dump.canonical(candidate, {"A"}, projection, {"A"}))
        candidate["a"]["properties"] = []
        with self.assertRaisesRegex(ValueError, "missing kept slots on A"):
            realm_dump.canonical(candidate, {"A"}, projection, {"A"})

    def test_native_namespace_public_order_excludes_dispatch_storage(self):
        base = fixture()
        projection = {"%Global%": {"native_namespace": "test", "own_key_order": ["A"],
            "own_symbol_order": [], "properties": [{"key": "A"}, {"key": "dispatch-storage"}]}}
        actual = realm_dump.canonical(base, {"A"}, projection, {"%Global%"})
        self.assertNotIn('"key":"B"', actual)
        base["$global"]["properties"] = []
        with self.assertRaisesRegex(ValueError, "missing kept slots on %Global%"):
            realm_dump.canonical(base, {"A"}, projection, {"%Global%"})

    def test_serialized_accessor_null_value_projects_its_getter(self):
        base = fixture()
        base["$global"]["properties"][0]["descriptor"] = {"get": {"ref": "a"}, "set": {"type": "undefined"}}
        projection = {"%Global%": {"properties": [{"key": "A", "value": None, "get": "A.<get>", "set": None}]},
                      "A.<get>": {"properties": []}}
        actual = realm_dump.canonical(base, {"A.<get>"}, projection)
        self.assertEqual(len(actual.splitlines()), 2)
        self.assertIn('descriptor.get', actual)

    def test_projected_oracle_keeps_only_fixture_intrinsic_roots(self):
        base = fixture()
        base["$global"]["intrinsics"] = [{"name": "Kept", "value": {"ref": "a"}},
                                           {"name": "Outside", "value": {"ref": "b"}}]
        projection = {"%Global%": {"properties": []}, "Kept": {"properties": []}}
        expected = realm_dump.canonical(base, {"Kept"}, projection)
        candidate = copy.deepcopy(base)
        candidate["$global"]["intrinsics"].insert(0, {"name": "OutsideAlias", "value": {"ref": "a"}})
        candidate["b"]["type"] = "outside changes do not enter R"
        self.assertEqual(expected, realm_dump.canonical(candidate, {"Kept"}, projection))
        candidate["$global"]["intrinsics"] = []
        self.assertNotEqual(expected, realm_dump.canonical(candidate, {"Kept"}, projection))

    def test_cycle_identity_is_path_independent(self):
        base = fixture()
        candidate = copy.deepcopy(base)
        candidate["renamed"] = candidate.pop("a")
        candidate["renamed"]["path"] = "renamed"
        candidate["renamed"]["prototype"]["ref"] = "renamed"
        candidate["$global"]["properties"][0]["descriptor"]["value"]["ref"] = "renamed"
        self.assertEqual(realm_dump.canonical(base), realm_dump.canonical(candidate))
        with self.assertRaisesRegex(ValueError, "missing kept slots on %Global%"):
            realm_dump.canonical(base, {"Missing"}, {"%Global%": {"properties": [{"key": "Missing"}]}})
        candidate["renamed"]["prototype"]["ref"] = "dangling"
        with self.assertRaisesRegex(ValueError, "dangling"):
            realm_dump.canonical(candidate)


class KeptIteratorSeedTests(unittest.TestCase):
    def test_buffer_iterator_seed_uses_only_selected_members(self):
        spec = importlib.util.spec_from_file_location("realm_kept_oracle", Path(__file__).parents[4] / 'scripts/realm_kept_oracle.py')
        oracle = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(oracle)
        projection = {
            'Buffer': {'constructor': {}, 'properties': [{'key':'alloc'}]},
            'Buffer.prototype': {'prototype':'Uint8Array.prototype'},
            'Uint8Array.prototype': {'prototype':'%TypedArray%.prototype'},
            '%TypedArray%.prototype': {'properties':[{'key':'values'}]},
        }
        paths = {'Buffer': ('globalThis["Buffer"]', [])}
        self.assertEqual(oracle.array_iterator_seed(paths, projection), '(globalThis["Buffer"]).alloc(0).values()')
        projection['%TypedArray%.prototype']['properties'] = []
        with self.assertRaisesRegex(ValueError, 'no selected pristine'):
            oracle.array_iterator_seed(paths, projection)
        projection['Array.prototype'] = {'properties':[{'key':'@@iterator'}]}
        self.assertEqual(oracle.array_iterator_seed({}, projection), '[][Symbol.iterator]()')


if __name__ == "__main__":
    unittest.main()
