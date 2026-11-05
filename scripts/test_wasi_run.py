"""Regression tests for the WASI gate's failure detection, without a toolchain."""

from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import wasi_imports_check
import wasi_run


def core_module(*imports, signatures=None):
    """A core wasm module whose import section lists (module, name) pairs.

    A preview 1 function is declared with its standard signature, anything else
    as `() -> ()`; `signatures` overrides either, keyed by name, as the check's
    `params-results` text (`i` is i32, `l` is i64).
    """
    signatures = {**wasi_imports_check._PREVIEW1, **(signatures or {})}
    code = {"i": b"\x7f", "l": b"\x7e"}
    types = []
    for _, field in imports:
        params, results = signatures.get(field, "-").split("-")
        types.append(b"\x60" + bytes([len(params)]) + b"".join(code[c] for c in params)
                     + bytes([len(results)]) + b"".join(code[c] for c in results))
    def name(text):
        return bytes([len(text)]) + text.encode()
    body = bytes([len(imports)]) + b"".join(
        name(m) + name(n) + b"\0" + bytes([i]) for i, (m, n) in enumerate(imports))
    type_section = bytes([len(types)]) + b"".join(types)
    return (b"\0asm\x01\0\0\0" + b"\x01" + bytes([len(type_section)]) + type_section
            + b"\x02" + bytes([len(body)]) + body)


class CaseFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="wasi runner space ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "fixture.ts"
        self.source.write_text("console.log('ok')")
        self.source.with_suffix(".out").write_bytes(b"ok\nexit 3\n")
        self.output = self.root / "output"
        self.output.mkdir()

    def run_case(self):
        return wasi_run.run_case(self.root / "perry.exe", "wasmtime", self.source, self.output, 1)

    def compile_ok(self, *args, **kwargs):
        (self.output / "fixture.wasm").write_bytes(b"\0asm")
        return subprocess.CompletedProcess(args[0], 0, b"", b"")


class RunnerTests(CaseFixture):
    def test_compiler_failure_never_runs_wasmtime(self):
        with patch("wasi_run.subprocess.run", return_value=subprocess.CompletedProcess([], 1, b"", b"link failed")) as run:
            result = self.run_case()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "compile")
        self.assertEqual(run.call_count, 1)

    def test_retained_inputs_use_case_local_paths_without_switching_backend(self):
        with patch("wasi_run.subprocess.run", return_value=subprocess.CompletedProcess([], 1, b"", b"link failed")) as run:
            result = wasi_run.run_case(self.root / "perry.exe", "wasmtime", self.source,
                                       self.output, 1, keep_intermediates=True)
        self.assertFalse(result["passed"])
        self.assertEqual(run.call_count, 1)
        command = run.call_args.args[0]
        env = run.call_args.kwargs["env"]
        self.assertIn("--keep-intermediates", command)
        for name in ["TMP", "TEMP", "TMPDIR"]:
            self.assertEqual(Path(env[name]), self.output.resolve() / "intermediates")
            self.assertTrue(Path(env[name]).is_dir())
        self.assertEqual(Path(env["PERRY_SAVE_LL"]), self.output.resolve() / "ir")
        self.assertNotIn("PERRY_LLVM_KEEP_IR", env)
        self.assertEqual((self.output / "fixture.compile.log").read_bytes(), b"link failed")

    def test_success_without_output_cannot_reuse_a_stale_component(self):
        (self.output / "fixture.wasm").write_bytes(b"stale")
        with patch("wasi_run.subprocess.run", return_value=subprocess.CompletedProcess([], 0, b"", b"")) as run:
            result = self.run_case()
        self.assertFalse(result["passed"])
        self.assertEqual(run.call_count, 1)

    def test_crlf_and_nonzero_expected_exit_are_compared(self):
        def execute(command, **kwargs):
            if command[0] == "wasmtime":
                return subprocess.CompletedProcess(command, 3, b"ok\r\n", b"")
            return self.compile_ok(command, **kwargs)
        with patch("wasi_run.subprocess.run", side_effect=execute):
            result = self.run_case()
        self.assertTrue(result["passed"])

    def test_wrong_exit_fails_even_with_matching_stdout(self):
        def execute(command, **kwargs):
            if command[0] == "wasmtime":
                return subprocess.CompletedProcess(command, 0, b"ok\n", b"")
            return self.compile_ok(command, **kwargs)
        with patch("wasi_run.subprocess.run", side_effect=execute):
            result = self.run_case()
        self.assertFalse(result["passed"])
        self.assertIn("exit 0", result["detail"])

    def test_runtime_timeout_is_a_failed_run(self):
        def execute(command, **kwargs):
            if command[0] == "wasmtime":
                raise subprocess.TimeoutExpired(command, 1)
            return self.compile_ok(command, **kwargs)
        with patch("wasi_run.subprocess.run", side_effect=execute):
            result = self.run_case()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "run")
        self.assertIn("timed out", result["detail"])

    def test_preopens_and_paths_with_spaces_remain_separate_arguments(self):
        self.source.with_suffix(".json").write_text(
            '{"wasmtime_args": ["--dir", "."], "program_args": ["argument with spaces", "--flag"]}')
        def execute(command, **kwargs):
            return (
                subprocess.CompletedProcess(command, 3, b"ok\n", b"")
                if command[0] == "wasmtime" else self.compile_ok(command, **kwargs))
        with patch("wasi_run.subprocess.run", side_effect=execute) as run:
            result = self.run_case()
        self.assertTrue(result["passed"])
        command = run.call_args_list[1].args[0]
        self.assertEqual(command[:4], ["wasmtime", "run", "--dir", "."])
        self.assertEqual(command[4], str(self.output / "fixture.wasm"))
        self.assertEqual(command[5:], ["argument with spaces", "--flag"])

    def test_stdin_from_the_case_config_is_piped_to_the_program(self):
        self.source.with_suffix(".json").write_text('{"stdin": "line one\\nline two\\n"}')
        def execute(command, **kwargs):
            return (
                subprocess.CompletedProcess(command, 3, b"ok\n", b"")
                if command[0] == "wasmtime" else self.compile_ok(command, **kwargs))
        with patch("wasi_run.subprocess.run", side_effect=execute) as run:
            self.assertTrue(self.run_case()["passed"])
        self.assertNotIn("input", run.call_args_list[0].kwargs)
        self.assertEqual(run.call_args_list[1].kwargs["input"], b"line one\nline two\n")

    def test_without_stdin_in_the_config_the_runner_passes_no_input(self):
        def execute(command, **kwargs):
            return (
                subprocess.CompletedProcess(command, 3, b"ok\n", b"")
                if command[0] == "wasmtime" else self.compile_ok(command, **kwargs))
        with patch("wasi_run.subprocess.run", side_effect=execute) as run:
            self.assertTrue(self.run_case()["passed"])
        self.assertNotIn("input", run.call_args_list[1].kwargs)


