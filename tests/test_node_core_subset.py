import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "node_core_subset.py"
SPEC = importlib.util.spec_from_file_location("node_core_subset", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class NormalizeTests(unittest.TestCase):
    def test_canonicalizes_generated_http_date_headers(self):
        node = (
            '> data "HTTP/1.1 200 OK\\r\\n'
            'Date: Sat, 22 Aug 2026 20:15:36 GMT\\r\\n\\r\\n"'
        )
        perry = (
            '> data "HTTP/1.1 200 OK\\r\\n'
            'Date: Sat, 22 Aug 2026 20:33:38 GMT\\r\\n\\r\\n"'
        )
        self.assertEqual(MODULE.normalize(node), MODULE.normalize(perry))

    def test_preserves_malformed_or_non_http_dates(self):
        value = "Date: tomorrow\ncreated Sat, 22 Aug 2026 20:15:36 GMT"
        self.assertEqual(MODULE.normalize(value), value)

    def test_preserves_impossible_numeric_http_dates(self):
        value = "Date: Sat, 99 Aug 2026 29:78:61 GMT"
        self.assertEqual(MODULE.normalize(value), value)


@unittest.skipUnless(shutil.which("node"), "Node is required for oracle probes")
class RunnerTests(unittest.TestCase):
    def run_case(self, source):
        with tempfile.TemporaryDirectory(prefix="node-core-classification-") as tmp:
            root = Path(tmp)
            parallel = root / "test" / "parallel"
            parallel.mkdir(parents=True)
            (parallel / "test-path-audit.js").write_text(source)
            calls = root / "compile-calls"
            compiler = root / "perry"
            # The compiler double runs the same staged JS with Node. These
            # tests exercise runner classification, not compiler parity.
            compiler.write_text(
                f"#!{sys.executable}\n"
                "import pathlib, shlex, sys\n"
                f"pathlib.Path({str(calls)!r}).touch()\n"
                "out = pathlib.Path(sys.argv[sys.argv.index('-o') + 1])\n"
                "out.write_text('#!/bin/sh\\nexec node ' + "
                "shlex.quote(sys.argv[2]) + '\\n')\n"
                "out.chmod(0o755)\n"
            )
            compiler.chmod(0o755)
            report = root / "report.json"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--root", str(root),
                 "--api", "path", "--perry-bin", str(compiler),
                 "--report", str(report), "--quiet"],
                capture_output=True, text=True, timeout=30,
                env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"),
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            data = json.loads(report.read_text())
            self.assertTrue(data["complete"])
            return data, calls.exists()

    def test_explicit_skip_is_not_compiled_or_counted_as_pass(self):
        data, compiled = self.run_case(
            "require('../common').skip('subject never ran');\n"
            "throw new Error('unreachable');\n"
        )
        self.assertFalse(compiled)
        self.assertEqual(data["totals"]["node-skip"], 1)
        self.assertEqual(data["totals"]["pass"], 0)
        self.assertEqual(data["judged"], 0)
        self.assertEqual(data["parity_pct"], 0.0)
        self.assertIn("subject never ran", json.dumps(data["samples"]))

    def test_existing_oracle_classification_is_preserved(self):
        for source, bucket, compiled in [
            ("console.log('executed');", "pass", True),
            ("require('../common').printSkipMessage('not an exit');\n"
             "require('assert').strictEqual(2 + 2, 4);", "pass", True),
            ("throw new Error('oracle failure');", "node-skip", False),
            ("const cp = require('child_process');\n"
             "const script = 'require(' + JSON.stringify(require.resolve('../common')) + "
             "').skip(\"child only\")';\n"
             "require('assert').strictEqual(cp.spawnSync(process.execPath, "
             "['-e', script]).status, 0);\n"
             "console.log('parent executed');", "pass", True),
            ("const {Worker} = require('worker_threads');\n"
             "const script = 'require(' + JSON.stringify(require.resolve('../common')) + "
             "').skip(\"worker only\")';\n"
             "new Worker(script, {eval: true}).on('exit', code => {\n"
             "require('assert').strictEqual(code, 0);\n"
             "console.log('parent executed'); });", "pass", True),
        ]:
            with self.subTest(bucket=bucket, source=source):
                data, actual_compiled = self.run_case(source)
                self.assertEqual(actual_compiled, compiled)
                self.assertEqual(data["totals"][bucket], 1)
                self.assertEqual(data["judged"], int(compiled))


if __name__ == "__main__":
    unittest.main()
