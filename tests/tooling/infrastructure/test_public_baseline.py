import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from datetime import datetime, timezone
from unittest.mock import patch

from benchmarks import public_baseline as pb

from benchmarks.benchmark_gate import ArtifactError, build_artifact
from benchmarks.public_baseline import (
    EXPECTED_SUITE_BENCHMARKS,
    HARNESS_PATHS,
    README_END,
    README_START,
    ROOT,
    SOURCE_PATHS,
    _normalize_cargo_workspace_version,
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


class CargoFingerprintTests(unittest.TestCase):
    """Exercise Git discovery, byte normalization, hashing and the real validator."""

    MANIFEST = b'''[workspace]
members = ["crates/perry"]
[workspace.package]
version = "0.5.1635"
edition = "2021"
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
[workspace.dependencies.other]
version = "2.0"
[profile.release]
opt-level = 3
'''

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        self.manifest = root / "Cargo.toml"
        self.manifest.write_bytes(self.MANIFEST)
        for args in (("init", "-q"), ("add", "Cargo.toml")):
            subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)
        root_patch = patch.object(pb, "ROOT", root)
        root_patch.start()
        self.addCleanup(root_patch.stop)
        self.original = pb.tracked_fingerprint(SOURCE_PATHS)
        # In-memory validator fixture only; never rewrite published evidence.
        self.artifact = json.loads(pb.DEFAULT_ARTIFACT.read_text())
        self.artifact["generated_at"] = datetime.now(timezone.utc).isoformat()
        self.artifact["freshness"] = {
            "source_fingerprint": self.original,
            "harness_fingerprint": pb.tracked_fingerprint(HARNESS_PATHS),
        }
        pb.validate_public(self.artifact, 45)

    def test_version_only_bump_keeps_fingerprint_and_gate_green(self):
        changed = self.MANIFEST.replace(b'0.5.1635', b'0.5.1636')
        self.assertNotEqual(changed, self.MANIFEST)
        self.manifest.write_bytes(changed)
        self.assertEqual(pb.tracked_fingerprint(SOURCE_PATHS), self.original)
        pb.validate_public(self.artifact, 45)

    def test_real_manifest_changes_move_fingerprint_and_turn_gate_red(self):
        changes = (
            ("profile", b'opt-level = 3', b'opt-level = 2'),
            ("inline dependency", b'version = "1.0"', b'version = "1.1"'),
            ("dependency table", b'version = "2.0"', b'version = "2.1"'),
            ("feature flags", b'["derive"]', b'["derive", "rc"]'),
            ("workspace member", b'"crates/perry"', b'"crates/other"'),
            ("edition", b'edition = "2021"', b'edition = "2024"'),
        )
        for label, old, new in changes:
            with self.subTest(input=label):
                changed = self.MANIFEST.replace(old, new)
                self.assertNotEqual(changed, self.MANIFEST)
                self.manifest.write_bytes(changed)
                self.assertNotEqual(pb.tracked_fingerprint(SOURCE_PATHS), self.original)
                with self.assertRaisesRegex(ArtifactError, "benchmark inputs changed"):
                    pb.validate_public(self.artifact, 45)
                self.manifest.write_bytes(self.MANIFEST)
                self.assertEqual(pb.tracked_fingerprint(SOURCE_PATHS), self.original)
                pb.validate_public(self.artifact, 45)


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

    def test_only_workspace_package_version_value_is_normalized(self):
        for quote in (b'"', b"'"):
            with self.subTest(quote=quote):
                base = (
                    b'[workspace.package] # release metadata\n'
                    b'  version = ' + quote + b'0.5.1635' + quote + b' # release\n'
                    b'edition = "2021"\n'
                    b'[workspace.dependencies.serde] # dependency metadata\n'
                    b'version = "1.0"\n'
                )
                self.assertEqual(
                    _normalize_cargo_workspace_version(base),
                    base.replace(b'0.5.1635', b'0.0.0'),
                )

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
