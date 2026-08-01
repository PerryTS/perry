"""Representation-selection knob isolation gate (#7128).

Perry ships one bisection knob per unboxed representation, and every
representation-selection measurement to date has been read through them: build
twice, flip one knob, attribute the difference. **That attribution is a
non-sequitur unless the knob moves exactly one representation**, and twice it
did not:

* `PERRY_CANONICAL_I32_LOCALS=0` also switched off every `Ptr<Shape>`
  consumption in the program (`ptr-shape` went 7 selected / 3 consumed →
  7 selected / **0** consumed, with all six consumption sites printing
  `NEVER FIRES`). Two of the four workloads whose object moves under that knob
  were therefore measuring two representations at once.
* `PERRY_CANONICAL_STR_LOCALS=0` also switched off three string lowerings that
  never consult a selected `Str` local, so **24 of 26** census workloads
  emitted differently under it — including workloads whose `canonical-str`
  count is zero.

Neither defect was visible from the knob's name, from the census table, or from
any test. Both were found by measurement, after a day of A/B runs had already
been taken through them.

## What this gate asserts

For each knob `K`, with `K=0` and every other knob at its default:

1. **No cross-representation count leak.** Every census key *not* owned by `K`
   reads exactly what the default build reads, on every workload.
2. **No cross-representation emission leak.** A workload in which `K`'s
   representation promotes *nothing* must compile to a **byte-identical**
   object. This is the half that catches defect 2: it leaves every census count
   alone and still changes what ships.
3. **The knob is live** — corpus-wide it must take at least one owned count
   down AND change at least one object. A knob that moves nothing has stopped
   being an instrument (CLAUDE.md, "the gate runs but its subject never did").

Plus two controls, because a diff-based gate that cannot tell "different" from
"noisy" proves nothing:

* **determinism** — the same compiler, same flags, twice, must produce the same
  bytes. This used to be false on aarch64 Linux (the temp `.ll` name carried pid
  + nanotime and clang records a unit's source basename into the ELF object), so
  the gate detected the host and skipped the emission half there. #7131/#7135
  content-addressed that name; the skip is gone and a disagreement is now a
  hard failure on every host — see [`repsel_determinism`];
* **inert-variable** — `K=1` and an unrelated `PERRY_TOTALLY_UNRELATED=0` must
  both reproduce the default object bit-for-bit. If they do not, the diff
  signal is not attributable to the knob at all.

## Why the owned-signal table is not simply the census keys

Two representation sites exist that no census key counts, and both would make
rule 2 fire spuriously:

* a specialized-ABI entry's `i32` parameter slot is canonical-i32 storage
  (`codegen/function.rs`), but it is a parameter, so it is never `select()`ed
  as a canonical slot;
* a proven-`this` receiver is a `Ptr<Shape>` consumption that was never
  selected either (the census reports it as `consumed_receiver`).

So [`KNOB_SIGNALS`] names, per knob, everything that means "this representation
has a site here" — census keys plus those two derived quantities. Getting this
table wrong makes the gate red on a correct compiler, which is the failure mode
that gets a gate deleted; it is derived from the report, not guessed.
"""

from __future__ import annotations

import argparse
import hashlib
import platform
import re
import shutil
import tempfile
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .capture import resolve_perry
from .common import HarnessError, REPO_ROOT
from .repsel_census import (
    CENSUS_KEYS,
    DEFAULT_BASELINE,
    compile_and_census,
    load_baseline,
)
from .repsel_determinism import digest_objects, nondeterminism_report


#: `SpecParamRep::label()` spelling for a canonical-i32 parameter slot.
SPEC_I32_LABEL = re.compile(r"^i32$")

#: Derived signals, computed here rather than added to the census, so
#: `benchmarks/repsel_census/baseline.json` (which has conflicted repeatedly)
#: does not have to move for an instrument fix.
DERIVED_SIGNALS = ("spec-abi-i32-slot", "consumed-receiver")


