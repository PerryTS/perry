"""Temp-directory hygiene check (#7144).

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

What this module checks is the first half, end to end, on the real compiler:
compile the census corpus with `TMPDIR` pointed at an empty directory of our
own, then look in that directory. It must be empty.

Two design notes, because both alternatives were tried and are wrong:

* **"No growth run-over-run" is not the property.** Compiling the same corpus
  twice leaves the same content-addressed names, so a repeat-and-compare check
  is *green on the broken compiler*. Growth needs new IR, which is why the leak
  was invisible in CI and only ever showed up on developer machines. The
  property that goes red immediately is the absolute one: nothing left at all.
* **`TMPDIR` isolation is not politeness, it is what makes the check sound.**
  Counting entries in the shared system temp directory measures every other
  process on the box — on a machine running several compiles at once, that is
  noise large enough to swamp the signal in either direction.

The `PERRY_DEBUG_SYMBOLS` layout is deliberately exempt: under `-g` the object's
DWARF names the `.ll` by absolute path, so the file has to outlive the compile.
This check runs without it, which is how every other gate compiles too.
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

#: Every temp name `crates/perry-codegen/src/linker.rs` creates — the `.ll`/`.o`
#: pair and its scratch directory (`perry_llvm_*`), the multi-codegen-unit
#: staging objects (`perry_cgu_*`, #5391), and the bitcode-link intermediates
#: (`perry_bc_*`). This gate's subject is that module's file lifecycle, so it
#: fails on these and only these.
#:
#: Anything else found is REPORTED, loudly, and does not fail: as of #7144 the
#: compile driver leaks a `perry-objs-<pid>-<nanos>/` staging directory on the
#: `--no-link` path (`run_pipeline.rs` removes it on both *link* exits and there
#: is no third one), which is a real defect but a different module's, and a gate
#: that goes red for someone else's bug gets muted rather than fixed. Widen this
#: to "nothing at all" once the driver's path is closed.
OWNED_PREFIXES = ("perry_llvm", "perry_cgu", "perry_bc")


def classify(leftovers: list[str]) -> tuple[list[str], list[str]]:
    """Split leftovers into "this gate's subject" and "somebody else's".

    Classified on the FIRST path component: everything under a leaked scratch
    directory is leaked by whoever leaked the directory.
    """
    owned: list[str] = []
    other: list[str] = []
    for rel in leftovers:
        top = rel.split("/", 1)[0]
        (owned if top.startswith(OWNED_PREFIXES) else other).append(rel)
    return owned, other


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
    owned, other = classify(leftovers)

    if other:
        shown = other[:MAX_REPORTED]
        printer(
            f"Not this gate's subject — {len(other)} entr"
            f"{'y' if len(other) == 1 else 'ies'} left by a module other than "
            "perry-codegen's clang driver:"
        )
        for name in shown:
            printer(f"    {name}")
        if len(other) > len(shown):
            printer(f"    … and {len(other) - len(shown)} more")
        printer(
            "  `perry-objs-<pid>-<nanos>/` is the compile driver's object "
            "staging dir;\n"
            "  `run_pipeline.rs` removes it on both *link* exits and `--no-link` "
            "returns\n"
            "  before either. Reported, not failed: a gate that goes red for "
            "another\n"
            "  module's defect gets muted rather than fixed.\n"
        )

    if not owned:
        printer(
            f"Temp directory is clean: {compiles} compile(s) left 0 clang-driver "
            "files behind. The #7144 leak is not present."
        )
        return 0

    shown = owned[:MAX_REPORTED]
    printer(
        f"TEMP FILES LEAKED: {compiles} compile(s) left {len(owned)} "
        f"entr{'y' if len(owned) == 1 else 'ies'} in a temp directory that "
        "started empty.\n"
    )
    for name in shown:
        printer(f"    {name}")
    if len(owned) > len(shown):
        printer(f"    … and {len(owned) - len(shown)} more")
    printer(
        "\n"
        "  This is #7144. The `.ll` handed to `clang -c` is content-addressed\n"
        "  (#7131 — clang records its basename into the ELF object), so the\n"
        "  leftovers are bounded by DISTINCT IR EVER COMPILED, not by compiles:\n"
        "  a repeat-and-compare check stays green while a developer machine\n"
        "  fills up. Measured before the fix: 1627 files / 951.8 MB after a day\n"
        "  of compiler work; 29 GB on a longer-lived box.\n"
        "\n"
        "  The fix is not a more careful unlink — that races a sibling worker\n"
        "  holding the same IR, which is why #7135 stopped deleting at all. It\n"
        "  is to stop sharing: `crates/perry-codegen/src/linker.rs` gives each\n"
        "  compile a private scratch directory and removes it on success, while\n"
        "  the basename inside it stays a pure function of the IR so\n"
        "  `census-determinism` keeps passing.\n"
        "\n"
        "  Exempt by design: `PERRY_DEBUG_SYMBOLS` builds keep their `.ll` —\n"
        "  the object's DWARF names it by absolute path. If this fired under\n"
        "  `-g`, that is the expected behaviour and not this gate's business."
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
    assert verdict(["perry_llvm_2791e842224ea99c.ll"], compiles=52, printer=quiet) == 1
    # An empty scratch directory left behind is the same defect, smaller.
    assert verdict(["perry_llvm_scratch_1a2b_0"], compiles=1, printer=quiet) == 1
    # …and a file inside one is attributed to whoever leaked the directory.
    assert verdict(["perry_llvm_scratch_1a2b_0/x.ll"], compiles=1, printer=quiet) == 1
    for owned in ("perry_cgu_1_2_0.o", "perry_bc_1_2_linked.bc"):
        assert verdict([owned], compiles=1, printer=quiet) == 1, owned

    # Another module's leftovers are reported, not failed — see OWNED_PREFIXES.
    assert verdict(["perry-objs-9-1/m.o"], compiles=1, printer=quiet) == 0
    lines: list[str] = []
    assert verdict(["perry-objs-9-1/m.o"], compiles=1, printer=lines.append) == 0
    joined = "\n".join(lines)
    assert "perry-objs" in joined and "run_pipeline.rs" in joined, joined
    # A mixture still fails, and the failure is about the owned half.
    assert verdict(["perry-objs-9-1/m.o", "perry_llvm_a.ll"], compiles=1, printer=quiet) == 1

    assert classify(["perry_llvm_a.ll", "perry-objs-9-1/m.o"]) == (
        ["perry_llvm_a.ll"],
        ["perry-objs-9-1/m.o"],
    )

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
    for expected in ("#7144", "#7131", "PERRY_DEBUG_SYMBOLS", "linker.rs"):
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
