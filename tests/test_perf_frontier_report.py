import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "perf_frontier_report.py"

SPEC = importlib.util.spec_from_file_location("perf_frontier_report", SCRIPT_PATH)
assert SPEC is not None
REPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(REPORT)


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def benchmark_report(*, correctness=True):
    rows = {}
    for name in REPORT.REQUIRED_BENCHMARK_ROWS:
        entry = {
            "perry_ms": 100,
            "perry_rss_kb": 50_000,
            "node_ms": 50,
            "node_rss_kb": 40_000,
        }
        if correctness:
            entry["correctness"] = {
                "status": "pass",
                "actual_lines": ["checksum:1"],
                "expected_lines": ["checksum:1"],
                "reason": "matched",
            }
        rows[name] = entry
    return {"commit": "abcd1234", "benchmarks": rows}


def trace_summary(name):
    return {
        "schema_version": 1,
        "workload": name,
        "present": True,
        "gc_cycle_count": 1,
        "pause_us": {"total": 0, "max": 0, "avg": 0},
        "trigger_kind_counts": {"manual": 1},
        "byte_totals": {
            "copied_bytes": 0,
            "promoted_bytes": 0,
            "moved_bytes": 0,
            "old_page_moved_bytes": 0,
            "productive_reclaim_bytes": 0,
        },
    }


def math_report(*, checksum_gate="pass"):
    return {
        "schema_version": 1,
        "checksum_gate": checksum_gate,
        "checksum_relative_delta": 1e-6 if checksum_gate == "pass" else 1e-3,
        "node": {"median_ms": 10.0, "checksum": 100.0},
        "perry": {"median_ms": 20.0, "checksum": 100.0001},
        "perry_to_node_ratio": 2.0,
    }


def slice_report():
    return {
        "schema_version": 1,
        "rows": [
            {
                "name": "class_method_no_field_access",
                "source": "/tmp/slice.ts",
                "node_ms": 10.0,
                "perry_ms": 100.0,
                "perry_to_node_ratio": 10.0,
            }
        ],
    }


def profile_summary():
    return {
        "schema_version": 1,
        "status": "pass",
        "requested": True,
        "row": "class_method_no_field_access",
        "top_non_gc_costs": [{"symbol": "js_object_get_own_field_or_undef", "samples": 10}],
    }