@dataclass(frozen=True)
class Knob:
    """One bisection knob, and everything it is allowed to move."""

    env: str
    #: Census keys this knob owns. Empty for a knob that is not a
    #: representation at all (see `PERRY_STATIC_STRING_LOWERING`).
    keys: tuple[str, ...]
    #: Derived signals from [`DERIVED_SIGNALS`] that also mean "a site of this
    #: representation exists in this workload".
    signals: tuple[str, ...] = ()
    #: Keys this knob may legitimately take DOWN but never up, because the
    #: analysis it gates FEEDS them. Each entry carries the reason; an
    #: undocumented one is a leak, not a dependency.
    #:
    #: This is the one place the "a knob owns exactly one representation" rule
    #: bends, and it bends for a real reason: a representation whose proof is
    #: withdrawn cannot be selected. Allowing only the downward direction keeps
    #: it from becoming a licence — a knob that ADDS promotions of another
    #: representation is still a leak.
    downstream: tuple[tuple[str, str], ...] = ()
    #: What the knob is, for the report.
    what: str = ""

    @property
    def owned(self) -> tuple[str, ...]:
        return self.keys + self.signals + tuple(k for k, _ in self.downstream)


#: The knob table. Kept HERE and not in the baseline JSON, for the same reason
#: `LIVENESS_FLOORS` is: it is an assertion about the compiler, and no
#: `--update` path may widen it.
KNOBS: tuple[Knob, ...] = (
    Knob(
        "PERRY_CANONICAL_I32_LOCALS",
        ("canonical-i32", "canonical-u32"),
        ("spec-abi-i32-slot",),
        (),
        "repsel Phase 1 — canonical unboxed i32/u32 slots",
    ),
    Knob(
        "PERRY_CANONICAL_STR_LOCALS",
        ("canonical-str",),
        (),
        (),
        "repsel Phase 3a — canonical (tagged-at-rest) Str slots",
    ),
    Knob(
        "PERRY_PTR_SHAPE_LOCALS",
        ("ptr-shape", "ptr-shape-consumed"),
        ("consumed-receiver",),
        (),
        "repsel Phase 3b/5a — Ptr<Shape> receivers",
    ),
    Knob(
        "PERRY_PTR_NUMARRAY_LOCALS",
        ("ptr-numarray",),
        (),
        (),
        "repsel Phase 4a.3 — Ptr<NumArray> locals",
    ),
    Knob(
        "PERRY_INT_VALUED_LOCALS",
        ("int-valued-ta",),
        (),
        (
            (
                "canonical-i32",
                "`int_valued_ta_locals` is merged into `integer_locals` "
                "(`collectors/hir_facts.rs`), which is the candidate set canonical-i32 "
                "admission draws from. With the knob off the local is no longer PROVEN "
                "integer, so canonical-i32 cannot select it — a withdrawn proof, not a "
                "second representation being switched off. Measured on "
                "`fixture_int_valued_ta`: canonical-i32 3 -> 2.",
            ),
        ),
        "native-i32 residency for int-TA-seeded locals (#6898)",
    ),
    Knob(
        "PERRY_STATIC_STRING_LOWERING",
        (),
        (),
        (),
        "#7128 — string fast paths keyed on a value's STATIC string type. Not "
        "a representation: it owns no census key, so it must move no count at "
        "all, and rule 2 does not apply to it",
    ),
)

#: An env var the compiler must not read. The default object has to reproduce
#: bit-for-bit under it, or the object diff is measuring the environment rather
#: than the knob.
INERT_VAR = "PERRY_TOTALLY_UNRELATED"


@dataclass
class Arm:
    """One (workload, env) compile."""

    counts: dict[str, int]
    signals: dict[str, int]
    digest: str
    objects: list[str] = field(default_factory=list)


def _spec_abi_i32_slots(report: dict[str, Any]) -> int:
    """Count `i32` parameter slots across selected specialized-ABI entries.

    `codegen/function.rs` allocates those as canonical-i32 storage under
    `PERRY_CANONICAL_I32_LOCALS`, and reverts them to a boxed double slot when
    the knob is off. Nothing in the census counts them, so without this the
    gate would demand a byte-identical object for a workload that legitimately
    has canonical-i32 sites.
    """
    total = 0
    for entry in report.get("entries", []):
        if entry.get("analysis") != "spec-abi" or entry.get("outcome") != "selected":
            continue
        total += sum(1 for label in (entry.get("rep") or "").split(",") if SPEC_I32_LABEL.match(label.strip()))
    return total


