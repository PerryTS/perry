"""Unit tests for the representation-selection promotion census (#7106).

The census is a gate, so most of these tests assert the FAILING direction.
CLAUDE.md's "Four ways a gate can be unable to fail" is the design brief: a
census that reports `Ptr<Shape>: 0` and exits green is exactly the state the
project was already in, so the tests that matter are the ones that show the
verdict logic going red.
"""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


if sys.version_info < (3, 11):
    print("SKIP: Python 3.11+ is required for stdlib TOML parsing")
    raise SystemExit(0)


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "compiler_output_regression.py"

SPEC = importlib.util.spec_from_file_location("compiler_output_regression", SCRIPT_PATH)
assert SPEC is not None
HARNESS = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)

from compiler_output_harness import repsel_census as CENSUS
from compiler_output_harness.common import HarnessError


def report(
    *,
    selected: dict[str, int] | None = None,
    denied: dict[str, int] | None = None,
    entries: list[dict] | None = None,
    schema_version: int = 1,
) -> dict:
    selected = selected or {}
    denied = denied or {}
    return {
        "schema_version": schema_version,
        "summary": {
            "selected": sum(selected.values()),
            "denied": sum(denied.values()),
            "by_analysis": [
                {
                    "analysis": analysis,
                    "target_rep": analysis,
                    "rule_source": "x.rs",
                    "selected": selected.get(analysis, 0),
                    "denied": denied.get(analysis, 0),
                }
                for analysis in CENSUS.EXPECTED_ANALYSES
            ],
        },
        "entries": entries or [],
    }


def win(analysis: str, rep: str) -> dict:
    return {"analysis": analysis, "outcome": "selected", "rep": rep}


class CensusExtraction(unittest.TestCase):
    def test_every_key_is_present_even_when_zero(self):
        counts = CENSUS.census_from_report(report())["counts"]
        self.assertEqual(set(counts), set(CENSUS.CENSUS_KEYS))
        self.assertTrue(all(v == 0 for v in counts.values()), counts)

    def test_canonical_slot_is_split_per_representation(self):
        """The #7034 signal is `Ptr<Shape>` 0 WHILE `Str` is nonzero.

        One aggregate `canonical-slot` number would hide it, so the census
        keys on the rep, not the analysis.
        """
        payload = report(
            selected={"canonical-slot": 4},
            entries=[
                win("canonical-slot", "I32"),
                win("canonical-slot", "I32"),
                win("canonical-slot", "U32"),
                win("canonical-slot", "Str"),
            ],
        )
        counts = CENSUS.census_from_report(payload)["counts"]
        self.assertEqual(counts["canonical-i32"], 2)
        self.assertEqual(counts["canonical-u32"], 1)
        self.assertEqual(counts["canonical-str"], 1)
        self.assertEqual(counts["ptr-shape"], 0)

    def test_taptr_slots_are_counted_inside_spec_abi_tuples(self):
        payload = report(
            selected={"spec-abi": 2},
            entries=[
                win("spec-abi", "ta4x256,ta4x16,i32,f64"),
                win("spec-abi", "i32"),
            ],
        )
        counts = CENSUS.census_from_report(payload)["counts"]
        self.assertEqual(counts["spec-abi-entry"], 2)
        self.assertEqual(counts["spec-abi-taptr-slot"], 2)

    def test_denials_are_not_counted_as_promotions(self):
        payload = report(
            denied={"ptr-shape": 3},
            entries=[{"analysis": "ptr-shape", "outcome": "denied", "rep": "Boxed"}],
        )
        result = CENSUS.census_from_report(payload)
        self.assertEqual(result["counts"]["ptr-shape"], 0)
        self.assertEqual(result["candidates"]["ptr-shape"], 3)

    def test_schema_drift_is_loud(self):
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(report(schema_version=2))

    def test_a_missing_analysis_row_is_an_error_not_a_zero(self):
        """An absent key and a zero key are indistinguishable downstream.

        The compiler is required to enumerate `Analysis::ALL`; if it stops,
        the census must say so rather than quietly report zeros for whatever
        vanished.
        """
        payload = report()
        payload["summary"]["by_analysis"] = [
            row
            for row in payload["summary"]["by_analysis"]
            if row["analysis"] != "ptr-numarray"
        ]
        with self.assertRaises(HarnessError) as ctx:
            CENSUS.census_from_report(payload)
        self.assertIn("ptr-numarray", str(ctx.exception))

    def test_an_unknown_canonical_rep_is_an_error_not_a_silent_drop(self):
        """A new `SlotRep` variant must not vanish from the census.

        Without this, adding a seventh representation would show up as
        "nothing changed" — the census would count its promotions into no key
        at all.
        """
        payload = report(
            selected={"canonical-slot": 1},
            entries=[win("canonical-slot", "F64Unboxed")],
        )
        with self.assertRaises(HarnessError) as ctx:
            CENSUS.census_from_report(payload)
        self.assertIn("F64Unboxed", str(ctx.exception))

    def test_summary_and_entries_must_agree(self):
        payload = report(
            selected={"canonical-slot": 5},
            entries=[win("canonical-slot", "I32")],
        )
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(payload)


