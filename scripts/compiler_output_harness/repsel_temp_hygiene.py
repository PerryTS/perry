"""Temp-directory hygiene check (#7144, #7167).

Compile the census corpus with `TMPDIR` pointed at an empty directory of our
own, then look in that directory. **It must be empty** — no allowlist, no
"known" leaks, nothing.

It did not start that way. #7144 shipped this gate failing only on
`perry-codegen`'s clang-driver names and merely *reporting* everything else,
because a second leak was live at the time (#7167) and a gate that goes red for
another module's defect gets muted rather than fixed. #7167 closed that path
and the carve-out went with it. The two leaks it has caught:

**#7144 — the clang driver's `.ll`.**
`compile_ll_to_object` writes the module's LLVM IR to a temp `.ll`, hands the
path to `clang -c`, and reads back the object. #7131 made that name a pure
function of the IR — it had to, because clang records a translation unit's
source basename into the ELF object — and #7135, shipping that fix, stopped
deleting the file: workers holding identical IR now *shared* the path, so a
per-call unlink could race a sibling that had computed the path but not yet
opened it.

Nothing else deleted them. The file count is bounded by the number of
**distinct IR contents ever compiled** on the machine, which sounds benign
until you notice that iterating on the compiler changes the IR on essentially
every rebuild:

    leftover perry_llvm_*.ll files: 1627, total 951.8 MB     (one dev box, one day)
    ~29,000 files, 29 GB                                     (another, a month)

#7144 removed the sharing instead of the deletion: the `.ll` now lives in a
directory that belongs to one call, so unlinking it is unobservable to anyone
else, and the *basename* clang records is untouched — `census-determinism` is
the check that the second half still holds.

**#7167 — the compile driver's staged objects.**
`run_pipeline.rs` staged every emitted `.o` in a `perry-objs-<pid>-<nanos>/`
directory and removed it on the paths that *link*. `--no-link` returns before
those, so it removed nothing: one fresh directory plus its objects per compile,
unbounded in **compiles** rather than in distinct IR, and the objects are far
larger than the `.ll`s. 3086 such directories had accumulated on one dev box.
Every harness in this package compiles with `--no-link`, so running the census
was itself the heaviest source of the leak.

The fix was not a third `remove_dir`. On `--no-link` the objects are the
*product*, so they are delivered to `-o` and no staging directory is created at
all; when linking, the directory is removed by a `Drop` guard so every exit
cleans up through one site. See `crates/perry/src/commands/compile/
object_staging.rs`.

Design notes, because the alternatives were tried and are wrong:

* **"No growth run-over-run" is not the property.** Compiling the same corpus
  twice leaves the same content-addressed names, so a repeat-and-compare check
  is *green on the broken compiler*. Growth needs new IR, which is why the leak
  was invisible in CI and only ever showed up on developer machines. The
  property that goes red immediately is the absolute one: nothing left at all.
* **`TMPDIR` isolation is not politeness, it is what makes the check sound.**
  Counting entries in the shared system temp directory measures every other
  process on the box — on a machine running several compiles at once, that is
  noise large enough to swamp the signal in either direction.
* **No allowlist.** Naming the leaks this gate is allowed to fail on means the
  next leak — under a name nobody has written yet — passes silently. #7167 is
  the worked example: it was known, printed on every run, and could not turn a
  run red.

`PERRY_DEBUG_SYMBOLS` is *not* exempt, though it was going to be. `-g` was
documented as putting the `.ll`'s absolute path into DWARF; measured on a real
Perry module it emits no DWARF at all (Perry's codegen produces no DI metadata,
and `clang -g` on a `.ll` lowers what the IR already has rather than
synthesising a compile unit). One layout, no exemption — see
`debug_symbols_do_not_change_what_the_object_records`.
"""

from __future__ import annotations

import argparse
import platform
import shutil
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Callable

from .capture import resolve_perry
from .common import HarnessError, REPO_ROOT
from .repsel_census import DEFAULT_BASELINE, compile_and_census, load_baseline


#: Compiles per workload. Two, not one, so the corpus contains pairs of
#: compiles with *identical* IR — the case that shares a content-addressed name
#: and the reason #7135 could not simply keep deleting the file. They run
#: concurrently for the same reason (#7140: a serial check never opens the
#: window).
DEFAULT_REPEAT = 2

