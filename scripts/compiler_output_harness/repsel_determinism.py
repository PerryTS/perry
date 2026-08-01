"""Object-emission determinism check (#7131).

Object-hash A/B is the cheapest and least deniable instrument this project has
for "did this change what ships": compile twice, hash the objects, and a
difference is not an opinion. Every representation-selection finding of the
#7113/#7119/#7121/#7128 series rests on it.

The instrument is only sound if the compiler is a function of its inputs. It
was not, on ELF:

    clang -c perry_llvm_<pid>_<nanos>_<counter>.ll -o out.o

records the **source basename** of the translation unit into the object as an
`STT_FILE` symbol, so two identical compiles differed by exactly the digits of
the pid and the clock (#7131 — 26/26 census workloads nondeterministic on a
Raspberry Pi 5, ~10 bytes apart on `suite_01_startup`). Mach-O does not record
that name in the `.o` at all, which is why macOS looked clean while carrying the
same defect. #7135 fixed it by content-addressing the `.ll` basename; this
module is the check that says so, and keeps saying so.

Measured properties of the ELF path, so a future reader does not have to
re-derive which names matter (aarch64 Debian clang 19.1.7, no `-g`):

* the `.ll` **source basename** IS recorded — `STT_FILE`, `.strtab`;
* its **directory** and the process **CWD** are NOT (that needs DWARF, i.e.
  `-g`, which Perry only passes under `PERRY_DEBUG_SYMBOLS`);
* the `-o` **output** path is NOT recorded anywhere;
* `ld -r` (the multi-codegen-unit merge, #5391) records neither its input nor
  its output paths.

So only the `.ll` name had to become a function of content — and, symmetrically,
every *output* name must stay unique per **process**, not merely per call. #7135
content-addressed both and dropped the pid from the `.o`; two `perry` processes
compiling identical IR then agreed on the object path and deleted it out from
under each other, because the counter that was left is per-process state that
every process starts at 0. This check found that, because it runs its repeats
concurrently — a serial check never opens the window.

Why this is a check and not a comment: the property is invisible on the host
most of this project's compiler work happens on. A macOS-only reviewer cannot
tell a fixed compiler from a broken one, which is exactly how the defect
survived from #509 to #7131. Run it on Linux.
"""

from __future__ import annotations

import argparse
import hashlib
import platform
import shutil
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Callable

from .capture import resolve_perry
from .common import HarnessError, REPO_ROOT
from .repsel_census import DEFAULT_BASELINE, compile_and_census, load_baseline


#: How many times each workload is compiled. Two is the minimum that can
#: observe a difference; more is a stronger sample for a flaky (rather than
#: systematically clock-keyed) source of variation.
DEFAULT_REPEAT = 2


def nondeterminism_report(varied: list[str], total: int, *, repeat: int = 2) -> str:
    """The message shown when the same compiler twice disagreed with itself.

    Kept in one place because two callers need to say the same thing: this
    check, and the knob-isolation gate's determinism control (#7128), whose
    every object comparison is void if this property does not hold.
    """
    shown = ", ".join(varied[:4]) + ("…" if len(varied) > 4 else "")
    return (
        f"OBJECT EMISSION IS NONDETERMINISTIC: {len(varied)}/{total} workload(s) "
        f"compiled {repeat}x with identical flags and environment produced "
        f"different bytes ({shown}).\n"
        "\n"
        "  This is a REGRESSION, not a host property. It was one until #7131:\n"
        "  the temp `.ll` name carried pid + wall-clock nanos, and clang records\n"
        "  a translation unit's source basename into the ELF object. #7135\n"
        "  content-addressed that name. If this fires again, object-hash A/B is\n"
        "  invalid on this host and every measurement taken through it is void.\n"
        "\n"
        "  To localise: keep two objects for one workload and compare\n"
        "    readelf -sW a.o | grep FILE   # the #7131 shape: names differ here\n"
        "    cmp -l a.o b.o | head         # anywhere else is a NEW cause\n"
        "  A differing `STT_FILE` symbol is the old defect returning. Differences\n"
        "  elsewhere in `.text` are a nondeterministic codegen ordering instead —\n"
        "  the macOS analogue was closure-source iteration order permuting\n"
        "  `@.str.N` numbering per process (#7038/#7039)."
    )


def digest_objects(paths: list[str]) -> str:
    """SHA-256 over every object a compile emitted, in a stable order.

    Shared with the knob-isolation gate so "the objects are the same" means one
    thing in this package rather than two implementations that could drift.
    """
    h = hashlib.sha256()
    for path in sorted(paths):
        h.update(Path(path).read_bytes())
    return h.hexdigest()


