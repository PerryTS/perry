"""Tests for the GC ratchet gate.

The point of most of these is not that the checker *passes* on good input — a
checker that always passes would satisfy that. The point is that it *fails* on
each kind of bad input, one test per failure mode, including one parametric test
that walks every gating metric in every profile and proves each can independently
turn the job red. ``gc-stress`` was ``continue-on-error: true`` for long enough
that a regression sat behind it through three merges; a gate nobody has watched
fail is indistinguishable from that.
"""

from __future__ import annotations

import copy
import json
import unittest
from pathlib import Path

from benchmarks.gc_ratchet.gc_ratchet import (
    ALL_METRICS,
    DEFAULT_ARTIFACT,
    GC_METRICS,
    PROFILES,
    RatchetError,
    distribution,
    evaluate,
    parse_gc_diag,
    parse_gcmetrics,
    tolerances_from_json,
    validate_artifact,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
TOLERANCES_PATH = REPO_ROOT / "benchmarks" / "gc_ratchet" / "tolerances.json"

BASE_VALUES = {
    "heap_used_bytes": 1_000_000.0,
    "heap_total_bytes": 20_971_520.0,
    "minor_cycles": 80.0,
    "step_cycles": 80.0,
    "copied_objects": 20_000.0,
    "copied_bytes": 1_500_000.0,
    "promoted_objects": 4_000.0,
    "promoted_bytes": 500_000.0,
    "freed_bytes": 100_000_000.0,
    "rss_bytes": 30_000_000.0,
    "peak_rss_bytes": 31_000_000.0,
    "wall_ms": 200.0,
}


def _metrics(overrides=None):
    values = dict(BASE_VALUES)
    values.update(overrides or {})
    return {metric: distribution([values[metric]] * 7) for metric in ALL_METRICS}


def _probe(name="01_probe", overrides=None, correctness="pass"):
    return {
        name: {
            "stdout": f"probe:{name}\nchecksum:1\n",
            "correctness": {"status": correctness, "oracle_version": "v26.5.0", "reason": ""},
            "metrics": _metrics(overrides),
        }
    }


def _baseline(probes=None):
    probes = probes if probes is not None else _probe()
    return {
        "schema_version": 1,
        "kind": "gc-ratchet-baseline",
        "artifact_id": "gc-ratchet-v1",
        "commit": "deadbeef",
        "generated_at": "2026-07-30T00:00:00+00:00",
        "platform": "darwin-arm64",
        "host": {"cpu_count": 8, "load_average": {"1m": 1.0}},
        "run_config": {"repeats": 7, "warmup": 1, "traced_runs": 2, "probes": sorted(probes)},
        "tolerances": json.loads(TOLERANCES_PATH.read_text(encoding="utf-8")),
        "probes": probes,
    }


def _measurement(probes=None, platform="darwin-arm64", repeats=7):
    probes = probes if probes is not None else _probe()
    return {
        "schema_version": 1,
        "kind": "gc-ratchet-measurement",
        "generated_at": "2026-07-30T01:00:00+00:00",
        "platform": platform,
        "host": {"cpu_count": 8, "load_average": {"1m": 1.0}},
        "run_config": {"repeats": repeats, "warmup": 1, "traced_runs": 2, "probes": sorted(probes)},
        "probes": probes,
    }


def _hard(failures):
    return [failure for failure in failures if not failure.startswith("NOTE")]


class ParsingTests(unittest.TestCase):
    def test_parses_gcmetric_lines_and_ignores_noise(self):
        stderr = (
            "some unrelated warning\n"
            "#gcmetric heap_used_bytes=1234\n"
            "#gcmetric heap_total_bytes=20971520\n"
            "#gcmetricmalformed=1\n"
        )
        self.assertEqual(
            parse_gcmetrics(stderr),
            {"heap_used_bytes": 1234, "heap_total_bytes": 20971520},
        )

    def test_parses_and_aggregates_copy_minor_accounting(self):
        stderr = (
            "[gc-copy-minor] eligible=true fallback=none\n"
            "[gc-copy-minor] ran copied_objects=10 copied_bytes=100 "
            "promoted_objects=1 promoted_bytes=8 freed_bytes=900\n"
            "[gc-step] pre_in_use=1 post_in_use=2 sweep_freed=3\n"
            "[gc-copy-minor] ran copied_objects=5 copied_bytes=50 "
            "promoted_objects=0 promoted_bytes=0 freed_bytes=100\n"
        )
        parsed = parse_gc_diag(stderr)
        self.assertEqual(parsed["minor_cycles"], 2)
        self.assertEqual(parsed["step_cycles"], 1)
        self.assertEqual(parsed["copied_objects"], 15)
        self.assertEqual(parsed["copied_bytes"], 150)
        self.assertEqual(parsed["freed_bytes"], 1000)

    def test_eligible_lines_do_not_count_as_cycles(self):
        # "[gc-copy-minor] eligible=..." is a decision trace, not a collection.
        # Counting it would inflate minor_cycles and make the metric depend on
        # how often the policy was *consulted* rather than how often it ran.
        parsed = parse_gc_diag("[gc-copy-minor] eligible=true fallback=none\n")
        self.assertEqual(parsed["minor_cycles"], 0)


class ToleranceTests(unittest.TestCase):
    def test_shipped_tolerances_parse_and_cover_every_metric(self):
        profiles = tolerances_from_json(json.loads(TOLERANCES_PATH.read_text(encoding="utf-8")))
        for profile in PROFILES:
            self.assertEqual(set(profiles[profile]), set(ALL_METRICS))

    def test_every_tolerance_states_a_rationale(self):
        profiles = tolerances_from_json(json.loads(TOLERANCES_PATH.read_text(encoding="utf-8")))
        for profile, entries in profiles.items():
            for metric, tolerance in entries.items():
                self.assertTrue(
                    tolerance.rationale.strip(),
                    f"{profile}.{metric} has no rationale",
                )

    def test_profile_with_nothing_gating_is_rejected(self):
        payload = json.loads(TOLERANCES_PATH.read_text(encoding="utf-8"))
        for entry in payload["shared_ci"].values():
            entry["gating"] = False
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(payload)
        self.assertIn("could never fail", str(caught.exception))

    def test_rationale_may_not_be_blank(self):
        payload = json.loads(TOLERANCES_PATH.read_text(encoding="utf-8"))
        payload["shared_ci"]["heap_used_bytes"]["rationale"] = "   "
        with self.assertRaises(RatchetError):
            tolerances_from_json(payload)

    def test_gc_counters_are_two_sided(self):
        profiles = tolerances_from_json(json.loads(TOLERANCES_PATH.read_text(encoding="utf-8")))
        for profile in PROFILES:
            for metric in GC_METRICS:
                self.assertEqual(
                    profiles[profile][metric].direction,
                    "either",
                    f"{profile}.{metric} must fail on drift in either direction",
                )


class GatePassTests(unittest.TestCase):
    def test_identical_measurement_passes(self):
        baseline = _baseline()
        rows, failures = evaluate(baseline, _measurement(), profile="shared_ci")
        self.assertEqual(_hard(failures), [])
        self.assertTrue(rows)
        self.assertTrue(all(row.status == "ok" for row in rows))

    def test_change_inside_the_band_passes(self):
        # heap_used_bytes allowance is max(2% of 1,000,000, 65,536) = 65,536.
        baseline = _baseline()
        current = _measurement(_probe(overrides={"heap_used_bytes": 1_065_536.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertEqual(_hard(failures), [])

    def test_noisy_wall_time_does_not_fail_shared_ci(self):
        baseline = _baseline()
        current = _measurement(_probe(overrides={"wall_ms": 1_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertEqual(_hard(failures), [])

    def test_memory_growth_does_not_fail_shared_ci(self):
        baseline = _baseline()
        current = _measurement(_probe(overrides={"peak_rss_bytes": 300_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertEqual(_hard(failures), [])


class GateFailTests(unittest.TestCase):
    def test_retention_regression_fails(self):
        baseline = _baseline()
        current = _measurement(_probe(overrides={"heap_used_bytes": 2_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("heap_used_bytes" in failure for failure in _hard(failures)))

    def test_one_falsely_retained_nursery_block_fails(self):
        # The smallest regression the campaign could plausibly introduce: a
        # single 1 MiB nursery block kept alive by a stale stack word.
        baseline = _baseline()
        current = _measurement(_probe(overrides={"heap_used_bytes": 1_000_000.0 + 1_048_576}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("heap_used_bytes" in failure for failure in _hard(failures)))

    def test_evacuation_collapse_fails_even_though_it_is_a_decrease(self):
        # Pinning objects instead of evacuating them makes copied_objects fall.
        # A one-sided "growth is bad" gate would wave this through.
        baseline = _baseline()
        current = _measurement(_probe(overrides={"copied_objects": 10.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("copied_objects" in failure for failure in _hard(failures)))

    def test_reclaiming_less_fails(self):
        baseline = _baseline()
        current = _measurement(_probe(overrides={"freed_bytes": 50_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("freed_bytes" in failure for failure in _hard(failures)))

    def test_missing_probe_fails_instead_of_being_skipped(self):
        baseline = _baseline()
        current = _measurement({})
        current["run_config"]["probes"] = []
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("missing" in failure for failure in _hard(failures)))

    def test_extra_probe_fails(self):
        baseline = _baseline()
        probes = dict(_probe())
        probes.update(_probe("02_new"))
        _, failures = evaluate(baseline, _measurement(probes), profile="shared_ci")
        self.assertTrue(any("02_new" in failure for failure in _hard(failures)))

    def test_node_oracle_failure_fails(self):
        baseline = _baseline()
        current = _measurement(_probe(correctness="fail"))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("Node oracle" in failure for failure in _hard(failures)))

    def test_changed_observable_output_fails(self):
        baseline = _baseline()
        probes = _probe()
        probes["01_probe"]["stdout"] = "probe:01_probe\nchecksum:999\n"
        _, failures = evaluate(baseline, _measurement(probes), profile="shared_ci")
        self.assertTrue(any("observable output" in failure for failure in _hard(failures)))

    def test_fewer_repeats_fails(self):
        baseline = _baseline()
        _, failures = evaluate(baseline, _measurement(repeats=3), profile="shared_ci")
        self.assertTrue(any("repeats" in failure for failure in _hard(failures)))

    def test_platform_mismatch_fails_by_default(self):
        baseline = _baseline()
        _, failures = evaluate(baseline, _measurement(platform="linux-x86_64"), profile="shared_ci")
        self.assertTrue(any("platform mismatch" in failure for failure in _hard(failures)))

    def test_platform_mismatch_can_be_downgraded_explicitly(self):
        baseline = _baseline()
        _, failures = evaluate(
            baseline,
            _measurement(platform="linux-x86_64"),
            profile="shared_ci",
            allow_platform_mismatch=True,
        )
        self.assertEqual(_hard(failures), [])
        self.assertTrue(any(failure.startswith("NOTE") for failure in failures))

    def test_pinned_host_profile_gates_memory_and_time(self):
        baseline = _baseline()
        current = _measurement(
            _probe(overrides={"peak_rss_bytes": 40_000_000.0, "wall_ms": 400.0})
        )
        _, failures = evaluate(baseline, current, profile="pinned_host")
        joined = " ".join(_hard(failures))
        self.assertIn("peak_rss_bytes", joined)
        self.assertIn("wall_ms", joined)

    def test_every_gating_metric_can_independently_fail(self):
        """Walk every gating metric in every profile and prove it turns red.

        Without this, a band could quietly be set wide enough that the metric is
        gating in name only, and nothing would ever notice.
        """
        payload = json.loads(TOLERANCES_PATH.read_text(encoding="utf-8"))
        profiles = tolerances_from_json(payload)
        for profile in PROFILES:
            for metric, tolerance in profiles[profile].items():
                if not tolerance.gating:
                    continue
                with self.subTest(profile=profile, metric=metric):
                    base = BASE_VALUES[metric]
                    breach = base + tolerance.allowance(base) * 1.5 + 1
                    current = _measurement(_probe(overrides={metric: breach}))
                    _, failures = evaluate(_baseline(), current, profile=profile)
                    self.assertTrue(
                        any(metric in failure for failure in _hard(failures)),
                        f"{profile}.{metric} is marked gating but did not fail on a breach",
                    )


class ArtifactValidationTests(unittest.TestCase):
    def test_pinned_artifact_is_valid(self):
        self.assertTrue(
            DEFAULT_ARTIFACT.exists(),
            f"pinned baseline missing at {DEFAULT_ARTIFACT}",
        )
        validate_artifact(json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8")))

    def test_pinned_artifact_records_provenance(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        self.assertRegex(artifact["commit"], r"^[0-9a-f]{40}$")
        self.assertIn("load_average", artifact["host"])
        self.assertIsNotNone(artifact["toolchain"]["rustc"])
        binaries = artifact["toolchain"]["binaries"]
        for key in ("perry", "libperry_runtime.a", "libperry_stdlib.a"):
            self.assertRegex(binaries[key]["sha256"], r"^[0-9a-f]{64}$")

    def test_pinned_artifact_probes_all_ran_a_collection(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        for name, entry in artifact["probes"].items():
            self.assertGreaterEqual(
                entry["metrics"]["minor_cycles"]["median"],
                1,
                f"{name} pinned without exercising the collector",
            )

    def test_pinned_artifact_probes_match_the_node_oracle(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        for name, entry in artifact["probes"].items():
            self.assertEqual(
                entry["correctness"]["status"],
                "pass",
                f"{name} was pinned without a passing Node oracle diff",
            )

    def test_pinned_artifact_retention_is_deterministic(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        for name, entry in artifact["probes"].items():
            for metric in ("heap_used_bytes", "heap_total_bytes"):
                self.assertEqual(
                    entry["metrics"][metric]["spread"],
                    0,
                    f"{name}.{metric} was not deterministic when pinned; "
                    "it must not be in a gating family",
                )

    def test_tampered_summary_is_rejected(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        name = sorted(artifact["probes"])[0]
        tampered = copy.deepcopy(artifact)
        tampered["probes"][name]["metrics"]["heap_used_bytes"]["median"] = 1
        with self.assertRaises(RatchetError):
            validate_artifact(tampered)

    def test_probe_without_a_collection_cannot_be_pinned(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        name = sorted(artifact["probes"])[0]
        tampered = copy.deepcopy(artifact)
        tampered["probes"][name]["metrics"]["minor_cycles"] = distribution([0, 0])
        with self.assertRaises(RatchetError):
            validate_artifact(tampered)


if __name__ == "__main__":
    unittest.main()