#: Cap on how many leftover paths a failure prints. A leaking compiler leaks one
#: per compile, and 52 lines of the same shape teaches nothing the first 12 did
#: not.
MAX_REPORTED = 12

def leftovers_under(root: Path) -> list[str]:
    """Every path under `root`, relative to it, deepest entries first.

    Directories are reported too: the pre-#7144 failure is a stray *file*, but
    a half-finished cleanup that leaves empty scratch directories behind is the
    same defect wearing a smaller coat, and it is just as unbounded.
    """
    if not root.exists():
        raise HarnessError(f"the isolated temp root vanished during the run: {root}")
    found = [p.relative_to(root).as_posix() for p in root.rglob("*")]
    found.sort()
    return found


def verdict(
    leftovers: list[str],
    *,
    compiles: int,
    printer: Callable[[str], None] = print,
) -> int:
    """Turn "what was left in the temp dir" into an exit code.

    Split from the compile loop so the decision can be exercised without a
    compiler (CLAUDE.md failure mode 4: a gate nobody can re-check is not a
    gate), and so a run that compiled *nothing* is a harness error rather than
    a green line — an empty directory is exactly what you get from doing no
    work at all.
    """
    if compiles <= 0:
        raise HarnessError(
            "no compiles ran, so an empty temp directory proves nothing; "
            "this run checked nothing"
        )

    if not leftovers:
        printer(
            f"Temp directory is clean: {compiles} compile(s) left 0 entries "
            "behind. Neither the #7144 nor the #7167 leak is present."
        )
        return 0

    shown = leftovers[:MAX_REPORTED]
    printer(
        f"TEMP FILES LEAKED: {compiles} compile(s) left {len(leftovers)} "
        f"entr{'y' if len(leftovers) == 1 else 'ies'} in a temp directory that "
        "started empty.\n"
    )
    for name in shown:
        printer(f"    {name}")
    if len(leftovers) > len(shown):
        printer(f"    … and {len(leftovers) - len(shown)} more")
    printer(
        "\n"
        "  Nothing may survive a compile in the temp directory. Two known\n"
        "  leaks produced this failure before; the name above says which, and\n"
        "  a THIRD name means a new one.\n"
        "\n"
        "  `perry_llvm_*` / `perry_cgu_*` / `perry_bc_*` — #7144, the clang\n"
        "  driver. The `.ll` handed to `clang -c` is content-addressed (#7131:\n"
        "  clang records its basename into the ELF object), so the leftovers\n"
        "  are bounded by DISTINCT IR EVER COMPILED, not by compiles — a\n"
        "  repeat-and-compare check stays green while a developer machine fills\n"
        "  up. 1627 files / 951.8 MB after a day; 29 GB on a longer-lived box.\n"
        "  The fix was not a more careful unlink (that races a sibling worker\n"
        "  holding the same IR, which is why #7135 stopped deleting at all) but\n"
        "  to stop sharing: `crates/perry-codegen/src/linker.rs` gives each\n"
        "  compile a private scratch directory. `PERRY_DEBUG_SYMBOLS` is not an\n"
        "  exemption — measured, `-g` emits no DWARF from a Perry `.ll` at all.\n"
        "\n"
        "  `perry-objs-*` — #7167, the compile driver. `run_pipeline.rs` staged\n"
        "  objects in a per-invocation temp directory and removed it on the link\n"
        "  exits only, so every `--no-link` compile leaked one, unbounded in\n"
        "  COMPILES. The fix was to stop creating it: on `--no-link` the objects\n"
        "  are the product and go to `-o`, and when linking the directory is\n"
        "  removed by `Drop` so no exit has to remember.\n"
        "\n"
        "  Anything else is a new leak. This gate has no allowlist on purpose:\n"
        "  #7167 was known, printed on every run, and could not turn the run\n"
        "  red for a full release. A gate that names its exceptions cannot see\n"
        "  the leak nobody has written yet."
    )
    return 1


