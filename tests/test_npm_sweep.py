import csv
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "test-compat" / "npm-sweep" / "run.py"

SPEC = importlib.util.spec_from_file_location("npm_sweep", SCRIPT_PATH)
assert SPEC is not None
SWEEP = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = SWEEP
SPEC.loader.exec_module(SWEEP)


class NpmSweepTests(unittest.TestCase):
    def test_parse_package_spec_handles_scoped_versions(self):
        plain = SWEEP.parse_package_spec("express@latest")
        scoped = SWEEP.parse_package_spec("@types/node@26.0.0")
        unversioned_scoped = SWEEP.parse_package_spec("@scope/pkg")

        self.assertEqual((plain.name, plain.version), ("express", "latest"))
        self.assertEqual((scoped.name, scoped.version), ("@types/node", "26.0.0"))
        self.assertEqual((unversioned_scoped.name, unversioned_scoped.version), ("@scope/pkg", "latest"))

    def test_first_failure_line_prefers_actionable_marker(self):
        line = SWEEP.first_failure_line(
            "build started\nError: Linking failed\n",
            "Undefined symbol: __perry_missing_wrapper\nlater details\n",
        )

        self.assertEqual(line, "Undefined symbol: __perry_missing_wrapper")

    def test_run_command_reports_timeout(self):
        result = SWEEP.run_command(
            [sys.executable, "-c", "import time; time.sleep(2)"],
            REPO_ROOT,
            timeout_secs=1,
        )

        self.assertEqual(result.exit_code, 124)
        self.assertTrue(result.timed_out)

    def test_dry_run_writes_json_csv_summary_and_history(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            out_dir = root / "out"
            history = root / "history.csv"

            rc = SWEEP.main(
                [
                    "--dry-run",
                    "--packages",
                    "nanoid,@types/node@26.0.0",
                    "--out-dir",
                    str(out_dir),
                    "--history",
                    str(history),
                ]
            )

            self.assertEqual(rc, 0)
            results = json.loads((out_dir / "results.json").read_text(encoding="utf-8"))
            self.assertEqual(results["metadata"]["mode"], "dry-run")
            self.assertEqual([row["status"] for row in results["results"]], ["planned", "planned"])
            self.assertTrue((out_dir / "summary.md").exists())

            with (out_dir / "results.csv").open(encoding="utf-8") as handle:
                csv_rows = list(csv.DictReader(handle))
            with history.open(encoding="utf-8") as handle:
                history_rows = list(csv.DictReader(handle))
            self.assertEqual([row["package"] for row in csv_rows], ["nanoid", "@types/node"])
            self.assertEqual([row["status"] for row in history_rows], ["planned", "planned"])


if __name__ == "__main__":
    unittest.main()
