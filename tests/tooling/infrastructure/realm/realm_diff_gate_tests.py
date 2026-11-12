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


def log(names_line=True, top=(), names=tuple(NAMES), exec_only=(), fixture_exec=(), dumper_exec=()):
    lines = []
    if names_line:
        lines.append(f"[realm-reach-names] {NAMES_FILE} {len(NAMES)}")
    lines.append("[realm-reach-json] " + json.dumps(
        {"names": list(names), "top": list(top), "full": 10, "exec_only": list(exec_only),
         "fixture_exec": list(fixture_exec), "dumper_exec": list(dumper_exec)}))
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

    def test_additional_executing_body_keeps_exact_union_after_global_bootstrap(self):
        extra = "%Layout:RESULT%"
        output = log(names=tuple(NAMES) + (extra,), exec_only=(extra,),
                     fixture_exec=("js_get_global_this",),
                     dumper_exec=("js_get_global_this", "descriptor"))
        gate.check_restricted(output, NAMES_FILE, NAMES)
        with self.assertRaisesRegex(gate.GateError, "not the names file"):
            gate.check_restricted(output.replace(extra, "wrong", 1), NAMES_FILE, NAMES)

    def test_covered_executing_closure_has_no_exec_only_names(self):
        output = log(names=tuple(NAMES) + ("extra",), exec_only=("extra",),
                     fixture_exec=("descriptor", "bootstrap"), dumper_exec=("descriptor",))
        with self.assertRaisesRegex(gate.GateError, "covers executing bodies"):
            gate.check_restricted(output, NAMES_FILE, NAMES)

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
        # specifier: keyed by them, node:fs and node:path/posix would be lost.
        self.assertIn("%NativeNamespace:events%", names)
        self.assertNotIn("%NativeNamespace:fs%", names)
        self.assertNotIn("%NativeNamespace:path/posix%", names)
        self.assertIn("%NativeNamespace:path.posix%", names)

    def test_a_global_receiver_cell_does_not_keep_its_module_import(self):
        names = self.names("hello")
        self.assertIn("%NativeNamespace:console%", names)
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
            names_input = extra_env.get("PERRY_REALM_REACH_NAMES", str(refs / "hello.names.json"))
            return log().replace(str(NAMES_FILE), names_input)
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