def check_temp_hygiene(args: argparse.Namespace) -> int:
    """Compile the census corpus with an isolated `TMPDIR` and inspect it."""
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

    repeat = max(1, int(args.repeat))

    print("Temp-directory hygiene (#7144)")
    print("==============================\n")
    print(f"compiler: {' '.join(perry)}")
    print(f"host:     {platform.system()} {platform.machine()}")
    print(f"corpus:   {len(workloads)} workload(s) x {repeat} compile(s)\n")

    outer = Path(tempfile.mkdtemp(prefix="repsel-temp-hygiene-"))
    # The directory the *compiler* will treat as its temp dir. Separate from
    # `outer` so the harness's own scratch never counts as a leftover.
    isolated = outer / "compiler-tmp"
    isolated.mkdir()
    try:
        jobs = [(w, i) for w in workloads for i in range(repeat)]

        def run(job: tuple[dict[str, Any], int]) -> None:
            workload, _index = job
            compile_and_census(
                perry,
                REPO_ROOT / workload["source"],
                timeout=args.compile_timeout,
                # TMP/TEMP alongside TMPDIR so the check means the same thing
                # if this ever runs on Windows, where `env::temp_dir()` reads
                # those instead.
                extra_env={
                    "TMPDIR": str(isolated),
                    "TMP": str(isolated),
                    "TEMP": str(isolated),
                },
            )

        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            list(pool.map(run, jobs))

        left = leftovers_under(isolated)
        return verdict(left, compiles=len(jobs))
    finally:
        shutil.rmtree(outer, ignore_errors=True)


def self_test(_args: argparse.Namespace) -> int:
    """Prove the verdict can go red, and that it refuses a vacuous run."""
    quiet: Callable[[str], None] = lambda _line: None

    assert verdict([], compiles=52, printer=quiet) == 0

    # #7144's family — the clang driver's own names.
    assert verdict(["perry_llvm_2791e842224ea99c.ll"], compiles=52, printer=quiet) == 1
    # An empty scratch directory left behind is the same defect, smaller.
    assert verdict(["perry_llvm_scratch_1a2b_0"], compiles=1, printer=quiet) == 1
    # …and a file inside one is attributed to whoever leaked the directory.
    assert verdict(["perry_llvm_scratch_1a2b_0/x.ll"], compiles=1, printer=quiet) == 1
    for owned in ("perry_cgu_1_2_0.o", "perry_bc_1_2_linked.bc"):
        assert verdict([owned], compiles=1, printer=quiet) == 1, owned

    # #7167's family. These used to return 0 — reported, not failed — while the
    # compile driver's `--no-link` path was still leaking them. The flip from 0
    # to 1 IS the widening, so it is asserted directly rather than implied by
    # the absence of an allowlist.
    assert verdict(["perry-objs-9-1"], compiles=1, printer=quiet) == 1
    assert verdict(["perry-objs-9-1/m.o"], compiles=1, printer=quiet) == 1
    lines: list[str] = []
    verdict(["perry-objs-9-1/m.o"], compiles=1, printer=lines.append)
    joined = "\n".join(lines)
    assert "#7167" in joined and "run_pipeline.rs" in joined, joined

    # A name from neither family must fail too. This is the case an allowlist
    # cannot cover, and the reason there is no allowlist: the next leak has a
    # name nobody has written yet.
    assert verdict(["perry-embed-4242/bundle.o"], compiles=1, printer=quiet) == 1
    assert verdict(["something-nobody-has-written-yet"], compiles=1, printer=quiet) == 1
    lines = []
    verdict(["brand-new-leak-name"], compiles=1, printer=lines.append)
    assert "new leak" in "\n".join(lines)

    # A run that compiled nothing finds an empty directory for the wrong
    # reason. It must not be able to report success.
    try:
        verdict([], compiles=0, printer=quiet)
    except HarnessError:
        pass
    else:  # pragma: no cover - the raise below is the failure report
        raise AssertionError("verdict() called a zero-compile run clean")

    lines = []
    verdict(["perry_llvm_a.ll", "perry_llvm_b.ll"], compiles=2, printer=lines.append)
    report = "\n".join(lines)
    for expected in (
        "#7144",
        "#7131",
        "PERRY_DEBUG_SYMBOLS",
        "linker.rs",
        "#7167",
        "run_pipeline.rs",
    ):
        assert expected in report, f"failure report must mention {expected}: {report}"

    # The truncation must announce itself rather than quietly dropping paths.
    lines = []
    verdict(
        [f"perry_llvm_{i}.ll" for i in range(MAX_REPORTED + 5)],
        compiles=1,
        printer=lines.append,
    )
    assert "and 5 more" in "\n".join(lines)

    print("repsel temp-hygiene self-test OK")
    return 0
