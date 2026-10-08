"""Regression tests for the WASI gate's failure detection, without a toolchain."""

from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import wasi_run


class RunnerTests(unittest.TestCase):
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

    def test_compiler_failure_never_runs_wasmtime(self):
        with patch("wasi_run.subprocess.run", return_value=subprocess.CompletedProcess([], 1, b"", b"link failed")) as run:
            result = self.run_case()
        self.assertFalse(result["passed"])
        self.assertEqual(result["stage"], "compile")
        self.assertEqual(run.call_count, 1)

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


if __name__ == "__main__":
    unittest.main()
