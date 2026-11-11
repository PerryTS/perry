#!/usr/bin/env python3
"""The realm-diff gate must refuse a dumper built without its names input."""
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location(
    "realm_diff_gate", Path(__file__).parents[4] / 'scripts/realm_diff_gate.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

NAMES_FILE = Path("/x/hello.names.json")
NAMES = ["console", "globalThis"]


OWN = ["Map", "node:fs.existsSync"]


def log(names_line=True, top=(), names=tuple(NAMES), own=tuple(OWN), seed=None):
    """seed=None: the compiler closed the names united with its own names;
    seed=False: it printed no seed (no input); otherwise that seed (+ own)."""
    lines = []
    if names_line:
        lines.append(f"[realm-reach-names] {NAMES_FILE} {len(NAMES)}")
    lines.append("[realm-reach-json] " + json.dumps(
        {"names": list(names) + list(own), "top": list(top), "full": 10, "own": list(own),
         "seed": None if seed is False else list(names if seed is None else seed) + list(own)}))
    return "\n".join(lines) + "\n"


class RestrictedCheck(unittest.TestCase):
    def test_a_restricted_partial_dumper_passes(self):
        gate.check_restricted(log(), NAMES_FILE, NAMES)

    def test_a_dumper_built_without_the_input_fails(self):
        with self.assertRaisesRegex(gate.GateError, "not built with"):
            gate.check_restricted(log(names_line=False, top=["global value"]), NAMES_FILE, NAMES)

    def test_another_names_file_or_count_fails(self):
        other = log().replace(str(NAMES_FILE), "/x/lad-07-json.names.json")
        with self.assertRaisesRegex(gate.GateError, "not built with"):
            gate.check_restricted(other, NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not built with"):
            gate.check_restricted(log(), NAMES_FILE, NAMES + ["Map"])

    def test_a_top_or_incomplete_r_fails(self):
        with self.assertRaisesRegex(gate.GateError, "still top"):
            gate.check_restricted(log(top=["with"]), NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not the names file united"):
            gate.check_restricted(log(names=("console",)), NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not the names file united"):
            gate.check_restricted(log(names=tuple(NAMES) + ("Object",)), NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not the names file united"):
            gate.check_restricted(log(seed=False), NAMES_FILE, NAMES)

    def test_the_dumpers_own_names_are_part_of_r(self):
        reach = gate.check_restricted(log(), NAMES_FILE, NAMES)
        self.assertEqual(sorted(reach["own"]), sorted(OWN))
        # A seed without the dumper's own names is not the union.
        with self.assertRaisesRegex(gate.GateError, "missing"):
            gate.check_restricted(log(seed=["console"]), NAMES_FILE, NAMES)

    def test_one_reach_line_is_required(self):
        with self.assertRaisesRegex(gate.GateError, "one \\[realm-reach-json\\]"):
            gate.reach_of(log() + log(names_line=False))



class RestrictedSource(unittest.TestCase):
    def test_imports_outside_r_are_bound_to_undefined(self):
        text = gate.dumper_source({"node:events"}).read_text()
        self.assertIn('from "node:events";', text)
        self.assertNotIn('from "node:child_process";', text)
        self.assertRegex(text, r"const realm_native_\d+: any = undefined; // node:child_process is outside R")

    def test_the_plain_dumper_keeps_every_import(self):
        text = gate.dumper_source().read_text()
        self.assertEqual(text, gate.DUMPER.read_text())
        self.assertNotIn("is outside R", text)


def kept_imports(names):
    text = gate.dumper_source(set(names)).read_text()
    return sorted(spec for _, spec in gate.NATIVE_IMPORT.findall(text))


class ImportNames(unittest.TestCase):
    """An import is kept exactly when R records its import name, checked
    against names files the compiler wrote (realm-dumps/*.names.json)."""

    def names(self, program):
        return json.loads((gate.REFS / f"{program}.names.json").read_text())["names"]

    def test_the_dumper_keys_its_native_roots_by_the_import_name(self):
        text = gate.DUMPER.read_text()
        roots = dict((var, key) for key, var in re.findall(
            r'^    \["([^"]+)", (realm_native_\d+)\],$', text, re.M))
        imports = gate.NATIVE_IMPORT.findall(text)
        self.assertEqual(len(roots), len(imports))
        for var, spec in imports:
            self.assertEqual(roots[var], gate.import_name(spec))

    def test_a_program_importing_modules_keeps_exactly_those_imports(self):
        names = self.names("node-imports")
        self.assertEqual(kept_imports(names), ["node:events", "node:fs", "node:path/posix"])
        # The native receiver cells are spelled by the runtime, not by the
        # specifier: keyed by them, node:path/posix would be lost (its cell is
        # path.posix). fs's cell is its namespace item's (fs/namespace.rs,
        # alias node:fs.default): the `default` member every namespace import
        # observes (node: `ns.default === require("fs")`).
        self.assertIn("%NativeNamespace:events%", names)
        self.assertIn("%NativeNamespace:fs%", names)
        self.assertNotIn("%NativeNamespace:path/posix%", names)
        self.assertIn("%NativeNamespace:path.posix%", names)
        # Exactly the receiver cells the three imports reach: Buffer (node:fs
        # births Buffer), stream (fs.ReadStream extends stream.Readable),
        # path.win32 (path/posix.win32); any other cell is an unreached root.
        self.assertEqual(sorted(n for n in names if n.startswith("%NativeNamespace:")), [
            "%NativeNamespace:buffer.Buffer%", "%NativeNamespace:events%",
            "%NativeNamespace:fs%", "%NativeNamespace:path.posix%",
            "%NativeNamespace:path.win32%", "%NativeNamespace:stream%"])

    def test_a_global_receiver_cell_does_not_keep_its_module_import(self):
        names = self.names("hello")
        # console is one declaration (it mints per access, A2-DESIGN §9 Q6): its
        # receiver cell is the global itself, not a %NativeNamespace:console% root.
        self.assertIn("console", names)
        self.assertNotIn("%NativeNamespace:console%", names)
        self.assertEqual(kept_imports(names), [])


class PerRunDirectory(unittest.TestCase):
    def test_each_run_works_in_its_own_directory_and_removes_it(self):
        probe = ("import importlib.util, sys; s = importlib.util.spec_from_file_location('g', sys.argv[1]); "
                 "g = importlib.util.module_from_spec(s); s.loader.exec_module(g); "
                 "print(g.dumper_source().parent)")
        script = str(Path(gate.__file__))
        dirs = [Path(subprocess.run([sys.executable, "-c", probe, script], capture_output=True,
                                    text=True, check=True).stdout.strip()) for _ in range(2)]
        self.assertNotEqual(dirs[0], dirs[1])
        for d in dirs:
            self.assertEqual(d.name, "src")
            self.assertFalse(d.parent.exists())


class OneSource(unittest.TestCase):
    def test_reference_and_check_compile_the_same_dumper_source(self):
        import shutil, sys, tempfile
        sources = {}
        tmp = Path(tempfile.mkdtemp(prefix="realm-diff-gate-test-"))
        refs = tmp / "realm-dumps"
        refs.mkdir()
        saved = (gate.REFS, gate.witness_names, gate.compile_ts, gate.dump, sys.argv)
        def compile_ts(perry, source, binary, extra_env):
            mode = "check" if "PERRY_REALM_REACH_NAMES" in extra_env else "reference"
            sources[mode] = Path(source).read_text()
            return log().replace(str(NAMES_FILE), str(refs / "hello.names.json"))
        try:
            gate.REFS = refs
            gate.witness_names = lambda perry, program, out: list(NAMES)
            gate.compile_ts = compile_ts
            gate.dump = lambda binary, names=None, walk=False: "{}\n"
            for extra in (["--regenerate"], []):
                sys.argv = ["gate", "--perry", "perry", "--output", str(tmp / "out"), "--only", "hello"] + extra
                gate.main()
        finally:
            gate.REFS, gate.witness_names, gate.compile_ts, gate.dump, sys.argv = saved
            shutil.rmtree(tmp)
        self.assertEqual(set(sources), {"reference", "check"})
        self.assertIn("is outside R", sources["reference"])
        self.assertEqual(sources["reference"], sources["check"])


class IdentityCheck(unittest.TestCase):
    def records(self, minting):
        return [{"name": n, "one": n not in minting} for n in gate.MINTING + ["%NativeNamespace:fs%"]]

    def test_the_minting_set_equals_the_list(self):
        self.assertEqual(gate.check_identity(self.records(gate.MINTING)), gate.MINTING)

    def test_a_cell_that_stops_or_starts_minting_fails(self):
        with self.assertRaisesRegex(gate.GateError, "stops"):
            gate.check_identity(self.records(gate.MINTING[1:]))
        with self.assertRaisesRegex(gate.GateError, "starts"):
            gate.check_identity(self.records(gate.MINTING + ["%NativeNamespace:fs%"]))

    def test_a_second_record_or_none_fails(self):
        with self.assertRaisesRegex(gate.GateError, "repeat"):
            gate.check_identity(self.records(gate.MINTING) + [{"name": "%NativeNamespace:fs%", "one": True}])
        with self.assertRaisesRegex(gate.GateError, "missing"):
            gate.check_identity([])

    def test_identity_lines_are_read_from_the_dump(self):
        out = '@realm {}\n@identity {"name": "a", "one": true}\n'
        self.assertEqual(gate.identities(out), [{"name": "a", "one": True}])



class OrderCheck(unittest.TestCase):
    # The fixtures are not the runtime's cells: no listed disagreement.
    def setUp(self):
        self.saved = gate.ORDER_DISAGREES
        gate.ORDER_DISAGREES = []

    def tearDown(self):
        gate.ORDER_DISAGREES = self.saved

    def rec(self, name, keys):
        return {"name": name, "spec": name, "key": "", "keys": keys}

    def test_shared_members_in_node_order_agree_and_only_one_side_is_not_an_order_question(self):
        records = [self.rec("node:a", ["x", "perry_only", "y"]), self.rec("node:streams", ["s"])]
        node = {"node:a": ["x", "node_only", "y"], "node:streams": None}
        self.assertEqual(gate.check_order(records, node), (["node:a"], ["node:streams"]))

    def test_a_cell_out_of_node_order_fails_by_name(self):
        with self.assertRaisesRegex(gate.GateError, r"starts \['node:a'\]"):
            gate.check_order([self.rec("node:a", ["y", "x"])], {"node:a": ["x", "y"]})

    def test_a_listed_disagreement_that_stops_fails(self):
        gate.ORDER_DISAGREES = ["node:a"]
        with self.assertRaisesRegex(gate.GateError, r"stops \['node:a'\]"):
            gate.check_order([self.rec("node:a", ["x", "y"])], {"node:a": ["x", "y"]})

    def test_a_cell_without_a_node_entry_or_a_repeat_fails(self):
        with self.assertRaisesRegex(gate.GateError, "no entry"):
            gate.check_order([self.rec("node:a", ["x"])], {})
        with self.assertRaisesRegex(gate.GateError, "repeat"):
            gate.check_order([self.rec("node:a", ["x"])] * 2, {"node:a": ["x"]})
        with self.assertRaisesRegex(gate.GateError, "missing"):
            gate.check_order([], {})

    def test_order_lines_are_read_from_the_dump(self):
        out = '@realm {}\n@order {"name": "a", "spec": "a", "key": "", "keys": ["k"]}\n'
        self.assertEqual(gate.orders(out), [{"name": "a", "spec": "a", "key": "", "keys": ["k"]}])


if __name__ == "__main__":
    unittest.main()
