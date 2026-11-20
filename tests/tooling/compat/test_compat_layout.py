"""Check relocated compatibility entrypoints through their public CLIs."""

import csv
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class CompatLayoutTests(unittest.TestCase):
    def test_npm_default_manifest_and_output_use_repository_root(self):
        with tempfile.TemporaryDirectory(prefix="perry-compat-layout-") as temp:
            root = Path(temp).resolve()
            suite = root / "tests/compat/npm-sweep"
            suite.mkdir(parents=True)
            shutil.copy2(ROOT / "tests/compat/npm-sweep/run.py", suite / "run.py")
            (suite / "packages.json").write_text(json.dumps({
                "packages": [{"name": "layout-fixture", "version": "1.2.3"}],
            }), encoding="utf-8")
            bindir = root / "bin"
            bindir.mkdir()
            # These executable adapters only supply metadata; no package installs run.
            for name, output in [("node", "v26.12.1"), ("npm", "11.0.0"),
                                 ("git", "fixture-revision")]:
                binary = bindir / name
                binary.write_text(f"#!{sys.executable}\nprint({output!r})\n", encoding="utf-8")
                binary.chmod(0o755)
            env = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ["PATH"],
                       PYTHONDONTWRITEBYTECODE="1", TMPDIR=temp)
            env.pop("PERRY_BIN", None)
            result = subprocess.run(
                [sys.executable, str(suite / "run.py"), "--dry-run", "--strict"],
                cwd=root, env=env, capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            output = root / ".npm-sweep-results"
            self.assertTrue((output / "results.json").exists(), result.stdout)
            report = json.loads((output / "results.json").read_text(encoding="utf-8"))
            self.assertEqual(report["metadata"]["git_sha"], "fixture-revision")
            self.assertEqual(report["metadata"]["perry_bin"], str(root / "target/release/perry"))
            self.assertEqual(report["metadata"]["package_count"], 1)
            self.assertEqual([(r["package"], r["requested"], r["status"])
                              for r in report["results"]],
                             [("layout-fixture", "layout-fixture@1.2.3", "planned")])
            with (output / "results.csv").open(encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle))
            self.assertEqual([(r["package"], r["status"]) for r in rows],
                             [("layout-fixture", "planned")])
            self.assertTrue((output / "summary.md").exists())
            self.assertFalse((root / "tests/.npm-sweep-results").exists())


if __name__ == "__main__":
    unittest.main()
