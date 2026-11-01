"""Exercise docs generation with an external Cargo artifact and failure controls."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RegenerateApiDocs(unittest.TestCase):
    def run_fixture(self, failure=""):
        with tempfile.TemporaryDirectory(prefix="perry-docs-target-") as temporary:
            base = Path(temporary)
            checkout = base / "checkout"
            scripts = checkout / "scripts"
            scripts.mkdir(parents=True)
            shutil.copy2(ROOT / "scripts/regen_api_docs.sh", scripts / "regen_api_docs.sh")
            documents = [checkout / "docs/src/api/reference.md", checkout / "docs/api/perry.d.ts"]
            for path in documents:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("previous contents\n")
            artifact = base / "external target with spaces" / "release" / "perry"
            artifact.parent.mkdir(parents=True)
            artifact.write_text("""#!/usr/bin/env python3
import os, sys
kind = sys.argv[1].split('=', 1)[1]
if kind == 'dts' and os.environ.get('PERRY_DOCS_FIXTURE_FAILURE') == 'printer':
    print('incomplete declarations')
    sys.exit(17)
print('new ' + kind)
""")
            artifact.chmod(0o755)
            binaries = base / "bin"
            binaries.mkdir()
            cargo = binaries / "cargo"
            cargo.write_text("""#!/usr/bin/env python3
import json, os, sys
assert sys.argv[1:] == ['build', '--release', '-p', 'perry', '--message-format=json-render-diagnostics']
mode = os.environ.get('PERRY_DOCS_FIXTURE_FAILURE')
message = dict(reason='compiler-artifact', target=dict(name='perry', kind=['bin']), executable=os.environ['PERRY_DOCS_FIXTURE_ARTIFACT'])
if mode != 'missing-artifact':
    print(json.dumps(message))
sys.exit(23 if mode == 'build' else 0)
""")
            cargo.chmod(0o755)
            subprocess.run(["git", "init", "-q", str(checkout)], check=True)
            env = {**os.environ, "PATH": str(binaries) + os.pathsep + os.environ["PATH"],
                   "CARGO_TARGET_DIR": str(artifact.parent.parent),
                   "PERRY_DOCS_FIXTURE_ARTIFACT": str(artifact),
                   "PERRY_DOCS_FIXTURE_FAILURE": failure}
            result = subprocess.run(["bash", str(scripts / "regen_api_docs.sh")],
                                    cwd=checkout, env=env, capture_output=True, text=True)
            contents = [path.read_text() for path in documents]
            leftovers = list((checkout / "docs").rglob(".*.??????"))
            self.assertEqual(leftovers, [])
            if failure:
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(contents, ["previous contents\n"] * 2)
            else:
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(contents, ["new markdown\n", "new dts\n"])

    def test_external_target_with_spaces_uses_the_cargo_artifact(self):
        self.run_fixture()

    def test_failed_second_printer_preserves_both_documents(self):
        self.run_fixture("printer")

    def test_missing_executable_preserves_both_documents(self):
        self.run_fixture("missing-artifact")

    def test_failed_build_cannot_publish_an_artifact_message(self):
        self.run_fixture("build")


if __name__ == "__main__":
    unittest.main()
