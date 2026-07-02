import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SOURCE = REPO_ROOT / "tests" / "typed_feedback_runtime_evidence.ts"


def resolve_perry() -> list[str]:
    candidate = os.environ.get("PERRY_BIN")
    if candidate:
        path = Path(candidate)
        if path.is_absolute():
            return [str(path)]
        if path.exists() or os.sep in candidate:
            return [str((REPO_ROOT / path).resolve())]
        return [candidate]
    return ["cargo", "run", "--quiet", "-p", "perry", "--"]


class TypedFeedbackRuntimeEvidenceTest(unittest.TestCase):
    maxDiff = None

    def run_cmd(self, cmd: list[str], *, env: dict[str, str] | None = None, timeout: int = 240) -> subprocess.CompletedProcess[str]:
        proc = subprocess.run(
            cmd,
            cwd=REPO_ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
        if proc.returncode != 0:
            self.fail(
                "command failed\n"
                f"cmd: {' '.join(cmd)}\n"
                f"exit: {proc.returncode}\n"
                f"stdout:\n{proc.stdout}\n"
                f"stderr:\n{proc.stderr}"
            )
        return proc

    def test_compiled_program_links_and_runs_typed_feedback_helpers(self) -> None:
        perry = resolve_perry()
        with tempfile.TemporaryDirectory() as temp:
            temp_path = Path(temp)
            binary = temp_path / "typed-feedback-runtime-evidence"
            trace_path = temp_path / "nested" / "typed-feedback-trace.json"

            compile_env = {**os.environ, "PERRY_NO_CACHE": "1", "PERRY_TYPED_FEEDBACK": "1"}
            if shutil.which("clang"):
                compile_env.setdefault("PERRY_LLVM_CLANG", shutil.which("clang") or "")
            self.run_cmd(
                perry + ["compile", "--no-cache", str(SOURCE), "-o", str(binary)],
                env=compile_env,
                timeout=300,
            )
            self.assertTrue(binary.exists(), "compile did not produce a standalone binary")

            # The optimized standalone runtime can be built without the diagnostics
            # feature, so JSON trace emission is covered by perry-runtime unit tests.
            # This test is intentionally a link/run proof for generated helper
            # retention under the auto-optimized compile path.
            run_env = {
                **os.environ,
                "PERRY_TYPED_FEEDBACK": "1",
                "PERRY_TYPED_FEEDBACK_TRACE": str(trace_path),
            }
            proc = self.run_cmd([str(binary)], env=run_env, timeout=60)
            self.assertEqual(
                [
                    "4",
                    "5",
                    "not-number",
                    "6",
                    "21",
                    "31",
                    "2",
                    "not-number",
                ],
                proc.stdout.strip().splitlines(),
            )


if __name__ == "__main__":
    unittest.main()
