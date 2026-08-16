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
import os
import stat
import sys
import tempfile
import subprocess
import unittest
from unittest import mock
from pathlib import Path

from benchmarks.gc_ratchet.gc_ratchet import (
    code_tree_hash,
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
    inspect_artifact,
    main,
    measure,
    parse_gc_diag,
    parse_gcmetrics,
    parse_scan_fallbacks,
    probe_overrides_from_json,
    probe_run_env,
    render,
    render_classification,
    run_once,
    tolerances_from_json,
    validate_artifact,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
TOLERANCES_PATH = REPO_ROOT / "benchmarks" / "gc_ratchet" / "tolerances.json"


def _artifact_with_synthetic_receipt():
    """The pinned artifact plus a well-formed selective-re-pin receipt.

    Derived FROM the pin rather than hard-coded. The tests that tamper with a
    receipt are testing the *validator*, not whatever happens to be pinned
    today -- sourcing their fixture from the live baseline is what tied them to
    one historical selective re-pin and broke them at the next full one.

    Two cells, because a single-cell receipt cannot catch an inspector that
    validates only `cells[0]`.
    """
    artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
    artifact.pop("accepted_deterministic_deltas", None)

    chosen = []
    for probe_name, probe in sorted(artifact["probes"].items()):
        for metric in sorted(probe.get("metrics", {})):
            if metric not in DETERMINISTIC_METRICS:
                continue
            pinned = probe["metrics"][metric].get("median")
            if isinstance(pinned, bool) or not isinstance(pinned, (int, float)):
                continue
            chosen.append((probe_name, metric, pinned))
            break
        if len(chosen) == 2:
            break
    assert len(chosen) == 2, "pinned artifact has too few deterministic cells to build a receipt"

    cause = "b" * 40
    artifact["accepted_deterministic_deltas"] = {
        "commit": "a" * 40,
        "code_tree": "c" * 40,
        "generated_at": "2026-01-01T00:00:00+00:00",
        "notes": "Synthetic receipt built by the test suite from the pinned artifact.",
        "measurement": {
            "platform": "test-harness",
            "repeats": 3,
            "traced_runs": 2,
            "binaries": {
                name: {"size": 1, "sha256": "d" * 64}
                for name in ("perry", "libperry_runtime.a", "libperry_stdlib.a")
            },
        },
        "causes": {
            cause: {
                "pull_request": 1,
                "category": "synthetic",
                "evidence": "constructed by the test suite",
            }
        },
        "cells": [
            {
                "probe": probe,
                "metric": metric,
                # previous must differ from accepted, or the inspector reports
                # "records no change" -- a receipt row for a cell that did not
                # move is itself a defect.
                "previous_median": pinned + 1,
                "accepted_median": pinned,
                "causes": [cause],
            }
            for probe, metric, pinned in chosen
        ],
    }
    return artifact


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
    def test_measurement_refuses_a_host_without_wait4_before_launching(self):
        with mock.patch.object(os, "wait4", None, create=True):
            with self.assertRaisesRegex(RatchetError, "requires os.wait4"):
                run_once(["this-command-must-not-run"])

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

        Six of the fourteen shipped probes pin ``minor_cycles`` at 1 and the
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
        """Nothing moved AT ALL — neither copied nor promoted — is the failure.

        #7558 narrowed this from ``copied_objects`` to
        ``copied_objects + promoted_objects``. Both are parsed from the same
        ``[gc-copy-minor] ran`` line, so either alone names a *destination*;
        only the sum answers "did the copying minor move anything".
        """
        baseline = _baseline()
        current = _measurement(
            _probe(overrides={"copied_objects": 0.0, "promoted_objects": 0.0})
        )
        _, failures = evaluate(baseline, current, profile="shared_ci")
        self.assertTrue(any("evacuated nothing" in failure for failure in _hard(failures)))

    def test_copying_that_became_promotion_is_a_fingerprint_breach_not_a_liveness_one(self):
        """The #7558 shape, and the reason the liveness probe had to change.

        Removing explicit ``gc()``'s conservative stack scan re-enabled the
        adaptive-tenuring seed on ``gc()``-driven workloads, ``tenuring_survivals``
        fell 4 -> 1 on two probes, and every survivor went straight to old-gen:
        ``copied_objects`` 5,823 -> 0 with ``promoted_objects`` 0 -> 6,077. That
        is a copying minor that moved MORE, so it must not be reported as one
        that did not run — while still being a two-sided band breach on
        ``copied_objects``, because it IS a change in the collector's
        behavioural fingerprint and must be re-pinned deliberately.
        """
        baseline = _baseline()
        current = _measurement(
            _probe(overrides={"copied_objects": 0.0, "promoted_objects": 24_000.0})
        )
        _, failures = evaluate(baseline, current, profile="shared_ci")
        hard = _hard(failures)
        self.assertFalse(
            any("evacuated nothing" in failure for failure in hard),
            "a minor that promoted every survivor still evacuated them; calling "
            "that 'the collector did not run' would misdirect the next reader",
        )
        rows, _ = evaluate(baseline, current, profile="shared_ci")
        copied = [
            row
            for row in rows
            if row.probe == "01_probe" and row.metric == "copied_objects"
        ]
        self.assertEqual(len(copied), 1)
        self.assertEqual(
            copied[0].status,
            "REGRESSION",
            "the evacuation counters are a two-sided fingerprint: a collapse to "
            "zero copies must still turn the job red and force a deliberate re-pin",
        )

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

    def test_a_full_re_pin_without_a_receipt_is_valid(self):
        """The contract #8204 exercised, which nothing covered.

        `accepted_deterministic_deltas` is the receipt for a SELECTIVE re-pin --
        the dangerous kind, which can turn one red row green while leaving no
        machine-readable answer to which rows moved or why. A FULL re-pin has
        artifact-wide provenance instead, and the validator says so explicitly:
        `if receipt is None: return`.

        This existed only as a docstring. #8204 did a full re-pin (130 of 168
        cells moved), correctly carried no receipt, and three tests here that
        hard-subscripted the key errored with `KeyError`, reddening
        `windows-build` on every open PR. The gate punished the correct action,
        so pin the permission as a test rather than a comment.
        """
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        self.assertNotIn(
            "accepted_deterministic_deltas",
            artifact,
            "the pinned baseline is a full re-pin; update this test if that changes",
        )
        validate_artifact(artifact)

    def test_a_receipt_on_the_pin_must_name_real_cells_and_real_causes(self):
        """The durable half of the old cell-by-cell assertion.

        What that test actually pinned was one historical selective re-pin:
        #8069's exact 21 cells and causes {7928, 7960, 7961}. That is a snapshot,
        not an invariant -- any later re-pin breaks it by construction, which is
        precisely what happened. The invariant worth keeping is structural: a
        receipt, IF present, must describe the artifact it ships with.
        """
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        receipt = artifact.get("accepted_deterministic_deltas")
        if receipt is None:
            self.skipTest("pinned baseline is a full re-pin (no selective receipt)")
        probes = artifact["probes"]
        for cell in receipt["cells"]:
            self.assertIn(cell["probe"], probes)
            self.assertIn(cell["metric"], probes[cell["probe"]]["metrics"])
            self.assertEqual(
                cell["accepted_median"],
                probes[cell["probe"]]["metrics"][cell["metric"]]["median"],
                f"{cell['probe']}.{cell['metric']} receipt disagrees with the pin",
            )
            for commit in cell["causes"]:
                self.assertIn(commit, receipt["causes"])
        for cause in receipt["causes"].values():
            self.assertIsInstance(cause["pull_request"], int)
            self.assertGreater(cause["pull_request"], 0)

    def test_selective_refresh_receipt_cannot_disagree_with_the_pin(self):
        artifact = _artifact_with_synthetic_receipt()
        validate_artifact(artifact)  # control: the fixture itself is valid
        tampered = copy.deepcopy(artifact)
        tampered["accepted_deterministic_deltas"]["cells"][0]["accepted_median"] += 1
        with self.assertRaisesRegex(RatchetError, "does not match pinned median"):
            validate_artifact(tampered)

    def test_selective_refresh_receipt_rejects_a_malformed_timestamp(self):
        artifact = _artifact_with_synthetic_receipt()
        tampered = copy.deepcopy(artifact)
        tampered["accepted_deterministic_deltas"]["generated_at"] = "unknown"
        with self.assertRaisesRegex(RatchetError, "ISO-8601 UTC timestamp"):
            validate_artifact(tampered)

    def test_selective_refresh_does_not_allow_a_future_unexplained_delta(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        current = copy.deepcopy(artifact)
        current["kind"] = "gc-ratchet-measurement"
        probe = "02_survivor_promotion"
        metric = "copied_objects"
        pinned = artifact["probes"][probe]["metrics"][metric]["median"]
        tolerance = tolerances_from_json(artifact["tolerances"])["shared_ci"][metric]
        breach = pinned + tolerance.allowance(pinned) + 1
        current["probes"][probe]["metrics"][metric] = distribution([breach, breach])

        _, failures = evaluate(artifact, current, profile="shared_ci")
        self.assertTrue(
            any(probe in failure and metric in failure for failure in _hard(failures)),
            "accepted provenance must explain the pin, never suppress future drift",
        )

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


@unittest.skipUnless(callable(getattr(os, "wait4", None)), "measurement requires os.wait4")
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


class FailOpenPerCellTests(unittest.TestCase):
    """One unfit cell must cost one cell, not the whole gate's coverage (#7554).

    The failure this class exists to prevent already happened, and it is the
    most expensive kind: not a gate that passed when it should have failed, but
    a gate that *measured nothing at all* while looking busy. One cell of the
    pinned artifact — ``12_large_live_set.heap_used_bytes``, spread 6,768 bytes —
    failed the artifact-validation step, that step runs before the measurement
    step, and so for three days none of the twelve probes executed on any
    branch. Two GC pacing changes (#7594, #7596) merged inside that window and
    each had to hand-run a both-arms A/B in place of the gate.

    So the assertions here are about **blast radius**, and the load-bearing one
    is ``test_an_unfit_cell_does_not_hide_a_regression_elsewhere``: it plants a
    real, gating regression on a *different* probe in the same run and requires
    that it still be named. Under the old behaviour that regression was
    invisible, because nothing ran.
    """

    def _unfit_cell_baseline(self):
        """Two probes; ``01_probe``'s retention cell carries the #7554 defect."""
        probes = _pair()
        base = BASE_VALUES["heap_used_bytes"]
        probes["01_probe"]["metrics"]["heap_used_bytes"] = distribution([base] * 6 + [base + 6768])
        return _baseline(probes)

    def test_an_unfit_cell_is_demoted_rather_than_aborting_the_run(self):
        baseline = self._unfit_cell_baseline()
        rows, failures = evaluate(baseline, _measurement(_pair()), profile="shared_ci")
        # Every cell of both probes was still evaluated.
        self.assertEqual(len(rows), 2 * len(ALL_METRICS))
        row = next(r for r in rows if r.probe == "01_probe" and r.metric == "heap_used_bytes")
        self.assertFalse(row.gating, "an unfit cell must not stay in the gating family")
        self.assertIn("unfit", row.status)
        # Demoted, but NOT waved through: the defect is still a hard failure.
        self.assertTrue(
            any("UNFIT PINNED CELL" in failure for failure in _hard(failures)),
            f"the defect must still fail the job, got {failures}",
        )

    def test_an_unfit_cell_does_not_hide_a_regression_elsewhere(self):
        """The #7554 assertion. A bad cell must not cost the rest of the matrix."""
        baseline = self._unfit_cell_baseline()
        # A real, gating regression on the OTHER probe: half the heap reclaimed.
        current = _measurement(_pair(other_overrides={"freed_bytes": 50_000_000.0}))
        _, failures = evaluate(baseline, current, profile="shared_ci")
        hard = _hard(failures)
        self.assertTrue(
            any("02_other: freed_bytes" in failure for failure in hard),
            f"the unrelated regression must still be named, got {hard}",
        )
        self.assertTrue(any("UNFIT PINNED CELL" in failure for failure in hard))

    def test_an_unfit_probe_demotes_only_that_probe(self):
        probes = _pair()
        probes["01_probe"]["correctness"]["status"] = "unchecked"
        baseline = _baseline(probes)
        current = _measurement(_pair(other_overrides={"copied_objects": 1.0}))
        rows, failures = evaluate(baseline, current, profile="shared_ci")
        for row in rows:
            if row.probe == "01_probe":
                self.assertFalse(row.gating, f"{row.metric} on an unfit probe must be demoted")
        self.assertTrue(
            any("02_other" in failure for failure in _hard(failures)),
            "the fit probe must still be able to fail the job",
        )

    def test_structural_preflight_defers_every_defect_it_waves_through(self):
        """`--scope structural` may DEFER a defect; it may never DROP one.

        This is the test that keeps the flag from being suppression. For each
        non-fatal defect shape, preflight passes (so the probes get to run) and
        ``check`` then fails on that same defect (so the job still goes red).
        A change that made ``check`` forgiving would break this, not just make
        the gate quieter.
        """
        shapes = {
            "nondeterministic cell": self._unfit_cell_baseline(),
        }
        probes = _pair()
        probes["01_probe"]["correctness"]["status"] = "unchecked"
        shapes["unverified probe"] = _baseline(probes)
        probes = _pair()
        probes["01_probe"]["metrics"]["minor_cycles"] = distribution([0.0] * 7)
        shapes["probe that never collected"] = _baseline(probes)

        for label, baseline in shapes.items():
            with self.subTest(shape=label):
                defects = inspect_artifact(baseline)
                self.assertTrue(defects, "the shape must actually be a defect")
                self.assertFalse(
                    any(defect.fatal for defect in defects),
                    "this shape is meant to be non-fatal",
                )
                with tempfile.TemporaryDirectory() as tmp:
                    path = Path(tmp) / "artifact.json"
                    path.write_text(json.dumps(baseline), encoding="utf-8")
                    # Preflight lets it through, so the probes run...
                    self.assertEqual(
                        main(["validate", "--artifact", str(path), "--scope", "structural"]),
                        0,
                        "structural preflight must not zero the run's coverage",
                    )
                    # ...and the maintainer default still refuses it outright.
                    self.assertEqual(
                        main(["validate", "--artifact", str(path), "--scope", "all"]), 1
                    )
                # ...but check still turns the job red on the very same defect.
                _, failures = evaluate(baseline, _measurement(_pair()), profile="shared_ci")
                self.assertTrue(
                    any("UNFIT PINNED" in failure for failure in _hard(failures)),
                    f"{label}: deferred by preflight and then dropped by check",
                )

    def test_a_tampered_artifact_is_still_fatal_at_structural_scope(self):
        """Integrity is not fitness. A tampered artifact must still stop everything."""
        probes = _probe()
        probes["01_probe"]["metrics"]["heap_used_bytes"]["median"] = 1
        baseline = _baseline(probes)
        self.assertTrue(any(defect.fatal for defect in inspect_artifact(baseline)))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "artifact.json"
            path.write_text(json.dumps(baseline), encoding="utf-8")
            self.assertEqual(
                main(["validate", "--artifact", str(path), "--scope", "structural"]),
                2,
                "a tampered artifact must not be deferred to check",
            )
        with self.assertRaises(RatchetError):
            evaluate(baseline, _measurement(), profile="shared_ci")

    def test_validate_defaults_to_the_strict_scope(self):
        # The lenient scope must be opt-in. A maintainer running `validate` by
        # hand gets the full refusal; only the CI preflight asks for less.
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "artifact.json"
            path.write_text(json.dumps(self._unfit_cell_baseline()), encoding="utf-8")
            self.assertEqual(main(["validate", "--artifact", str(path)]), 1)

    def test_pinning_an_unfit_artifact_is_still_refused(self):
        # `assemble` calls validate_artifact, which is raise-on-any-defect. The
        # fail-open path is for artifacts already in the tree; it must not make
        # it possible to freeze a new one that is unfit.
        with self.assertRaises(RatchetError) as caught:
            validate_artifact(self._unfit_cell_baseline())
        self.assertIn("bit-identity", str(caught.exception))

    def test_the_shipped_artifact_has_no_deferred_defects(self):
        # The fail-open path exists for emergencies. If the artifact in the tree
        # is relying on it, the ratchet is running degraded and someone should
        # know.
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        self.assertEqual(
            [defect.describe() for defect in inspect_artifact(artifact)],
            [],
            "the pinned artifact should be fit, not merely tolerated",
        )


# ---------------------------------------------------------------------------
# Per-probe run environment — the large-Eden arm (#7481)
# ---------------------------------------------------------------------------

#: A stand-in for `perry` that also records the environment it was *compiled*
#: under, so a test can assert the probe's declared knobs did not leak into the
#: compile step. Perry's object cache keys on every codegen env var (#6394), so
#: a runtime knob passed at compile time would change the cache key without
#: changing a byte of emitted code.
_STUB_PERRY_RECORDING_COMPILE_ENV = """#!{python}
import json, os, stat, sys
source = sys.argv[1]
out = sys.argv[sys.argv.index("-o") + 1]
here = os.path.dirname(os.path.abspath(source)) or "."
with open(os.path.join(here, "compile-env.json"), "w") as handle:
    json.dump({{k: v for k, v in os.environ.items() if k.startswith("PERRY_")}}, handle)
body = open(os.path.join(here, source)).read()
with open(out, "w") as handle:
    handle.write("#!{python}\\n" + body)
os.chmod(out, os.stat(out).st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
"""

#: A stub probe whose retention depends on the arm. This is what makes the
#: delivery testable: if the harness merely *recorded* `run_env` without passing
#: it to the child, `heap_used_bytes` would come back at the unarmed value and
#: the assertion below would fail. A test that only checked the recorded dict
#: would pass in exactly that broken state.
_STUB_ARMED_PROBE = """
import os, sys
armed = os.environ.get("PERRY_GC_SCAVENGE_NURSERY_MB") == "64"
sys.stdout.write("probe:stub\\nchecksum:1\\n")
sys.stderr.write("#gcmetric heap_used_bytes=%d\\n" % (6000000 if armed else 5000000))
sys.stderr.write("#gcmetric heap_total_bytes=20971520\\n")
sys.stderr.write("#gcmetric rss_bytes=30000000\\n")
if os.environ.get("PERRY_GC_DIAG"):
    sys.stderr.write(
        "[gc-copy-minor] ran copied_objects=%d copied_bytes=64 promoted_objects=0 "
        "promoted_bytes=0 freed_bytes=128\\n" % (11 if armed else 7)
    )
"""

#: The directive, wrapped in a module docstring so the stub probe — which is
#: Python, because these tests must run without a compiler — stays executable
#: while carrying a line the harness reads as TypeScript.
_ARM_DIRECTIVE = '"""\n// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64\n"""\n'


class ProbeRunEnvParsingTests(unittest.TestCase):
    """A probe declares the collector configuration it is a probe *of*."""

    def _source(self, tmp, text):
        path = Path(tmp) / "13_stub.ts"
        path.write_text(text, encoding="utf-8")
        return path

    def test_a_probe_with_no_directive_declares_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(probe_run_env(self._source(tmp, "// just a comment\n")), {})

    def test_directives_are_read_in_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = self._source(
                tmp,
                "// header\n"
                "// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64\n"
                "// gc-ratchet-env: PERRY_GC_SCAVENGE=1\n"
                "const x = 1;\n",
            )
            self.assertEqual(
                probe_run_env(source),
                {"PERRY_GC_SCAVENGE_NURSERY_MB": "64", "PERRY_GC_SCAVENGE": "1"},
            )

    def test_a_non_perry_variable_is_refused(self):
        # The probe set is reviewed as workloads. It must not double as a way to
        # change the process the harness runs.
        with tempfile.TemporaryDirectory() as tmp:
            source = self._source(tmp, "// gc-ratchet-env: PATH=/tmp/evil\n")
            with self.assertRaises(RatchetError) as caught:
                probe_run_env(source)
        self.assertIn("PERRY_*", str(caught.exception))

    def test_a_variable_the_harness_owns_is_refused(self):
        for name, value in (
            ("PERRY_CONSERVATIVE_STACK_SCAN", "off"),
            ("PERRY_GC_DIAG", "1"),
        ):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                source = self._source(tmp, f"// gc-ratchet-env: {name}={value}\n")
                with self.assertRaises(RatchetError) as caught:
                    probe_run_env(source)
                self.assertIn("harness", str(caught.exception))

    def test_a_repeated_variable_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = self._source(
                tmp,
                "// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64\n"
                "// gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=32\n",
            )
            with self.assertRaises(RatchetError) as caught:
                probe_run_env(source)
        self.assertIn("twice", str(caught.exception))

    def test_the_shipped_large_eden_probe_still_declares_its_arm(self):
        # The subject of `13_large_eden_survivors` is the large-Eden cadence.
        # Without the directive it is a healthy probe of something else, and
        # nothing else in the tree would say so. Same discipline as
        # `gc_root_dominance_allowlist.json`: the claim has to keep matching.
        source = (
            REPO_ROOT
            / "benchmarks"
            / "gc_ratchet"
            / "probes"
            / "13_large_eden_survivors.ts"
        )
        self.assertEqual(probe_run_env(source), {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"})

    def test_the_shipped_grow_then_churn_probe_still_declares_its_arm(self):
        # `14_grow_then_churn` is a probe of the major-pacing backoff's
        # TRANSITION, and the mechanism is a ratio: the shift climbs per
        # unproductive full and the boundary is `baseline << (1 + shift)`. Its
        # two directives are what set the ABSOLUTE scale — the nursery cap
        # decides how early the first collection lands and therefore how small
        # the first post-full baseline is, and the pacing floor sets where the
        # first escalation can happen at all. At the shipped 16 MB / 32 MB the
        # same three escalations need a live set in the hundreds of MB. So
        # losing either line leaves a probe that still passes, still collects,
        # and no longer reaches the cap it exists to hold.
        source = (
            REPO_ROOT / "benchmarks" / "gc_ratchet" / "probes" / "14_grow_then_churn.ts"
        )
        self.assertEqual(
            probe_run_env(source),
            {
                "PERRY_GC_SCAVENGE_NURSERY_MB": "1",
                "PERRY_GC_MAJOR_PACING_FLOOR_MB": "1",
            },
        )


@unittest.skipUnless(callable(getattr(os, "wait4", None)), "measurement requires os.wait4")
class ProbeRunEnvDeliveryTests(unittest.TestCase):
    """The declared arm must reach the child process, not merely the artifact.

    #7024 and #7025 are the shape this guards against: a knob that disabled the
    path it was measuring, and a counter that summed two collectors so a cell
    could pass having run zero copying minors. A `run_env` the harness records
    but never exports would be exactly that — an arm that is documented, gated,
    and inert.
    """

    def _fixture(self, tmp, *, armed):
        root = Path(tmp)
        perry = root / "stub-perry"
        perry.write_text(
            _STUB_PERRY_RECORDING_COMPILE_ENV.format(python=sys.executable), encoding="utf-8"
        )
        perry.chmod(perry.stat().st_mode | stat.S_IEXEC)
        probes_dir = root / "probes"
        probes_dir.mkdir()
        (probes_dir / "13_stub.ts").write_text(
            (_ARM_DIRECTIVE if armed else "") + _STUB_ARMED_PROBE, encoding="utf-8"
        )
        return perry, probes_dir

    def test_the_declared_arm_reaches_the_probe(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp, armed=True)
            payload = measure(perry=perry, probes_dir=probes_dir, repeats=3, node=None, warmup=0)
        entry = payload["probes"]["13_stub"]
        self.assertEqual(entry["run_env"], {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"})
        self.assertEqual(entry["metrics"]["heap_used_bytes"]["median"], 6_000_000)
        self.assertEqual(entry["metrics"]["copied_objects"]["median"], 11)

    def test_removing_the_directive_moves_the_numbers(self):
        # The perturbation that proves the arm is load-bearing rather than
        # decorative: same workload, directive deleted, different collector,
        # different numbers.
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp, armed=False)
            payload = measure(perry=perry, probes_dir=probes_dir, repeats=3, node=None, warmup=0)
        entry = payload["probes"]["13_stub"]
        self.assertEqual(entry["run_env"], {})
        self.assertEqual(entry["metrics"]["heap_used_bytes"]["median"], 5_000_000)
        self.assertEqual(entry["metrics"]["copied_objects"]["median"], 7)

    def test_the_arm_does_not_leak_into_the_compile(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp, armed=True)
            measure(perry=perry, probes_dir=probes_dir, repeats=3, node=None, warmup=0)
            seen = json.loads((probes_dir / "compile-env.json").read_text(encoding="utf-8"))
        self.assertNotIn("PERRY_GC_SCAVENGE_NURSERY_MB", seen)

    def test_classify_runs_both_scan_modes_under_the_declared_arm(self):
        with tempfile.TemporaryDirectory() as tmp:
            perry, probes_dir = self._fixture(tmp, armed=True)
            payload = classify(perry=perry, probes_dir=probes_dir, repeats=3, warmup=0)
        (row,) = payload["probes"]
        self.assertEqual(row["run_env"], {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"})
        # Both arms saw the armed value, so the split is 6,000,000 / 6,000,000
        # and the residue is zero — not 6,000,000 against an unarmed 5,000,000,
        # which is what a `classify` that dropped the arm would report as a
        # 1 MB false-root excess that does not exist.
        self.assertEqual(row["heap_used_bytes"], 6_000_000)
        self.assertEqual(row["heap_used_precise_bytes"], 6_000_000)
        self.assertEqual(row["false_root_excess_bytes"], 0)


class ProbeRunEnvGateTests(unittest.TestCase):
    """A run under a different arm is not a comparison."""

    def _armed_pair(self):
        baseline = _baseline(_pair())
        baseline["probes"]["01_probe"]["run_env"] = {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"}
        current = _measurement(copy.deepcopy(_pair()))
        current["probes"]["01_probe"]["run_env"] = {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"}
        return baseline, current

    def test_matching_arms_pass(self):
        baseline, current = self._armed_pair()
        _, failures = evaluate(baseline, current, profile="pinned_host")
        self.assertEqual(_hard(failures), [])

    def test_a_dropped_arm_fails_even_though_every_band_is_satisfied(self):
        # This is the failure mode the check exists for. Every metric is
        # byte-identical to the baseline; the only thing that changed is which
        # collector produced them.
        baseline, current = self._armed_pair()
        current["probes"]["01_probe"]["run_env"] = {}
        _, failures = evaluate(baseline, current, profile="pinned_host")
        self.assertTrue(
            any("different collector configuration" in failure for failure in _hard(failures)),
            _hard(failures),
        )

    def test_a_changed_arm_value_fails(self):
        baseline, current = self._armed_pair()
        current["probes"]["01_probe"]["run_env"] = {"PERRY_GC_SCAVENGE_NURSERY_MB": "32"}
        _, failures = evaluate(baseline, current, profile="pinned_host")
        self.assertTrue(
            any("different collector configuration" in failure for failure in _hard(failures)),
            _hard(failures),
        )

    def test_an_added_arm_fails(self):
        # The mirror image: the baseline pinned the default and the run armed
        # itself. Also a comparison of two collectors.
        baseline, current = self._armed_pair()
        current["probes"]["02_other"]["run_env"] = {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"}
        _, failures = evaluate(baseline, current, profile="pinned_host")
        self.assertTrue(
            any("02_other" in failure and "different collector" in failure for failure in _hard(failures)),
            _hard(failures),
        )

    def test_an_unreadable_run_env_is_a_fatal_artifact_defect(self):
        baseline, current = self._armed_pair()
        baseline["probes"]["01_probe"]["run_env"] = {"PERRY_GC_SCAVENGE_NURSERY_MB": 64}
        self.assertTrue(any(defect.fatal for defect in inspect_artifact(baseline)))
        with self.assertRaises(RatchetError):
            evaluate(baseline, current, profile="pinned_host")

    def test_the_rendered_table_names_the_armed_probes(self):
        baseline, current = self._armed_pair()
        rows, _ = evaluate(baseline, current, profile="pinned_host")
        text = render(rows, baseline, "pinned_host")
        self.assertIn("non-default collector configuration", text)
        self.assertIn("PERRY_GC_SCAVENGE_NURSERY_MB=64", text)

    def test_the_shipped_artifact_pins_the_large_eden_arm(self):
        artifact = json.loads(DEFAULT_ARTIFACT.read_text(encoding="utf-8"))
        self.assertEqual(
            artifact["probes"]["13_large_eden_survivors"]["run_env"],
            {"PERRY_GC_SCAVENGE_NURSERY_MB": "64"},
        )


if __name__ == "__main__":
    unittest.main()

class RebaseStableProvenanceTests(unittest.TestCase):
    """#7666 follow-up: `commit` alone does not survive the merge that ships it.

    A gc-ratchet re-pin is written on a branch and REBASED at merge time (the
    maintainer adds the version bump), which orphans the commit the pin
    recorded. #7666's artifact named `a8f73122d`; that object still resolves
    locally but is NOT an ancestor of `main`, so a future reader attributing a
    moved cell looks up a commit that is not in the history. This recurs on
    every pin, so the artifact also records the tree hash of `crates/` — the
    code whose behaviour a probe measures, stable across both the rebase and
    the version bump (which touches only Cargo.toml / Cargo.lock / CLAUDE.md).
    """

    def test_the_shipped_baseline_records_a_code_tree(self):
        baseline = Path(__file__).resolve().parents[1] / "benchmarks" / "gc_ratchet" / "baseline" / "gc-ratchet-v1.json"
        artifact = json.loads(baseline.read_text(encoding="utf-8"))
        self.assertIn(
            "code_tree",
            artifact,
            "the pinned artifact must carry a rebase-stable provenance field; "
            "`commit` alone is orphaned by the merge that ships the pin",
        )
        self.assertRegex(
            artifact["code_tree"],
            r"^[0-9a-f]{40}$",
            "code_tree must be a full git tree hash",
        )

    def test_code_tree_hash_is_a_tree_not_a_commit(self):
        """It must name `HEAD:crates`, not `HEAD`.

        A commit hash here would be exactly the field it replaces, and would
        change on every version bump — which is half of what makes `commit`
        useless for this.
        """
        value = code_tree_hash()
        if value == "unknown":
            self.skipTest("git unavailable")
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=Path(__file__).resolve().parents[1], capture_output=True, text=True, check=False,
        ).stdout.strip()
        self.assertNotEqual(
            value, head, "code_tree must be a TREE hash, not the commit hash"
        )
        expected = subprocess.run(
            ["git", "rev-parse", "HEAD:crates"],
            cwd=Path(__file__).resolve().parents[1], capture_output=True, text=True, check=False,
        ).stdout.strip()
        self.assertEqual(value, expected)
