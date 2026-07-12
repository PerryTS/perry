import json
import contextlib
import io
import tempfile
import unittest
from pathlib import Path

from benchmarks.benchmark_gate import (
    ArtifactError,
    build_artifact,
    evaluate_regressions,
    load_artifact,
    main,
    summarize_http,
)


def runtime(available=True, version="1.0.0", command=None):
    return {
        "available": available,
        "version": version if available else None,
        "command": command or ["runtime"],
    }


def record(name, perry, node=None, bun=None, rss=None):
    runtimes = {
        "perry": {"wall_ms": perry, "rss_kb": rss or [5000] * len(perry)},
    }
    if node is not None:
        runtimes["node"] = {"wall_ms": node, "rss_kb": [30000] * len(node)}
    if bun is not None:
        runtimes["bun"] = {"wall_ms": bun, "rss_kb": [25000] * len(bun)}
    return {
        "name": name,
        "runtimes": runtimes,
        "correctness": {"status": "pass", "reference": "node"},
    }


class BenchmarkArtifactTests(unittest.TestCase):
    def test_builds_distributions_and_peer_ratios(self):
        artifact = build_artifact(
            records=[record("fast", [9, 10, 11], [18, 20, 22], [6, 7, 8])],
            requested_samples=3,
            runtimes={
                "perry": runtime(command=["target/release/perry", "<source>"]),
                "node": runtime(command=["node", "<source.ts>"]),
                "bun": runtime(command=["bun", "run", "<source.ts>"]),
            },
            commit="abc123",
            generated_at="2026-07-12T00:00:00Z",
        )

        entry = artifact["benchmarks"]["fast"]
        self.assertEqual(artifact["schema_version"], 2)
        self.assertEqual(entry["runtimes"]["perry"]["wall_ms"]["samples"], [9, 10, 11])
        self.assertEqual(entry["runtimes"]["perry"]["wall_ms"]["sample_count"], 3)
        self.assertEqual(entry["runtimes"]["perry"]["wall_ms"]["median"], 10)
        self.assertEqual(entry["ratios"]["perry_to_node"]["wall_time"], 0.5)
        self.assertAlmostEqual(entry["ratios"]["perry_to_bun"]["wall_time"], 10 / 7, places=6)

    def test_rejects_incomplete_available_runtime_samples(self):
        with self.assertRaisesRegex(ArtifactError, "fast.*bun.*2/3"):
            build_artifact(
                records=[record("fast", [9, 10, 11], [18, 20, 22], [6, 7])],
                requested_samples=3,
                runtimes={
                    "perry": runtime(),
                    "node": runtime(),
                    "bun": runtime(),
                },
                commit="abc123",
                generated_at="2026-07-12T00:00:00Z",
            )

    def test_rejects_zero_rss_as_an_incomplete_measurement(self):
        with self.assertRaisesRegex(ArtifactError, "fast.*perry.*zero RSS"):
            build_artifact(
                records=[record("fast", [9, 10, 11], [18, 20, 22], rss=[5000, 0, 5000])],
                requested_samples=3,
                runtimes={"perry": runtime(), "node": runtime(), "bun": runtime(False)},
                commit="abc123",
                generated_at="2026-07-12T00:00:00Z",
            )

    def test_rejects_runtime_metadata_without_a_command(self):
        with self.assertRaisesRegex(ArtifactError, "bun.*pinned command"):
            build_artifact(
                records=[record("fast", [9, 10, 11], [18, 20, 22])],
                requested_samples=3,
                runtimes={
                    "perry": runtime(),
                    "node": runtime(),
                    "bun": {"available": False, "version": None, "command": []},
                },
                commit="abc123",
                generated_at="2026-07-12T00:00:00Z",
            )

    def test_build_cli_rejects_malformed_json_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            records = root / "records.jsonl"
            metadata = root / "metadata.json"
            records.write_text("{not-json}\n")
            metadata.write_text("{}")
            with contextlib.redirect_stderr(io.StringIO()):
                exit_code = main([
                    "build",
                    "--records", str(records),
                    "--runtime-metadata", str(metadata),
                    "--output", str(root / "out.json"),
                    "--runs", "3",
                ])
        self.assertEqual(exit_code, 2)

    def test_bun_absent_is_explicit_and_does_not_invalidate_results(self):
        artifact = build_artifact(
            records=[record("fast", [9, 10, 11], [18, 20, 22])],
            requested_samples=3,
            runtimes={
                "perry": runtime(),
                "node": runtime(),
                "bun": runtime(available=False, command=["bun", "run", "<source.ts>"]),
            },
            commit="abc123",
            generated_at="2026-07-12T00:00:00Z",
        )

        self.assertFalse(artifact["runtimes"]["bun"]["available"])
        self.assertNotIn("bun", artifact["benchmarks"]["fast"]["runtimes"])
        self.assertIsNone(artifact["benchmarks"]["fast"]["ratios"]["perry_to_bun"])

    def test_loads_legacy_baseline_schema(self):
        legacy = {
            "commit": "old",
            "benchmarks": {
                "fast": {"perry_ms": 9, "node_ms": 18, "perry_rss_kb": 5000}
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "baseline.json"
            path.write_text(json.dumps(legacy))
            loaded = load_artifact(path, require_complete=False)

        entry = loaded["benchmarks"]["fast"]
        self.assertEqual(entry["runtimes"]["perry"]["wall_ms"]["median"], 9)
        self.assertEqual(entry["ratios"]["perry_to_node"]["wall_time"], 0.5)

    def test_load_rejects_tampered_sample_count(self):
        artifact = build_artifact(
            records=[record("fast", [9, 10, 11], [18, 20, 22])],
            requested_samples=3,
            runtimes={"perry": runtime(), "node": runtime(), "bun": runtime(False)},
            commit="abc123",
            generated_at="2026-07-12T00:00:00Z",
        )
        artifact["benchmarks"]["fast"]["runtimes"]["perry"]["wall_ms"]["samples"].pop()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "current.json"
            path.write_text(json.dumps(artifact))
            with self.assertRaisesRegex(ArtifactError, "perry.*2/3"):
                load_artifact(path)


class RegressionGateTests(unittest.TestCase):
    def artifact(self, samples, node_samples=None):
        return build_artifact(
            records=[record("fast", samples, node_samples or [18, 18, 18, 18, 18])],
            requested_samples=5,
            runtimes={"perry": runtime(), "node": runtime(), "bun": runtime(False)},
            commit="current",
            generated_at="2026-07-12T00:00:00Z",
        )

    def test_fast_stable_regression_is_not_hidden_by_global_escape_hatch(self):
        baseline = {
            "commit": "base",
            "benchmarks": {"fast": {"perry_ms": 9, "node_ms": 18, "perry_rss_kb": 5000}},
        }
        report = evaluate_regressions(
            baseline,
            self.artifact([12, 12, 12, 12, 12]),
            speed_threshold_pct=20,
            memory_threshold_pct=30,
        )
        self.assertEqual([item.name for item in report.regressions], ["fast"])
        self.assertLess(report.rows[0].speed_noise_ms, 3)

    def test_fast_noisy_samples_are_calibrated_per_benchmark(self):
        baseline = {
            "commit": "base",
            "benchmarks": {"fast": {"perry_ms": 9, "node_ms": 18, "perry_rss_kb": 5000}},
        }
        report = evaluate_regressions(
            baseline,
            self.artifact([7, 8, 15, 21, 22]),
            speed_threshold_pct=20,
            memory_threshold_pct=30,
        )
        self.assertEqual(report.regressions, [])
        self.assertGreater(report.rows[0].speed_noise_ms, 6)

    def test_percentage_threshold_must_be_exceeded(self):
        baseline = {
            "commit": "base",
            "benchmarks": {"fast": {"perry_ms": 10, "node_ms": 18, "perry_rss_kb": 5000}},
        }
        report = evaluate_regressions(
            baseline,
            self.artifact([12, 12, 12, 12, 12]),
            speed_threshold_pct=20,
            memory_threshold_pct=30,
        )
        self.assertEqual(report.regressions, [])

    def test_peer_relative_trend_filters_runner_wide_drift(self):
        baseline = {
            "commit": "base",
            "benchmarks": {"fast": {"perry_ms": 9, "node_ms": 18, "perry_rss_kb": 5000}},
        }
        report = evaluate_regressions(
            baseline,
            self.artifact([12, 12, 12, 12, 12], [24, 24, 24, 24, 24]),
            speed_threshold_pct=20,
            memory_threshold_pct=30,
        )
        self.assertEqual(report.regressions, [])
        self.assertEqual(report.rows[0].node_ratio_delta_pct, 0)


class HttpSummaryTests(unittest.TestCase):
    def test_summarizes_throughput_and_latency_distributions(self):
        rows = []
        for language, base in (("perry", 1000), ("node", 800), ("bun", 900)):
            for run in range(1, 4):
                rows.append({
                    "workload": "http_fastify_minimal",
                    "language": language,
                    "run": run,
                    "exit_code": 0,
                    "rps": base + run,
                    "p50_ms": 1.0,
                    "p95_ms": 2.0,
                    "p99_ms": 3.0,
                    "success_rate": 1.0,
                })
        result = summarize_http(
            {"rows": rows},
            expected_samples=3,
            expected_workloads=("http_fastify_minimal",),
        )
        workload = result["workloads"]["http_fastify_minimal"]
        self.assertEqual(workload["runtimes"]["perry"]["rps"]["sample_count"], 3)
        self.assertAlmostEqual(workload["ratios"]["perry_to_node_rps"], 1002 / 802, places=6)
        self.assertEqual(workload["runtimes"]["bun"]["p99_ms"]["median"], 3.0)

    def test_rejects_incomplete_http_samples(self):
        rows = [{
            "workload": "http_fastify_minimal",
            "language": "perry",
            "run": 1,
            "exit_code": 0,
            "rps": 1000,
            "p50_ms": 1,
            "p95_ms": 2,
            "p99_ms": 3,
            "success_rate": 1,
        }]
        with self.assertRaisesRegex(ArtifactError, "perry.*1/3"):
            summarize_http(
                {"rows": rows},
                expected_samples=3,
                expected_runtimes=("perry",),
                expected_workloads=("http_fastify_minimal",),
            )

    def test_rejects_http_rows_with_missing_metrics(self):
        rows = []
        for run in range(1, 4):
            rows.append({
                "workload": "http_fastify_minimal",
                "language": "perry",
                "run": run,
                "exit_code": 0,
                "rps": 1000,
                "p50_ms": 1,
                "p95_ms": 2,
                "success_rate": 1,
            })
        with self.assertRaisesRegex(ArtifactError, "missing p99_ms"):
            summarize_http(
                {"rows": rows},
                expected_samples=3,
                expected_runtimes=("perry",),
                expected_workloads=("http_fastify_minimal",),
            )


if __name__ == "__main__":
    unittest.main()
