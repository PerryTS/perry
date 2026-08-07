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
import stat
import sys
import tempfile
import unittest
from pathlib import Path

from benchmarks.gc_ratchet.gc_ratchet import (
    ALL_METRICS,
    DEFAULT_ARTIFACT,
    DETERMINISTIC_METRICS,
    GC_METRICS,
    MIN_EXCLUSION_RUNS,
    PROFILES,
    SCAN_MODE_ENV,
    RatchetError,
    classify,
    distribution,
    evaluate,
    gated_anywhere,
    parse_gc_diag,
    parse_gcmetrics,
    parse_scan_fallbacks,
    probe_overrides_from_json,
    render,
    render_classification,
    tolerances_from_json,
    validate_artifact,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
TOLERANCES_PATH = REPO_ROOT / "benchmarks" / "gc_ratchet" / "tolerances.json"


def _shipped_tolerances():
    return json.loads(TOLERANCES_PATH.read_text(encoding="utf-8"))


def _tolerances():
    """The shipped bands without the shipped probe overrides.

    An override that names a probe the artifact does not contain is a hard
    failure by design, so the synthetic single-probe fixtures below cannot carry
    the real ``12_large_live_set`` entry. The override machinery is exercised
    explicitly in ``ProbeOverrideTests`` instead, against fixtures whose probe
    names match.
    """
    payload = _shipped_tolerances()
    payload.pop("probe_overrides", None)
    return payload


def _override_entry():
    return {
        "gating": False,
        "rationale": "measured non-deterministic on this workload; see the issue",
        "evidence": {
            "observed_runs": 21,
            "observed_spread": 4536,
            "measured_on": "2026-08-06, pinned quiet host",
            "issue": "https://github.com/PerryTS/perry/issues/7554",
        },
    }


def _with_override(probe="01_probe", metric="heap_used_bytes", entry=None):
    """Shipped bands plus one probe override, ready to hand to a fixture."""
    payload = _tolerances()
    payload["probe_overrides"] = {
        probe: {metric: entry if entry is not None else _override_entry()}
    }
    return payload


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


def _baseline(probes=None, tolerances=None):
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
        "tolerances": tolerances if tolerances is not None else _tolerances(),
        "probes": probes,
    }


