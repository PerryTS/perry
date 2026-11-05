"""Exercise feature-matrix discovery and reports without compiling program cases."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]


class FeatureLayoutTests(unittest.TestCase):
    def test_default_layout_preserves_reports_and_check(self):
        with tempfile.TemporaryDirectory(prefix="perry-feature-layout-") as tmp:
            root = Path(tmp)
            scripts = root / "scripts"
            scripts.mkdir()
            shutil.copy2(REPO_ROOT / "scripts/gen_feature_matrix.py", scripts)
            features = root / "tests/features"
            probes = features / "probes"
            probes.mkdir(parents=True)
            names = ["pass", "node-fail", "runtime-fail", "diff", "compile-fail"]
            manifest = ['[settings]', 'node_args = ["--experimental-strip-types"]']
            for name in names:
                (probes / f"{name}.ts").write_text("// inert probe\n", encoding="utf-8")
                manifest.extend([
                    '[[probe]]', 'category = "syntax"', f'name = "{name}"',
                    f'path = "probes/{name}.ts"',
                ])
            (features / "feature_matrix.toml").write_text("\n".join(manifest), encoding="utf-8")
            node = root / "node"
            node.write_text(
                f"#!{sys.executable}\n"
                "from pathlib import Path\nimport sys\n"
                "assert sys.argv[1] == '--experimental-strip-types'\n"
                "if Path(sys.argv[-1]).stem == 'node-fail':\n"
                "    print('oracle failed'); sys.exit(3)\n"
                "print('oracle')\n", encoding="utf-8",
            )
            compiler = root / "compiler"
            compiler.write_text(
                f"#!{sys.executable}\n"
                "from pathlib import Path\nimport sys\n"
                "assert sys.argv[2] == '-o'\n"
                "name = Path(sys.argv[1]).stem\n"
                "if name == 'compile-fail':\n"
                "    print('compiler failed'); sys.exit(4)\n"
                "output = 'different' if name == 'diff' else 'oracle'\n"
                "exit_code = 5 if name == 'runtime-fail' else 0\n"
                "binary = Path(sys.argv[3])\n"
                "binary.write_text(f'#!{sys.executable}\\nprint({output!r})\\nraise SystemExit({exit_code})\\n')\n"
                "binary.chmod(0o755)\n", encoding="utf-8",
            )
            node.chmod(0o755)
            compiler.chmod(0o755)
            command = [
                sys.executable, str(scripts / "gen_feature_matrix.py"),
                "--node-bin", str(node), "--perry-bin", str(compiler),
                "--report", str(root / "report.json"),
                "--generated-output", str(root / "generated.md"),
            ]
            env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", TMPDIR=tmp)

            def run(*extra):
                return subprocess.run(command + list(extra), cwd=root, env=env,
                                      capture_output=True, text=True, timeout=30)

            generated = run()
            self.assertEqual((generated.returncode, generated.stdout, generated.stderr),
                             (0, "", ""), generated.stderr)
            report = json.loads((root / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(report["config"], "tests/features/feature_matrix.toml")
            self.assertEqual(report["summary"], {
                "probes": 5, "passing": 1,
                "by_status": {"PASS": 1, "NODE-FAIL": 1, "RUNTIME-FAIL": 1,
                              "DIFF": 1, "COMPILE-FAIL": 1},
            })
            self.assertEqual([(p["name"], p["status"], p["node_exit"], p["perry_exit"])
                              for p in report["probes"]], [
                ("compile-fail", "COMPILE-FAIL", 0, 4),
                ("diff", "DIFF", 0, 0), ("node-fail", "NODE-FAIL", 3, None),
                ("pass", "PASS", 0, 0), ("runtime-fail", "RUNTIME-FAIL", 0, 5),
            ])
            self.assertEqual([p["path"] for p in report["probes"]], [
                f"tests/features/probes/{name}.ts"
                for name in ["compile-fail", "diff", "node-fail", "pass", "runtime-fail"]
            ])
            committed = features / "feature_matrix.md"
            markdown = committed.read_text(encoding="utf-8")
            self.assertEqual(markdown, (root / "generated.md").read_text(encoding="utf-8"))
            self.assertIn("from `tests/features/feature_matrix.toml`.", markdown)
            checked = run("--check")
            self.assertEqual((checked.returncode, checked.stdout, checked.stderr),
                             (0, "tests/features/feature_matrix.md is up to date\n", ""))
            # Explicit paths remain a supported interface alongside the defaults.
            explicit = run("--config", str(features / "feature_matrix.toml"),
                           "--output", str(committed), "--check")
            self.assertEqual((explicit.returncode, explicit.stdout, explicit.stderr),
                             (checked.returncode, checked.stdout, checked.stderr))
            committed.write_text("stale\n", encoding="utf-8")
            stale = run("--check")
            self.assertEqual(stale.returncode, 1)
            self.assertEqual(stale.stdout, "")
            self.assertIn("--- tests/features/feature_matrix.md (committed)", stale.stderr)
            self.assertEqual(committed.read_text(encoding="utf-8"), "stale\n")


if __name__ == "__main__":
    unittest.main()