def verdict(
    digests: dict[str, list[str]], *, repeat: int, printer: Callable[[str], None] = print
) -> int:
    """Turn per-workload digest lists into an exit code.

    Split from the compile loop so the verdict can be exercised without a
    compiler — a gate whose decision logic is only reachable through a 52-compile
    run is a gate nobody re-checks.
    """
    names = sorted(digests)
    if not names:
        raise HarnessError("no workloads were compiled; nothing was checked")
    for name in names:
        seen = digests[name]
        if len(seen) < 2:
            raise HarnessError(
                f"{name} was compiled {len(seen)}x; determinism needs at least 2 "
                "observations, so this run proves nothing"
            )
    varied = [n for n in names if len(set(digests[n])) > 1]

    printer("Per-workload emission")
    printer("---------------------")
    for name in names:
        mark = "DIFFERS" if name in varied else "same"
        printer(f"  {name:<34} {mark:>8}  {digests[name][0][:16]}")
    printer("")

    if varied:
        printer(nondeterminism_report(varied, len(names), repeat=repeat))
        return 1
    printer(
        f"Emission is deterministic: {len(names)}/{len(names)} workload(s) "
        f"compiled {repeat}x produced byte-identical objects. Object-hash A/B "
        "is valid on this host."
    )
    return 0


def check_determinism(args: argparse.Namespace) -> int:
    """Compile every census workload `--repeat` times and compare the bytes."""
    perry = resolve_perry(getattr(args, "perry", None))
    baseline = load_baseline(Path(args.baseline) if args.baseline else DEFAULT_BASELINE)
    workloads: list[dict[str, Any]] = baseline["workloads"]
    if getattr(args, "workload", None):
        wanted = set(args.workload)
        workloads = [w for w in workloads if w["name"] in wanted]
        missing = wanted - {w["name"] for w in workloads}
        if missing:
            raise HarnessError(f"unknown workload(s): {', '.join(sorted(missing))}")
    if not workloads:
        raise HarnessError("no workloads selected")

    repeat = max(2, int(args.repeat))

    print("Object-emission determinism (#7131)")
    print("===================================\n")
    print(f"compiler: {' '.join(perry)}")
    print(f"host:     {platform.system()} {platform.machine()}")
    print(f"corpus:   {len(workloads)} workload(s) x {repeat} compile(s)\n")

    tmp = Path(tempfile.mkdtemp(prefix="repsel-determinism-"))
    try:
        # Repeats run through the same pool as the corpus on purpose: two
        # workers holding IDENTICAL IR now share one content-addressed `.ll`,
        # so racing them is part of the subject, not a confound (#7135 CR).
        jobs = [(w, i) for w in workloads for i in range(repeat)]

        def run(job: tuple[dict[str, Any], int]) -> tuple[str, str]:
            workload, index = job
            source = REPO_ROOT / workload["source"]
            census = compile_and_census(
                perry,
                source,
                timeout=args.compile_timeout,
                object_out=tmp / workload["name"] / str(index) / "out.o",
            )
            return workload["name"], digest_objects(census["objects"])

        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            observed = list(pool.map(run, jobs))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    digests: dict[str, list[str]] = {}
    for name, digest in observed:
        digests.setdefault(name, []).append(digest)
    return verdict(digests, repeat=repeat)


def self_test(_args: argparse.Namespace) -> int:
    """Prove the verdict can go red, and that it refuses a vacuous run.

    CLAUDE.md failure mode 4: a gate must assert its subject was live. A
    determinism check handed one observation per workload has compared nothing,
    and must say so rather than printing a green line.
    """
    quiet: Callable[[str], None] = lambda _line: None

    assert verdict({"w": ["a", "a"], "v": ["b", "b"]}, repeat=2, printer=quiet) == 0

    assert verdict({"w": ["a", "a"], "v": ["b", "c"]}, repeat=2, printer=quiet) == 1
    assert verdict({"w": ["a", "b"]}, repeat=2, printer=quiet) == 1

    # Nondeterminism on a LATER repeat must count too — a check that only
    # compared the first two observations would miss a 1-in-3 flake.
    assert verdict({"w": ["a", "a", "z"]}, repeat=3, printer=quiet) == 1

    for vacuous in ({}, {"w": ["a"]}):
        try:
            verdict(vacuous, repeat=2, printer=quiet)  # type: ignore[arg-type]
        except HarnessError:
            pass
        else:  # pragma: no cover - the assertion below is the failure report
            raise AssertionError(
                f"verdict({vacuous!r}) returned a verdict having compared nothing"
            )

    report = nondeterminism_report(["w", "v"], 26, repeat=2)
    assert "2/26" in report and "#7131" in report and "STT_FILE" in report, report

    print("repsel determinism self-test OK")
    return 0
