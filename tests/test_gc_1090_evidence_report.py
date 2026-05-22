import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "gc_1090_evidence_report.py"

SPEC = importlib.util.spec_from_file_location("gc_1090_evidence_report", SCRIPT_PATH)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


REQUIRED_BENCHMARKS = (
    "bench_json_roundtrip",
    "bench_gc_pressure",
    "07_object_create",
    "12_binary_trees",
)


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def copied_workload(
    *,
    fallback_reason="none",
    conservative_pinned_bytes=0,
    copy_only_pinned_bytes=0,
    malloc_registry_rebuilds=0,
):
    counts = {reason: 0 for reason in REPORT.FALLBACK_REASONS}
    counts[fallback_reason] = 1
    return {
        "fallback_reason_counts": counts,
        "conservative_pinned_bytes": conservative_pinned_bytes,
        "legacy_copy_only_scanner_pinned": {"bytes": copy_only_pinned_bytes},
        "copying_nursery": {
            "copied_objects": 1,
            "copied_bytes": 16,
            "promoted_objects": 0,
            "promoted_bytes": 0,
            "malloc_registry_rebuilds": malloc_registry_rebuilds,
        },
    }


def copied_report(**overrides):
    workloads = {
        name: copied_workload()
        for name in REPORT.STRICT_COPIED_MINOR_WORKLOADS
    }
    workloads.update(overrides)
    return {
        "summary": {
            "cycles": len(workloads),
            "fallback_reason_counts": {"none": len(workloads)},
            "conservative_pinned_bytes": 0,
            "legacy_copy_only_scanner_pinned": {"bytes": 0},
            "copying_nursery": {
                "copied_objects": len(workloads),
                "copied_bytes": len(workloads) * 16,
                "promoted_objects": 0,
                "promoted_bytes": 0,
                "malloc_registry_rebuilds": 0,
            },
        },
        "workloads": workloads,
    }


def target_report():
    return {
        "summary": {
            "cycles": 1,
            "fallback_reason_counts": {"none": 1},
            "copying_nursery": {
                "copied_objects": 1,
                "copied_bytes": 16,
                "promoted_objects": 0,
                "promoted_bytes": 0,
                "malloc_registry_rebuilds": 0,
            },
            "old_page_accounting": {},
        }
    }


def benchmark_report(multiplier=1, correctness="pass"):
    benchmarks = {}
    for name in REQUIRED_BENCHMARKS:
        benchmarks[name] = {
            "perry_ms": 100 * multiplier,
            "perry_rss_kb": 100_000 * multiplier,
            "correctness": {
                "status": correctness,
                "reason": "matched",
                "actual_lines": ["checksum:1"],
                "expected_lines": ["checksum:1"],
            },
        }
    return {"commit": "abc", "benchmarks": benchmarks}


class Gc1090EvidenceReportTests(unittest.TestCase):
    def make_root(self, *, head_copied=None, head_benchmarks=None, head_memory_failed=0):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        metadata = {
            "base_ref": "origin/main",
            "head_ref": "HEAD",
            "base_sha": "a" * 40,
            "head_sha": "b" * 40,
            "commands": {
                "base": {
                    "build": {"status": "pass", "exit_code": 0},
                    "memory_stability": {"status": "pass", "exit_code": 0},
                    "benchmarks": {"status": "pass", "exit_code": 0},
                },
                "head": {
                    "build": {"status": "pass", "exit_code": 0},
                    "memory_stability": {"status": "pass", "exit_code": 0},
                    "benchmarks": {"status": "pass", "exit_code": 0},
                },
            },
        }
        write_json(root / "metadata.json", metadata)
        for label in ("base", "head"):
            write_json(
                root / label / "memory" / "reports" / "memory_stability_summary.json",
                {
                    "script": "run_memory_stability_tests.sh",
                    "passed": 58,
                    "failed": head_memory_failed if label == "head" else 0,
                    "skipped": 0,
                },
            )
            write_json(
                root / label / "memory" / "reports" / "copied_minor_fallback_report.json",
                head_copied if label == "head" and head_copied is not None else copied_report(),
            )
            write_json(
                root / label / "memory" / "reports" / "target_collector_gates_report.json",
                target_report(),
            )
            write_json(
                root / label / "benchmarks" / "full.json",
                head_benchmarks
                if label == "head" and head_benchmarks is not None
                else benchmark_report(),
            )
        return temp, root

    def collect(self, **kwargs):
        temp, root = self.make_root(**kwargs)
        self.addCleanup(temp.cleanup)
        return REPORT.collect_report(root, "base", "head")

    def test_pass_case(self):
        packet = self.collect()
        self.assertEqual(packet["status"], "pass")
        self.assertEqual(packet["errors"], [])

    def test_main_writes_packet_files(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        exit_code = REPORT.main(["--root", str(root)])
        self.assertEqual(exit_code, 0)
        self.assertTrue((root / "gc-1090-packet.json").exists())
        self.assertTrue((root / "gc-1090-packet.md").exists())
        packet = json.loads((root / "gc-1090-packet.json").read_text(encoding="utf-8"))
        self.assertEqual(packet["status"], "pass")
        self.assertIn("# #1090 GC Evidence Packet: PASS", (root / "gc-1090-packet.md").read_text(encoding="utf-8"))

    def test_fails_conservative_stack(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(fallback_reason="conservative_stack")
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("fallback reasons other than none" in error for error in packet["errors"])
        )

    def test_fails_conservative_pinned_bytes(self):
        packet = self.collect(
            head_copied=copied_report(
                json_roundtrip=copied_workload(conservative_pinned_bytes=8)
            )
        )
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(
            any("conservative_pinned_bytes=8" in error for error in packet["errors"])
        )

    def test_fails_benchmark_correctness(self):
        packet = self.collect(head_benchmarks=benchmark_report(correctness="fail"))
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("correctness failed" in error for error in packet["errors"]))

    def test_fails_memory_stability(self):
        packet = self.collect(head_memory_failed=1)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("memory stability failed=1" in error for error in packet["errors"]))


if __name__ == "__main__":
    unittest.main()
