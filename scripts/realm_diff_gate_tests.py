#!/usr/bin/env python3
"""The realm-diff gate must refuse a dumper built without its names input."""
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "realm_diff_gate", Path(__file__).with_name("realm_diff_gate.py"))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

NAMES_FILE = Path("/x/hello.names.json")
NAMES = ["console", "globalThis"]


def log(names_line=True, top=(), names=tuple(NAMES)):
    lines = []
    if names_line:
        lines.append(f"[realm-reach-names] {NAMES_FILE} {len(NAMES)}")
    lines.append("[realm-reach-json] " + json.dumps(
        {"names": list(names), "top": list(top), "full": 10}))
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
        with self.assertRaisesRegex(gate.GateError, "not the names file"):
            gate.check_restricted(log(names=("console",)), NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not the names file"):
            gate.check_restricted(log(names=tuple(NAMES) + ("Object",)), NAMES_FILE, NAMES)

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

if __name__ == "__main__":
    unittest.main()
