#!/usr/bin/env python3
"""The profiler's memory must never become the workload RSS measurement."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


@unittest.skipUnless(sys.platform == "linux" and shutil.which("taskset"), "Linux affinity required")
class RssScopeTest(unittest.TestCase):
    def test_profiler_with_large_heap_does_not_inflate_tiny_workload(self):
        source = Path(__file__).with_name("measure_linux_rss_ab.py")
        spec = importlib.util.spec_from_file_location("rss_harness", source)
        harness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(harness)
        with tempfile.TemporaryDirectory(prefix="sp-rss-scope-") as directory:
            root = Path(directory)
            fake = root / "perf"
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import sys,subprocess\n"
                "from pathlib import Path\n"
                "heap=bytearray(64*1024*1024)\n"
                "args=sys.argv[1:]\n"
                "result=subprocess.run(args[args.index('--')+1:])\n"
                "Path(args[args.index('-o')+1]).write_text("
                "'1000000;;instructions:u;1;100.00;;\\n')\n"
                "sys.exit(result.returncode)\n")
            fake.chmod(0o755)
            oracle = root / "oracle"
            oracle.write_bytes(b"")
            arm = dict(source=str(root), target=str(root), runtime=str(root))
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps(dict(
                arms=dict(base=arm, head=arm),
                workloads=[dict(name="tiny", cwd=str(root), oracle=str(oracle),
                                binaries=dict(base="/usr/bin/true", head="/usr/bin/true"))])))
            run = subprocess.run

            def substitute_profiler(command, *args, **kwargs):
                command = [str(fake) if part == "perf" else part for part in command]
                return run(command, *args, **kwargs)

            output = root / "results"
            argv = [str(source), "--manifest", str(manifest), "--output", str(output),
                    "--runs", "10", "--cpu", str(min(os.sched_getaffinity(0)))]
            with patch.object(sys, "argv", argv), \
                    patch.object(harness.subprocess, "run", substitute_profiler), \
                    contextlib.redirect_stdout(io.StringIO()):
                harness.main()
            rows = json.loads((output / "runs.json").read_text())
            self.assertEqual(len(rows), 40)
            self.assertTrue((output / "done").exists())
            self.assertLess(max(row["rss_kib"] for row in rows), 32 * 1024,
                            "RSS included the profiler's 64 MiB allocation")
            # Interrupt after only the base half of a pair. Resuming must
            # repeat both arms together and preserve earlier raw receipts.
            (output / "done").unlink()
            (output / "runs.json").write_text(json.dumps(rows[:-1]))
            with patch.object(sys, "argv", argv + ["--resume"]), \
                    patch.object(harness.subprocess, "run", substitute_profiler), \
                    contextlib.redirect_stdout(io.StringIO()):
                harness.main()
            resumed = json.loads((output / "runs.json").read_text())
            self.assertEqual(len(resumed), 40)
            repeated = [r for r in resumed if r["round"] == 9 and r["mode"] == rows[-1]["mode"]]
            self.assertEqual({r["arm"] for r in repeated}, {"base", "head"})
            self.assertEqual({r["attempt"] for r in repeated}, {1})
            self.assertTrue(Path(rows[-2]["receipt"] + ".perf").exists())
            self.assertTrue((output / "done").exists())


if __name__ == "__main__":
    unittest.main()