def _compile_arm(
    perry: list[str],
    source: Path,
    env: dict[str, str],
    *,
    timeout: int,
    workdir: Path,
) -> Arm:
    census = compile_and_census(
        perry,
        source,
        timeout=timeout,
        extra_env=env,
        object_out=workdir / "out.o",
        with_report=True,
    )
    report = census["report"]
    return Arm(
        counts={key: int(census["counts"].get(key, 0)) for key in CENSUS_KEYS},
        signals={
            "spec-abi-i32-slot": _spec_abi_i32_slots(report),
            "consumed-receiver": int(census.get("consumed_receiver", 0)),
        },
        digest=digest_objects(census["objects"]),
        objects=list(census["objects"]),
    )


def _resolve_source(rel: str) -> Path:
    path = Path(rel)
    return path if path.is_absolute() else REPO_ROOT / path


def _arm_env(var: str | None, value: str = "0") -> dict[str, str]:
    return {} if var is None else {var: value}


def check_isolation(args: argparse.Namespace) -> int:
    baseline = load_baseline(Path(args.baseline) if args.baseline else DEFAULT_BASELINE)
    perry = resolve_perry(args.perry)
    workloads = baseline["workloads"]
    if args.workload:
        wanted = set(args.workload)
        workloads = [w for w in workloads if w["name"] in wanted]
        unknown = wanted - {w["name"] for w in workloads}
        if unknown:
            raise HarnessError(f"unknown census workload(s): {sorted(unknown)}")
    knobs = KNOBS
    if args.knob:
        wanted = set(args.knob)
        knobs = tuple(k for k in KNOBS if k.env in wanted)
        unknown = wanted - {k.env for k in knobs}
        if unknown:
            raise HarnessError(f"unknown knob(s): {sorted(unknown)}")

    print("Representation-selection knob isolation (#7128)")
    print("==============================================\n")
    print(f"compiler: {' '.join(perry)}")
    print(f"host:     {platform.system()} {platform.machine()}")
    print(f"corpus:   {len(workloads)} workload(s), {len(knobs)} knob(s)\n")

    tmp = Path(tempfile.mkdtemp(prefix="repsel-knob-iso-"))
    try:
        # ── arm plan ──────────────────────────────────────────────────────
        # `default` twice: the second copy is the determinism control.
        arm_names: list[tuple[str, dict[str, str]]] = [
            ("default", {}),
            ("default#2", {}),
            (f"inert:{INERT_VAR}=0", _arm_env(INERT_VAR)),
        ]
        for knob in knobs:
            arm_names.append((f"{knob.env}=0", _arm_env(knob.env)))
            arm_names.append((f"{knob.env}=1", _arm_env(knob.env, "1")))

        jobs: list[tuple[str, str, Path, dict[str, str], Path]] = []
        for workload in workloads:
            source = _resolve_source(workload["source"])
            for arm, env in arm_names:
                slug = re.sub(r"[^A-Za-z0-9]+", "_", arm)
                jobs.append(
                    (workload["name"], arm, source, env, tmp / workload["name"] / slug)
                )

        def run(job: tuple[str, str, Path, dict[str, str], Path]) -> tuple[tuple[str, str], Arm]:
            name, arm, source, env, workdir = job
            return (name, arm), _compile_arm(
                perry, source, env, timeout=args.compile_timeout, workdir=workdir
            )

        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            results: dict[tuple[str, str], Arm] = dict(pool.map(run, jobs))

        return _verdict(workloads, knobs, results)
    finally:
        if not args.keep_objects:
            shutil.rmtree(tmp, ignore_errors=True)
        else:
            print(f"\n(objects kept in {tmp})")


