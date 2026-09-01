import json
import tempfile
import unittest
from pathlib import Path

from benchmarks.benchmark_gate import ArtifactError, build_artifact
from benchmarks.public_baseline import (
    EXPECTED_SUITE_BENCHMARKS,
    HARNESS_PATHS,
    README_END,
    README_START,
    ROOT,
    SOURCE_PATHS,
    _CARGO_VERSION_RE,
    _cargo_profile_tables,
    _is_resolved_path,
    _normalize_checkout_newlines,
    _replace_block,
    _validate_component_measurement_config,
    _validate_suite,
    distribution,
    load_measurement_config,
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

    def test_cargo_version_bump_does_not_change_fingerprint_input(self):
        # A workspace version bump must not move the source fingerprint: the
        # volatile version line is normalized out before hashing. Regression
        # guard — before this, the freshness gate reddened on every PR that
        # followed a version bump (Cargo.toml is a fingerprinted source path).
        def normalize(data):
            normalized = _CARGO_VERSION_RE.sub(b'version = "0.0.0"', data)
            return _cargo_profile_tables(normalized)

        base = (
            b'[workspace.package]\nversion = "0.5.1258"\nedition = "2021"\n'
            b'[profile.release]\nopt-level = 3\n'
        )
        bumped = (
            b'[workspace.package]\nversion = "0.5.1300"\nedition = "2021"\n'
            b'[profile.release]\nopt-level = 3\n'
        )
        self.assertEqual(normalize(base), normalize(bumped))

        # Workspace/dependency plumbing is outside the extract, while a build
        # profile change must still move it.
        dependency_change = base.replace(
            b"[profile.release]", b'foo = "1"\n[profile.release]'
        )
        profile_change = base.replace(b"opt-level = 3", b"opt-level = 2")
        self.assertEqual(normalize(base), normalize(dependency_change))
        self.assertNotEqual(normalize(base), normalize(profile_change))

    def test_fingerprint_input_is_checkout_line_ending_independent(self):
        self.assertEqual(
            _normalize_checkout_newlines(b"alpha\r\nbeta\r\n"),
            _normalize_checkout_newlines(b"alpha\nbeta\n"),
        )

    def test_resolved_paths_are_host_independent(self):
        self.assertTrue(_is_resolved_path("target/release/perry"))
        self.assertTrue(_is_resolved_path(r"target\release\perry.exe"))
        self.assertFalse(_is_resolved_path("perry"))

    def test_measurement_config_is_the_fingerprinted_protocol(self):
        config = load_measurement_config()
        self.assertEqual(config["components"]["suite"]["measured_runs"], 5)
        self.assertEqual(config["components"]["honest_bench"]["workloads"], [1, 3])
        self.assertEqual(
            HARNESS_PATHS,
            (
                "benchmarks/public-baseline-config.json",
                "benchmarks/honest_bench/results/expected.json",
            ),
        )
        for plumbing in (
            "benchmarks/public_baseline.py",
            "benchmarks/run_public_baseline.sh",
            "benchmarks/json_polyglot/run.sh",
        ):
            self.assertNotIn(plumbing, HARNESS_PATHS)
        self.assertIn(
            "benchmarks/honest_bench/workloads/1_json_pipeline/perry/*.ts",
            SOURCE_PATHS,
        )
        self.assertIn("benchmarks/polyglot/bench.*", SOURCE_PATHS)

    def test_measurement_config_rejects_an_invalid_run_count(self):
        config = load_measurement_config()
        config["components"]["polyglot"]["measured_runs"] = 1
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            path.write_text(json.dumps(config), encoding="utf-8")
            with self.assertRaisesRegex(ArtifactError, "must be at least 2"):
                load_measurement_config(path)

    def test_component_metadata_must_match_measurement_config(self):
        config = load_measurement_config()
        components = {
            name: {
                "run_config": {"requested_samples": values["measured_runs"]}
            }
            for name, values in config["components"].items()
        }
        for name in ("app_patterns", "honest_bench"):
            components[name]["run_config"]["warmup"] = config["components"][name][
                "warmup_runs"
            ]
        _validate_component_measurement_config(components, config)

        components["app_patterns"]["run_config"]["requested_samples"] -= 1
        with self.assertRaisesRegex(ArtifactError, "does not match"):
            _validate_component_measurement_config(components, config)

    def test_measurement_drivers_consume_the_configured_parameters(self):
        orchestrator = (ROOT / "benchmarks/run_public_baseline.sh").read_text()
        for variable in (
            "SUITE_RUNS",
            "POLYGLOT_RUNS",
            "JSON_POLYGLOT_RUNS",
            "APP_WARMUP",
            "APP_RUNS",
            "HONEST_WORKLOADS",
            "HONEST_WARMUP",
            "HONEST_RUNS",
        ):
            self.assertIn(f'"${variable}"', orchestrator)

        app_runner = (ROOT / "benchmarks/app-patterns/run.sh").read_text()
        self.assertIn('hyperfine --warmup "$WARMUP" --runs "$RUNS"', app_runner)
        self.assertIn('"requested_samples": requested', app_runner)

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
