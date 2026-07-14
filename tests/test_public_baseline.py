import unittest

from benchmarks.benchmark_gate import ArtifactError, build_artifact
from benchmarks.public_baseline import (
    EXPECTED_SUITE_BENCHMARKS,
    README_END,
    README_START,
    _replace_block,
    _validate_suite,
    distribution,
    normalize_honest,
    readme_block,
    utc_z_timestamp,
)


def metric(values):
    return {"wall_ms": distribution(values), "rss_kb": distribution([100] * len(values))}


class PublicBaselineTests(unittest.TestCase):
    @staticmethod
    def honest_metadata():
        return {
            "commit": "abc",
            "generated_at": "2026-07-12T00:00:00Z",
            "harness": {"warmup": 1, "measured": 2},
            "commands": {runtime: [runtime] for runtime in ("perry", "node", "bun")},
            "toolchains": {runtime: "1.0" for runtime in ("perry", "node", "bun")},
            "executables": {runtime: f"/{runtime}" for runtime in ("perry", "node", "bun")},
        }

    def test_timestamp_normalization_uses_utc_z_suffix(self):
        self.assertEqual(
            utc_z_timestamp("2026-07-12T02:03:04.123456+00:00"),
            "2026-07-12T02:03:04.123456Z",
        )
        self.assertEqual(
            utc_z_timestamp("2026-07-12T04:03:04+02:00"),
            "2026-07-12T02:03:04Z",
        )

    def test_honest_component_requires_complete_correct_samples(self):
        metadata = self.honest_metadata()
        rows = []
        for workload in ("image_convolution", "json_pipeline_small", "json_pipeline_full"):
            for runtime in ("perry", "node", "bun"):
                for run in (1, 2):
                    rows.append({
                        "workload": workload,
                        "language": runtime,
                        "command": [f"/{runtime}", workload],
                        "run": run,
                        "wall_ms": 10 + run,
                        "max_rss_kb": 100,
                        "exit_code": 0,
                        "output_match": True,
                    })
        component = normalize_honest({"rows": rows}, metadata)
        self.assertEqual(component["run_config"]["requested_samples"], 2)
        self.assertEqual(
            component["benchmarks"]["json_pipeline_small"]["runtimes"]["bun"]["wall_ms"]["samples"],
            [11.0, 12.0],
        )

        rows.pop()
        with self.assertRaisesRegex(ArtifactError, "bun has 1/2"):
            normalize_honest({"rows": rows}, metadata)

    def test_honest_component_rejects_correctness_failure(self):
        metadata = self.honest_metadata()
        rows = []
        for workload in ("image_convolution", "json_pipeline_small", "json_pipeline_full"):
            for runtime in ("perry", "node", "bun"):
                for run in (1, 2):
                    rows.append({
                        "workload": workload,
                        "language": runtime,
                        "command": [f"/{runtime}", workload],
                        "run": run,
                        "wall_ms": 10,
                        "max_rss_kb": 100,
                        "exit_code": 0,
                        "output_match": not (
                            workload == "image_convolution" and runtime == "perry" and run == 2
                        ),
                    })
        with self.assertRaisesRegex(ArtifactError, "perry correctness failed"):
            normalize_honest({"rows": rows}, metadata)

    def test_generated_readme_reports_losses_and_wins(self):
        suite = {}
        keys = (
            "13_factorial", "09_method_calls", "14_closure", "12_binary_trees",
            "08_string_concat", "11_prime_sieve", "15_mandelbrot", "16_matrix_multiply",
        )
        for index, key in enumerate(keys):
            suite[key] = {
                "runtimes": {
                    "perry": metric([5, 5]),
                    "node": metric([10 if index else 2, 10 if index else 2]),
                    "bun": metric([9 if index else 3, 9 if index else 3]),
                }
            }
        json_entry = {
            "runtimes": {
                "perry": metric([20, 20]),
                "node": metric([30, 30]),
                "bun": metric([25, 25]),
            }
        }
        artifact = {
            "commit": "abcdef1234567890",
            "components": {
                "suite": {"benchmarks": suite},
                "json_polyglot": {"benchmarks": {"roundtrip": json_entry}},
            },
        }
        block = readme_block(artifact)
        self.assertIn("loss vs both", block)
        self.assertIn("win vs both", block)
        self.assertIn("`abcdef123456`", block)

    def test_generated_marker_replacement_is_deterministic(self):
        original = f"before\n{README_START}\nold\n{README_END}\nafter\n"
        block = f"{README_START}\nnew\n{README_END}"
        self.assertEqual(
            _replace_block(original, block),
            f"before\n{README_START}\nnew\n{README_END}\nafter\n",
        )

    def test_suite_validation_requires_every_workload_and_passing_correctness(self):
        records = []
        for name in EXPECTED_SUITE_BENCHMARKS:
            records.append({
                "name": name,
                "runtimes": {
                    "perry": {"wall_ms": [1, 1], "rss_kb": [100, 100]},
                    "node": {"wall_ms": [2, 2], "rss_kb": [200, 200]},
                    "bun": {"wall_ms": [2, 2], "rss_kb": [200, 200]},
                },
                "correctness": {"status": "pass", "reference": "node"},
            })
        runtimes = {
            runtime: {"available": True, "version": "1", "command": [runtime]}
            for runtime in ("perry", "node", "bun")
        }
        artifact = build_artifact(
            records=records,
            requested_samples=2,
            runtimes=runtimes,
            commit="abc",
            generated_at="2026-07-12T00:00:00Z",
        )
        _validate_suite(artifact)

        removed_name, removed = artifact["benchmarks"].popitem()
        with self.assertRaisesRegex(ArtifactError, "set mismatch"):
            _validate_suite(artifact)
        artifact["benchmarks"][removed_name] = removed
        removed["correctness"]["status"] = "fail"
        with self.assertRaisesRegex(ArtifactError, "correctness did not pass"):
            _validate_suite(artifact)


if __name__ == "__main__":
    unittest.main()
