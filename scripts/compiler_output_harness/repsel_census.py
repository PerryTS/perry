"""Representation-selection promotion census (#7106).

Perry's performance story rests on six unboxed representations. Until #7034
nobody knew how many values each one actually promotes on real code, because
nothing reported it: an agent had to hand-instrument the compiler to discover
that `Ptr<Shape>` promotes **nothing at all** on `batch.ts` — the
object/property-heavy program the representation exists for. Independently
confirmed: `PERRY_PTR_SHAPE_LOCALS=0` and the default produce a byte-identical
binary.

This module turns that into a standing, gated measurement. It runs
`perry compile --opt-report=json --no-link` over a corpus, extracts a
**per-representation** count of promotions per workload, and compares each
count against a ratcheted floor.

## Why the counts are per representation, never aggregated

The interesting signal in #7034 is that `Ptr<Shape>` is 0 *while* canonical
`Str` is nonzero. One aggregate number hides exactly that. So the census keys
on the representation, splitting `canonical-slot` into its three reps
(`I32`/`U32`/`Str`) and counting `TaPtr` parameter slots inside specialized-ABI
entries.

## Why a floor and not a diff

CLAUDE.md, "Four ways a gate can be unable to fail", case 4: *the gate runs but
its subject never did*. A census that faithfully reports `Ptr<Shape>: 0` and
exits green is worth nothing — that is the state the project is in today. So:

1. Every workload carries a **floor** per representation. A count below its
   floor is red. `--update` rewrites floors from observation.
2. Floors alone are not enough, because today's honest floor for
   `Ptr<Shape>` on real code *is* zero, and a zero floor can never fail. So
   the corpus includes **liveness fixtures**: hand-written programs that must
   promote a specific representation. Their minimums live in
   [`LIVENESS_FLOORS`] — **in this file, not in the baseline JSON** — so that
   re-running `--update` after a breakage cannot silently write them down to
   zero and leave a permanently-green gate behind.
3. A representation whose census key is nonzero *nowhere* in the corpus fails
   the run outright ([`check_instrument_liveness`]). A counter that is zero
   because nothing was promoted and a counter that is zero because nobody
   increments it look identical in a report; this distinguishes them.

Point 2 is what makes the gate falsifiable in both directions:
`PERRY_PTR_SHAPE_LOCALS=0` takes the `ptr_shape` fixture to zero and the run
goes red, while the default build is green.

## What this census can and cannot see

It counts what `--opt-report` (#6952) records, which is the set of analyses in
`perry_codegen::opt_report::Analysis::ALL`. Two things are deliberately out of
scope and are **not** silently reported as zero:

- The masked-window / buffer-view `TaPtr` *region* machinery
  (`stmt/masked_window_region.rs`) is region-shaped rather than a per-value
  promotion, and has no `opt_report` analysis. `spec-abi-taptr-slot` covers
  `TaPtr` only in its specialized-ABI *parameter* form.
- Denials are recorded as context (`candidates`) but never gated: the count of
  values a rule rejected is a property of the corpus, not of the compiler.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import tempfile
from pathlib import Path
from typing import Any, Iterable

from .capture import resolve_perry
from .common import HarnessError, REPO_ROOT, run_command, utc_now


DEFAULT_BASELINE = REPO_ROOT / "benchmarks/repsel_census/baseline.json"

BASELINE_SCHEMA_VERSION = 1

#: The `--opt-report` JSON schema this census understands. A bump upstream must
#: be a loud failure here, not a silently-empty census.
SUPPORTED_REPORT_SCHEMA = 2

#: Every analysis the report is expected to enumerate. Kept in lockstep with
#: `perry_codegen::opt_report::Analysis::ALL`; a missing row means the compiler
#: stopped emitting explicit zeros and the census can no longer tell "zero"
#: from "absent".
EXPECTED_ANALYSES = (
    "ptr-shape",
    "ptr-numarray",
    "canonical-slot",
    "int-valued-ta",
    "spec-abi",
)

#: Census keys, one per representation, in report order.
#:
#: `ptr-shape-consumed` is a SEPARATE key from `ptr-shape`, deliberately. See
#: [`CONSUMPTION_INSTRUMENTED`]: `ptr-shape` keeps its old meaning (the analysis
#: proved this many values) and its old ratcheted floors, which must not be
#: reinterpreted retroactively. The new key answers the different question.
CENSUS_KEYS: tuple[str, ...] = (
    "ptr-shape",
    "ptr-shape-consumed",
    "ptr-numarray",
    "canonical-i32",
    "canonical-u32",
    "canonical-str",
    "int-valued-ta",
    "spec-abi-entry",
    "spec-abi-taptr-slot",
)

#: Analyses whose CONSUMPTION is instrumented in the compiler, mapped to their
#: census key. **Held in code, never in the baseline.**
#:
#: A promotion is `selected` when an analysis proves a value. It is `consumed`
#: only when codegen goes on to emit the representation-specific form for it.
#: The two were conflated until #7107 read the emitted IR by hand and found
#: that `batch.ts` reports two `Ptr<Shape>` promotions and applies exactly one:
#: `totals` is proven, reported as a win, and keeps the guarded diamond at
#: every access site. The whole 1,532-byte binary saving came from the other.
#:
#: This table is why the census does not simply grow a `-consumed` column for
#: every representation. An uninstrumented analysis would report `consumed: 0`,
#: which is indistinguishable from "instrumented and never applied" — the exact
#: ambiguity this census exists to remove, reintroduced one level down. So
#: `consumed` is reported ONLY for analyses that record it, and
#: [`check_consumption_instrumentation`] fails if the compiler starts emitting
#: consumption for an analysis this table does not know about.
CONSUMPTION_INSTRUMENTED: dict[str, str] = {
    "ptr-shape": "ptr-shape-consumed",
}

#: `SlotRep` debug spelling -> census key, for the `canonical-slot` analysis.
CANONICAL_REPS = {
    "I32": "canonical-i32",
    "U32": "canonical-u32",
    "Str": "canonical-str",
}

#: `SpecParamRep::label()` spelling for a `TaPtr` slot: `ta<kind>` or
#: `ta<kind>x<len>`.
TAPTR_LABEL = re.compile(r"^ta\d+(?:x-?\d+)?$")

#: Liveness minimums for the hand-written fixtures, **held in code on purpose**.
#:
#: The baseline JSON is regenerable from observation; these are not. If a
#: change breaks a representation and someone re-runs `--update`, the baseline
#: floors follow the breakage down to zero — and a zero floor can never fail
#: again. These minimums are the backstop: `--update` refuses to write a
#: fixture floor below them, and `--gate` checks them independently of the
#: baseline file.
#:
#: Each entry says: *this program, compiled by this compiler, must promote at
#: least this many values of this representation.* That is the assertion that
#: the instrument is alive, separate from any claim about real-world code.
LIVENESS_FLOORS: dict[str, dict[str, int]] = {
    # The consumed minimum is what makes the consumption counter falsifiable.
    # `ptr-shape: 1` alone cannot fail when the counter is dead, and a
    # `ptr-shape-consumed` floor of 0 could never go red either -- which is the
    # state every non-fixture workload in this corpus is genuinely in. The
    # fixture's `p` is a function-body local with an in-loop field store, so it
    # is consumed; verified against emitted IR, not against this counter.
    "fixture_ptr_shape": {"ptr-shape": 1, "ptr-shape-consumed": 1},
    "fixture_ptr_numarray": {"ptr-numarray": 1},
    "fixture_canonical_slots": {
        "canonical-i32": 1,
        "canonical-u32": 1,
        "canonical-str": 1,
    },
    "fixture_int_valued_ta": {"int-valued-ta": 1},
    "fixture_spec_abi_taptr": {"spec-abi-entry": 1, "spec-abi-taptr-slot": 1},
}

#: Workloads allowed to produce **zero candidates** — no analysis considered any
#: value in them. Held in code for the same reason as [`LIVENESS_FLOORS`]: it is
#: an assertion about the compiler, and `--update` must not be able to widen it.
#:
#: A promotion count of zero is a fact about the corpus. Zero *candidates* is a
#: fact about the compiler: it says no analysis reached the program at all, so
#: there is no rule to point at and nothing to argue with. Before #7106's
#: follow-up, **8 of the 18 real workloads were in that state** — every one
#: because its hot loop is at module top level, which `codegen/entry.rs`
#: excludes from canonical selection before any per-value rule runs. The
#: promotion counts were identical either way, which is exactly why the census
#: alone could not see it.
#:
#: An entry here must name a program that genuinely has nothing to analyse.
ZERO_CANDIDATE_ALLOWLIST: dict[str, str] = {
    "suite_01_startup": (
        'a single `console.log("started")` — the program declares no bindings '
        "at all, so there is legitimately nothing for any representation "
        "analysis to consider"
    ),
}


# ── Report -> census ───────────────────────────────────────────────────────


def census_from_report(report: dict[str, Any]) -> dict[str, Any]:
    """Reduce one `--opt-report=json` payload to per-representation counts.

    Pure and total: every key in [`CENSUS_KEYS`] is present in the result even
    when it is zero. Raises rather than guessing when the payload does not look
    like the schema this census was written against — a census that quietly
    degrades to all-zeros is the exact failure this exists to prevent.
    """
    schema = report.get("schema_version")
    if schema != SUPPORTED_REPORT_SCHEMA:
        raise HarnessError(
            f"--opt-report schema_version {schema!r} is not the supported "
            f"{SUPPORTED_REPORT_SCHEMA}; the census must be updated deliberately "
            "rather than silently reporting zeros"
        )

    rows = report.get("summary", {}).get("by_analysis")
    if not isinstance(rows, list):
        raise HarnessError("--opt-report JSON has no summary.by_analysis list")
    by_analysis = {row["analysis"]: row for row in rows if isinstance(row, dict)}
    missing = [a for a in EXPECTED_ANALYSES if a not in by_analysis]
    if missing:
        raise HarnessError(
            "--opt-report omitted analyses "
            f"{missing} from summary.by_analysis. Every analysis must render an "
            "explicit row (perry_codegen::opt_report::Analysis::ALL) — an absent "
            "key and a zero key are indistinguishable to this census."
        )

    entries = report.get("entries")
    if not isinstance(entries, list):
        raise HarnessError("--opt-report JSON has no entries list")

    missing_consumption = [
        a for a in CONSUMPTION_INSTRUMENTED if "consumed" not in by_analysis.get(a, {})
    ]
    if missing_consumption:
        raise HarnessError(
            f"--opt-report omitted the `consumed` tally for {missing_consumption}. "
            "The census counts consumption, not selection; a report that cannot "
            "distinguish them is the instrument this census replaced."
        )

    counts = {key: 0 for key in CENSUS_KEYS}
    counts["ptr-shape"] = int(by_analysis["ptr-shape"]["selected"])
    counts["ptr-numarray"] = int(by_analysis["ptr-numarray"]["selected"])
    counts["int-valued-ta"] = int(by_analysis["int-valued-ta"]["selected"])
    counts["spec-abi-entry"] = int(by_analysis["spec-abi"]["selected"])

    unknown_canonical: set[str] = set()
    for entry in entries:
        if entry.get("outcome") != "selected":
            continue
        analysis = entry.get("analysis")
        rep = entry.get("rep") or ""
        if analysis == "canonical-slot":
            key = CANONICAL_REPS.get(rep)
            if key is None:
                unknown_canonical.add(rep)
                continue
            counts[key] += 1
        elif analysis == "spec-abi":
            counts["spec-abi-taptr-slot"] += sum(
                1 for label in rep.split(",") if TAPTR_LABEL.match(label.strip())
            )
    if unknown_canonical:
        raise HarnessError(
            f"canonical-slot reported unknown representation(s) {sorted(unknown_canonical)}; "
            "a new SlotRep variant needs a census key in CANONICAL_REPS, otherwise "
            "its promotions vanish from the census"
        )

    # Consumption, counted from ENTRIES rather than the summary tally, and only
    # for values in a `local` position. Phase 5a's proven-`this` receiver is a
    # real consumption of the same representation but was never `select()`ed, so
    # folding it in here would make `consumed` exceed `selected` and quietly
    # break the one invariant that says the two columns describe the same
    # population. It is reported separately instead (`consumed_receiver`).
    consumed_receiver = 0
    unconsumed_mechanisms: dict[str, int] = {}
    seen_consumed: set[tuple[str, Any]] = set()
    unknown_consumed: set[str] = set()
    for entry in entries:
        analysis = entry.get("analysis")
        outcome = entry.get("outcome")
        if outcome == "consumed":
            if analysis not in CONSUMPTION_INSTRUMENTED:
                unknown_consumed.add(str(analysis))
                continue
            if entry.get("position") != "local":
                consumed_receiver += 1
                continue
            # One value consumed at five access sites is ONE consumed value.
            key = (str(analysis), entry.get("local_id"), entry.get("function"))
            if key in seen_consumed:
                continue
            seen_consumed.add(key)
            counts[CONSUMPTION_INSTRUMENTED[analysis]] += 1
        elif outcome == "unconsumed":
            rule = str(entry.get("rule") or "<unnamed>")
            unconsumed_mechanisms[rule] = unconsumed_mechanisms.get(rule, 0) + 1
    if unknown_consumed:
        raise HarnessError(
            f"--opt-report recorded consumption for analysis/analyses "
            f"{sorted(unknown_consumed)}, which CONSUMPTION_INSTRUMENTED does not "
            "know about. Add a census key for it: an instrumented analysis whose "
            "consumption is not counted is a promotion the census still cannot "
            "tell apart from a wasted one."
        )

    canonical_total = sum(counts[k] for k in CANONICAL_REPS.values())
    reported = int(by_analysis["canonical-slot"]["selected"])
    if canonical_total != reported:
        raise HarnessError(
            f"canonical-slot tally disagrees with its entries: summary says "
            f"{reported} selected, entries account for {canonical_total}"
        )

    candidates = {
        row: int(by_analysis[row]["selected"]) + int(by_analysis[row]["denied"])
        for row in EXPECTED_ANALYSES
    }
    return {
        "counts": counts,
        "candidates": candidates,
        "unconsumed_mechanisms": unconsumed_mechanisms,
        "consumed_receiver": consumed_receiver,
    }


# ── Running the compiler ───────────────────────────────────────────────────


def compile_and_census(
    perry: list[str],
    source: Path,
    *,
    timeout: int,
    extra_env: dict[str, str] | None = None,
    keep_report: Path | None = None,
) -> dict[str, Any]:
    """Compile `source` with `--opt-report=json --no-link` and reduce it.

    `--no-link` on purpose: the report is produced during codegen, so the
    census never needs `libperry_runtime.a` and cannot be fooled by a stale
    one. `--no-cache` is redundant (`--opt-report` forces it) but stated so the
    intent survives a change upstream.
    """
    if not source.exists():
        raise HarnessError(f"census source not found: {source}")
    env = dict(os.environ)
    env.pop("PERRY_OPT_REPORT", None)
    env["PERRY_NO_AUTO_OPTIMIZE"] = "1"
    if extra_env:
        env.update(extra_env)
    with tempfile.TemporaryDirectory(prefix="repsel-census-") as tmp:
        tmpdir = Path(tmp)
        cmd = perry + [
            "compile",
            str(source),
            "-o",
            str(tmpdir / "census.o"),
            "--opt-report=json",
            "--no-link",
            "--no-cache",
        ]
        try:
            result = run_command(
                cmd,
                cwd=tmpdir,
                env=env,
                timeout=timeout,
                check=True,
            )
        except OSError as exc:
            # A missing/unusable compiler must exit 2 (harness error), NOT 1
            # (gate verdict). CI's sabotage step asserts on exit 1 plus the
            # reason string precisely so a broken toolchain can never be
            # mistaken for "the census correctly went red".
            raise HarnessError(f"could not run the compiler {perry[0]!r}: {exc}") from exc
    payload = _extract_json(result.stderr)
    if keep_report is not None:
        keep_report.parent.mkdir(parents=True, exist_ok=True)
        keep_report.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    census = census_from_report(payload)
    census["source"] = str(source.relative_to(REPO_ROOT))
    return census


def _extract_json(stderr: str) -> dict[str, Any]:
    """Pull the report object out of the compiler's stderr.

    The report is written to stderr so it cannot contaminate a `--format json`
    stdout payload; other diagnostics share that stream, so locate the object
    rather than parsing the whole buffer.
    """
    start = stderr.find('{\n  "schema_version"')
    if start < 0:
        start = stderr.find('{"schema_version"')
    if start < 0:
        raise HarnessError(
            "no --opt-report JSON object found on the compiler's stderr. "
            "Either codegen ran for no module (an object-cache hit that "
            "--opt-report failed to suppress) or the flag was not honoured.\n"
            f"stderr tail:\n{stderr[-2000:]}"
        )
    decoder = json.JSONDecoder()
    try:
        payload, _ = decoder.raw_decode(stderr[start:])
    except json.JSONDecodeError as exc:  # pragma: no cover - defensive
        raise HarnessError(f"could not decode the --opt-report JSON: {exc}") from exc
    return payload


# ── Baseline ───────────────────────────────────────────────────────────────


def load_baseline(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise HarnessError(
            f"census baseline not found: {path}. Generate one with\n"
            "  python3 scripts/compiler_output_regression.py census --update"
        )
    data = json.loads(path.read_text(encoding="utf-8"))
    if int(data.get("schema_version", 0)) != BASELINE_SCHEMA_VERSION:
        raise HarnessError(
            f"census baseline schema_version must be {BASELINE_SCHEMA_VERSION}"
        )
    workloads = data.get("workloads")
    if not isinstance(workloads, list) or not workloads:
        raise HarnessError("census baseline must list workloads")
    seen: set[str] = set()
    for workload in workloads:
        name = workload.get("name")
        if not name:
            raise HarnessError("every census workload needs a name")
        if name in seen:
            raise HarnessError(f"duplicate census workload {name!r}")
        seen.add(name)
        if not workload.get("source"):
            raise HarnessError(f"census workload {name!r} needs a source")
        floors = workload.get("floors")
        if not isinstance(floors, dict):
            raise HarnessError(f"census workload {name!r} needs a floors table")
        unknown = set(floors) - set(CENSUS_KEYS)
        if unknown:
            raise HarnessError(
                f"census workload {name!r} has unknown floor key(s) {sorted(unknown)}"
            )
    missing_fixtures = set(LIVENESS_FLOORS) - seen
    if missing_fixtures:
        raise HarnessError(
            f"census baseline is missing liveness fixture(s) {sorted(missing_fixtures)}. "
            "The fixtures are what prove the census can observe a promotion at all; "
            "dropping one from the baseline silently disarms the gate."
        )
    return data


# ── Verdicts ───────────────────────────────────────────────────────────────


def check_workload(
    name: str, floors: dict[str, int], counts: dict[str, int]
) -> tuple[list[str], list[str]]:
    """Return `(regressions, improvements)` for one workload."""
    regressions: list[str] = []
    improvements: list[str] = []
    for key in CENSUS_KEYS:
        floor = int(floors.get(key, 0))
        observed = int(counts.get(key, 0))
        if observed < floor:
            regressions.append(
                f"{name}: {key} promoted {observed}, floor is {floor} "
                f"(-{floor - observed})"
            )
        elif observed > floor:
            improvements.append(
                f"{name}: {key} promoted {observed}, floor is {floor} (+{observed - floor})"
            )
    return regressions, improvements


def check_liveness_fixtures(observed: dict[str, dict[str, Any]]) -> list[str]:
    """Every fixture must promote what it was written to promote.

    Independent of the baseline file on purpose — see [`LIVENESS_FLOORS`].
    """
    failures: list[str] = []
    for name, minimums in LIVENESS_FLOORS.items():
        if name not in observed:
            failures.append(
                f"liveness fixture {name!r} did not run; the census cannot claim "
                "to observe promotions it never measured"
            )
            continue
        counts = observed[name]["counts"]
        for key, minimum in minimums.items():
            if int(counts.get(key, 0)) < minimum:
                failures.append(
                    f"liveness fixture {name!r} promoted {counts.get(key, 0)} "
                    f"{key} value(s); it is written to promote at least {minimum}. "
                    "Either the representation stopped firing or the census counter "
                    "for it is dead."
                )
    return failures


def check_instrument_liveness(observed: dict[str, dict[str, Any]]) -> list[str]:
    """No census key may read zero across the ENTIRE corpus.

    A key that is zero everywhere is indistinguishable from a counter nobody
    increments — which is literally what `Ptr<NumArray>` was before #7106: an
    `Analysis` variant with a `target_rep` string and no `select()` call site
    anywhere in the tree.
    """
    totals = {key: 0 for key in CENSUS_KEYS}
    for entry in observed.values():
        for key in CENSUS_KEYS:
            totals[key] += int(entry["counts"].get(key, 0))
    return [
        f"census key {key!r} is zero across every workload in the corpus. "
        "That is either a dead counter or a representation that no longer "
        "fires anywhere; both are regressions."
        for key in CENSUS_KEYS
        if totals[key] == 0
    ]


def check_consumption_invariant(observed: dict[str, dict[str, Any]]) -> list[str]:
    """`consumed` may never exceed `selected` for the same representation.

    Not a style rule — it is the assertion that the two columns describe one
    population. If consumption is ever recorded for a value that was never
    selected (Phase 5a's proven-`this` receiver is exactly such a value), the
    consumed column stops meaning "of the promotions we counted, this many were
    applied" and starts meaning nothing in particular, while still looking like
    an improvement.
    """
    failures: list[str] = []
    for name, entry in sorted(observed.items()):
        counts = entry["counts"]
        for analysis, consumed_key in CONSUMPTION_INSTRUMENTED.items():
            selected = int(counts.get(analysis, 0))
            consumed = int(counts.get(consumed_key, 0))
            if consumed > selected:
                failures.append(
                    f"{name}: {consumed_key} is {consumed} but only {selected} "
                    f"{analysis} value(s) were selected. Consumption is being counted "
                    "for values outside the selected population, so the column no "
                    "longer means what its name says."
                )
    return failures


def check_unconsumed_is_explained(observed: dict[str, dict[str, Any]]) -> list[str]:
    """A workload with wasted promotions must be able to NAME a mechanism.

    `selected > consumed` says the compiler proved values and emitted nothing
    for them. That on its own is a number; it is not yet information. The
    mechanism recorders (`module_init_context`, `scalar_replaced`, …) are what
    turn it into something a reader can act on or argue with.

    Without this check, deleting a mechanism recorder is invisible: the
    consumed column is unchanged, the floors still pass, and the census goes
    green having lost the only part of the finding that says WHY. That is
    CLAUDE.md failure mode 4 — the gate runs but its subject did not.

    A residue is allowed: `selected - consumed` may exceed the named
    mechanisms, because a promotion with no access site at all is dropped by
    nobody. What is not allowed is wasted promotions and ZERO named mechanisms.
    """
    failures: list[str] = []
    for name, entry in sorted(observed.items()):
        counts = entry["counts"]
        wasted = sum(
            int(counts.get(a, 0)) - int(counts.get(k, 0))
            for a, k in CONSUMPTION_INSTRUMENTED.items()
        )
        if wasted <= 0:
            continue
        if sum(int(v) for v in entry.get("unconsumed_mechanisms", {}).values()) > 0:
            continue
        failures.append(
            f"{name}: {wasted} selected promotion(s) were not consumed, and not one "
            "of them names a mechanism. A wasted promotion with no rule attached is "
            "the state this census was built to end: it reads exactly like an honest "
            "zero. Either a mechanism recorder was removed, or a new way to drop a "
            "proof exists and needs one."
        )
    return failures


def check_analysis_reach(observed: dict[str, dict[str, Any]]) -> list[str]:
    """Every corpus workload must be REACHED by at least one analysis.

    The census counts promotions, so it can say "this workload promoted
    nothing". It cannot, on its own, say whether that is because every rule
    considered the values and said no, or because no rule ever ran. A denial
    names a rule and can be argued with; zero candidates names nothing.

    That distinction is not academic: it is how the module-init exclusion in
    `codegen/entry.rs` stayed invisible through #7034 and #7104. Both readings
    produce the identical promotion table.

    So: a workload whose candidate total is zero across every analysis fails,
    unless it is in [`ZERO_CANDIDATE_ALLOWLIST`] with a stated reason.
    """
    failures: list[str] = []
    for name, entry in sorted(observed.items()):
        candidates = entry.get("candidates", {})
        if sum(int(v) for v in candidates.values()) > 0:
            continue
        if name in ZERO_CANDIDATE_ALLOWLIST:
            continue
        failures.append(
            f"{name}: zero candidates across every analysis — no representation "
            "analysis considered a single value in this workload. That is not "
            '"considered and denied", it is "never looked at", and it is the '
            "state the census cannot distinguish from an honest zero. Either "
            "an analysis regressed, or the workload genuinely has nothing to "
            "analyse and belongs in ZERO_CANDIDATE_ALLOWLIST with a reason."
        )
    return failures


# ── Rendering ──────────────────────────────────────────────────────────────


def render_table(
    baseline: dict[str, Any], observed: dict[str, dict[str, Any]]
) -> str:
    head = ["workload"] + list(CENSUS_KEYS)
    rows = [head]
    for workload in baseline["workloads"]:
        name = workload["name"]
        if name not in observed:
            continue
        counts = observed[name]["counts"]
        floors = workload.get("floors", {})
        cells = [name]
        for key in CENSUS_KEYS:
            got = int(counts.get(key, 0))
            floor = int(floors.get(key, 0))
            cells.append(str(got) if got == floor else f"{got} (floor {floor})")
        rows.append(cells)
    totals = ["TOTAL"] + [
        str(sum(int(e["counts"].get(key, 0)) for e in observed.values()))
        for key in CENSUS_KEYS
    ]
    rows.append(totals)
    widths = [max(len(row[i]) for row in rows) for i in range(len(head))]
    lines = []
    for index, row in enumerate(rows):
        lines.append("  ".join(cell.ljust(widths[i]) for i, cell in enumerate(row)).rstrip())
        if index == 0:
            lines.append("  ".join("-" * w for w in widths))
    return "\n".join(lines)


def render_consumption_report(
    baseline: dict[str, Any], observed: dict[str, dict[str, Any]]
) -> str:
    """Selected vs CONSUMED, and the named mechanism for every wasted promotion.

    This is the honest version of the promotion table. `selected` counts
    `select()` calls; `consumed` counts values codegen actually emitted the
    representation for. Where they differ, the difference is the compiler
    proving things it then throws away.
    """
    lines = []
    for analysis, consumed_key in CONSUMPTION_INSTRUMENTED.items():
        selected = sum(int(e["counts"].get(analysis, 0)) for e in observed.values())
        consumed = sum(int(e["counts"].get(consumed_key, 0)) for e in observed.values())
        lines.append(
            f"  {analysis:<22} {selected} selected, {consumed} consumed "
            f"({selected - consumed} proven and thrown away)"
        )
    mechanisms: dict[str, int] = {}
    receiver = 0
    for entry in observed.values():
        for rule, n in entry.get("unconsumed_mechanisms", {}).items():
            mechanisms[rule] = mechanisms.get(rule, 0) + int(n)
        receiver += int(entry.get("consumed_receiver", 0))
    if mechanisms:
        lines.append("  mechanisms that dropped a selected promotion:")
        for rule, n in sorted(mechanisms.items(), key=lambda kv: (-kv[1], kv[0])):
            lines.append(f"    {rule:<24} {n}")
    if receiver:
        lines.append(
            f"  (plus {receiver} consumption(s) of a proven `this` receiver, which is "
            "never counted as a selection at all — see CONSUMPTION_INSTRUMENTED)"
        )
    uninstrumented = [k for k in CENSUS_KEYS if k not in CONSUMPTION_INSTRUMENTED
                      and not k.endswith("-consumed")]
    lines.append(
        "  NOT INSTRUMENTED (no consumption data, reported as absent not as zero): "
        + ", ".join(uninstrumented)
    )
    return "\n".join(lines)


def render_zero_report(
    baseline: dict[str, Any], observed: dict[str, dict[str, Any]]
) -> str:
    """State plainly which representations promote nothing on real code.

    This is the #7034 finding, printed on every run so it stops being something
    a person has to rediscover by hand-instrumenting the compiler.
    """
    real = [
        workload["name"]
        for workload in baseline["workloads"]
        if workload.get("role") != "liveness" and workload["name"] in observed
    ]
    lines = []
    for key in CENSUS_KEYS:
        promoting = [n for n in real if int(observed[n]["counts"].get(key, 0)) > 0]
        lines.append(
            f"  {key:<22} promotes in {len(promoting)}/{len(real)} non-fixture workload(s)"
            + (f": {', '.join(promoting)}" if promoting else "")
        )
    return "\n".join(lines)


# ── CLI ────────────────────────────────────────────────────────────────────


def _resolve_source(rel: str) -> Path:
    path = Path(rel)
    return path if path.is_absolute() else REPO_ROOT / path


def _selected_workloads(
    baseline: dict[str, Any], only: Iterable[str] | None
) -> list[dict[str, Any]]:
    workloads = baseline["workloads"]
    if not only:
        return workloads
    wanted = set(only)
    picked = [w for w in workloads if w["name"] in wanted]
    unknown = wanted - {w["name"] for w in picked}
    if unknown:
        raise HarnessError(f"unknown census workload(s): {sorted(unknown)}")
    return picked


def census(args: argparse.Namespace) -> int:
    baseline_path = Path(args.baseline) if args.baseline else DEFAULT_BASELINE
    baseline = load_baseline(baseline_path)
    perry = resolve_perry(args.perry)
    workloads = _selected_workloads(baseline, args.workload)
    keep_dir = Path(args.keep_reports).resolve() if args.keep_reports else None

    extra_env: dict[str, str] = {}
    for pair in args.env or []:
        key, _, value = pair.partition("=")
        extra_env[key] = value

    observed: dict[str, dict[str, Any]] = {}
    for workload in workloads:
        name = workload["name"]
        source = _resolve_source(workload["source"])
        observed[name] = compile_and_census(
            perry,
            source,
            timeout=args.compile_timeout,
            extra_env=extra_env,
            keep_report=(keep_dir / f"{name}.json") if keep_dir else None,
        )

    if args.update:
        return _update(baseline, baseline_path, observed, workloads)

    print("Representation-selection promotion census (#7106)")
    print("=================================================\n")
    print(render_table(baseline, observed))
    print("\nSelected vs consumed")
    print("--------------------")
    print(render_consumption_report(baseline, observed))
    print("\nPromotion coverage on non-fixture workloads")
    print("------------------------------------------")
    print(render_zero_report(baseline, observed))
    print()

    partial = len(workloads) != len(baseline["workloads"])
    regressions: list[str] = []
    improvements: list[str] = []
    for workload in workloads:
        name = workload["name"]
        reg, imp = check_workload(name, workload.get("floors", {}), observed[name]["counts"])
        regressions += reg
        improvements += imp
    liveness = check_liveness_fixtures(observed) if not partial else []
    dead = check_instrument_liveness(observed) if not partial else []
    unreached = check_analysis_reach(observed) if not partial else []
    # Always checked, even for a --workload subset: it is an internal
    # consistency assertion about the counter, not a corpus-wide claim.
    invariant = check_consumption_invariant(observed)
    unexplained = check_unconsumed_is_explained(observed)

    if partial:
        print(
            "NOTE: --workload was given, so the corpus-wide liveness assertions were\n"
            "      skipped. This run cannot be used as a gate.\n"
        )

    if improvements:
        print("Improvements (advisory — re-run with --update to ratchet):")
        for line in improvements:
            print(f"  {line}")
        print()

    failed = False
    for label, problems in (
        ("REGRESSION", regressions),
        ("DEAD INSTRUMENT", liveness + dead),
        ("UNREACHED BY EVERY ANALYSIS", unreached),
        ("CONSUMPTION COUNTER IS INCOHERENT", invariant),
        ("WASTED PROMOTION WITH NO NAMED MECHANISM", unexplained),
    ):
        if not problems:
            continue
        failed = True
        print(f"{label}:")
        for line in problems:
            print(f"  {line}")
        print()

    if failed and args.gate:
        print(
            "Census FAILED. If the drop is intentional, say so explicitly by\n"
            "re-running with --update and justifying the new floors in the PR —\n"
            "but note that LIVENESS_FLOORS in scripts/compiler_output_harness/\n"
            "repsel_census.py cannot be lowered that way, by design."
        )
        return 1
    if failed:
        print("(--gate not passed; reporting only)")
        return 0
    print("Census OK.")
    return 0


def _update(
    baseline: dict[str, Any],
    path: Path,
    observed: dict[str, dict[str, Any]],
    workloads: list[dict[str, Any]],
) -> int:
    for workload in workloads:
        name = workload["name"]
        counts = observed[name]["counts"]
        minimums = LIVENESS_FLOORS.get(name, {})
        below = {
            key: (counts.get(key, 0), minimum)
            for key, minimum in minimums.items()
            if int(counts.get(key, 0)) < minimum
        }
        if below:
            detail = ", ".join(
                f"{key}: observed {got}, minimum {want}" for key, (got, want) in below.items()
            )
            raise HarnessError(
                f"refusing to write a baseline for liveness fixture {name!r}: {detail}.\n"
                "LIVENESS_FLOORS is the backstop that stops a broken build from being\n"
                "re-baselined into a permanently-green gate. Fix the compiler (or the\n"
                "fixture, if the source genuinely no longer exercises the rep) instead."
            )
        workload["floors"] = {key: int(counts.get(key, 0)) for key in CENSUS_KEYS}
        workload["candidates"] = observed[name]["candidates"]
        # Context, never gated: which mechanism ate each wasted promotion.
        workload["unconsumed_mechanisms"] = observed[name].get("unconsumed_mechanisms", {})
    baseline["generated_at"] = utc_now()
    path.write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {path.relative_to(REPO_ROOT)} ({len(workloads)} workload(s)).")
    return 0


# ── Self-test ──────────────────────────────────────────────────────────────


def self_test(_args: argparse.Namespace) -> int:
    """Prove the verdict logic can go red, without compiling anything.

    Deliberately asserts the *failing* direction first: a gate whose only test
    is that it passes on a good tree is a gate nobody has watched fail.
    """
    report = {
        "schema_version": 2,
        "summary": {
            "selected": 3,
            "denied": 1,
            "by_analysis": [
                {
                    "analysis": a,
                    "target_rep": a,
                    "rule_source": "x",
                    "selected": s,
                    "denied": d,
                    "consumed": 0,
                    "unconsumed": 0,
                }
                for a, s, d in (
                    ("ptr-shape", 0, 1),
                    ("ptr-numarray", 1, 0),
                    ("canonical-slot", 2, 0),
                    ("int-valued-ta", 0, 0),
                    ("spec-abi", 0, 0),
                )
            ],
        },
        "entries": [
            {"analysis": "canonical-slot", "outcome": "selected", "rep": "I32"},
            {"analysis": "canonical-slot", "outcome": "selected", "rep": "Str"},
            {"analysis": "ptr-numarray", "outcome": "selected", "rep": "Ptr<NumArray>"},
            {"analysis": "ptr-shape", "outcome": "denied", "rep": "Boxed"},
        ],
    }
    result = census_from_report(report)
    counts = result["counts"]
    assert counts["ptr-shape"] == 0, counts
    assert counts["ptr-shape-consumed"] == 0, counts
    assert counts["canonical-i32"] == 1, counts
    assert counts["canonical-str"] == 1, counts
    assert counts["canonical-u32"] == 0, counts
    assert counts["ptr-numarray"] == 1, counts
    assert set(counts) == set(CENSUS_KEYS), counts

    regressions, improvements = check_workload("w", {"ptr-shape": 1}, counts)
    assert regressions, "a count below its floor must be a regression"
    regressions, improvements = check_workload("w", {"canonical-i32": 0}, counts)
    assert not regressions and improvements, "a count above its floor is an improvement"

    dead = check_instrument_liveness({"w": {"counts": counts}})
    assert any("ptr-shape" in d for d in dead), dead

    failures = check_liveness_fixtures({"fixture_ptr_shape": {"counts": counts}})
    assert any("fixture_ptr_shape" in f for f in failures), failures

    for broken, why in (
        ({"schema_version": 99}, "schema drift"),
        (
            {
                "schema_version": 1,
                "summary": {"by_analysis": [{"analysis": "ptr-shape", "selected": 0, "denied": 0}]},
                "entries": [],
            },
            "missing analyses",
        ),
    ):
        try:
            census_from_report(broken)
        except HarnessError:
            pass
        else:  # pragma: no cover - the assertion IS the test
            raise AssertionError(f"census_from_report accepted {why}")

    # ── The consumption column, asserted failing-direction first ───────────
    #
    # The whole point of this column is a promotion that is SELECTED and then
    # emitted nothing. Build exactly that report and check the census can see
    # it, because the pre-#7107 instrument could not.
    wasted = {
        "schema_version": 2,
        "summary": {
            "selected": 2,
            "denied": 0,
            "by_analysis": [
                {
                    "analysis": a,
                    "target_rep": a,
                    "rule_source": "x",
                    "selected": sel,
                    "denied": 0,
                    "consumed": con,
                    "unconsumed": unc,
                }
                for a, sel, con, unc in (
                    ("ptr-shape", 2, 1, 1),
                    ("ptr-numarray", 0, 0, 0),
                    ("canonical-slot", 0, 0, 0),
                    ("int-valued-ta", 0, 0, 0),
                    ("spec-abi", 0, 0, 0),
                )
            ],
        },
        "entries": [
            {"analysis": "ptr-shape", "outcome": "selected", "rep": "Ptr<Shape>"},
            {"analysis": "ptr-shape", "outcome": "selected", "rep": "Ptr<Shape>"},
            # `acc`: consumed at three access sites, but it is ONE value.
            {"analysis": "ptr-shape", "outcome": "consumed", "rep": "Ptr<Shape>",
             "position": "local", "local_id": 9, "function": "totalsRow"},
            {"analysis": "ptr-shape", "outcome": "consumed", "rep": "Ptr<Shape>",
             "position": "local", "local_id": 9, "function": "totalsRow"},
            {"analysis": "ptr-shape", "outcome": "consumed", "rep": "Ptr<Shape>",
             "position": "local", "local_id": 9, "function": "totalsRow"},
            # `totals`: proven, counted as a win, dropped by the context gate.
            {"analysis": "ptr-shape", "outcome": "unconsumed", "rep": "Ptr<Shape>",
             "position": "local", "local_id": 4, "function": "module_init",
             "rule": "module_init_context"},
            # A proven `this`, which was never selected: must NOT inflate the
            # consumed column, or the invariant below stops holding.
            {"analysis": "ptr-shape", "outcome": "consumed", "rep": "Ptr<Shape>",
             "position": "param", "local_id": None, "function": "C.m"},
        ],
    }
    result = census_from_report(wasted)
    counts = result["counts"]
    assert counts["ptr-shape"] == 2, counts
    assert counts["ptr-shape-consumed"] == 1, (
        "three access sites on ONE local must count as one consumed value, and a "
        "proven `this` must not count at all"
    )
    assert result["consumed_receiver"] == 1, result
    assert result["unconsumed_mechanisms"] == {"module_init_context": 1}, result

    # A floor on the consumed column must be able to go red while the SELECTED
    # column stays green. That is the regression the pre-#7107 census could not
    # express at all: `batch` selects 2 either way.
    regressions, _ = check_workload(
        "batch", {"ptr-shape": 2, "ptr-shape-consumed": 2}, counts
    )
    assert any("ptr-shape-consumed" in r for r in regressions), regressions
    regressions, _ = check_workload("batch", {"ptr-shape": 2}, counts)
    assert not regressions, "the selected column alone cannot see the drop"

    # Consumption recorded for a value that was never selected is incoherent.
    assert check_consumption_invariant(
        {"w": {"counts": {"ptr-shape": 0, "ptr-shape-consumed": 1}}}
    ), "consumed > selected must be a failure"
    assert not check_consumption_invariant({"w": {"counts": counts}})

    # An analysis that starts recording consumption without a census key must
    # be loud, not silently uncounted.
    rogue = json.loads(json.dumps(wasted))
    rogue["entries"].append(
        {"analysis": "canonical-slot", "outcome": "consumed", "rep": "I32",
         "position": "local", "local_id": 1, "function": "f"}
    )
    try:
        census_from_report(rogue)
    except HarnessError:
        pass
    else:  # pragma: no cover - the assertion IS the test
        raise AssertionError("uninstrumented consumption was silently dropped")

    # A report that cannot distinguish selection from consumption is the OLD
    # instrument, and must be rejected rather than read as "nothing consumed".
    v1 = json.loads(json.dumps(wasted))
    for row in v1["summary"]["by_analysis"]:
        del row["consumed"]
    try:
        census_from_report(v1)
    except HarnessError:
        pass
    else:  # pragma: no cover
        raise AssertionError("a report with no consumed tally was accepted")

    # A wasted promotion that names no mechanism must be red: that is what
    # deleting a drop-recorder looks like, and the consumed column alone
    # cannot see it.
    assert check_unconsumed_is_explained(
        {"w": {"counts": {"ptr-shape": 2, "ptr-shape-consumed": 1}, "unconsumed_mechanisms": {}}}
    ), "a wasted promotion with no named mechanism must fail"
    assert not check_unconsumed_is_explained(
        {
            "w": {
                "counts": {"ptr-shape": 2, "ptr-shape-consumed": 1},
                "unconsumed_mechanisms": {"module_init_context": 1},
            }
        }
    )
    assert not check_unconsumed_is_explained(
        {"w": {"counts": {"ptr-shape": 1, "ptr-shape-consumed": 1}, "unconsumed_mechanisms": {}}}
    ), "nothing wasted means nothing to explain"

    print("repsel census self-test OK")
    return 0
