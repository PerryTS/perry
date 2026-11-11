"""Exercise empty live heaps and compile-time trace instrumentation end to end."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from benchmarks.gc_ratchet.gc_ratchet import (
    RatchetError, binary_fingerprints, classify, measure,
)


COMPILER = r'''
import json, os, pathlib, sys
source = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[sys.argv.index("-o") + 1])
config = json.loads(source.read_text())
instruments = os.environ.get("PERRY_GC_INSTRUMENTS") == "1"
with open("compiles.jsonl", "a") as log:
    log.write(json.dumps({"output": str(output), "instruments": instruments}) + "\n")
program = "#!" + sys.executable + "\n"
program += "config = " + repr(config) + "\ninstruments = " + repr(instruments) + "\n"
program += r"""
import os, sys
traced = os.environ.get("PERRY_GC_DIAG") == "1"
if traced and not instruments:
    sys.exit(86)
if traced and config.get("trace_exit"):
    sys.exit(87)
print("wrong" if traced and config.get("trace_wrong_output") else "probe:empty")
for key, value in config["metrics"].items():
    print("#gcmetric " + key + "=" + str(value), file=sys.stderr)
if traced:
    print("[gc-copy-minor] ran copied_objects=5 copied_bytes=320 promoted_objects=0 promoted_bytes=0 freed_bytes=640", file=sys.stderr)
    print("[gc-scan-fallback] site=manual_collect automatic=false count=1", file=sys.stderr)
"""
output.write_text(program)
output.chmod(0o755)
'''


class MeasurementRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.perry = self.root / "perry"
        self.perry.write_text("#!" + sys.executable + "\n" + COMPILER)
        self.perry.chmod(0o755)
        self.probes = self.root / "probes"
        self.probes.mkdir()
        self.source = self.probes / "01_empty.ts"
        self.config = {"metrics": {
            "heap_used_bytes": 0, "heap_total_bytes": 4096, "rss_bytes": 8192,
        }}
        env = {k: v for k, v in os.environ.items() if not k.startswith("PERRY_")}
        patch = mock.patch.dict(os.environ, env, clear=True)
        patch.start()
        self.addCleanup(patch.stop)

    def run_measure(self):
        self.source.write_text(json.dumps(self.config))
        return measure(perry=self.perry, probes_dir=self.probes, repeats=3,
                       node=None, warmup=0)

    def test_zero_live_bytes_keep_real_collection_counters(self):
        payload = self.run_measure()
        metrics = payload["probes"]["01_empty"]["metrics"]
        self.assertEqual(metrics["heap_used_bytes"]["samples"], [0, 0, 0])
        self.assertEqual(metrics["copied_objects"]["samples"], [5, 5])
        compiles = [json.loads(line) for line in
                    (self.probes / "compiles.jsonl").read_text().splitlines()]
        self.assertEqual([c["instruments"] for c in compiles], [False, True])
        self.assertNotEqual(compiles[0]["output"], compiles[1]["output"])

    def test_classify_zero_live_heap_still_records_scan_sites(self):
        self.source.write_text(json.dumps(self.config))
        payload = classify(perry=self.perry, probes_dir=self.probes, repeats=1, warmup=0)
        row = payload["probes"][0]
        self.assertEqual(row["heap_used_precise_bytes"], 0)
        self.assertIn("manual_collect", row["scan_fallback_sites"])

    def test_missing_or_invalid_memory_measurements_still_fail(self):
        for metric, value in (("heap_used_bytes", None), ("heap_used_bytes", -1),
                              ("heap_total_bytes", 0), ("rss_bytes", 0)):
            with self.subTest(metric=metric, value=value):
                old = self.config["metrics"].pop(metric)
                if value is not None:
                    self.config["metrics"][metric] = value
                with self.assertRaises(RatchetError):
                    self.run_measure()
                self.config["metrics"][metric] = old

    def test_trace_failure_or_different_output_is_not_silent(self):
        for fault in ("trace_exit", "trace_wrong_output"):
            with self.subTest(fault=fault):
                self.config[fault] = True
                with self.assertRaisesRegex(RatchetError, "traced probe"):
                    self.run_measure()
                del self.config[fault]

    def test_fingerprints_include_the_archives_used_for_traces(self):
        for name in ("libperry_runtime.a", "libperry_stdlib.a",
                     "libperry_runtime_instr.a", "libperry_stdlib_instr.a"):
            (self.root / name).write_text(name)
        before = binary_fingerprints(self.perry)
        (self.root / "libperry_runtime_instr.a").write_text("changed")
        after = binary_fingerprints(self.perry)
        self.assertEqual(before["libperry_runtime.a"], after["libperry_runtime.a"])
        self.assertNotEqual(before["libperry_runtime_instr.a"]["sha256"],
                            after["libperry_runtime_instr.a"]["sha256"])


if __name__ == "__main__":
    unittest.main()
