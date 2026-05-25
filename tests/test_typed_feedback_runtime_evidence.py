import json
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

    def test_compiled_program_emits_typed_feedback_trace(self) -> None:
        perry = resolve_perry()
        with tempfile.TemporaryDirectory() as temp:
            temp_path = Path(temp)
            binary = temp_path / "typed-feedback-runtime-evidence"
            trace_path = temp_path / "nested" / "typed-feedback-trace.json"

            compile_env = {**os.environ, "PERRY_NO_CACHE": "1"}
            if shutil.which("clang"):
                compile_env.setdefault("PERRY_LLVM_CLANG", shutil.which("clang") or "")
            self.run_cmd(
                perry + ["compile", "--no-cache", str(SOURCE), "-o", str(binary)],
                env=compile_env,
                timeout=300,
            )

            run_env = {**os.environ, "PERRY_TYPED_FEEDBACK_TRACE": str(trace_path)}
            proc = self.run_cmd([str(binary)], env=run_env, timeout=60)
            self.assertIn("4", proc.stdout)
            self.assertTrue(trace_path.exists(), "compiled program did not write typed-feedback trace")

            data = json.loads(trace_path.read_text(encoding="utf-8"))
            sites = data.get("sites", [])
            self.assertGreater(len(sites), 0)
            required = {
                "site_id",
                "source_label",
                "guard_name",
                "fallback_name",
                "guard_passes",
                "guard_failures",
                "fallback_calls",
                "invalidations",
                "observed_kinds",
            }
            for site in sites:
                self.assertTrue(required.issubset(site), site)
                for kind in site["observed_kinds"]:
                    self.assertNotIn("object_addr", kind)
                    self.assertNotIn("shape_addr", kind)

            array_set = [
                site
                for site in sites
                if site.get("guard_name") == "numeric_array_index_set_guard"
            ]
            self.assertTrue(array_set, sites)
            self.assertTrue(any(site["guard_passes"] >= 1 for site in array_set), array_set)
            self.assertTrue(any(site["fallback_calls"] >= 1 for site in array_set), array_set)
            array_kinds = [kind for site in array_set for kind in site["observed_kinds"]]
            self.assertTrue(any(kind.get("source") == "array" for kind in array_kinds), array_kinds)
            self.assertTrue(any(kind.get("value_kind") == "number" for kind in array_kinds), array_kinds)
            self.assertTrue(any(kind.get("value_kind") == "string" for kind in array_kinds), array_kinds)

            raw_set = [
                site
                for site in sites
                if site.get("guard_name") == "class_field_set_guard"
            ]
            self.assertTrue(raw_set, sites)
            self.assertTrue(any(site["guard_passes"] >= 1 for site in raw_set), raw_set)
            self.assertTrue(any(site["fallback_calls"] >= 1 for site in raw_set), raw_set)
            raw_kinds = [kind for site in raw_set for kind in site["observed_kinds"]]
            self.assertTrue(
                any(
                    kind.get("source") == "numeric_write"
                    and kind.get("field_index") == 0
                    and kind.get("value_kind") == "number"
                    for kind in raw_kinds
                ),
                raw_kinds,
            )
            self.assertTrue(
                any(
                    kind.get("source") == "numeric_write"
                    and kind.get("field_index") == 0
                    and kind.get("value_kind") == "string"
                    for kind in raw_kinds
                ),
                raw_kinds,
            )

            raw_get = [
                site
                for site in sites
                if site.get("guard_name") == "class_field_get_guard"
            ]
            self.assertTrue(any(site["guard_passes"] >= 1 for site in raw_get), raw_get)


if __name__ == "__main__":
    unittest.main()