def _verdict(
    workloads: list[dict[str, Any]],
    knobs: tuple[Knob, ...],
    results: dict[tuple[str, str], Arm],
) -> int:
    names = [w["name"] for w in workloads]

    # ── control 1: determinism ────────────────────────────────────────────
    # Every object comparison below is void unless the compiler is a function
    # of its inputs. Until #7131/#7135 it was not on ELF, and this control
    # SKIPPED the emission half there — which meant the half of the gate that
    # caught the `PERRY_CANONICAL_STR_LOCALS` defect could not run on Linux at
    # all. The skip is gone: a disagreement here is a regression to fix, not a
    # host to route around (CLAUDE.md — a mode that still exists is a decision
    # that hasn't been made).
    nondeterministic = [
        n for n in names if results[(n, "default")].digest != results[(n, "default#2")].digest
    ]
    if nondeterministic:
        print(nondeterminism_report(nondeterministic, len(names)))
        print()
        print(
            "Knob isolation FAILED at its determinism control: nothing below it can "
            "be trusted, so no knob was judged."
        )
        return 1

    failures: list[str] = []
    notes: list[str] = []

    # ── control 2: an inert variable must not move the object ─────────────
    for n in names:
        if results[(n, f"inert:{INERT_VAR}=0")].digest != results[(n, "default")].digest:
            failures.append(
                f"CONTROL: {n} compiled differently with {INERT_VAR}=0 set, an env var "
                "the compiler does not read. The object diff below is not attributable "
                "to any knob."
            )
    for knob in knobs:
        for n in names:
            if results[(n, f"{knob.env}=1")].digest != results[(n, "default")].digest:
                failures.append(
                    f"CONTROL: {n} compiled differently with {knob.env}=1, which is the "
                    "default. The knob is keyed into codegen beyond its documented "
                    "off-state."
                )

    # ── rule 1 / rule 2, per knob ─────────────────────────────────────────
    rows: list[str] = []
    for knob in knobs:
        moved_counts = 0
        moved_objects = 0
        for n in names:
            base = results[(n, "default")]
            off = results[(n, f"{knob.env}=0")]

            lost = False
            downstream = dict(knob.downstream)
            for key in CENSUS_KEYS:
                if key in knob.keys:
                    lost = lost or off.counts[key] < base.counts[key]
                    continue
                if key in downstream:
                    # Only the downward direction, and only with a reason on
                    # record. An UPWARD move means the knob is creating
                    # promotions of another representation, which no proof
                    # dependency can explain.
                    if off.counts[key] > base.counts[key]:
                        failures.append(
                            f"COUNT LEAK: {knob.env}=0 RAISED {key} on {n} "
                            f"({base.counts[key]} -> {off.counts[key]}). A withdrawn proof "
                            "can only remove promotions; this knob is adding them."
                        )
                    continue
                if off.counts[key] != base.counts[key]:
                    failures.append(
                        f"COUNT LEAK: {knob.env}=0 changed {key} on {n} "
                        f"({base.counts[key]} -> {off.counts[key]}). That key belongs to a "
                        "different representation, so any A/B through this knob measures "
                        "more than one."
                    )
            for sig in DERIVED_SIGNALS:
                if sig in knob.signals:
                    lost = lost or off.signals[sig] < base.signals[sig]
                    continue
                if off.signals[sig] != base.signals[sig]:
                    failures.append(
                        f"COUNT LEAK: {knob.env}=0 changed {sig} on {n} "
                        f"({base.signals[sig]} -> {off.signals[sig]})."
                    )

            moved_counts += int(lost)
            differs = off.digest != base.digest
            moved_objects += int(differs)
            promotes = (
                sum(base.counts[k] for k in knob.keys)
                + sum(base.signals[s] for s in knob.signals)
                + sum(base.counts[k] for k, _ in knob.downstream)
            )
            # A knob that owns no census key is not a representation, so
            # "promotes nothing" is true of every workload and rule 2 would
            # forbid the knob from doing anything at all. Rule 1 (no count may
            # move) plus rule 3 (it must still be live) are what constrain it.
            if differs and promotes == 0 and knob.owned:
                failures.append(
                    f"EMISSION LEAK: {knob.env}=0 changed the emitted object on {n}, which "
                    f"promotes nothing this knob owns ({', '.join(knob.owned) or 'no census key'}"
                    " = 0). The knob is reaching sites outside its own representation."
                )
        rows.append(
            f"  {knob.env:<30} {moved_counts:>3} workload(s) lose a promotion, "
            f"{moved_objects:>3} change the object"
        )

        # ── rule 3: the knob must be an instrument ────────────────────────
        if knob.keys and moved_counts == 0:
            failures.append(
                f"DEAD KNOB: {knob.env}=0 took no promotion of {', '.join(knob.keys)} away "
                "anywhere in the corpus. Either the representation stopped firing or the "
                "knob no longer reaches it; both make every A/B through it vacuous."
            )
        if moved_objects == 0:
            failures.append(
                f"DEAD KNOB: {knob.env}=0 left every object in the corpus byte-identical. "
                "An arm that emits the same bytes as the default cannot be evidence about "
                "anything."
            )

    print("Per-knob effect")
    print("---------------")
    for row in rows:
        print(row)
    print()

    documented = [(k, key, why) for k in knobs for key, why in k.downstream]
    if documented:
        print("Documented proof dependencies (a knob may only LOWER these)")
        print("----------------------------------------------------------")
        for knob, key, why in documented:
            print(f"  {knob.env}=0 may lower {key}:")
            for line in _wrap(why):
                print(f"      {line}")
        print()

    if notes:
        for note in notes:
            print(note)

    if failures:
        print("FAILURES:")
        for line in failures:
            print(f"  {line}")
        print()
        print(
            "Knob isolation FAILED. A knob that moves a representation it does not name "
            "silently invalidates every measurement taken through it — see #7128."
        )
        return 1

    print("Knob isolation OK: every knob moves its own representation and nothing else.")
    return 0


