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
    consumed: dict[str, int] | None = None,
    unconsumed: dict[str, int] | None = None,
    schema_version: int = 2,
) -> dict:
    selected = selected or {}
    denied = denied or {}
    consumed = consumed or {}
    unconsumed = unconsumed or {}
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
                    "consumed": consumed.get(analysis, 0),
                    "unconsumed": unconsumed.get(analysis, 0),
                }
                for analysis in CENSUS.EXPECTED_ANALYSES
            ],
        },
        "entries": entries or [],
    }


def win(analysis: str, rep: str) -> dict:
    return {"analysis": analysis, "outcome": "selected", "rep": rep}


def consumed_entry(
    analysis: str,
    local_id,
    function: str = "f",
    position: str = "local",
    site: str = "ptr_shape_set",
) -> dict:
    return {
        "analysis": analysis,
        "outcome": "consumed",
        "rep": "Ptr<Shape>",
        "position": position,
        "local_id": local_id,
        "function": function,
        "site": site,
    }


def unconsumed_entry(analysis: str, rule: str, local_id=1) -> dict:
    return {
        "analysis": analysis,
        "outcome": "unconsumed",
        "rep": "Ptr<Shape>",
        "position": "local",
        "local_id": local_id,
        "function": "module_init",
        "rule": rule,
    }


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
            CENSUS.census_from_report(report(schema_version=99))

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


