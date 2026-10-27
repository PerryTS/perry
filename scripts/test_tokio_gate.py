#!/usr/bin/env python3
"""Exercise the real offline CLI and the final CI gate, without compiling Rust."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts/tokio_inventory.py"
WORKFLOW = ROOT / ".github/workflows/test.yml"
CLEAN_LOCK = 'version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n'
DIRTY_LOCK = CLEAN_LOCK + '''
[[package]]
name = "wasm-bindgen-futures"
version = "0.4.79"
dependencies = ["tokio"]

[[package]]
name = "tokio"
version = "1.53.3"
'''


def job(name):
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)",
                      WORKFLOW.read_text(), re.M | re.S)
    if not match:
        raise AssertionError(f"missing CI job {name}")
    return "\n".join(line for line in match.group(1).splitlines()
                     if not line.lstrip().startswith("#"))


class TokioGateTests(unittest.TestCase):
    def test_offline_cli_rejects_regressions_without_cargo_or_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            (root / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
            scripts = root / "scripts"
            scripts.mkdir()
            baseline = scripts / "tokio_inventory.json"
            baseline.write_text(json.dumps({"edges": [], "lockfile": {}, "not_tokio": []}))
            binary = root / "bin"
            binary.mkdir()
            cargo = binary / "cargo"
            cargo.write_text('#!/bin/sh\nprintf invoked > "$NO_TOKIO_CARGO_MARKER"\nexit 97\n')
            cargo.chmod(0o755)
            env = {**os.environ, "PATH": str(binary) + os.pathsep + os.environ["PATH"],
                   "NO_TOKIO_CARGO_MARKER": str(root / "cargo-ran")}

            def run(expected, *flags):
                before = {str(p): p.read_bytes() for p in root.rglob("*")
                          if p.is_file() and ".git" not in p.parts}
                result = subprocess.run([sys.executable, str(SCRIPT), *flags], cwd=root,
                                        env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                self.assertFalse((root / "cargo-ran").exists(), "offline ban invoked Cargo")
                after = {str(p): p.read_bytes() for p in root.rglob("*")
                         if p.is_file() and ".git" not in p.parts}
                self.assertEqual(before, after, "the check mutated repository inputs")

            lock = root / "Cargo.lock"
            lock.write_text(CLEAN_LOCK)
            run(0, "--offline")
            lock.write_text(DIRTY_LOCK)
            run(1, "--offline")
            run(1, "--update")  # Baseline refresh cannot legitimize Tokio.
            lock.write_text(CLEAN_LOCK)
            nested = root / "fixture/host"
            nested.mkdir(parents=True)
            manifest = nested / "Cargo.toml"
            manifest.write_text('''[package]
name = "nested"
version = "1.0.0"
[target.'cfg(target_os = "emscripten")'.dev-dependencies]
alias = { package = "tokio", version = "1", optional = true }
''')
            run(1, "--offline")
            manifest.write_text('[package]\nname = "nested"\nversion = "1.0.0"\n')
            (nested / "Cargo.lock").write_text(DIRTY_LOCK)
            run(1, "--offline")
            (nested / "Cargo.lock").write_text(CLEAN_LOCK)
            run(0, "--offline")

    def test_independent_preflight_is_not_optional(self):
        preflight = job("tokio-ban")
        self.assertIn("needs: route", preflight)
        self.assertNotIn("needs: plan", preflight)
        self.assertIn("github.event_name == 'pull_request'", preflight)
        self.assertIn("needs.route.outputs.run_core == 'true'", preflight)
        self.assertIn("scripts/tokio_inventory.py --offline", preflight)
        self.assertIn("scripts/test_tokio_gate.py", preflight)
        self.assertNotIn("continue-on-error", preflight)
        self.assertNotIn("|| true", preflight)
        self.assertNotIn("cargo ", preflight)
        self.assertIn("      - tokio-ban\n", job("gate"))

    def test_actual_final_gate_requires_explicit_preflight_success(self):
        # Run the workflow's actual shell, rather than a Python reimplementation
        # of its policy. jq is supplied by the Ubuntu runner for this CI check.
        shell = job("gate").split("        run: |\n", 1)[1]
        shell = "\n".join(line[10:] for line in shell.splitlines() if line.startswith("          "))
        for state in ["success", "skipped", "failure", "cancelled", None]:
            with self.subTest(preflight=state):
                needs = {
                    "route": {"result": "success"},
                    "plan": {"result": "success", "outputs": {
                        "plan": json.dumps({"jobs": {"wasi_execution": False}})}},
                    "lint": {"result": "success"},
                    "wasi-execution": {"result": "skipped"},
                }
                if state is not None:
                    needs["tokio-ban"] = {"result": state}
                result = subprocess.run(["bash", "-c", shell], capture_output=True, text=True,
                                        env={**os.environ, "NEEDS": json.dumps(needs), "TIER": "pr"},
                                        timeout=30)
                self.assertEqual(result.returncode, 0 if state == "success" else 1,
                                 result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