class Verdicts(unittest.TestCase):
    def test_a_count_below_its_floor_is_a_regression(self):
        counts = {key: 0 for key in CENSUS.CENSUS_KEYS}
        regressions, improvements = CENSUS.check_workload("w", {"ptr-shape": 2}, counts)
        self.assertEqual(len(regressions), 1)
        self.assertIn("floor is 2", regressions[0])
        self.assertFalse(improvements)

    def test_a_count_above_its_floor_is_an_advisory_not_a_failure(self):
        counts = {key: 0 for key in CENSUS.CENSUS_KEYS}
        counts["ptr-shape"] = 3
        regressions, improvements = CENSUS.check_workload("w", {"ptr-shape": 1}, counts)
        self.assertFalse(regressions)
        self.assertEqual(len(improvements), 1)

    def test_a_zero_floor_alone_cannot_fail(self):
        """The reason liveness fixtures exist, stated as a test.

        `Ptr<Shape>` on `batch.ts` is honestly zero today, so its floor is
        zero, so that row can never go red. A gate built only from floors
        would therefore be unable to detect the representation dying.
        """
        counts = {key: 0 for key in CENSUS.CENSUS_KEYS}
        regressions, _ = CENSUS.check_workload("batch", {"ptr-shape": 0}, counts)
        self.assertFalse(regressions)

    def test_a_key_that_is_zero_corpus_wide_fails(self):
        counts = {key: 1 for key in CENSUS.CENSUS_KEYS}
        counts["ptr-shape"] = 0
        problems = CENSUS.check_instrument_liveness({"w": {"counts": counts}})
        self.assertEqual(len(problems), 1)
        self.assertIn("ptr-shape", problems[0])

    def test_liveness_fixtures_fail_when_their_representation_stops_firing(self):
        observed = {
            name: {"counts": {key: minimum for key, minimum in minimums.items()}}
            for name, minimums in CENSUS.LIVENESS_FLOORS.items()
        }
        self.assertFalse(CENSUS.check_liveness_fixtures(observed))
        observed["fixture_ptr_shape"]["counts"]["ptr-shape"] = 0
        failures = CENSUS.check_liveness_fixtures(observed)
        self.assertEqual(len(failures), 1)
        self.assertIn("fixture_ptr_shape", failures[0])

    def test_a_fixture_that_never_ran_fails(self):
        """Silence is not success — CLAUDE.md failure mode (4)."""
        failures = CENSUS.check_liveness_fixtures({})
        self.assertEqual(len(failures), len(CENSUS.LIVENESS_FLOORS))
        self.assertTrue(all("did not run" in f for f in failures))


class Baseline(unittest.TestCase):
    def test_the_shipped_baseline_loads_and_covers_every_fixture(self):
        data = CENSUS.load_baseline(CENSUS.DEFAULT_BASELINE)
        names = {w["name"] for w in data["workloads"]}
        for fixture in CENSUS.LIVENESS_FLOORS:
            self.assertIn(fixture, names)

    def test_the_shipped_baseline_records_the_fixture_minimums(self):
        """The committed floors must already be at or above LIVENESS_FLOORS.

        If they are not, the fixture is not actually promoting what it claims
        and the liveness assertion is decorative.
        """
        data = CENSUS.load_baseline(CENSUS.DEFAULT_BASELINE)
        by_name = {w["name"]: w for w in data["workloads"]}
        for fixture, minimums in CENSUS.LIVENESS_FLOORS.items():
            floors = by_name[fixture]["floors"]
            for key, minimum in minimums.items():
                self.assertGreaterEqual(
                    int(floors.get(key, 0)),
                    minimum,
                    f"{fixture}.{key} baseline floor is below its liveness minimum",
                )

    def test_every_baseline_source_exists(self):
        data = CENSUS.load_baseline(CENSUS.DEFAULT_BASELINE)
        for workload in data["workloads"]:
            self.assertTrue(
                (REPO_ROOT / workload["source"]).exists(),
                f"{workload['name']} source is missing: {workload['source']}",
            )

    def test_dropping_a_fixture_from_the_baseline_is_rejected(self):
        """Deleting a liveness row would silently disarm the gate."""
        data = json.loads(CENSUS.DEFAULT_BASELINE.read_text(encoding="utf-8"))
        data["workloads"] = [
            w for w in data["workloads"] if w["name"] != "fixture_ptr_shape"
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "baseline.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(HarnessError) as ctx:
                CENSUS.load_baseline(path)
        self.assertIn("fixture_ptr_shape", str(ctx.exception))

    def test_unknown_floor_keys_are_rejected(self):
        data = {
            "schema_version": 1,
            "workloads": [
                {"name": name, "source": "x.ts", "floors": {}}
                for name in CENSUS.LIVENESS_FLOORS
            ],
        }
        data["workloads"][0]["floors"] = {"ptr-shpae": 1}
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "baseline.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(HarnessError):
                CENSUS.load_baseline(path)


class ReportExtraction(unittest.TestCase):
    def test_json_is_located_amid_other_stderr_noise(self):
        payload = json.dumps(report(), indent=2)
        stderr = f"warning: something\n{payload}\nlinking...\n"
        self.assertEqual(
            CENSUS._extract_json(stderr)["schema_version"],
            CENSUS.SUPPORTED_REPORT_SCHEMA,
        )

    def test_a_missing_report_is_an_error_not_an_empty_census(self):
        """An object-cache hit produces no report.

        Treating that as "zero promotions" would make the gate green for the
        wrong reason on every cached build.
        """
        with self.assertRaises(HarnessError):
            CENSUS._extract_json("nothing here\n")


if __name__ == "__main__":
    unittest.main()