def _wrap(text: str, width: int = 74) -> list[str]:
    import textwrap

    return textwrap.wrap(" ".join(text.split()), width=width)


def self_test(_args: argparse.Namespace) -> int:
    """Prove the verdict logic can go red, without compiling anything.

    Both defects #7128 fixed are replayed here as synthetic arm tables, so the
    two branches that catch them are exercised on every run rather than only on
    a host with a compiler.
    """
    workloads = [{"name": "w", "source": "x.ts"}, {"name": "v", "source": "y.ts"}]

    def arm(counts: dict[str, int], digest: str, **signals: int) -> Arm:
        full = {key: 0 for key in CENSUS_KEYS}
        full.update(counts)
        sig = {s: 0 for s in DERIVED_SIGNALS}
        sig.update(signals)
        return Arm(counts=full, signals=sig, digest=digest)

    i32 = next(k for k in KNOBS if k.env == "PERRY_CANONICAL_I32_LOCALS")
    strk = next(k for k in KNOBS if k.env == "PERRY_CANONICAL_STR_LOCALS")

    def table(default: dict[str, Arm], off: dict[str, Arm], knob: Knob) -> dict[tuple[str, str], Arm]:
        out: dict[tuple[str, str], Arm] = {}
        for name, a in default.items():
            out[(name, "default")] = a
            out[(name, "default#2")] = a
            out[(name, f"inert:{INERT_VAR}=0")] = a
            out[(name, f"{knob.env}=1")] = a
        for name, a in off.items():
            out[(name, f"{knob.env}=0")] = a
        return out

    # Defect A, exactly as measured: the i32 knob takes `ptr-shape-consumed`
    # from 1 to 0 while doing its own job on `canonical-i32`.
    leak = table(
        {
            "w": arm({"canonical-i32": 5, "ptr-shape": 2, "ptr-shape-consumed": 1}, "aa"),
            "v": arm({"canonical-i32": 2}, "bb"),
        },
        {
            "w": arm({"canonical-i32": 0, "ptr-shape": 2, "ptr-shape-consumed": 0}, "cc"),
            "v": arm({"canonical-i32": 0}, "dd"),
        },
        i32,
    )
    verdict = _capture(_verdict, workloads, (i32,), leak)
    assert verdict.code == 1, verdict.out
    assert "COUNT LEAK" in verdict.out and "ptr-shape-consumed" in verdict.out, verdict.out

    # Defect B: every count is untouched and the object still moves on a
    # workload that selects no Str local at all.
    emission = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0}, "cc"), "v": arm({}, "zz")},
        strk,
    )
    verdict = _capture(_verdict, workloads, (strk,), emission)
    assert verdict.code == 1, verdict.out
    assert "EMISSION LEAK" in verdict.out, verdict.out

    # The fixed shape: same counts elsewhere, object changes only where the
    # representation actually promotes.
    clean = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0}, "cc"), "v": arm({}, "bb")},
        strk,
    )
    verdict = _capture(_verdict, workloads, (strk,), clean)
    assert verdict.code == 0, verdict.out

    # A knob that moves nothing is dead, not clean.
    dead = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        strk,
    )
    verdict = _capture(_verdict, workloads, (strk,), dead)
    assert verdict.code == 1 and "DEAD KNOB" in verdict.out, verdict.out

    # An inert variable that moves the object means the diff is not the knob's.
    contaminated = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0}, "cc"), "v": arm({}, "bb")},
        strk,
    )
    contaminated[("v", f"inert:{INERT_VAR}=0")] = arm({}, "qq")
    verdict = _capture(_verdict, workloads, (strk,), contaminated)
    assert verdict.code == 1 and "CONTROL" in verdict.out, verdict.out

    # A nondeterministic compiler fails the gate outright (#7131). It used to
    # skip the emission half, which is how the half that caught defect B above
    # became unrunnable on Linux — the host where it mattered most. Note the
    # arms here are otherwise CLEAN: the determinism control must reject them on
    # its own, before any knob is judged.
    flaky = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0}, "cc"), "v": arm({}, "bb")},
        strk,
    )
    flaky[("w", "default#2")] = arm({"canonical-str": 1}, "AA")
    verdict = _capture(_verdict, workloads, (strk,), flaky)
    assert verdict.code == 1, verdict.out
    assert "NONDETERMINISTIC" in verdict.out.upper(), verdict.out
    assert "#7131" in verdict.out, verdict.out
    # …and it must stop there rather than reporting per-knob verdicts drawn
    # from bytes it just declared untrustworthy.
    assert "EMISSION LEAK" not in verdict.out and "DEAD KNOB" not in verdict.out, verdict.out

    # `PERRY_STATIC_STRING_LOWERING` owns no census key: it must move no count,
    # and rule 2 must NOT demand a byte-identical object of it.
    static = next(k for k in KNOBS if k.env == "PERRY_STATIC_STRING_LOWERING")
    assert static.keys == () and static.signals == ()
    ok = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 1}, "cc"), "v": arm({}, "dd")},
        static,
    )
    verdict = _capture(_verdict, workloads, (static,), ok)
    assert verdict.code == 0, verdict.out
    moved = table(
        {"w": arm({"canonical-str": 1}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0}, "cc"), "v": arm({}, "dd")},
        static,
    )
    verdict = _capture(_verdict, workloads, (static,), moved)
    assert verdict.code == 1 and "COUNT LEAK" in verdict.out, verdict.out

    # A documented proof dependency may lower the downstream key and only that.
    intk = next(k for k in KNOBS if k.env == "PERRY_INT_VALUED_LOCALS")
    assert dict(intk.downstream).get("canonical-i32"), "the dependency must carry a reason"
    down_ok = table(
        {"w": arm({"int-valued-ta": 1, "canonical-i32": 3}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"int-valued-ta": 0, "canonical-i32": 2}, "cc"), "v": arm({}, "bb")},
        intk,
    )
    verdict = _capture(_verdict, workloads, (intk,), down_ok)
    assert verdict.code == 0, verdict.out
    # …but never raise it. A knob that ADDS another representation's promotions
    # is a leak no withdrawn proof can explain.
    down_bad = table(
        {"w": arm({"int-valued-ta": 1, "canonical-i32": 3}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"int-valued-ta": 0, "canonical-i32": 4}, "cc"), "v": arm({}, "bb")},
        intk,
    )
    verdict = _capture(_verdict, workloads, (intk,), down_bad)
    assert verdict.code == 1 and "RAISED" in verdict.out, verdict.out
    # An UNDOCUMENTED cross-representation move is still a leak: the Str knob
    # has no dependency on canonical-i32, so the identical shape must go red.
    undocumented = table(
        {"w": arm({"canonical-str": 1, "canonical-i32": 3}, "aa"), "v": arm({}, "bb")},
        {"w": arm({"canonical-str": 0, "canonical-i32": 2}, "cc"), "v": arm({}, "bb")},
        strk,
    )
    verdict = _capture(_verdict, workloads, (strk,), undocumented)
    assert verdict.code == 1 and "COUNT LEAK" in verdict.out, verdict.out

    print("repsel knob-isolation self-test OK")
    return 0


@dataclass
class _Captured:
    code: int
    out: str


def _capture(fn: Any, *fn_args: Any) -> _Captured:
    import contextlib
    import io

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        code = fn(*fn_args)
    return _Captured(code=code, out=buf.getvalue())


__all__ = [
    "KNOBS",
    "Knob",
    "check_isolation",
    "self_test",
]