class PerfFrontierReportTests(unittest.TestCase):
    def make_root(
        self,
        *,
        correctness=True,
        traces=True,
        profile=True,
        checksum_gate="pass",
    ):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        write_json(
            root / "metadata.json",
            {
                "base_ref": "origin/main",
                "head_ref": "HEAD",
                "base_sha": "a" * 40,
                "head_sha": "b" * 40,
                "trace_rows": list(REPORT.REQUIRED_BENCHMARK_ROWS),
                "commands": {
                    "base": {
                        "build": {"status": "pass", "exit_code": 0},
                        "memory_stability": {"status": "pass", "exit_code": 0},
                        "benchmarks": {"status": "pass", "exit_code": 0},
                        "direct_traces": {"status": "pass", "exit_code": 0},
                        "benchmark_math": {"status": "pass", "exit_code": 0},
                    },
                    "head": {
                        "build": {"status": "pass", "exit_code": 0},
                        "memory_stability": {"status": "pass", "exit_code": 0},
                        "benchmarks": {"status": "pass", "exit_code": 0},
                        "direct_traces": {"status": "pass", "exit_code": 0},
                        "benchmark_math": {"status": "pass", "exit_code": 0},
                    },
                },
            },
        )
        for label in ("base", "head"):
            write_json(
                root / label / "memory" / "reports" / "memory_stability_summary.json",
                {"passed": 10, "failed": 0, "skipped": 0},
            )
            write_json(
                root / label / "benchmarks" / "full.json",
                benchmark_report(correctness=correctness),
            )
            write_json(
                root / label / "benchmark-math" / "math-benchmark.json",
                math_report(checksum_gate=checksum_gate if label == "head" else "pass"),
            )
            write_json(
                root / label / "benchmark-math" / "slice-results.json",
                slice_report(),
            )
            if traces:
                for row in REPORT.REQUIRED_BENCHMARK_ROWS:
                    write_json(
                        root / label / "direct-traces" / "summaries" / f"{row}.json",
                        trace_summary(row),
                    )
        if profile:
            write_json(root / "profile_summary.json", profile_summary())
        return temp, root

    def collect(self, **kwargs):
        temp, root = self.make_root(**kwargs)
        self.addCleanup(temp.cleanup)
        return REPORT.collect_report(root, gate=True)

    def test_exact_sha_validation(self):
        self.assertTrue(REPORT.exact_sha("a" * 40))
        self.assertFalse(REPORT.exact_sha("a" * 39))
        self.assertFalse(REPORT.exact_sha("g" * 40))

    def test_gate_fails_missing_correctness(self):
        packet = self.collect(correctness=False)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("correctness" in error for error in packet["errors"]))

    def test_gate_fails_missing_trace_and_profile(self):
        packet = self.collect(traces=False, profile=False)
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("trace summary missing" in error for error in packet["errors"]))
        self.assertTrue(any("profile_summary.json is missing" in error for error in packet["errors"]))

    def test_typed_checksum_tolerance(self):
        self.assertTrue(REPORT.checksum_within_tolerance(100.0, 100.0001))
        self.assertFalse(REPORT.checksum_within_tolerance(100.0, 100.01))
        packet = self.collect(checksum_gate="fail")
        self.assertEqual(packet["status"], "fail")
        self.assertTrue(any("checksum relative delta" in error for error in packet["errors"]))

    def test_classification_priority(self):
        entry = {"perry_ms": 100}
        gc_trace = {
            "gc_cycle_count": 4,
            "pause_us": {"total": 30_000},
            "trigger_kind_counts": {"arena_bytes": 4},
            "byte_totals": {"productive_reclaim_bytes": 0},
        }
        self.assertEqual(
            REPORT.classify_row("bench_gc_pressure", entry, gc_trace)["class"],
            REPORT.CLASS_GC_BOUND,
        )
        policy_trace = {
            "gc_cycle_count": 4,
            "pause_us": {"total": 1_000},
            "trigger_kind_counts": {"arena_bytes": 4},
            "byte_totals": {"copied_bytes": 1000, "productive_reclaim_bytes": 0},
        }
        self.assertEqual(
            REPORT.classify_row("bench_gc_pressure", entry, policy_trace)["class"],
            REPORT.CLASS_TRIGGER_POLICY_BOUND,
        )

    def test_classification_taxonomy_is_exact(self):
        packet = self.collect()
        classes = {
            entry["class"]
            for entry in packet["classification"].values()
            if isinstance(entry, dict)
        }
        self.assertLessEqual(classes, REPORT.ALLOWED_CLASSIFICATIONS)
        self.assertEqual(
            REPORT.classify_row("07_object_create", {"perry_ms": 100}, None)["class"],
            REPORT.CLASS_PROPERTY_METHOD_DISPATCH_BOUND,
        )
        self.assertEqual(
            REPORT.classify_row("bench_json_roundtrip", {"perry_ms": 100}, None)["class"],
            REPORT.CLASS_HELPER_RUNTIME_CALL_BOUND,
        )
        self.assertEqual(
            REPORT.classify_row("12_binary_trees", {"perry_ms": 100}, None)["class"],
            REPORT.CLASS_BOUNDS_CHECK_LOOP_BOUND,
        )

    def test_gate_rejects_unknown_classification(self):
        errors = REPORT.classification_taxonomy_errors(
            {"row": {"class": "numeric representation", "reasons": [], "evidence": {}}}
        )
        self.assertTrue(errors)
        self.assertIn("allowed taxonomy", errors[0])

    def test_baseline_reference_points_to_locked_snapshot(self):
        temp, root = self.make_root()
        self.addCleanup(temp.cleanup)
        baseline_path = root / "locked-baseline.json"
        write_json(
            baseline_path,
            {
                "schema_version": REPORT.SCHEMA_VERSION,
                "generated_at": "2026-05-22T00:00:00Z",
                "baseline_sha": "c" * 40,
                "comparison_base_sha": "a" * 40,
                "selected_rows": list(REPORT.REQUIRED_BENCHMARK_ROWS),
            },
        )

        packet_path = root / "packet.json"
        status = REPORT.main(
            [
                "packet",
                "--root",
                str(root),
                "--json-out",
                str(packet_path),
                "--baseline-in",
                str(baseline_path),
                "--gate",
            ]
        )
        self.assertEqual(status, 0)
        packet = json.loads(packet_path.read_text(encoding="utf-8"))
        self.assertEqual(packet["status"], "pass", packet["errors"])
        self.assertEqual(packet["baseline"]["input_path"], str(baseline_path))
        self.assertEqual(packet["baseline"]["baseline_sha"], "c" * 40)


if __name__ == "__main__":
    unittest.main()