def _pair(overrides=None, other_overrides=None):
    """Two probes.

    An override that covers every probe of a metric is refused (it would be a
    profile-level ``"gating": false`` assembled out of parts), so any fixture
    exercising an override needs at least one probe the override does not touch.
    """
    return _probe("01_probe", overrides) | _probe("02_other", other_overrides)


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
        profiles = tolerances_from_json(_shipped_tolerances())
        for profile in PROFILES:
            self.assertEqual(set(profiles[profile]), set(ALL_METRICS))

    def test_every_tolerance_states_a_rationale(self):
        profiles = tolerances_from_json(_shipped_tolerances())
        for profile, entries in profiles.items():
            for metric, tolerance in entries.items():
                self.assertTrue(
                    tolerance.rationale.strip(),
                    f"{profile}.{metric} has no rationale",
                )

    def test_profile_with_nothing_gating_is_rejected(self):
        payload = _shipped_tolerances()
        for entry in payload["shared_ci"].values():
            entry["gating"] = False
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(payload)
        self.assertIn("could never fail", str(caught.exception))

    def test_rationale_may_not_be_blank(self):
        payload = _shipped_tolerances()
        payload["shared_ci"]["heap_used_bytes"]["rationale"] = "   "
        with self.assertRaises(RatchetError):
            tolerances_from_json(payload)

    def test_gc_counters_are_two_sided(self):
        profiles = tolerances_from_json(_shipped_tolerances())
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

    def test_a_probe_that_stopped_collecting_fails(self):
        """The bands alone could not catch this, which is why it is asserted.

        Six of the twelve shipped probes pin ``minor_cycles`` at 1 and the
        allowance floor is also 1, so a collapse from 1 to 0 lands exactly on
        ``delta == -allowance`` and scored "ok". A collector that stopped
        running copying minors — the largest regression this ratchet exists to
        catch — was reported as passing.
        """
        baseline = _baseline(_probe(overrides={"minor_cycles": 1.0}))
        current = _measurement(_probe(overrides={"minor_cycles": 0.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(
            any("ran no minor collection" in failure for failure in _hard(failures)),
            "a probe that ran no collection at all was not reported",
        )

    def test_a_probe_that_evacuated_nothing_fails(self):
        baseline = _baseline()
        current = _measurement(_probe(overrides={"copied_objects": 0.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("evacuated nothing" in failure for failure in _hard(failures)))

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

    def test_unverified_correctness_fails(self):
        # A run that could not reach the oracle has not shown the probe still
        # computes anything. Passing it would make "we did not check"
        # indistinguishable from "we checked and it was fine".
        baseline = _baseline()
        current = _measurement(_probe(correctness="unchecked"))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("not verified" in failure for failure in _hard(failures)))

    def test_baseline_cannot_be_pinned_without_an_oracle_diff(self):
        artifact = _baseline(_probe(correctness="unchecked"))
        with self.assertRaises(RatchetError):
            validate_artifact(artifact)

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
        payload = _shipped_tolerances()
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
        """Every *gating* retention cell in the pinned artifact must be bit-identical.

        The band on these metrics is justified in tolerances.json as pure
        anti-brittleness margin over an observed spread of 0.000%, not as a noise
        allowance, so a gating cell whose own samples disagree contradicts the
        reason its band is that tight.

        The exemption is narrow on purpose: a cell is skipped only when a
        ``probe_overrides`` entry has already taken it out of the gating family
        under *every* profile, which the override schema forces to carry checked
        evidence with it. Anything else still fails, and
        ``ProbeOverrideTests.test_a_nondeterministic_gating_cell_cannot_be_pinned``
        proves this rule can still refuse an artifact.
        """
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        profiles = tolerances_from_json(artifact["tolerances"])
        overrides = probe_overrides_from_json(artifact["tolerances"])
        checked = 0
        for name, entry in artifact["probes"].items():
            for metric in ("heap_used_bytes", "heap_total_bytes"):
                if not gated_anywhere(profiles, overrides, name, metric):
                    continue
                checked += 1
                self.assertEqual(
                    entry["metrics"][metric]["spread"],
                    0,
                    f"{name}.{metric} was not deterministic when pinned; "
                    "it must not be in a gating family",
                )
        self.assertGreater(checked, 0, "no gating retention cell was checked at all")

    def test_shipped_overrides_name_probes_that_exist(self):
        # An exclusion that matches nothing must be deleted, not left behind to
        # outlive its reason.
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        for probe in probe_overrides_from_json(_shipped_tolerances()):
            self.assertIn(probe, artifact["probes"])

    def test_artifact_embeds_the_shipped_tolerances(self):
        """The gate reads the artifact's copy, so a drifted tolerances.json is a lie.

        ``evaluate`` takes its bands from ``baseline["tolerances"]``, not from
        the file. Editing the file without re-pinning would leave the gate
        running the old bands while the file claims otherwise — a gate measuring
        something other than what its configuration says.
        """
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        self.assertEqual(
            artifact["tolerances"],
            _shipped_tolerances(),
            "benchmarks/gc_ratchet/tolerances.json and the copy embedded in the pinned "
            "artifact disagree; re-pin, or sync the artifact deliberately",
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


class ProbeOverrideTests(unittest.TestCase):
    """Per-probe exclusions: they must work, and they must not become a back door.

    The mechanism exists because ``tolerances.json`` was keyed per metric per
    profile, so the only way to stop gating one non-deterministic cell was to
    stop gating that metric on all twelve probes (#7554). The risk it introduces
    is obvious — an exclusion is a hole in a gate — so most of these tests are
    about the ways an exclusion is refused.
    """

    def test_an_overridden_cell_cannot_fail_the_job(self):
        baseline = _baseline(_pair(), _with_override())
        current = _measurement(_pair({"heap_used_bytes": 9_000_000.0}))
        rows, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertEqual(_hard(failures), [])
        row = next(r for r in rows if r.probe == "01_probe" and r.metric == "heap_used_bytes")
        self.assertFalse(row.gating)
        # Excluded, not dropped: the breach is still measured and still shown.
        self.assertEqual(row.status, "drift (informational)")

    def test_an_overridden_cell_is_still_reported_with_its_reason(self):
        baseline = _baseline(_pair(), _with_override())
        rows, _ = evaluate(baseline, _measurement(_pair()), profile="shared_ci")
        report = render(rows, baseline, "shared_ci")
        self.assertIn("excluded from the gating family", report)
        self.assertIn("01_probe`.heap_used_bytes", report)
        self.assertIn("measured non-deterministic on this workload", report)
        self.assertIn("21 runs", report)

    def test_an_override_does_not_leak_to_other_probes(self):
        # The whole point: the other probes keep gating the same metric.
        baseline = _baseline(_pair(), _with_override())
        current = _measurement(_pair(other_overrides={"heap_used_bytes": 9_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        joined = " ".join(_hard(failures))
        self.assertIn("02_other", joined)
        self.assertIn("heap_used_bytes", joined)

    def test_an_override_does_not_leak_to_other_metrics(self):
        baseline = _baseline(_pair(), _with_override())
        current = _measurement(_pair({"heap_total_bytes": 90_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("heap_total_bytes" in failure for failure in _hard(failures)))

    def test_an_override_may_not_re_gate(self):
        entry = _override_entry()
        entry["gating"] = True
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(_with_override(entry=entry))
        self.assertIn("only set gating to false", str(caught.exception))

    def test_an_override_needs_a_rationale(self):
        entry = _override_entry()
        entry["rationale"] = "  "
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(_with_override(entry=entry))
        self.assertIn("silent exclusion", str(caught.exception))

    def test_an_override_needs_evidence(self):
        entry = _override_entry()
        del entry["evidence"]
        with self.assertRaises(RatchetError):
            tolerances_from_json(_with_override(entry=entry))

    def test_an_override_needs_enough_runs_behind_it(self):
        entry = _override_entry()
        entry["evidence"]["observed_runs"] = MIN_EXCLUSION_RUNS - 1
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(_with_override(entry=entry))
        self.assertIn("cannot distinguish", str(caught.exception))

    def test_an_override_needs_a_non_zero_observed_spread(self):
        # Excluding a metric that was measured as deterministic is unjustified:
        # nothing has been shown to be ungateable.
        entry = _override_entry()
        entry["evidence"]["observed_spread"] = 0
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(_with_override(entry=entry))
        self.assertIn("must stay gated", str(caught.exception))

    def test_an_override_for_an_unknown_metric_is_rejected(self):
        with self.assertRaises(RatchetError):
            tolerances_from_json(_with_override(metric="heap_used_byte"))

    def test_an_override_that_matches_no_probe_is_rejected(self):
        artifact = _baseline(tolerances=_with_override(probe="99_does_not_exist"))
        with self.assertRaises(RatchetError) as caught:
            validate_artifact(artifact)
        self.assertIn("must be deleted", str(caught.exception))

    def test_overriding_every_probe_is_rejected(self):
        # One cell at a time, this would achieve exactly what "gating": false at
        # profile level does, without saying so where a reader would look.
        payload = _tolerances()
        payload["probe_overrides"] = {
            probe: {"heap_used_bytes": _override_entry()} for probe in ("01_probe", "02_other")
        }
        with self.assertRaises(RatchetError) as caught:
            validate_artifact(_baseline(_pair(), payload))
        self.assertIn("can never fail", str(caught.exception))

    def test_a_mistyped_section_is_rejected_rather_than_ignored(self):
        payload = _tolerances()
        payload["probe_override"] = {"01_probe": {"heap_used_bytes": _override_entry()}}
        with self.assertRaises(RatchetError) as caught:
            tolerances_from_json(payload)
        self.assertIn("unknown top-level section", str(caught.exception))

    def test_a_nondeterministic_gating_cell_cannot_be_pinned(self):
        """The assertion that caught #7554, now enforced at pinning time.

        It used to live only in the unit tests, so an artifact carrying a
        non-deterministic gating cell could be written and committed; the test
        then failed in the CI step that runs *before* the measurement step, and
        the ratchet measured nothing for two days.
        """
        for metric in DETERMINISTIC_METRICS:
            with self.subTest(metric=metric):
                probes = _probe()
                probes["01_probe"]["metrics"][metric] = distribution(
                    [BASE_VALUES[metric]] * 6 + [BASE_VALUES[metric] + 6768]
                )
                with self.assertRaises(RatchetError) as caught:
                    validate_artifact(_baseline(probes))
                self.assertIn("bit-identity", str(caught.exception))

    def test_the_same_cell_may_be_pinned_once_it_is_excluded(self):
        probes = _pair()
        probes["01_probe"]["metrics"]["heap_used_bytes"] = distribution(
            [BASE_VALUES["heap_used_bytes"]] * 6 + [BASE_VALUES["heap_used_bytes"] + 6768]
        )
        validate_artifact(_baseline(probes, _with_override()))

    def test_memory_and_timing_spread_is_still_allowed(self):
        # Only the bit-identical families are held to spread 0; RSS and wall
        # time have declared noise floors and must not be caught by this rule.
        probes = _probe()
        for metric in ("rss_bytes", "peak_rss_bytes", "wall_ms"):
            probes["01_probe"]["metrics"][metric] = distribution(
                [BASE_VALUES[metric]] * 6 + [BASE_VALUES[metric] * 1.01]
            )
        validate_artifact(_baseline(probes))


# ---------------------------------------------------------------------------
# classify (#7559)
# ---------------------------------------------------------------------------

#: A stand-in for `perry`. `compile_probe` invokes it as
#: `<perry> <source-name> -o <binary>`; it writes a Python script that plays a
#: probe: fixed stdout, `#gcmetric` lines on stderr, and a `heap_used_bytes`
#: that depends on the conservative-scan knob exactly the way a real probe's
#: does. That is what makes `classify` testable without a compiler.
_STUB_PERRY = """#!{python}
import os, stat, sys
source = sys.argv[1]
out = sys.argv[sys.argv.index("-o") + 1]
body = open(os.path.join(os.path.dirname(os.path.abspath(source)) or ".", source)).read()
with open(out, "w") as handle:
    handle.write("#!{python}\\n" + body)
os.chmod(out, os.stat(out).st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
"""

#: The stub probe. `precise` is the retention the collector's roots account
#: for; the conservative scan adds one whole 1 MiB block on top, which is the
#: #7559 shape.
_STUB_PROBE = """
import os, sys
precise = {precise}
excess = 0 if os.environ.get("PERRY_CONSERVATIVE_STACK_SCAN") == "off" else {excess}
sys.stdout.write("probe:stub\\nchecksum:{checksum}\\n")
sys.stderr.write("#gcmetric heap_used_bytes=%d\\n" % (precise + excess))
sys.stderr.write("#gcmetric heap_total_bytes=20971520\\n")
sys.stderr.write("#gcmetric rss_bytes=30000000\\n")
if os.environ.get("PERRY_GC_DIAG"):
    sys.stderr.write("[gc-scan-fallback] site=manual_collect automatic=false count=1\\n")
"""


class ScanFallbackParsingTests(unittest.TestCase):
    def test_parses_sites_and_keeps_the_highest_running_count(self):
        stderr = "\n".join(
            [
                "[gc-copy-minor] eligible=true fallback=none",
                "[gc-scan-fallback] site=manual_collect automatic=false count=1",
                "[gc-scan-fallback] site=manual_collect automatic=false count=2",
                "[gc-scan-fallback] site=old_reclaim_alloc_point automatic=true count=1",
                "not a diag line",
            ]
        )
        self.assertEqual(
            parse_scan_fallbacks(stderr),
            {
                "manual_collect": {"automatic": False, "count": 2},
                "old_reclaim_alloc_point": {"automatic": True, "count": 1},
            },
        )

    def test_a_run_with_no_conservative_scan_reports_no_sites(self):
        self.assertEqual(parse_scan_fallbacks("[gc-step] pre_in_use=1 post_in_use=1"), {})


class ClassifyTests(unittest.TestCase):
    """`classify` splits a retention reading into real retention and residue.

    The gate compares `heap_used_bytes`, which is read after the probe's own
    `gc()` — the one site in Perry that *forces* the conservative native-stack
    scan. #7559 was a +16.44% breach on `05_closure_capture` whose precise
    retention was byte-identical (5,329,880) at both endpoints: one extra stale
    stack word, amplified to a whole 1 MiB block because `heap_used_bytes` sums
    arena block offsets. These tests pin the tool that makes that difference a
    one-command answer instead of two compiler builds.
    """

    def _fixture(self, tmp, *, precise=5_329_880, excess=1_048_576, checksum=1, probes=("05_stub",)):
        root = Path(tmp)
        perry = root / "stub-perry"
        perry.write_text(_STUB_PERRY.format(python=sys.executable), encoding="utf-8")
        perry.chmod(perry.stat().st_mode | stat.S_IEXEC)
        probes_dir = root / "probes"
        probes_dir.mkdir()
        for name in probes:
            (probes_dir / f"{name}.ts").write_text(
                _STUB_PROBE.format(precise=precise, excess=excess, checksum=checksum),
                encoding="utf-8",
            )
        return perry, probes_dir

    def test_reports_the_false_root_excess_and_the_scan_site(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp)
            payload = classify(perry=perry, probes_dir=probes_dir, repeats=3, warmup=0)
        (row,) = payload["probes"]
        self.assertEqual(row["heap_used_bytes"], 5_329_880 + 1_048_576)
        self.assertEqual(row["heap_used_precise_bytes"], 5_329_880)
        self.assertEqual(row["false_root_excess_bytes"], 1_048_576)
        self.assertEqual(row["scan_fallback_sites"]["manual_collect"]["automatic"], False)
        self.assertEqual(row["automatic_scan_sites"], [])

    def test_a_probe_with_no_residue_reports_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp, excess=0)
            payload = classify(perry=perry, probes_dir=probes_dir, repeats=3, warmup=0)
        self.assertEqual(payload["probes"][0]["false_root_excess_bytes"], 0)
        self.assertEqual(payload["probes"][0]["false_root_excess_pct"], 0)

    def test_a_non_deterministic_precise_reading_is_an_error(self):
        # The precise number is the one this tool asks the reader to believe.
        # Reporting it as a spread would make "the collector retained the same
        # bytes" a claim nobody checked.
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp)
            (probes_dir / "05_stub.ts").write_text(
                "import os, random, sys\n"
                'sys.stdout.write("probe:stub\\nchecksum:1\\n")\n'
                'sys.stderr.write("#gcmetric heap_used_bytes=%d\\n" % (5000000 + random.randrange(1, 99)))\n'
                'sys.stderr.write("#gcmetric heap_total_bytes=20971520\\n")\n'
                'sys.stderr.write("#gcmetric rss_bytes=30000000\\n")\n',
                encoding="utf-8",
            )
            with self.assertRaises(RatchetError) as caught:
                classify(perry=perry, probes_dir=probes_dir, repeats=5, warmup=0)
        self.assertIn("not bit-identical", str(caught.exception))

    def test_a_probe_whose_output_depends_on_the_scan_is_an_error(self):
        # If disabling the scan changes what the probe computes, the scan was
        # load-bearing for its correctness and its precise retention is not
        # evidence about the collector. That must not be quietly tabulated.
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp)
            (probes_dir / "05_stub.ts").write_text(
                "import os, sys\n"
                'off = os.environ.get("PERRY_CONSERVATIVE_STACK_SCAN") == "off"\n'
                'sys.stdout.write("probe:stub\\nchecksum:%d\\n" % (0 if off else 1))\n'
                'sys.stderr.write("#gcmetric heap_used_bytes=5000000\\n")\n'
                'sys.stderr.write("#gcmetric heap_total_bytes=20971520\\n")\n'
                'sys.stderr.write("#gcmetric rss_bytes=30000000\\n")\n',
                encoding="utf-8",
            )
            with self.assertRaises(RatchetError) as caught:
                classify(perry=perry, probes_dir=probes_dir, repeats=2, warmup=0)
        self.assertIn("load-bearing", str(caught.exception))

    def test_the_conservative_reading_may_vary_and_its_spread_is_reported(self):
        # 12_large_live_set's conservative reading is genuinely unstable — that
        # spread is why #7554 had to stop gating the cell. Raising on it would
        # delete the evidence instead of reporting it.
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp)
            (probes_dir / "05_stub.ts").write_text(
                "import os, sys\n"
                'off = os.environ.get("PERRY_CONSERVATIVE_STACK_SCAN") == "off"\n'
                "state = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'n')\n"
                "n = 0\n"
                "if not off:\n"
                "    try:\n"
                "        n = int(open(state).read())\n"
                "    except Exception:\n"
                "        n = 0\n"
                "    open(state, 'w').write(str(n + 1))\n"
                'sys.stdout.write("probe:stub\\nchecksum:1\\n")\n'
                'sys.stderr.write("#gcmetric heap_used_bytes=%d\\n" % (5000000 + n * 1000))\n'
                'sys.stderr.write("#gcmetric heap_total_bytes=20971520\\n")\n'
                'sys.stderr.write("#gcmetric rss_bytes=30000000\\n")\n',
                encoding="utf-8",
            )
            payload = classify(perry=perry, probes_dir=probes_dir, repeats=3, warmup=0)
        row = payload["probes"][0]
        self.assertEqual(row["heap_used_spread_bytes"], 2000)
        self.assertEqual(row["heap_used_precise_bytes"], 5_000_000)

    def test_rendered_table_names_the_explicit_gc_site(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp)
            payload = classify(perry=perry, probes_dir=probes_dir, repeats=2, warmup=0)
        report = render_classification(payload)
        self.assertIn("explicit gc()", report)
        self.assertIn("1,048,576", report)

    def test_scan_mode_env_is_the_documented_knob(self):
        # The whole tool rests on this being the knob that disables the scan.
        # A rename in the runtime must break a test, not silently make every
        # `precise` column equal to its `conservative` one.
        self.assertEqual(SCAN_MODE_ENV, "PERRY_CONSERVATIVE_STACK_SCAN")
        runtime = (
            REPO_ROOT / "crates" / "perry-runtime" / "src" / "gc" / "roots" / "scan_mode.rs"
        ).read_text(encoding="utf-8")
        self.assertIn(f'std::env::var("{SCAN_MODE_ENV}")', runtime)


if __name__ == "__main__":
    unittest.main()
