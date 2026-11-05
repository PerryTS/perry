#!/usr/bin/env python3
import copy
import json
from pathlib import Path
import tempfile
import unittest

from realm_source_migration import insert_at_sites, migrate, simple_facts, tokens, placement_rank, logical_facts, native_default_facts, test_ranges, cfg_without_tests


def atomics_add():
    return {
        "name": "Atomics.add", "kind": "method",
        "properties": [
            {"key": "length", "symbol": False, "attributes": 4,
             "value": {"kind": "number_bits", "value": "4008000000000000"}},
            {"key": "name", "symbol": False, "attributes": 4,
             "value": {"kind": "string", "value": "add"}},
        ],
        "own_key_order": ["length", "name"], "own_symbol_order": [],
        "header": {"gc_type": 4, "reserved": 128, "capture_flags": 0},
        "prototype": "Function.prototype", "length": 3, "body": "js_atomics_add",
        "function_info": {"params": 3, "rest_fixed": 0, "flags": 3072,
                          "length": 0, "declared": 3, "plain_params": 0,
                          "source_symbol": "source:js_atomics_add", "native_method": False},
        "captures": [], "info_symbol": "perry_realm_info_ce5097e0ac446db8a939",
        "births": ["Function.prototype"],
    }


class MigrationTests(unittest.TestCase):
    def test_only_human_facts_are_emitted(self):
        self.assertEqual(simple_facts(atomics_add()), {})
        specialized = atomics_add()
        specialized["length"] = 1
        self.assertIsNone(simple_facts(specialized))
        specialized = atomics_add()
        specialized["header"]["reserved"] = 0
        self.assertIsNone(simple_facts(specialized))

    def test_constructible_birth_derives_header_and_synthetic_key(self):
        d = atomics_add()
        d["header"]["reserved"] = 0
        d["own_key_order"].append("prototype")
        self.assertEqual(simple_facts(d), dict(constructible=True))
        d["function_info"]["flags"] |= 512
        self.assertIsNone(simple_facts(d))

    def test_symbolic_key_binding_does_not_copy_raw_pointer_or_length(self):
        d = atomics_add()
        d["captures"] = [dict(kind="bytes",value="add"),dict(kind="raw_word",value="0x3")]
        self.assertEqual(simple_facts(d),dict(capture="property_key"))
        d["captures"][1]["value"]="0x4"
        self.assertIsNone(simple_facts(d))

    def test_lexer_does_not_turn_comments_or_strings_into_sites(self):
        source = '// fn_info!(fake,0)\n/* nested /* fn_info! */ */ r###"fn_info!(fake,0)"### fn_info!(real,1)'
        self.assertEqual(sum(t[0] == "fn_info" for t in tokens(source)), 1)

    def test_insertions_preserve_every_original_line(self):
        source = "one\nfn_info!(a,0)\nfn_info!(b,1)\n"
        positions = [source.index("a,0"), source.index("b,1")]
        changed = insert_at_sites(source, [(p, "#[realm(X)] ") for p in positions])
        self.assertEqual(source.splitlines()[0], changed.splitlines()[0])
        self.assertEqual(len(source.splitlines()), len(changed.splitlines()))
        self.assertEqual(changed.count("#[realm(X)]"), 2)

    def test_qualified_body_wins_over_same_module_leaf_collision(self):
        wrong = dict(body="duration_format::resolved_options_thunk", path="crates/perry-runtime/src/intl.rs", line=900)
        right = dict(body="display_names::resolved_options_thunk", path="crates/perry-runtime/src/intl.rs", line=1988)
        body = "crate::intl::display_names::resolved_options_thunk"
        self.assertEqual(min([wrong, right], key=lambda site: placement_rank(site, body, right["path"])), right)

    def test_accessor_pair_emits_both_halves_and_only_extra_births(self):
        declaration = dict(name="crypto", kind="alias", attributes=6, own_symbol_order=[],
                           get="crypto.<get>", set="crypto.<set>",
                           births=["Crypto", "crypto.<get>", "crypto.<set>"])
        self.assertEqual(logical_facts(declaration),
                         dict(attrs=6, get="crypto.<get>", set="crypto.<set>", births=["Crypto"]))

    def test_native_binding_emits_receiver_without_capture_encodings(self):
        declaration = atomics_add()
        declaration.update(name="node:fs.readFile", length=0,
                           body="crate::closure::BOUND_METHOD_FUNC_PTR",
                           own_key_order=["length", "name", "prototype"],
                           captures=[dict(kind="reference", value="node:fs.default"),
                                     dict(kind="bytes", value="readFile"),
                                     dict(kind="raw_word", value="0x8")],
                           births=["Function.prototype", "node:fs.default"])
        declaration["header"]["reserved"] = 0
        declaration["properties"][0]["value"]["value"] = "0000000000000000"
        declaration["properties"][1]["value"]["value"] = "readFile"
        declaration["function_info"].update(params=0, declared=0, flags=0, native_method=True,
                                             source_symbol="source:No symbol matches 0xbadddead.")
        self.assertEqual(native_default_facts(declaration),
                         dict(provider="bound_method", receiver="node:fs.default"))
        declaration["captures"][0] = dict(kind="raw_word", value="0x123456")
        self.assertIsNone(native_default_facts(declaration))

    def test_cfg_test_filter_preserves_later_production_declarations(self):
        source = """struct Counts { #[cfg(test)] count: usize, live: usize }
fn before() { fn_info!(before, 0); }
#[cfg(all(test, not(feature="x")))] fn unit() { fn_info!(test_only, 0); }
fn after() { fn_info!(after, 0); }
#[cfg(not(test))] fn release_only() { fn_info!(release_only, 0); }
"""
        ts = list(tokens(source))
        ranges = test_ranges(ts)
        kept = [ts[i+3][0] for i,t in enumerate(ts) if t[0]=="fn_info"
                and not any(start<=t[1]<end for start,end in ranges)]
        self.assertEqual(kept, ["before", "after", "release_only"])
        self.assertIsNone(cfg_without_tests(list(tokens('any(test,feature="x")'))))

    def test_cross_file_move_deletes_companion_without_shifting_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            src = root / "crates/perry-runtime/src"
            src.mkdir(parents=True)
            abi = root / "crates/perry-abi/src"
            abi.mkdir(parents=True)
            (abi / "lib.rs").write_text(
                "pub const FN_HAS_DECLARED: u32 = 1 << 11;\n"
                "pub const FN_BUILTIN: u32 = 1 << 10;\n"
                "pub const FN_REST_MASK: u32 = 65543;\n")
            original = 'pub fn js_atomics_add() {}\n\ncrate::realm_decl!("atomics.realm.json#*");\n'
            (src / "atomics.rs").write_text(original)
            installation = "fn install() {\n let info = crate::fn_info!(js_atomics_add,3; with_declared(3),with_flags(FN_BUILTIN));\n}\n"
            (src / "install_static.rs").write_text(installation)
            declaration = atomics_add()
            (src / "atomics.realm.json").write_text(json.dumps({"version": 1, "groups": {"objects": [declaration]}}))
            baseline = {"version": 1, "declarations": [{**copy.deepcopy(declaration), "source": "perry-runtime/src/atomics.rs", "line": 3}]}
            report = migrate(root, baseline, root / "evidence", apply=True)
            self.assertEqual(report["cross_file_entries"], 1)
            self.assertEqual(report["placements"][0]["line"], 2)
            self.assertFalse((src / "atomics.realm.json").exists())
            self.assertEqual((src / "atomics.rs").read_text(), "pub fn js_atomics_add() {}\n")
            updated = (src / "install_static.rs").read_text()
            self.assertIn("fn_info!(#[realm(Atomics.add)] js_atomics_add,3;", updated)
            self.assertEqual(updated.count("\n"), installation.count("\n"))
            self.assertNotIn("function_info", updated)
            self.assertNotIn("capture_flags", updated)


class ProjectionCarrierScanTests(unittest.TestCase):
    def test_named_projection_carrier_can_accept_another_physical_cell(self):
        import tempfile
        from pathlib import Path
        import realm_source_migration as migration
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            path=root/"crates/perry-runtime/src/example.rs"
            path.parent.mkdir(parents=True)
            path.write_text("fn_info!(#[realm(Foo, spec={length=0},emit=metadata(realm_foo))] body,1);")
            sites=migration.scan(root)
            self.assertEqual(len(sites["body"]),1)
            self.assertIn("emit=metadata",sites["body"][0]["annotation"]["facts"][0])
            path.write_text("fn_info!(#[realm(Foo,spec={length=0},emit=metadata)] body,1;; bind info,meta);")
            self.assertEqual(dict(migration.scan(root)),{})


if __name__ == "__main__":
    unittest.main()