class AnalysisReach(unittest.TestCase):
    """`check_analysis_reach` — "never looked at" is not "considered and denied".

    A promotion count of zero is a fact about the corpus. Zero *candidates* is a
    fact about the compiler, and the two produce an identical census table.
    """

    @staticmethod
    def _entry(**candidates: int) -> dict:
        full = {analysis: 0 for analysis in CENSUS.EXPECTED_ANALYSES}
        full.update(candidates)
        return {"counts": {key: 0 for key in CENSUS.CENSUS_KEYS}, "candidates": full}

    def test_a_workload_no_analysis_reached_fails(self):
        problems = CENSUS.check_analysis_reach({"suite_02_loop_overhead": self._entry()})
        self.assertEqual(len(problems), 1)
        self.assertIn("suite_02_loop_overhead", problems[0])
        self.assertIn("zero candidates", problems[0])

    def test_one_candidate_is_enough_to_be_reached(self):
        """Even an all-DENIED workload passes: a denial names a rule."""
        observed = {"suite_02_loop_overhead": self._entry(**{"canonical-slot": 1})}
        self.assertFalse(CENSUS.check_analysis_reach(observed))

    def test_promoting_nothing_is_not_by_itself_a_failure(self):
        """The point of the gate is reach, not promotion."""
        entry = self._entry(**{"ptr-shape": 4})
        self.assertEqual(sum(entry["counts"].values()), 0)
        self.assertFalse(CENSUS.check_analysis_reach({"batch": entry}))

    def test_the_allowlist_excuses_only_what_it_names(self):
        observed = {
            name: self._entry() for name in ("suite_01_startup", "suite_15_mandelbrot")
        }
        problems = CENSUS.check_analysis_reach(observed)
        self.assertEqual(len(problems), 1)
        self.assertIn("suite_15_mandelbrot", problems[0])

    def test_every_allowlisted_workload_states_a_reason(self):
        for name, reason in CENSUS.ZERO_CANDIDATE_ALLOWLIST.items():
            self.assertTrue(reason.strip(), f"{name} is allowlisted with no reason")

    def test_the_allowlist_lives_in_code_not_the_regenerable_baseline(self):
        """`--update` must not be able to widen it — same rule as LIVENESS_FLOORS."""
        baseline = json.loads((REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text())
        for workload in baseline["workloads"]:
            self.assertNotIn("zero_candidates_ok", workload)

    def test_the_shipped_baseline_has_no_unexplained_zero_candidate_workload(self):
        """The state this gate exists to prevent must not be in the baseline."""
        baseline = json.loads((REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text())
        observed = {
            w["name"]: {"counts": {}, "candidates": w["candidates"]}
            for w in baseline["workloads"]
        }
        self.assertFalse(CENSUS.check_analysis_reach(observed))


class Consumption(unittest.TestCase):
    """Selection vs consumption (#7107).

    Every test here is written so that it FAILS if consumption collapses back
    into selection -- which is the shape the census had before, and which was
    green while `batch.ts` proved two `Ptr<Shape>` values and applied one.
    """

    def test_consumption_is_counted_separately_from_selection(self):
        counts = CENSUS.census_from_report(
            report(
                selected={"ptr-shape": 2},
                consumed={"ptr-shape": 1},
                unconsumed={"ptr-shape": 1},
                entries=[
                    win("ptr-shape", "Ptr<Shape>"),
                    win("ptr-shape", "Ptr<Shape>"),
                    consumed_entry("ptr-shape", 9, "totalsRow"),
                    unconsumed_entry("ptr-shape", "module_init_context", 4),
                ],
            )
        )
        self.assertEqual(counts["counts"]["ptr-shape"], 2)
        self.assertEqual(counts["counts"]["ptr-shape-consumed"], 1)
        self.assertEqual(
            counts["unconsumed_mechanisms"], {"module_init_context": 1}
        )

    def test_one_value_consumed_at_many_sites_counts_once(self):
        """`acc` is read at three access sites; it is one promoted value.

        Counting access sites would make the consumed column drift upward with
        program size and eventually exceed `selected`, at which point it stops
        describing the same population and the comparison is meaningless.
        """
        counts = CENSUS.census_from_report(
            report(
                selected={"ptr-shape": 1},
                consumed={"ptr-shape": 3},
                entries=[win("ptr-shape", "Ptr<Shape>")]
                + [consumed_entry("ptr-shape", 9, "totalsRow") for _ in range(3)],
            )
        )
        self.assertEqual(counts["counts"]["ptr-shape-consumed"], 1)

    def test_a_proven_this_receiver_does_not_inflate_the_consumed_column(self):
        """Phase 5a consumes the representation for a value never selected.

        `suite_09_method_calls` emits two `__pshape` clones whose bodies consume
        the proof for `this`. Counting those would report 2 consumed against 1
        selected -- an "improvement" produced entirely by dead code, since those
        clones have zero call sites.
        """
        counts = CENSUS.census_from_report(
            report(
                selected={"ptr-shape": 1},
                consumed={"ptr-shape": 2},
                entries=[
                    win("ptr-shape", "Ptr<Shape>"),
                    consumed_entry("ptr-shape", None, "C.m", position="param"),
                    consumed_entry("ptr-shape", None, "C.n", position="param"),
                ],
            )
        )
        self.assertEqual(counts["counts"]["ptr-shape-consumed"], 0)
        self.assertEqual(counts["consumed_receiver"], 2)

    def test_consumed_above_selected_is_incoherent(self):
        self.assertTrue(
            CENSUS.check_consumption_invariant(
                {"w": {"counts": {"ptr-shape": 1, "ptr-shape-consumed": 2}}}
            )
        )

    def test_a_report_without_a_consumed_tally_is_rejected(self):
        payload = report(selected={"ptr-shape": 1})
        for row in payload["summary"]["by_analysis"]:
            row.pop("consumed")
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(payload)

    def test_consumption_for_an_uninstrumented_analysis_is_loud(self):
        payload = report(
            selected={"canonical-slot": 1},
            entries=[
                {"analysis": "canonical-slot", "outcome": "selected", "rep": "I32"},
                consumed_entry("canonical-slot", 1),
            ],
        )
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(payload)

    def test_a_consumed_floor_can_fail_while_the_selected_floor_passes(self):
        """The regression the old census could not express.

        `batch` selects 2 `Ptr<Shape>` values whether or not codegen applies
        either of them. Only the consumed column moves.
        """
        counts = {"ptr-shape": 2, "ptr-shape-consumed": 0}
        regressions, _ = CENSUS.check_workload(
            "batch", {"ptr-shape": 2, "ptr-shape-consumed": 1}, counts
        )
        self.assertTrue(any("ptr-shape-consumed" in r for r in regressions))
        regressions, _ = CENSUS.check_workload("batch", {"ptr-shape": 2}, counts)
        self.assertFalse(regressions)

    def test_wasted_promotions_must_name_a_mechanism(self):
        self.assertTrue(
            CENSUS.check_unconsumed_is_explained(
                {
                    "w": {
                        "counts": {"ptr-shape": 2, "ptr-shape-consumed": 1},
                        "unconsumed_mechanisms": {},
                    }
                }
            )
        )
        self.assertFalse(
            CENSUS.check_unconsumed_is_explained(
                {
                    "w": {
                        "counts": {"ptr-shape": 2, "ptr-shape-consumed": 1},
                        "unconsumed_mechanisms": {"scalar_replaced": 1},
                    }
                }
            )
        )

    def test_the_instrumentation_table_lives_in_code_not_the_baseline(self):
        """Same rule as LIVENESS_FLOORS and ZERO_CANDIDATE_ALLOWLIST.

        `--update` regenerates the baseline from observation. If which analyses
        are instrumented were regenerable, a build that stopped recording
        consumption would rewrite itself into a permanently-green gate.
        """
        source = (
            CENSUS.REPO_ROOT / "scripts/compiler_output_harness/repsel_census.py"
        ).read_text(encoding="utf-8")
        self.assertIn("CONSUMPTION_INSTRUMENTED: dict[str, str] = {", source)
        baseline = json.loads(
            (CENSUS.REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text()
        )
        self.assertNotIn("consumption_instrumented", baseline)

    def test_every_instrumented_analysis_has_a_census_key(self):
        for analysis, key in CENSUS.CONSUMPTION_INSTRUMENTED.items():
            self.assertIn(analysis, CENSUS.CENSUS_KEYS)
            self.assertIn(key, CENSUS.CENSUS_KEYS)

    def test_every_instrumented_analysis_has_a_nonzero_consumed_liveness_floor(self):
        """The gate on the gate.

        A `-consumed` floor of zero can never go red, exactly as a zero
        `ptr-shape` floor cannot -- which is why #7104 introduced the fixtures
        in the first place. Deleting the consumed minimum from LIVENESS_FLOORS
        would leave every other check intact and every census run green, so the
        minimum's EXISTENCE has to be asserted somewhere that is not itself the
        baseline.
        """
        for analysis, consumed_key in CENSUS.CONSUMPTION_INSTRUMENTED.items():
            with_floor = [
                fixture
                for fixture, minimums in CENSUS.LIVENESS_FLOORS.items()
                if int(minimums.get(consumed_key, 0)) > 0
            ]
            self.assertTrue(
                with_floor,
                f"{analysis} records consumption but no liveness fixture asserts a "
                f"nonzero {consumed_key}. Without one the consumed column is a "
                "counter nobody has watched go red.",
            )

    def test_every_consumption_site_must_fire_somewhere_in_the_corpus(self):
        """The per-site liveness gate.

        The consumed count is per VALUE, so one working recorder is enough to
        mark a value consumed and the other five can rot silently. This is what
        makes that visible -- and it is not hypothetical: when coverage was
        first measured, `class_field_get_number.shape_proven_load` and
        `ptr_shape_update` had never fired on any workload in the corpus.
        """
        every = {
            "w": {
                "consumption_sites": {s: 1 for s in CENSUS.CONSUMPTION_SITES},
            }
        }
        self.assertFalse(CENSUS.check_consumption_site_coverage(every))
        for missing in CENSUS.CONSUMPTION_SITES:
            partial = {
                "w": {
                    "consumption_sites": {
                        s: 1 for s in CENSUS.CONSUMPTION_SITES if s != missing
                    }
                }
            }
            failures = CENSUS.check_consumption_site_coverage(partial)
            self.assertTrue(
                any(missing in f for f in failures),
                f"a dead {missing!r} recorder must be visible",
            )

    def test_an_unregistered_consumption_site_is_loud(self):
        """A new consumption lowering must be registered, not absorbed.

        Folding an unknown site into an existing count is how a recorder stops
        firing without anyone noticing.
        """
        payload = report(
            selected={"ptr-shape": 1},
            consumed={"ptr-shape": 1},
            entries=[
                win("ptr-shape", "Ptr<Shape>"),
                consumed_entry("ptr-shape", 1, site="brand_new_lowering"),
            ],
        )
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(payload)

    def test_a_consumption_entry_with_no_site_is_loud(self):
        payload = report(
            selected={"ptr-shape": 1},
            consumed={"ptr-shape": 1},
            entries=[win("ptr-shape", "Ptr<Shape>")]
            + [{k: v for k, v in consumed_entry("ptr-shape", 1).items() if k != "site"}],
        )
        with self.assertRaises(HarnessError):
            CENSUS.census_from_report(payload)

    def test_the_site_registry_lives_in_code_not_the_baseline(self):
        source = (
            CENSUS.REPO_ROOT / "scripts/compiler_output_harness/repsel_census.py"
        ).read_text(encoding="utf-8")
        self.assertIn("CONSUMPTION_SITES: dict[str, str] = {", source)
        baseline = json.loads(
            (CENSUS.REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text()
        )
        self.assertNotIn("consumption_sites_registry", baseline)

    def test_the_shipped_baseline_exercises_every_consumption_site(self):
        """Pins the coverage the site fixture was added to provide.

        `--update` records per-workload site counts as context. If a site drops
        out of the shipped baseline, the corpus stopped reaching it and the
        gate above would have gone red on a real run -- this catches it without
        needing a compiler.
        """
        baseline = json.loads(
            (CENSUS.REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text()
        )
        seen: set[str] = set()
        for w in baseline["workloads"]:
            seen.update(w.get("consumption_sites", {}))
        for site in CENSUS.CONSUMPTION_SITES:
            self.assertIn(
                site,
                seen,
                f"no workload in the shipped baseline exercises {site!r}",
            )

    def test_the_shipped_baseline_shows_a_consumed_gap(self):
        """The finding itself, pinned.

        If this ever passes trivially because every workload consumes what it
        selects, that is a real improvement -- delete the test and say so in the
        PR. What it must not do is silently stop being true because the counter
        died.
        """
        baseline = json.loads(
            (CENSUS.REPO_ROOT / "benchmarks/repsel_census/baseline.json").read_text()
        )
        floors = {w["name"]: w["floors"] for w in baseline["workloads"]}
        selected = sum(f.get("ptr-shape", 0) for f in floors.values())
        consumed = sum(f.get("ptr-shape-consumed", 0) for f in floors.values())
        self.assertGreater(selected, 0)
        self.assertGreater(consumed, 0, "the consumed counter must not be dead")
        self.assertGreaterEqual(selected, consumed)
        self.assertEqual(
            floors["batch"]["ptr-shape"],
            2,
            "#7107's ratcheted selection floor must not be reinterpreted",
        )
        self.assertEqual(
            floors["batch"]["ptr-shape-consumed"],
            1,
            "batch proves two Ptr<Shape> values and applies one",
        )


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