class FlavourTests(CaseFixture):
    def run_p1(self):
        return wasi_run.run_case(self.root / "perry.exe", "wasmtime", self.source, self.output, 1,
                                 flavour="p1")

    def execute_with(self, module, status=3):
        def execute(command, **kwargs):
            if command[0] == "wasmtime":
                return subprocess.CompletedProcess(command, status, b"ok\n", b"")
            (self.output / "fixture.wasm").write_bytes(module)
            return subprocess.CompletedProcess(command, 0, b"", b"")
        return execute

    def test_p2_command_is_unchanged(self):
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(b"\0asm")) as run:
            self.assertTrue(self.run_case()["passed"])
        compile_command, run_command = (call.args[0] for call in run.call_args_list)
        self.assertEqual(compile_command[1:], [
            "compile", str(self.source), "--target", "wasi", "-o", str(self.output / "fixture.wasm")])
        self.assertEqual(run_command, ["wasmtime", "run", str(self.output / "fixture.wasm")])

    def test_p1_compiles_for_wasip1_and_enables_exceptions(self):
        module = core_module(("wasi_snapshot_preview1", "fd_write"))
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            result = self.run_p1()
        self.assertTrue(result["passed"])
        compile_command, run_command = (call.args[0] for call in run.call_args_list)
        self.assertEqual(compile_command[3:5], ["--target", "wasip1"])
        self.assertEqual(run_command[:4], ["wasmtime", "run", "-W", "exceptions=y"])

    def test_p1_preopens_follow_the_exception_flag(self):
        self.source.with_suffix(".json").write_text('{"wasmtime_args": ["--dir", "."]}')
        module = core_module(("wasi_snapshot_preview1", "fd_write"))
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            self.assertTrue(self.run_p1()["passed"])
        self.assertEqual(run.call_args_list[1].args[0][:6],
                         ["wasmtime", "run", "-W", "exceptions=y", "--dir", "."])

    def test_p1_module_with_another_import_fails_before_running(self):
        module = core_module(("wasi_snapshot_preview1", "fd_write"), ("env", "leftover"))
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            result = self.run_p1()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "imports")
        self.assertEqual(run.call_count, 1)

    def test_p1_preview1_import_with_a_non_standard_signature_fails_before_running(self):
        module = core_module(("wasi_snapshot_preview1", "fd_write"),
                             signatures={"fd_write": "iii-i"})
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            result = self.run_p1()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "imports")
        self.assertIn("signature", result["detail"])
        self.assertEqual(run.call_count, 1)

    def test_p1_perry_net_import_is_not_part_of_the_default_surface(self):
        module = core_module(("wasi_snapshot_preview1", "fd_write"), ("perry_net", "resolve"),
                             signatures={"resolve": "iiii-i"})
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            result = self.run_p1()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "imports")
        self.assertEqual(run.call_count, 1)

    def test_p1_perry_host_import_is_not_part_of_the_default_surface(self):
        module = core_module(("wasi_snapshot_preview1", "fd_write"),
                             ("perry_host", "discard_pages"),
                             signatures={"discard_pages": "ii-i"})
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(module)) as run:
            result = self.run_p1()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "imports")
        self.assertEqual(run.call_count, 1)

    def test_p1_component_output_is_rejected(self):
        component = b"\0asm\x0d\0\x01\0"
        with patch("wasi_run.subprocess.run", side_effect=self.execute_with(component)) as run:
            result = self.run_p1()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "imports")
        self.assertIn("not a core wasm binary", result["detail"])
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
