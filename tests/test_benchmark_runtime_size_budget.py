"""A recovered build failure must never become a runtime-size baseline."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("runtime_size_budget", ROOT / "scripts/runtime_size_budget.py")
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


class RuntimeSizeFallbackTests(unittest.TestCase):
    def test_recovered_build_failure_is_rejected_before_running_binary(self):
        warning = (
            "warning: auto-optimized runtime build failed; linking the full prebuilt runtime and stdlib instead (larger binary).\n"
            "  First error: error[E0425]: missing nanbox_handle_value\n"
        )
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            budget.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", warning)
        ) as run:
            with self.assertRaisesRegex(SystemExit, "compile fell back"):
                budget.measure("perry", Path("hello.ts"), Path(tmp))
            self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
