#!/usr/bin/env python3
"""GC performance/RSS ratchet: measurement, pinned artifact, and regression gate.

This harness freezes the observable behaviour of the *current* evacuating minor
collector so that a later change cannot quietly regress it. It exists because a
large GC architecture campaign (replacing shadow-stack precise roots with a
conservative stack scan plus per-object pinning) is about to start, and the
whole point of that campaign is that the gate decides, not argument.

NOT THE PUBLIC BASELINE
-----------------------
``benchmarks/run_public_baseline.sh`` / ``benchmarks/public_baseline.py`` /
``benchmarks/results/public-node-bun-v1.json`` are a different, maintainer-owned
artifact: published Perry-vs-Node-vs-Bun evidence, regenerated on a release
cadence, gating ``lint``. This module is an internal Perry-vs-Perry ratchet over
GC retention, evacuation accounting, and memory, regenerated only when a change
is deliberately accepted, gating the ``gc-ratchet`` CI job. Different directory,
different artifact, different gate. Never regenerate one from the other.

METRIC FAMILIES AND WHY THEY ARE SEPARATED
------------------------------------------
Measured on the pinned quiet host, 3 independent sessions x 7 repeats:

``retention``  ``heap_used_bytes``, ``heap_total_bytes``
    Read from ``process.memoryUsage()`` after an explicit full ``gc()``.
    Observed spread: **0.000%** — bit-identical across all 21 runs of all 8
    probes. For a deterministic program these are a pure function of the
    allocation sequence and collector policy, independent of CPU speed, core
    count, and machine load.

    ★ **Deterministic is not the same as semantic (#7559).** An explicit
    ``gc()`` is the one site in Perry that *forces* the conservative
    native-stack scan (``ManualGcScanGuard``, #4977; production resolves to
    ``SkipDisabled``), so this reading is taken under a root set nothing else in
    the language uses, and it includes whatever the native stack happened to
    look like a heap pointer to. ``js_arena_stats`` then sums each block's
    **bump-pointer offset**, and a block cannot be reset while it holds one
    marked object — so one stale stack word costs a whole 1 MiB nursery block.
    Measured across the 74 commits between the 2026-08-05 pin and v0.5.1321:
    the precise (scan-off) retention was byte-identical on 10 of 12 probes and
    fell on the other two, while this number moved on five, always by whole
    blocks. ``05_closure_capture``'s +16.44% breach was precisely that: precise
    retention 5,329,880 at *both* endpoints, false-root residue 1 -> 2 blocks.
    Run ``gc_ratchet.py classify`` before treating a retention row as a
    collector regression.

``gc``  ``minor_cycles``, ``copied_objects``, ``copied_bytes``,
        ``promoted_objects``, ``promoted_bytes``, ``freed_bytes``, ``step_cycles``
    Parsed from ``PERRY_GC_DIAG=1`` output in a separate, untimed pass.
    Observed spread: **0.000%**. This is the evacuating minor's own accounting,
    and it is the family that most directly answers "did the collector change
    what it does?" — pinning collapses ``copied_objects``, over-retention
    depresses ``freed_bytes`` and inflates ``promoted_bytes``.

``memory``  ``rss_bytes``, ``peak_rss_bytes``
    Observed spread: <=0.41% across sessions. Host-allocator dependent, so
    comparable only within one platform *and* one machine class.

``timing``  ``wall_ms``
    Observed spread: <=0.75% on medians on the quiet host, but that number does
    not transfer to a shared CI runner, where neighbour noise dominates. Gated
    only on the pinned host.

TRANSPORT
---------
Probes print ``probe:``/``checksum:`` lines on stdout, which are diffed
byte-for-byte against the pinned Node oracle, and ``#gcmetric key=value`` lines
on stderr, which are Perry-only.
"""

from __future__ import annotations

import argparse
import collections
import dataclasses
import json
import math
import os
import platform
import re
import resource
import shutil
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

SCHEMA_VERSION = 1
HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
PROBES_DIR = HERE / "probes"
DEFAULT_ARTIFACT = HERE / "baseline" / "gc-ratchet-v1.json"
DEFAULT_TOLERANCES = HERE / "tolerances.json"

GCMETRIC_RE = re.compile(r"^#gcmetric\s+([a-z0-9_]+)=(-?[0-9]+)\s*$")
COPY_MINOR_RE = re.compile(r"^\[gc-copy-minor\]\s+ran\s+(.*)$")
SCAN_FALLBACK_RE = re.compile(
    r"^\[gc-scan-fallback\]\s+site=(\w+)\s+automatic=(true|false)\s+count=(\d+)\s*$"
)
GC_STEP_PREFIX = "[gc-step]"

#: A probe may declare the collector configuration it is a probe *of*, as
#: ``// gc-ratchet-env: KEY=VALUE`` comment lines in its own source. See
#: ``probe_run_env``.
PROBE_ENV_RE = re.compile(r"^//\s*gc-ratchet-env:\s*([A-Za-z_][A-Za-z0-9_]*)=(\S+)\s*$")

#: Env names a probe may not declare, because the harness itself sets them and
#: a probe that also set them would be silently fighting the measurement rather
#: than describing itself. ``PERRY_CONSERVATIVE_STACK_SCAN`` is the axis
#: ``classify`` sweeps; ``PERRY_GC_DIAG`` is what separates the traced pass from
#: the timed one.
RESERVED_PROBE_ENV = frozenset(("PERRY_CONSERVATIVE_STACK_SCAN", "PERRY_GC_DIAG"))

#: Runtime knob that turns the conservative native-stack scan off. See
#: ``classify`` for why this harness cares.
SCAN_MODE_ENV = "PERRY_CONSERVATIVE_STACK_SCAN"

RETENTION_METRICS = ("heap_used_bytes", "heap_total_bytes")
GC_METRICS = (
    "minor_cycles",
    "step_cycles",
    "copied_objects",
    "copied_bytes",
    "promoted_objects",
    "promoted_bytes",
    "freed_bytes",
)
MEMORY_METRICS = ("rss_bytes", "peak_rss_bytes")
TIMING_METRICS = ("wall_ms",)

ALL_METRICS = RETENTION_METRICS + GC_METRICS + MEMORY_METRICS + TIMING_METRICS

#: Metrics collected once per repeat from a normal (untraced) run.
SAMPLED_METRICS = RETENTION_METRICS + MEMORY_METRICS + TIMING_METRICS

#: Metrics whose gating premise is *bit-identity*, not a noise allowance. Their
#: bands in ``tolerances.json`` are justified by an observed spread of 0.000%
#: over 21 runs, so a pinned probe whose samples disagree is not merely noisy:
#: it contradicts the reason its band is that tight. Such a cell may not be
#: gating (see ``validate_artifact`` and ``probe_overrides``).
DETERMINISTIC_METRICS = RETENTION_METRICS + GC_METRICS

PROFILES = ("shared_ci", "pinned_host")

#: Top-level keys ``tolerances.json`` may contain. Anything else is refused
#: rather than ignored: a mistyped ``probe_override`` section would silently not
#: apply, which is a gate quietly measuring something other than what its file
#: says it measures.
TOLERANCE_SECTIONS = frozenset(("_readme", "probe_overrides", *PROFILES))

#: Minimum repeats behind a probe-override exclusion. ``tolerances.json``
#: justifies every *inclusion* on 21 runs (3 sessions x 7); an *exclusion* is
#: the same claim with the opposite sign and is held to the same evidence.
MIN_EXCLUSION_RUNS = 21


class RatchetError(RuntimeError):
    """Raised when measurement, artifact validation, or comparison fails."""


@dataclass(frozen=True)
class ArtifactDefect:
    """One thing wrong with the pinned artifact, and how much of it that voids.

    WHY THIS IS A TYPE AND NOT AN EXCEPTION
    ---------------------------------------
    Artifact defects used to be raised one at a time from ``validate_artifact``,
    which meant the *first* one aborted everything. That is how #7554 cost three
    days of coverage: one cell — ``12_large_live_set.heap_used_bytes``, spread
    6,768 bytes — failed the artifact-validation step, and because that step runs
    *before* the measurement step, none of the twelve probes executed on any
    branch for three days. Two GC pacing changes (#7594, #7596) merged in that
    window and each had to substitute a hand-run both-arms A/B for the gate.

    The defect that caused it was a statement about **one cell**: this metric on
    this workload is not bit-identical, so the band whose premise is bit-identity
    cannot rest on it. Nothing about that claim voids the other 143 cells, and
    nothing about it makes the probes unrunnable. Collapsing the whole gate on it
    was a blast radius nobody chose.

    So a defect now carries its own scope, and the scope decides the blast
    radius:

    ``artifact``
        The artifact cannot be interpreted or has been tampered with: wrong
        schema, missing metric, a summary that disagrees with its own samples.
        Comparing anything against it would be meaningless, so this stays fatal
        and stays in preflight.
    ``probe``
        One probe was pinned unfit — no oracle diff, or no collection. Its rows
        are not evidence; the other probes' still are. The probe is demoted out
        of the gating family for the run and named as a failure.
    ``cell``
        One (probe, metric) cell contradicts the premise of its own band. The
        cell is demoted; every other cell is still gated.

    A demotion is NOT an excuse. Every non-fatal defect is still reported and
    still turns ``check`` red — it just does so *after* the probes have run, with
    the full table attached, so a regression somewhere else in the matrix is
    named in the same run instead of being hidden behind the abort. Fail-open per
    cell, fail-closed on the verdict.
    """

    scope: str
    message: str
    probe: str | None = None
    metric: str | None = None

    #: Scopes in widening order of blast radius.
    SCOPES = ("cell", "probe", "artifact")

    @property
    def fatal(self) -> bool:
        """True when the defect voids the whole artifact rather than part of it."""
        return self.scope == "artifact"

    @property
    def where(self) -> str:
        if self.scope == "cell":
            return f"{self.probe}.{self.metric}"
        if self.scope == "probe":
            return str(self.probe)
        return "artifact"

    def describe(self) -> str:
        return f"UNFIT PINNED {self.scope.upper()} `{self.where}` — {self.message}"


# ---------------------------------------------------------------------------
# Environment description
# ---------------------------------------------------------------------------


def code_tree_hash() -> str:
    """Tree hash of `crates/` — the code whose behaviour a probe measures.

    Rebase- and version-bump-stable, unlike the commit hash. Returns
    `"unknown"` rather than raising when git is unavailable, because a missing
    provenance note must not fail a measurement run.
    """
    try:
        out = subprocess.run(
            ["git", "rev-parse", "HEAD:crates"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        return out.stdout.strip() if out.returncode == 0 and out.stdout.strip() else "unknown"
    except OSError:
        return "unknown"


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def platform_key() -> str:
    """Coarse identity of the measurement environment.

    Retention and evacuation accounting are arch/OS sensitive in principle
    (allocation granularity, page size, pointer-sized side tables), so a
    comparison across platform keys is refused unless explicitly relaxed.
    Deliberately coarse: neither CPU model nor core count belongs here, because
    a metric that depends on them does not belong in a gating family.
    """
    return f"{platform.system().lower()}-{platform.machine().lower()}"


def load_average() -> tuple[float, float, float]:
    try:
        return tuple(round(value, 2) for value in os.getloadavg())  # type: ignore[return-value]
    except (OSError, AttributeError):
        return (float("nan"),) * 3


def _run_text(command: Sequence[str]) -> str | None:
    if not shutil.which(command[0]):
        return None
    completed = subprocess.run(list(command), capture_output=True, text=True, check=False)
    return completed.stdout.strip().splitlines()[0] if completed.stdout.strip() else None


def host_description() -> dict[str, Any]:
    """Everything a later reader needs to judge whether a number is comparable.

    The load average is recorded next to every number on purpose. This project
    has twice recorded a wrong conclusion from a loaded machine — a "0% lever"
    that was really 1.32x, and a "1.4x lever" that was really 0% — and a number
    without its load is not evidence.
    """
    load1, load5, load15 = load_average()
    info: dict[str, Any] = {
        "platform": platform_key(),
        "hostname": platform.node(),
        "system": platform.system(),
        "release": platform.release(),
        "machine": platform.machine(),
        "cpu_count": os.cpu_count(),
        "load_average": {"1m": load1, "5m": load5, "15m": load15},
    }
    if platform.system() == "Darwin":
        info["cpu_brand"] = _run_text(["sysctl", "-n", "machdep.cpu.brand_string"])
        memsize = _run_text(["sysctl", "-n", "hw.memsize"])
        if memsize and memsize.isdigit():
            info["memory_bytes"] = int(memsize)
        info["product_version"] = _run_text(["sw_vers", "-productVersion"])
    elif platform.system() == "Linux":
        try:
            for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
                if line.startswith("MemTotal:"):
                    info["memory_bytes"] = int(line.split()[1]) * 1024
                    break
        except OSError:
            pass
    return info


def portable_path(path: Path | str) -> str:
    """Render a path without the operator's home directory in it.

    Repo-relative inside the checkout, ``~``-relative under ``$HOME``, otherwise
    unchanged. The artifact this feeds is committed to a public repository, so an
    absolute build path here publishes whoever pinned the baseline.
    """
    resolved = os.path.realpath(str(path))
    for base, prefix in (
        (os.path.realpath(REPO_ROOT), ""),
        (os.path.realpath(os.path.expanduser("~")), "~/"),
    ):
        if resolved == base or resolved.startswith(base + os.sep):
            return prefix + os.path.relpath(resolved, base)
    return resolved


def binary_fingerprints(perry: Path) -> dict[str, Any]:
    """Content hashes of the binary and archives actually under test.

    Content hashes, never mtimes. ``perry-runtime`` and ``perry-stdlib`` are
    rlib-only; ``libperry_{runtime,stdlib}.a`` come from the ``-static`` wrapper
    crates. Building without those wrappers leaves a stale archive in place,
    which makes both arms of an A/B behave identically and yields a vacuous "no
    regression". A recorded hash makes that visible after the fact: two runs
    that claim different code but share a runtime hash did not build what they
    thought they built.
    """
    import hashlib

    def digest(path: Path) -> dict[str, Any] | None:
        if not path.exists():
            return None
        hasher = hashlib.sha256()
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                hasher.update(chunk)
        return {
            "path": portable_path(path),
            "size": path.stat().st_size,
            "sha256": hasher.hexdigest(),
        }

    runtime_dir = os.environ.get("PERRY_RUNTIME_DIR")
    search = Path(runtime_dir) if runtime_dir else perry.parent
    out: dict[str, Any] = {"perry": digest(perry), "runtime_dir": portable_path(search)}
    for lib in ("libperry_runtime.a", "libperry_stdlib.a"):
        out[lib] = digest(search / lib)
    return out


def toolchain_description(perry: Path) -> dict[str, Any]:
    return {
        "perry_version": _run_text([str(perry), "--version"]) or "unknown",
        "rustc": _run_text(["rustc", "--version"]),
        "cargo": _run_text(["cargo", "--version"]),
        "cc": _run_text(["cc", "--version"]),
        "python": sys.version.split()[0],
        "env": {
            "PERRY_NO_AUTO_OPTIMIZE": os.environ.get("PERRY_NO_AUTO_OPTIMIZE"),
            "PERRY_GEN_GC": os.environ.get("PERRY_GEN_GC"),
            "PERRY_GEN_GC_EVACUATE": os.environ.get("PERRY_GEN_GC_EVACUATE"),
            "PERRY_WRITE_BARRIERS": os.environ.get("PERRY_WRITE_BARRIERS"),
        },
        "binaries": binary_fingerprints(perry),
    }


# ---------------------------------------------------------------------------
# Running probes
# ---------------------------------------------------------------------------


def run_once(
    command: Sequence[str], *, extra_env: Mapping[str, str] | None = None
) -> dict[str, Any]:
    """Run a command, returning stdout, stderr, exit status, wall time, peak RSS.

    Peak RSS comes from ``os.wait4`` — this specific child's ``ru_maxrss`` — and
    not from ``getrusage(RUSAGE_CHILDREN)``, which is a running maximum over
    every child the process has ever reaped and would report each probe's peak
    as the largest peak seen so far. It also does not come from
    ``/usr/bin/time -l``, which calls ``sysctl(kern.clockrate)`` and fails in
    otherwise usable sandboxed macOS environments, masking a successful exit.
    Darwin reports ``ru_maxrss`` in bytes, Linux in KiB.
    """
    import time

    env = dict(os.environ)
    if extra_env:
        env.update(extra_env)

    # Temporary files rather than pipes: os.wait4() must reap the child itself
    # to obtain its rusage, so Popen.communicate() (which reaps) cannot be used,
    # and waiting on an unread pipe would deadlock on any probe whose output
    # outgrows the pipe buffer.
    with tempfile.TemporaryFile(mode="w+") as out_file, tempfile.TemporaryFile(
        mode="w+"
    ) as err_file:
        started = time.monotonic()
        process = subprocess.Popen(  # noqa: S603 - fixed argv, no shell
            list(command), stdout=out_file, stderr=err_file, env=env
        )
        _, status, usage = os.wait4(process.pid, 0)
        wall_ms = (time.monotonic() - started) * 1000.0
        returncode = os.waitstatus_to_exitcode(status)
        # os.wait4 reaped the child behind Popen's back, so Popen still thinks
        # it is running and its finalizer emits a ResourceWarning per probe run.
        # Tell it the outcome instead: this is bookkeeping, not a second wait.
        process.returncode = returncode
        out_file.seek(0)
        err_file.seek(0)
        stdout, stderr = out_file.read(), err_file.read()

    raw_peak = usage.ru_maxrss
    peak_bytes = int(raw_peak) if platform.system() == "Darwin" else int(raw_peak) * 1024
    return {
        "stdout": stdout,
        "stderr": stderr,
        "returncode": returncode,
        "wall_ms": wall_ms,
        "peak_rss_bytes": peak_bytes,
    }


def parse_gcmetrics(stderr: str) -> dict[str, int]:
    metrics: dict[str, int] = {}
    for line in stderr.splitlines():
        match = GCMETRIC_RE.match(line.strip())
        if match:
            metrics[match.group(1)] = int(match.group(2))
    return metrics


def parse_gc_diag(stderr: str) -> dict[str, int]:
    """Aggregate ``PERRY_GC_DIAG=1`` output into deterministic counters.

    ``[gc-copy-minor] ran copied_objects=.. copied_bytes=.. promoted_objects=..
    promoted_bytes=.. freed_bytes=..`` is the evacuating minor's own accounting;
    ``[gc-step]`` marks a completed collection step. Both are pure ``eprintln!``
    diagnostics: enabling them was verified not to change ``heap_used_bytes``,
    so the traced pass observes the same collector the untraced pass measures.
    """
    counters: collections.Counter[str] = collections.Counter()
    for line in stderr.splitlines():
        match = COPY_MINOR_RE.match(line)
        if match:
            counters["minor_cycles"] += 1
            for pair in match.group(1).split():
                if "=" not in pair:
                    continue
                key, _, value = pair.partition("=")
                try:
                    counters[key] += int(value)
                except ValueError:
                    continue
        elif line.startswith(GC_STEP_PREFIX):
            counters["step_cycles"] += 1
    return {metric: int(counters.get(metric, 0)) for metric in GC_METRICS}


def parse_scan_fallbacks(stderr: str) -> dict[str, dict[str, Any]]:
    """Which conservative native-stack scans a traced run actually performed.

    ``[gc-scan-fallback] site=<name> automatic=<bool> count=<n>`` is printed
    once per scan, with ``count`` running per site, so the largest ``count`` for
    a site is how often it fired. ``automatic`` separates the scans a program
    pays for without asking (allocation-point old-gen reclaim, the nursery-churn
    slack valve, emergency reclaim) from the ones user code requested by calling
    ``gc()`` — which is the distinction ``classify`` exists to surface.
    """
    sites: dict[str, dict[str, Any]] = {}
    for line in stderr.splitlines():
        match = SCAN_FALLBACK_RE.match(line.strip())
        if not match:
            continue
        name, automatic, count = match.group(1), match.group(2) == "true", int(match.group(3))
        entry = sites.setdefault(name, {"automatic": automatic, "count": 0})
        entry["count"] = max(entry["count"], count)
    return sites


def distribution(values: Sequence[float]) -> dict[str, Any]:
    """Raw samples plus the statistics the gate reasons about.

    ``spread_pct`` is ``(max-min)/median``, and it is the number that justifies
    a tolerance band. A metric whose observed spread already approaches the band
    you want to set is not gateable; the harness records the spread so that
    judgement is auditable instead of asserted.
    """
    samples = [float(value) for value in values]
    if not samples:
        raise RatchetError("distribution has no samples")
    if any(not math.isfinite(value) for value in samples):
        raise RatchetError("distribution has a non-finite sample")
    ordered = sorted(samples)
    median = statistics.median(ordered)
    spread = ordered[-1] - ordered[0]
    return {
        "samples": [_clean(value) for value in samples],
        "sample_count": len(samples),
        "median": _clean(median),
        "min": _clean(ordered[0]),
        "max": _clean(ordered[-1]),
        "stdev": _clean(statistics.pstdev(ordered)),
        "spread": _clean(spread),
        "spread_pct": _clean(100.0 * spread / median) if median else 0,
    }


def _clean(value: float) -> int | float:
    value = float(value)
    return int(value) if value.is_integer() else round(value, 6)


def probe_sources(probes_dir: Path) -> list[Path]:
    sources = sorted(probes_dir.glob("*.ts"))
    if not sources:
        raise RatchetError(f"no probes found in {probes_dir}")
    return sources


def probe_run_env(source: Path) -> dict[str, str]:
    """The collector configuration a probe declares for its own *runs*.

    A probe states this in its own source, as ``// gc-ratchet-env: KEY=VALUE``
    comment lines::

        // gc-ratchet-env: PERRY_GC_SCAVENGE_NURSERY_MB=64

    WHY A PROBE MAY NEED ONE
    ------------------------
    Twelve of the thirteen probes run at the shipped 16 MB nursery cap, so every
    copying minor this matrix has ever exercised is small and frequent. Both
    #7472 and #7481 faulted at a *larger* Eden and neither was reachable from
    here; #7481 says so directly ("a live copying-minor correctness signal at
    exactly the cadence the ratchet probes never exercise"). A cadence is a
    property of the run, not of the source, so the only way to pin it is to let
    a probe name the run it is a probe of.

    WHY IT LIVES IN THE PROBE SOURCE
    -------------------------------
    Because it is not possible to read the workload without reading the arm.
    ``tolerances.json`` was the alternative and it separates the two files a
    reader has to hold together to know what a row means.

    WHAT IS REFUSED, AND WHY EACH
    -----------------------------
    * A name outside ``PERRY_*`` — a probe declares collector configuration, not
      ``PATH`` or ``DYLD_*``. The probe set is reviewed as workloads; it must not
      become a way to change the process the harness runs.
    * ``PERRY_CONSERVATIVE_STACK_SCAN`` / ``PERRY_GC_DIAG`` — the harness sets
      both (the first is ``classify``'s axis, the second separates the traced
      pass from the timed one). A probe that also set them would silently
      contradict the measurement instead of describing itself.
    * A repeated key — two directives disagreeing about one variable has no
      defensible reading, and picking the last would make the losing line look
      effective.

    The declaration deliberately does **not** reach ``compile_probe``. These are
    runtime knobs read through ``OnceLock`` in ``perry-runtime``, and Perry's
    object cache keys on every codegen env var (#6394), so passing them at
    compile time would change the cache key without changing a byte of emitted
    code — a difference that looks like a difference and is not one.
    """
    declared: dict[str, str] = {}
    for lineno, line in enumerate(source.read_text(encoding="utf-8").splitlines(), start=1):
        match = PROBE_ENV_RE.match(line.strip())
        if not match:
            continue
        name, value = match.group(1), match.group(2)
        if not name.startswith("PERRY_"):
            raise RatchetError(
                f"{source.name}:{lineno}: gc-ratchet-env may only set PERRY_* variables, "
                f"got {name!r}"
            )
        if name in RESERVED_PROBE_ENV:
            raise RatchetError(
                f"{source.name}:{lineno}: {name} is set by the harness itself, so a probe "
                "declaring it would contradict the measurement rather than describe it"
            )
        if name in declared:
            raise RatchetError(
                f"{source.name}:{lineno}: gc-ratchet-env sets {name} twice "
                f"({declared[name]!r} then {value!r})"
            )
        declared[name] = value
    return declared


def _with_probe_env(
    probe_env: Mapping[str, str], extra: Mapping[str, str] | None = None
) -> dict[str, str]:
    """The probe's declared env, plus whatever the harness is adding this pass.

    Layered over ``os.environ`` by ``run_once``, so a knob exported into the
    shell applies to every probe that does *not* declare it and is overridden
    for the one that does — which is what makes an ad-hoc sweep over a knob
    still leave an armed probe measuring its own arm.
    """
    merged = dict(probe_env)
    if extra:
        merged.update(extra)
    return merged


def compile_probe(perry: Path, source: Path, out_dir: Path) -> Path:
    binary = out_dir / source.stem
    completed = subprocess.run(
        [str(perry), source.name, "-o", str(binary)],
        cwd=str(source.parent),
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0 or not binary.exists():
        raise RatchetError(
            f"failed to compile {source.name}:\n{completed.stdout}\n{completed.stderr}"
        )
    return binary


def _check_against_node(node: Path | None, source: Path, actual: str) -> dict[str, Any]:
    """Diff probe stdout against the pinned Node oracle.

    Exit 0 is not correctness. A probe that silently stops allocating still
    exits 0 and reports a beautifully small retained heap, so every probe's
    observable output is diffed against Node before its metrics are trusted.
    """
    if node is None:
        return {"status": "unchecked", "reason": "no Node oracle supplied", "oracle_version": None}
    version = subprocess.run(
        [str(node), "--version"], capture_output=True, text=True, check=False
    ).stdout.strip()
    completed = subprocess.run(
        [str(node), "--expose-gc", "--experimental-strip-types", str(source)],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        return {
            "status": "unchecked",
            "reason": f"node exited {completed.returncode}: {completed.stderr.strip()[:400]}",
            "oracle_version": version,
        }
    if completed.stdout == actual:
        return {"status": "pass", "oracle_version": version, "reason": ""}
    return {
        "status": "fail",
        "oracle_version": version,
        "reason": "stdout differs from Node oracle",
        "expected": completed.stdout.splitlines(),
        "actual": actual.splitlines(),
    }


def measure(
    *, perry: Path, probes_dir: Path, repeats: int, node: Path | None, warmup: int = 1
) -> dict[str, Any]:
    """Compile and run every probe, collecting all four metric families."""
    if repeats < 3:
        raise RatchetError("at least three repeats are required to state a spread")

    sources = probe_sources(probes_dir)
    results: dict[str, Any] = {}
    with tempfile.TemporaryDirectory(prefix="gc-ratchet-") as tmp:
        out_dir = Path(tmp)
        for source in sources:
            name = source.stem
            binary = compile_probe(perry, source, out_dir)
            probe_env = probe_run_env(source)

            for _ in range(warmup):
                run_once([str(binary)], extra_env=_with_probe_env(probe_env))

            samples: dict[str, list[float]] = {metric: [] for metric in SAMPLED_METRICS}
            stdouts: list[str] = []
            for _ in range(repeats):
                run = run_once([str(binary)], extra_env=_with_probe_env(probe_env))
                if run["returncode"] != 0:
                    raise RatchetError(f"{name}: probe exited {run['returncode']}\n{run['stderr']}")
                emitted = parse_gcmetrics(run["stderr"])
                for metric in RETENTION_METRICS + ("rss_bytes",):
                    if metric not in emitted:
                        raise RatchetError(f"{name}: probe emitted no {metric}")
                    if emitted[metric] <= 0:
                        raise RatchetError(f"{name}: probe emitted non-positive {metric}")
                    samples[metric].append(float(emitted[metric]))
                samples["peak_rss_bytes"].append(float(run["peak_rss_bytes"]))
                samples["wall_ms"].append(float(run["wall_ms"]))
                stdouts.append(run["stdout"])

            if len(set(stdouts)) != 1:
                raise RatchetError(f"{name}: probe stdout is not deterministic across repeats")

            # Separate traced pass. PERRY_GC_DIAG writes one line per collection
            # phase, which perturbs wall time, so it must not share a pass with
            # the timing samples. Two traced runs are taken and required to
            # agree: that is the harness proving, every time it runs, that the
            # counters it is about to gate on are actually deterministic.
            traced = [
                parse_gc_diag(
                    run_once(
                        [str(binary)],
                        extra_env=_with_probe_env(probe_env, {"PERRY_GC_DIAG": "1"}),
                    )["stderr"]
                )
                for _ in range(2)
            ]
            if traced[0] != traced[1]:
                differing = sorted(k for k in traced[0] if traced[0][k] != traced[1][k])
                raise RatchetError(
                    f"{name}: GC counters are not deterministic across traced runs "
                    f"({', '.join(differing)}); they cannot be gated"
                )
            # Deliberately NOT rejecting minor_cycles == 0 here. A collector that
            # has stopped running copying minors at all is the single largest
            # regression this ratchet exists to catch, and it must surface as a
            # REGRESSION row against the baseline's cycle count, not as a harness
            # error that misdiagnoses it as "your probe is too small". The
            # can't-pin-a-probe-that-never-collects rule lives in
            # validate_artifact, where it belongs: pinning time, not measure time.

            metrics = {metric: distribution(samples[metric]) for metric in SAMPLED_METRICS}
            for metric in GC_METRICS:
                metrics[metric] = distribution([traced[0][metric], traced[1][metric]])

            results[name] = {
                "stdout": stdouts[0],
                "correctness": _check_against_node(node, source, stdouts[0]),
                # Recorded per probe, and compared cell-for-cell by `evaluate`.
                # Without this the arm would be invisible in the artifact: a
                # deleted directive would silently retarget the probe at the
                # default cadence and every row would still be inside its band,
                # because the numbers would have been re-pinned to the new
                # configuration by the same act that lost the old one.
                "run_env": dict(probe_env),
                "metrics": metrics,
            }

    return {
        "schema_version": SCHEMA_VERSION,
        "kind": "gc-ratchet-measurement",
        "generated_at": utc_now(),
        "platform": platform_key(),
        "host": host_description(),
        "run_config": {
            "repeats": repeats,
            "warmup": warmup,
            "traced_runs": 2,
            "probes": [source.stem for source in sources],
        },
        "probes": results,
    }


# ---------------------------------------------------------------------------
# Classifying a retention breach (#7559)
# ---------------------------------------------------------------------------


def classify(
    *, perry: Path, probes_dir: Path, repeats: int = 3, warmup: int = 1
) -> dict[str, Any]:
    """Split each probe's retention into real retention and false-root residue.

    WHY THIS EXISTS
    ---------------
    ``heap_used_bytes`` is read from ``process.memoryUsage()`` immediately after
    the probe's own explicit ``gc()``. Until ``#7558`` an explicit ``gc()`` was
    the one place in Perry that *forced* the conservative native-stack scan
    (``#4977``'s ``ManualGcScanGuard``; the production default is ``Auto``,
    which skips it). Every probe's headline retention number was therefore
    measured under a root set nothing else in the language used, and it included
    whatever the native stack happened to look like a heap pointer to at that
    instant.

    ``#7558`` removed that force, so the expected reading on every row is now
    ``excess 0`` and this command has become a *check* as well as a split: a
    non-zero ``excess`` means either a forced scan came back at ``gc()`` or an
    automatic site started firing on that workload, and ``scan_fallback_sites``
    names which. The two arms still differ in general, because
    ``PERRY_CONSERVATIVE_STACK_SCAN=off`` also disables the remaining
    (automatic, and ``perry/gc`` ``minor()``) sites.

    That residue is not small and it is not proportional. ``js_arena_stats``
    sums each arena block's **bump-pointer offset**, and a block cannot be reset
    while it holds one marked object, so a single stale stack word costs a whole
    1 MiB nursery block. Measured on ``05_closure_capture`` (#7559): the false
    residue moved 1 MiB across a 74-commit window in which the same probe's
    precise retention was byte-identical — 5,329,880 at both endpoints — which
    is a +16.44% "regression" with every collector counter at +0.00%.

    So: ``conservative`` is what the gate compares, ``precise`` is what the
    collector actually retained, and ``excess`` is the difference. A retention
    breach whose ``excess`` moved and whose ``precise`` did not is a false-root
    artifact, not a collector regression.

    WHAT IT ASSERTS
    ---------------
    Turning the scan off must not change what a probe computes. If it does, the
    scan was load-bearing for that probe's correctness and its precise number is
    not evidence about anything — so a stdout difference is an error, not a row.

    The *precise* reading must additionally be bit-identical across repeats: it
    is the number this tool asks the reader to believe, so a run-to-run spread
    in it is an error rather than a row. The *conservative* reading is allowed
    to vary and its spread is reported instead — on ``12_large_live_set`` that
    spread is the whole reason #7554 had to stop gating the cell, and hiding it
    behind an exception would remove the evidence.
    """
    if repeats < 1:
        raise RatchetError("classify needs at least one repeat per scan mode")

    sources = probe_sources(probes_dir)
    rows: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="gc-ratchet-classify-") as tmp:
        out_dir = Path(tmp)
        for source in sources:
            name = source.stem
            binary = compile_probe(perry, source, out_dir)
            probe_env = probe_run_env(source)
            for _ in range(warmup):
                run_once([str(binary)], extra_env=_with_probe_env(probe_env))

            modes: dict[str, list[dict[str, int]]] = {}
            stdouts: dict[str, str] = {}
            for label, env in (("conservative", None), ("precise", {SCAN_MODE_ENV: "off"})):
                seen: list[dict[str, int]] = []
                for _ in range(repeats):
                    run = run_once([str(binary)], extra_env=_with_probe_env(probe_env, env))
                    if run["returncode"] != 0:
                        raise RatchetError(
                            f"{name}: probe exited {run['returncode']} with "
                            f"{SCAN_MODE_ENV}={'off' if env else '<default>'}\n{run['stderr']}"
                        )
                    emitted = parse_gcmetrics(run["stderr"])
                    for metric in RETENTION_METRICS:
                        if emitted.get(metric, 0) <= 0:
                            raise RatchetError(f"{name}: probe emitted no {metric} ({label})")
                    seen.append({metric: emitted[metric] for metric in RETENTION_METRICS})
                    stdouts.setdefault(label, run["stdout"])
                modes[label] = seen

            precise_samples = [sample["heap_used_bytes"] for sample in modes["precise"]]
            if len(set(precise_samples)) != 1:
                raise RatchetError(
                    f"{name}: precise retention is not bit-identical across {repeats} runs "
                    f"({precise_samples}); it is the number this tool asks the reader to "
                    f"believe, so it may not be reported as a spread"
                )

            if stdouts["conservative"] != stdouts["precise"]:
                raise RatchetError(
                    f"{name}: probe output changes when the conservative stack scan is "
                    f"disabled, so the scan is load-bearing for this probe and its precise "
                    f"retention is not evidence about the collector"
                )

            sites = parse_scan_fallbacks(
                run_once(
                    [str(binary)],
                    extra_env=_with_probe_env(probe_env, {"PERRY_GC_DIAG": "1"}),
                )["stderr"]
            )
            conservative_samples = [sample["heap_used_bytes"] for sample in modes["conservative"]]
            conservative = int(statistics.median(conservative_samples))
            precise = precise_samples[0]
            rows.append(
                {
                    "probe": name,
                    "run_env": dict(probe_env),
                    "heap_used_bytes": conservative,
                    "heap_used_samples": conservative_samples,
                    "heap_used_spread_bytes": max(conservative_samples) - min(conservative_samples),
                    "heap_used_precise_bytes": precise,
                    "false_root_excess_bytes": conservative - precise,
                    "false_root_excess_pct": _clean(
                        100.0 * (conservative - precise) / conservative if conservative else 0.0
                    ),
                    "heap_total_bytes": modes["conservative"][0]["heap_total_bytes"],
                    "heap_total_precise_bytes": modes["precise"][0]["heap_total_bytes"],
                    "scan_fallback_sites": sites,
                    "automatic_scan_sites": sorted(
                        site for site, entry in sites.items() if entry["automatic"]
                    ),
                }
            )

    return {
        "schema_version": SCHEMA_VERSION,
        "kind": "gc-ratchet-classification",
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "platform": platform_key(),
        "host": host_description(),
        "run_config": {"repeats": repeats, "warmup": warmup, "scan_mode_env": SCAN_MODE_ENV},
        "probes": rows,
    }


def render_classification(payload: Mapping[str, Any]) -> str:
    lines = [
        "## GC ratchet retention classification",
        "",
        "`conservative` is what `heap_used_bytes` gates on: measured after the probe's",
        f"own `gc()`, which forces the conservative native-stack scan. `precise` is the",
        f"same reading with `{SCAN_MODE_ENV}=off`, i.e. the retention the",
        "collector's own roots account for. `excess` is false-root residue, and it is",
        "quantised to whole arena blocks because `heap_used_bytes` sums block offsets:",
        "one stale stack word pins a whole 1 MiB nursery block.",
        "",
        "| Probe | conservative | spread | precise | excess | excess % | scan sites |",
        "|-------|-------------:|-------:|--------:|-------:|---------:|------------|",
    ]
    for row in payload["probes"]:
        sites = ", ".join(
            f"{site}×{entry['count']}{'' if entry['automatic'] else ' (explicit gc())'}"
            for site, entry in sorted(row["scan_fallback_sites"].items())
        )
        lines.append(
            f"| `{row['probe']}` | {row['heap_used_bytes']:,} | "
            f"{row['heap_used_spread_bytes']:,} | "
            f"{row['heap_used_precise_bytes']:,} | {row['false_root_excess_bytes']:,} | "
            f"{row['false_root_excess_pct']:.2f}% | {sites or 'none'} |"
        )
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Tolerances
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Tolerance:
    """A regression band for one metric under one profile.

    ``pct`` and ``abs`` combine as ``max(|baseline| * pct/100, abs)``, not as an
    intersection. The absolute floor keeps a probe whose retained heap is
    genuinely tiny from being gated on sub-kilobyte churn; the percentage keeps
    a large probe from being handed a free multi-megabyte pass.

    ``direction`` is ``"increase"`` when only growth is a regression (retention,
    memory, time) and ``"either"`` when any drift is one. ``"either"`` is
    correct for the evacuation counters: they are a behavioural fingerprint of
    the collector, not a score, so a collector that suddenly copies fewer
    objects has changed and must be re-pinned deliberately rather than silently
    congratulated.
    """

    pct: float
    abs: float
    direction: str
    gating: bool
    rationale: str

    def allowance(self, baseline: float) -> float:
        return max(abs(baseline) * self.pct / 100.0, self.abs)

    def to_json(self) -> dict[str, Any]:
        return {
            "pct": self.pct,
            "abs": self.abs,
            "direction": self.direction,
            "gating": self.gating,
            "rationale": self.rationale,
        }


def _tolerance_from_json(metric: str, raw: Mapping[str, Any]) -> Tolerance:
    for field in ("pct", "abs", "direction", "gating", "rationale"):
        if field not in raw:
            raise RatchetError(f"tolerance for {metric} is missing {field}")
    if raw["direction"] not in ("increase", "either"):
        raise RatchetError(f"tolerance for {metric} has an invalid direction")
    if not isinstance(raw["gating"], bool):
        raise RatchetError(f"tolerance for {metric} has a non-boolean gating flag")
    if float(raw["pct"]) < 0 or float(raw["abs"]) < 0:
        raise RatchetError(f"tolerance for {metric} is negative")
    if not str(raw["rationale"]).strip():
        raise RatchetError(
            f"tolerance for {metric} has no rationale; a band without a stated reason "
            "is how gates drift until they cannot fire"
        )
    return Tolerance(
        pct=float(raw["pct"]),
        abs=float(raw["abs"]),
        direction=str(raw["direction"]),
        gating=bool(raw["gating"]),
        rationale=str(raw["rationale"]),
    )


@dataclass(frozen=True)
class ProbeOverride:
    """One (probe, metric) cell removed from the gating family, with its reason.

    ``tolerances.json`` is keyed per metric per profile, which is the right
    granularity for a *band*: the band expresses a machine class's noise floor.
    It is the wrong granularity for the question "can this metric carry a gate
    at all on this workload", because that is a property of the workload. When
    those two got conflated the only available lever was to stop gating a metric
    on all twelve probes because one of them had become sample-dependent (#7554).

    So an override may only ever *remove* a cell from the gating family, never
    add one — re-gating is the profile's job — and it may not touch the band.
    A non-gating row is still measured, still compared, and still printed; it
    just cannot turn the job red.

    ``evidence`` is mandatory and is checked, not merely stored. An exclusion
    claims a metric is not deterministic on this probe; that claim needs at
    least as many runs behind it as the inclusion it overrules, and a spread
    that is actually non-zero. A silent exclusion is how gates rot.
    """

    probe: str
    metric: str
    rationale: str
    observed_runs: int
    observed_spread: float
    measured_on: str
    issue: str

    def applied_to(self, base: Tolerance) -> Tolerance:
        """The profile's band, with gating removed and the reason substituted."""
        return Tolerance(
            pct=base.pct,
            abs=base.abs,
            direction=base.direction,
            gating=False,
            rationale=self.rationale,
        )


def _probe_override_from_json(probe: str, metric: str, raw: Mapping[str, Any]) -> ProbeOverride:
    where = f"probe_overrides.{probe}.{metric}"
    if metric not in ALL_METRICS:
        raise RatchetError(f"{where}: unknown metric")
    unknown = sorted(set(raw) - {"gating", "rationale", "evidence"})
    if unknown:
        raise RatchetError(f"{where}: unexpected field(s) {unknown}")
    if raw.get("gating") is not False:
        raise RatchetError(
            f"{where}: an override may only set gating to false. It exists to take a cell "
            "out of a gating family; putting one back is the profile's job, and a band that "
            "gates on one probe but not another belongs in the profile where it can be read."
        )
    rationale = str(raw.get("rationale", "")).strip()
    if not rationale:
        raise RatchetError(
            f"{where}: has no rationale. Recording that a cell is non-gating without "
            "recording why is a silent exclusion, which is how gates rot."
        )
    evidence = raw.get("evidence")
    if not isinstance(evidence, Mapping):
        raise RatchetError(f"{where}: has no evidence block")
    unknown_evidence = sorted(
        set(evidence) - {"observed_runs", "observed_spread", "measured_on", "issue"}
    )
    if unknown_evidence:
        raise RatchetError(f"{where}: evidence has unexpected field(s) {unknown_evidence}")
    for field in ("observed_runs", "observed_spread", "measured_on", "issue"):
        if field not in evidence:
            raise RatchetError(f"{where}: evidence is missing {field}")
    runs = int(evidence["observed_runs"])
    if runs < MIN_EXCLUSION_RUNS:
        raise RatchetError(
            f"{where}: evidence rests on {runs} runs; at least {MIN_EXCLUSION_RUNS} are "
            "required, the same number every band in this file is justified by. Fewer runs "
            "cannot distinguish a non-deterministic metric from one bad sample."
        )
    spread = float(evidence["observed_spread"])
    if spread <= 0:
        raise RatchetError(
            f"{where}: evidence records a spread of {spread:g}. A metric that was observed "
            "to be deterministic has not been shown to be ungateable; it must stay gated."
        )
    for field in ("measured_on", "issue"):
        if not str(evidence[field]).strip():
            raise RatchetError(f"{where}: evidence.{field} is blank")
    return ProbeOverride(
        probe=probe,
        metric=metric,
        rationale=rationale,
        observed_runs=runs,
        observed_spread=spread,
        measured_on=str(evidence["measured_on"]).strip(),
        issue=str(evidence["issue"]).strip(),
    )


def probe_overrides_from_json(payload: Mapping[str, Any]) -> dict[str, dict[str, ProbeOverride]]:
    """Parse the optional ``probe_overrides`` section.

    Deliberately *not* nested per profile. An override answers "is this metric
    deterministic enough to gate on this workload", which is a property of the
    probe and the collector, not of the machine the numbers were taken on. Bands
    stay per profile; gateability does not.
    """
    raw = payload.get("probe_overrides", {})
    if not isinstance(raw, Mapping):
        raise RatchetError("probe_overrides must be an object keyed by probe name")
    overrides: dict[str, dict[str, ProbeOverride]] = {}
    for probe, metrics in raw.items():
        if not isinstance(metrics, Mapping) or not metrics:
            raise RatchetError(f"probe_overrides.{probe} must be a non-empty object")
        overrides[probe] = {
            metric: _probe_override_from_json(probe, metric, entry)
            for metric, entry in metrics.items()
        }
    return overrides


def resolve_tolerance(
    profile_tolerances: Mapping[str, Tolerance],
    overrides: Mapping[str, Mapping[str, ProbeOverride]],
    probe: str,
    metric: str,
) -> Tolerance:
    override = overrides.get(probe, {}).get(metric)
    base = profile_tolerances[metric]
    return override.applied_to(base) if override else base


def gated_anywhere(
    profiles: Mapping[str, Mapping[str, Tolerance]],
    overrides: Mapping[str, Mapping[str, ProbeOverride]],
    probe: str,
    metric: str,
) -> bool:
    """True when this cell can turn the job red under at least one profile."""
    return any(
        resolve_tolerance(profiles[profile], overrides, probe, metric).gating
        for profile in profiles
    )


def tolerances_from_json(payload: Mapping[str, Any]) -> dict[str, dict[str, Tolerance]]:
    unknown_sections = sorted(set(payload) - TOLERANCE_SECTIONS)
    if unknown_sections:
        raise RatchetError(
            f"tolerances have unknown top-level section(s) {unknown_sections}; a mistyped "
            "section would be silently ignored and the gate would not do what the file says"
        )
    probe_overrides_from_json(payload)
    profiles: dict[str, dict[str, Tolerance]] = {}
    for profile in PROFILES:
        if profile not in payload:
            raise RatchetError(f"tolerances are missing the {profile!r} profile")
        entries = payload[profile]
        parsed = {
            metric: _tolerance_from_json(metric, entries[metric])
            for metric in entries
            if metric in ALL_METRICS
        }
        unknown = sorted(set(entries) - set(ALL_METRICS))
        if unknown:
            raise RatchetError(f"{profile}: tolerance for unknown metric(s) {unknown}")
        missing = [metric for metric in ALL_METRICS if metric not in parsed]
        if missing:
            raise RatchetError(f"{profile}: no tolerance for {', '.join(missing)}")
        if not any(tolerance.gating for tolerance in parsed.values()):
            raise RatchetError(
                f"{profile}: no metric is gating, so this profile could never fail. "
                "A gate that cannot fail is not a gate."
            )
        profiles[profile] = parsed
    return profiles


# ---------------------------------------------------------------------------
# Artifact
# ---------------------------------------------------------------------------


def assemble(
    *,
    measurement: Mapping[str, Any],
    tolerances_payload: Mapping[str, Any],
    perry: Path,
    commit: str,
    suite: Mapping[str, Any] | None,
    notes: str,
) -> dict[str, Any]:
    tolerances_from_json(tolerances_payload)
    artifact = {
        "schema_version": SCHEMA_VERSION,
        "kind": "gc-ratchet-baseline",
        "artifact_id": "gc-ratchet-v1",
        "not_the_public_baseline": (
            "Internal Perry-vs-Perry GC ratchet. The public Node/Bun evidence is "
            "benchmarks/results/public-node-bun-v1.json, owned by "
            "benchmarks/run_public_baseline.sh. Never regenerate one from the other."
        ),
        "commit": commit,
        # Rebase-stable companion to `commit`. A gc-ratchet re-pin is written on
        # a branch and REBASED at merge time (the maintainer adds the version
        # bump), which orphans the commit the pin recorded: #7666's artifact
        # named `a8f73122d`, a hash that is not in `main`'s history. Provenance
        # stayed single and uniform -- this is not #7652's substance -- but a
        # future reader attributing a moved cell looks up a commit that does not
        # exist, and that recurs on EVERY pin.
        #
        # `crates/`'s tree hash identifies exactly the code that was measured. It
        # survives both the rebase and the version bump (which touches only
        # Cargo.toml / Cargo.lock / CLAUDE.md), so it still resolves after the
        # branch is gone: `git rev-parse <any-commit>:crates` on a candidate
        # commit either matches or does not.
        "code_tree": code_tree_hash(),
        "generated_at": utc_now(),
        "platform": measurement["platform"],
        "host": measurement["host"],
        "toolchain": toolchain_description(perry),
        "run_config": dict(measurement["run_config"]),
        "tolerances": dict(tolerances_payload),
        "notes": notes,
        "probes": measurement["probes"],
        "suite": suite,
    }
    validate_artifact(artifact)
    return artifact


def _validate_probe_overrides(
    profiles: Mapping[str, Mapping[str, Tolerance]],
    overrides: Mapping[str, Mapping[str, ProbeOverride]],
    probes: Mapping[str, Any],
) -> None:
    """Cross-check the override set against the probes it claims to describe.

    Two rules, both aimed at the same rot. An override that matches nothing is a
    failure, not a no-op — the same rule ``scripts/gc_root_dominance_allowlist.json``
    carries, so deleting a probe (or fixing the non-determinism and renaming it)
    forces the exclusion to be revisited instead of outliving its reason. And an
    override set that covers every probe for a metric has achieved, one cell at a
    time, exactly what ``"gating": false`` at profile level would have done, only
    without saying so anywhere a reader would look.
    """
    for probe, metrics in sorted(overrides.items()):
        if probe not in probes:
            raise RatchetError(
                f"probe_overrides names {probe!r}, which is not in this artifact "
                f"(probes: {', '.join(sorted(probes))}). An exclusion that matches nothing "
                "must be deleted, not left behind to outlive its reason."
            )
        for metric in sorted(metrics):
            if metric not in probes[probe].get("metrics", {}):
                raise RatchetError(f"probe_overrides.{probe}.{metric} is not a recorded metric")

    for profile, entries in sorted(profiles.items()):
        for metric, tolerance in sorted(entries.items()):
            if not tolerance.gating:
                continue
            if not any(
                resolve_tolerance(entries, overrides, probe, metric).gating for probe in probes
            ):
                raise RatchetError(
                    f"{profile}: {metric} is marked gating but every probe overrides it to "
                    "non-gating, so it can never fail. Say that once at profile level, with "
                    "the reason, instead of assembling it out of per-probe exclusions."
                )


def inspect_artifact(artifact: Mapping[str, Any]) -> list[ArtifactDefect]:
    """Collect *every* defect in the pinned artifact, each tagged with its scope.

    This never stops at the first problem. Two reasons, and the second is the
    one that cost real coverage.

    A maintainer re-pinning an artifact wants the whole list, not a fixpoint loop
    where each run reveals one more thing. And, more importantly, an aborting
    validator cannot distinguish "this artifact is unusable" from "one cell of
    this artifact is unusable" — so it treated the second as the first, and
    #7554's single bad cell zeroed the gate's coverage for three days. See
    ``ArtifactDefect`` for the scope taxonomy and what each scope voids.

    Unreadable *tolerances* are the one thing that still raises rather than
    returning a defect: without parseable bands there is no gating family to
    scope a defect against, so there is nothing to be partial about.
    """
    defects: list[ArtifactDefect] = []

    def artifact_defect(message: str) -> None:
        defects.append(ArtifactDefect(scope="artifact", message=message))

    def probe_defect(probe: str, message: str) -> None:
        defects.append(ArtifactDefect(scope="probe", message=message, probe=probe))

    def cell_defect(probe: str, metric: str, message: str) -> None:
        defects.append(ArtifactDefect(scope="cell", message=message, probe=probe, metric=metric))

    # Identity and shape. Each of these makes everything below it unreadable, so
    # they short-circuit — a defect list built from a payload that is not even a
    # baseline would be noise, not information.
    if artifact.get("schema_version") != SCHEMA_VERSION:
        artifact_defect(f"unsupported schema_version {artifact.get('schema_version')!r}")
        return defects
    if artifact.get("kind") != "gc-ratchet-baseline":
        artifact_defect("artifact is not a gc-ratchet baseline")
        return defects
    for field in ("commit", "generated_at", "platform"):
        if not isinstance(artifact.get(field), str) or not artifact[field].strip():
            artifact_defect(f"artifact has an invalid {field}")
    probes = artifact.get("probes")
    if not isinstance(probes, Mapping) or not probes:
        artifact_defect("artifact records no probes")
        return defects
    expected = artifact.get("run_config", {}).get("probes")
    if not isinstance(expected, list) or sorted(expected) != sorted(probes):
        artifact_defect("artifact probe set does not match its run_config")

    tolerance_payload = artifact.get("tolerances", {})
    profiles = tolerances_from_json(tolerance_payload)
    overrides = probe_overrides_from_json(tolerance_payload)
    _validate_probe_overrides(profiles, overrides, probes)

    for name, entry in probes.items():
        metrics = entry.get("metrics")
        if not isinstance(metrics, Mapping):
            artifact_defect(f"{name}: no metrics recorded")
            continue

        # `run_env` is what tells a reader which collector these numbers came
        # from, and `evaluate` compares it. An unreadable one would compare
        # unequal against every well-formed run and read as a regression in the
        # collector rather than as corruption of the artifact, so it is fatal
        # for the same reason a tampered summary is.
        recorded_env = entry.get("run_env", {})
        if not isinstance(recorded_env, Mapping) or not all(
            isinstance(key, str) and isinstance(value, str) for key, value in recorded_env.items()
        ):
            artifact_defect(f"{name}: run_env is not a string-to-string mapping")

        # Integrity of the recorded numbers. A missing metric or a summary that
        # disagrees with its own samples is tampering or corruption, not
        # unfitness: it stays fatal, because a partially-trusted artifact is not
        # a thing this gate should ever compare against.
        unreadable = False
        for metric in ALL_METRICS:
            if metric not in metrics:
                artifact_defect(f"{name}: baseline is missing {metric}")
                unreadable = True
                continue
            recorded = metrics[metric]
            samples = recorded.get("samples")
            if not isinstance(samples, list) or len(samples) < 2:
                artifact_defect(f"{name}: {metric} has too few samples")
                unreadable = True
                continue
            if recorded != distribution(samples):
                artifact_defect(f"{name}: {metric} summary is inconsistent with its samples")
                unreadable = True
        if unreadable:
            continue

        # A baseline may only be pinned from an oracle-verified run: "unchecked"
        # is as unacceptable here as "fail", because the whole artifact's
        # authority rests on the probes having been shown to compute the right
        # thing at the moment they were frozen. Scoped to the probe: an
        # unverified probe is not evidence, but it says nothing about the other
        # eleven.
        if entry.get("correctness", {}).get("status") != "pass":
            probe_defect(
                name,
                "baseline was pinned without a passing Node oracle diff "
                f"(status={entry.get('correctness', {}).get('status')!r}), so this probe's "
                "rows are not evidence about the collector",
            )
        if metrics["minor_cycles"]["median"] < 1:
            probe_defect(
                name,
                "baseline pinned a probe that ran no minor collection, so there is no "
                "evacuating-minor behaviour here to ratchet against",
            )

        # The bit-identity rule, enforced at PINNING time rather than only in
        # the unit tests. It used to live only in tests/test_gc_ratchet.py, so
        # #7446 was able to write an artifact whose 12_large_live_set retention
        # spread was 6,768 bytes; the test then failed in the CI step that runs
        # *before* the measurement step, and the ratchet measured nothing at all
        # for three days (#7554). It is scoped to the CELL because that is the
        # size of the claim: this metric on this workload is not bit-identical.
        # `assemble` still refuses outright (see `validate_artifact`), so a
        # maintainer cannot pin one by accident; `check` demotes it and carries
        # on, so an artifact that is already in the tree cannot zero the gate.
        for metric in DETERMINISTIC_METRICS:
            spread = metrics[metric]["spread"]
            if spread and gated_anywhere(profiles, overrides, name, metric):
                cell_defect(
                    name,
                    metric,
                    f"spread {spread:g} when pinned, but its band is justified by "
                    "bit-identity, not by a noise allowance. Either re-pin on a quiet host, "
                    "or take this one cell out of the gating family with a probe_overrides "
                    "entry that records the evidence.",
                )

    return defects


def validate_artifact(artifact: Mapping[str, Any]) -> None:
    """Refuse an artifact with ANY defect. This is the PIN-time contract.

    ``assemble`` calls this, so a maintainer cannot freeze an unfit artifact:
    the failure lands on their machine at the moment the judgement is being
    made. ``check`` deliberately does not call it — an artifact already in the
    tree must not be able to zero the gate's coverage, so there the non-fatal
    defects demote cells instead of aborting the run. Same defects, different
    blast radius, because pinning and comparing are different acts.
    """
    defects = inspect_artifact(artifact)
    if defects:
        raise RatchetError("; ".join(defect.message for defect in defects))


# ---------------------------------------------------------------------------
# Regression check
# ---------------------------------------------------------------------------


def _render_run_env(env: Mapping[str, str]) -> str:
    """A probe's declared arm, printable. ``<default>`` when it declares none."""
    if not env:
        return "<default>"
    return " ".join(f"{key}={value}" for key, value in sorted(env.items()))


@dataclass
class Row:
    probe: str
    metric: str
    baseline: float
    current: float
    delta: float
    delta_pct: float | None
    allowance: float
    gating: bool
    status: str


def evaluate(
    baseline: Mapping[str, Any],
    current: Mapping[str, Any],
    *,
    profile: str,
    allow_platform_mismatch: bool = False,
) -> tuple[list[Row], list[str]]:
    """Compare a fresh measurement against the pinned artifact.

    Failures come from two places on purpose: threshold breaches, and integrity
    problems. A missing probe, a probe whose output no longer matches the Node
    oracle, or too few repeats is a *failure*, not a skip. A gate that silently
    drops the workload it was watching is exactly the shape of the ``gc-stress``
    hole this ratchet exists to close — that job was ``continue-on-error: true``
    and a regression sat behind it through three merges.

    An unfit *pinned* cell is handled differently from an unfit measurement, and
    this is the #7554 repair. It does not abort: it demotes that cell (or probe)
    out of the gating family for this run and is reported as a failure like any
    other. So the run still measures all twelve probes, still evaluates the other
    143 cells, and still names a regression anywhere else in the matrix — while
    the defect itself keeps the job red. Aborting instead is what made one bad
    cell cost three days of total coverage.
    """
    defects = inspect_artifact(baseline)
    fatal = [defect for defect in defects if defect.fatal]
    if fatal:
        raise RatchetError("; ".join(defect.message for defect in fatal))
    unfit = [defect for defect in defects if not defect.fatal]
    unfit_probes = {defect.probe for defect in unfit if defect.scope == "probe"}
    unfit_cells = {(defect.probe, defect.metric) for defect in unfit if defect.scope == "cell"}

    if profile not in PROFILES:
        raise RatchetError(f"unknown profile {profile!r}; expected one of {PROFILES}")
    if current.get("kind") != "gc-ratchet-measurement":
        raise RatchetError("current payload is not a gc-ratchet measurement")

    failures: list[str] = []
    rows: list[Row] = []
    tolerances = tolerances_from_json(baseline["tolerances"])[profile]
    overrides = probe_overrides_from_json(baseline["tolerances"])

    # Reported first, so the reason a cell shows up demoted in the table is
    # already on screen by the time the reader reaches it.
    for defect in unfit:
        failures.append(defect.describe())

    if baseline["platform"] != current.get("platform"):
        message = (
            f"platform mismatch: baseline {baseline['platform']!r} vs "
            f"current {current.get('platform')!r}"
        )
        failures.append(f"NOTE (non-gating): {message}" if allow_platform_mismatch else message)

    baseline_repeats = int(baseline["run_config"]["repeats"])
    current_repeats = int(current.get("run_config", {}).get("repeats", 0))
    if current_repeats < baseline_repeats:
        failures.append(
            f"current run used {current_repeats} repeats, baseline pinned {baseline_repeats}"
        )

    baseline_probes = baseline["probes"]
    current_probes = current.get("probes", {})
    missing = sorted(set(baseline_probes) - set(current_probes))
    unexpected = sorted(set(current_probes) - set(baseline_probes))
    if missing:
        failures.append(f"probes missing from this run: {', '.join(missing)}")
    if unexpected:
        failures.append(
            f"probes present now but absent from the baseline: {', '.join(unexpected)} "
            "(regenerate the artifact deliberately)"
        )

    for name in sorted(set(baseline_probes) & set(current_probes)):
        base_entry = baseline_probes[name]
        cur_entry = current_probes[name]

        if cur_entry.get("stdout") != base_entry.get("stdout"):
            failures.append(f"{name}: observable output changed since the baseline")

        # The arm is part of what the row means. `13_large_eden_survivors` is
        # pinned at `PERRY_GC_SCAVENGE_NURSERY_MB=64` because its whole subject
        # is the large-Eden cadence (#7481); at the default 16 MB it is a
        # perfectly healthy probe of something else, and every band would still
        # be satisfied by the re-pin that lost the arm. So the declared
        # environment is compared like a metric: a run under a different one is
        # not a comparison, it is two measurements of two collectors.
        base_env = base_entry.get("run_env") or {}
        cur_env = cur_entry.get("run_env") or {}
        if base_env != cur_env:
            failures.append(
                f"{name}: ran under a different collector configuration than the baseline "
                f"pinned ({_render_run_env(cur_env)} vs {_render_run_env(base_env)}); the "
                "rows below compare two different collectors. Restore the probe's "
                "`gc-ratchet-env` directive, or re-pin deliberately."
            )

        # "unchecked" is a failure, not a pass. A run that could not reach the
        # Node oracle has not verified that the probe still computes anything;
        # its retained-heap numbers could just as well come from a probe that
        # silently stopped allocating. "We did not verify" must never be
        # indistinguishable from "verified".
        correctness = cur_entry.get("correctness", {})
        status = correctness.get("status")
        if status == "fail":
            failures.append(f"{name}: probe output no longer matches the Node oracle")
        elif status != "pass":
            reason = correctness.get("reason") or "no correctness report"
            failures.append(f"{name}: correctness was not verified against the Node oracle ({reason})")

        # Liveness, asserted rather than inferred from the bands. A ratchet over
        # the evacuating minor is meaningless if the evacuating minor did not
        # run, and the tolerance arithmetic cannot be relied on to notice: six
        # of the twelve probes pin `minor_cycles` at 1, where the allowance
        # floor is also 1, so a collapse from 1 to 0 is `delta == -allowance`
        # and scores "ok". A probe that stopped collecting would have been
        # reported as passing — CLAUDE.md's fourth failure mode (the gate runs
        # but its subject never did) sitting inside the gate meant to close it.
        #
        # ★ #7558: the second probe is `copied_objects + promoted_objects`, not
        # `copied_objects`. Both counters are parsed from the SAME
        # `[gc-copy-minor] ran` line — they are the evacuating minor's own
        # accounting of where it put each survivor (survivor space vs old-gen),
        # so their sum is "objects the copying minor MOVED" and either one alone
        # is a destination, not a liveness signal.
        #
        # This is not a loosening. `copied_objects` keeps its own two-sided 5%
        # band, so a workload that stops copying and starts promoting is still a
        # -100% REGRESSION row on the fingerprint; what changes is only that it
        # is no longer *also* reported as "the collector did not run". #7558 hit
        # exactly that: removing explicit `gc()`'s conservative scan re-enabled
        # the adaptive-tenuring seed on `gc()`-driven workloads (the seed
        # deliberately refuses input from a conservatively-scanned cycle —
        # `gc/tenuring.rs`), `tenuring_survivals` fell 4 -> 1 on two probes, and
        # every survivor went straight to old-gen. `copied_objects` 5,823 -> 0
        # with `promoted_objects` 0 -> 6,077 is a copying minor that moved MORE,
        # not one that stopped.
        #
        # It also removes a hole the re-pin would otherwise have opened: pinning
        # `copied_objects = 0` on those two probes would make the old rule's
        # `base > 0` guard permanently false there, i.e. a liveness assertion
        # that can no longer fail on the probes that most recently exercised it.
        moved = {
            key: (
                float(entry["metrics"]["copied_objects"]["median"])
                + float(entry["metrics"]["promoted_objects"]["median"])
            )
            for key, entry in (("base", base_entry), ("cur", cur_entry))
        }
        for metric, what, base_value, cur_value in (
            (
                "minor_cycles",
                "ran no minor collection",
                float(base_entry["metrics"]["minor_cycles"]["median"]),
                float(cur_entry["metrics"]["minor_cycles"]["median"]),
            ),
            (
                "copied_objects+promoted_objects",
                "evacuated nothing (the copying minor moved no object, to survivor "
                "space or to old-gen)",
                moved["base"],
                moved["cur"],
            ),
        ):
            if base_value > 0 and cur_value <= 0:
                failures.append(
                    f"{name}: {what} in this run ({metric} "
                    f"{base_value:,.0f} -> 0). The baseline it is "
                    "being compared against measures a collector that did; there is nothing "
                    "here to compare."
                )

        for metric in ALL_METRICS:
            tolerance = resolve_tolerance(tolerances, overrides, name, metric)
            # A cell the pinned artifact cannot support is demoted rather than
            # trusted: comparing against a number whose own premise failed would
            # dress a defect up as a verdict. The defect is already in
            # `failures`, so demoting it here loses no red.
            quarantined = name in unfit_probes or (name, metric) in unfit_cells
            if quarantined:
                tolerance = dataclasses.replace(tolerance, gating=False)
            base_median = float(base_entry["metrics"][metric]["median"])
            cur_median = float(cur_entry["metrics"][metric]["median"])
            delta = cur_median - base_median
            delta_pct = (100.0 * delta / base_median) if base_median else None
            allowance = tolerance.allowance(base_median)

            if delta > allowance:
                breach = True
            elif delta < -allowance:
                breach = tolerance.direction == "either"
            else:
                breach = False

            if breach and quarantined:
                status = "UNFIT (pinned cell unusable)"
            elif breach:
                status = "REGRESSION" if tolerance.gating else "drift (informational)"
            elif quarantined:
                status = "unfit (pinned cell unusable)"
            elif delta < -allowance:
                status = "improvement"
            else:
                status = "ok"

            rows.append(
                Row(
                    probe=name,
                    metric=metric,
                    baseline=base_median,
                    current=cur_median,
                    delta=delta,
                    delta_pct=delta_pct,
                    allowance=allowance,
                    gating=tolerance.gating,
                    status=status,
                )
            )
            if status == "REGRESSION":
                shown = "n/a" if delta_pct is None else f"{delta_pct:+.2f}%"
                failures.append(
                    f"{name}: {metric} {base_median:,.0f} -> {cur_median:,.0f} ({shown}), "
                    f"allowance {allowance:,.0f} [{tolerance.direction}]"
                )

    return rows, failures


def render(rows: Iterable[Row], baseline: Mapping[str, Any], profile: str) -> str:
    host = baseline.get("host", {})
    lines = [
        "## GC ratchet",
        "",
        f"- Profile: `{profile}`",
        f"- Baseline commit: `{baseline['commit']}`",
        f"- Baseline captured: {baseline['generated_at']} on `{baseline['platform']}`",
        f"- Baseline host: {host.get('cpu_brand') or host.get('machine')}, "
        f"{host.get('cpu_count')} cores, load at capture "
        f"{host.get('load_average', {}).get('1m')}",
        "",
        "Only rows marked gating can fail the job. Non-gating rows are recorded so a",
        "drift that is real but unmeasurable on this runner is still visible.",
        "",
    ]
    # Named where the numbers are read, not only in the probe source: a row from
    # a non-default arm answers a different question from its neighbours, and
    # the reader of a CI table has no other way to know which.
    armed = {
        name: entry["run_env"]
        for name, entry in sorted(baseline.get("probes", {}).items())
        if entry.get("run_env")
    }
    if armed:
        lines += [
            "Probes pinned under a non-default collector configuration:",
            "",
            *(f"- `{name}` — `{_render_run_env(env)}`" for name, env in armed.items()),
            "",
        ]
    lines += [
        "| Probe | Metric | Baseline | Current | Δ | Allowance | Gating | Status |",
        "|-------|--------|---------:|--------:|---:|----------:|:------:|--------|",
    ]
    for row in rows:
        delta = "-" if row.delta_pct is None else f"{row.delta_pct:+.2f}%"
        lines.append(
            f"| `{row.probe}` | {row.metric} | {row.baseline:,.0f} | {row.current:,.0f} | "
            f"{delta} | {row.allowance:,.0f} | {'yes' if row.gating else 'no'} | {row.status} |"
        )
    # An "unfit" row is a *defect in the baseline*, not a property of this run,
    # and the two are easy to confuse in a 144-row table. Name them separately
    # with what has to happen to clear them.
    unfit = [defect for defect in inspect_artifact(baseline) if not defect.fatal]
    if unfit:
        lines += [
            "",
            "### Pinned cells that could not be gated (baseline defects)",
            "",
            "These are demoted for this run so one bad cell cannot zero the gate's",
            "coverage (#7554). They still fail the job — fix by re-pinning on a quiet",
            "host, or by recording a `probe_overrides` entry with its evidence.",
            "",
        ]
        for defect in sorted(unfit, key=lambda d: (d.scope, d.where)):
            lines.append(f"- {defect.describe()}")

    # Print the exclusions with their reasons on every run. A reader who sees a
    # "no" in the Gating column must be able to find out why it is a no without
    # opening another file, or the exclusion is effectively invisible.
    overrides = probe_overrides_from_json(baseline.get("tolerances", {}))
    if overrides:
        lines += ["", "### Cells excluded from the gating family by probe override", ""]
        for probe in sorted(overrides):
            for metric in sorted(overrides[probe]):
                override = overrides[probe][metric]
                lines.append(
                    f"- `{probe}`.{metric} — {override.rationale} "
                    f"(observed spread {override.observed_spread:,.0f} over "
                    f"{override.observed_runs} runs, {override.measured_on}; {override.issue})"
                )
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _load(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise RatchetError(f"cannot read {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise RatchetError(f"{path} is not valid JSON: {exc}") from exc


def _write(path: Path, payload: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def _resolve_node(explicit: str | None) -> Path | None:
    if explicit:
        return Path(explicit)
    pinned = REPO_ROOT / ".node-version"
    if pinned.exists():
        version = pinned.read_text(encoding="utf-8").strip().lstrip("v")
        arch = "arm64" if platform.machine() == "arm64" else "x64"
        candidate = (
            Path.home()
            / f"node-v{version}-{platform.system().lower()}-{arch}"
            / "bin"
            / "node"
        )
        if candidate.exists():
            return candidate
    found = shutil.which("node")
    return Path(found) if found else None


def cmd_measure(args: argparse.Namespace) -> int:
    perry = Path(args.perry).resolve()
    if not perry.exists():
        raise RatchetError(f"perry binary not found at {perry}")
    payload = measure(
        perry=perry,
        probes_dir=Path(args.probes_dir).resolve(),
        repeats=args.repeats,
        node=_resolve_node(args.node),
        warmup=args.warmup,
    )
    _write(Path(args.output), payload)
    load = payload["host"]["load_average"]
    print(f"gc-ratchet: wrote measurement to {args.output}")
    print(f"  host {payload['host'].get('hostname')} load {load['1m']}/{load['5m']}/{load['15m']}")
    for name, entry in payload["probes"].items():
        spreads = " ".join(
            f"{metric}={entry['metrics'][metric]['spread_pct']}%" for metric in SAMPLED_METRICS
        )
        print(f"  {name}: correctness={entry['correctness']['status']} {spreads}")
    return 0


def cmd_classify(args: argparse.Namespace) -> int:
    perry = Path(args.perry).resolve()
    if not perry.exists():
        raise RatchetError(f"perry binary not found at {perry}")
    payload = classify(
        perry=perry,
        probes_dir=Path(args.probes_dir).resolve(),
        repeats=args.repeats,
        warmup=args.warmup,
    )
    if args.output:
        _write(Path(args.output), payload)
        print(f"gc-ratchet: wrote classification to {args.output}")
    print(render_classification(payload))
    return 0


def cmd_assemble(args: argparse.Namespace) -> int:
    artifact = assemble(
        measurement=_load(Path(args.measurement)),
        tolerances_payload=_load(Path(args.tolerances)),
        perry=Path(args.perry).resolve(),
        commit=args.commit,
        suite=_load(Path(args.suite)) if args.suite else None,
        notes=args.notes,
    )
    _write(Path(args.output), artifact)
    print(f"gc-ratchet: wrote pinned artifact to {args.output}")
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    baseline = _load(Path(args.artifact))
    current = _load(Path(args.current))
    rows, failures = evaluate(
        baseline,
        current,
        profile=args.profile,
        allow_platform_mismatch=args.allow_platform_mismatch,
    )
    report = render(rows, baseline, args.profile)
    print(report)
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    hard = [failure for failure in failures if not failure.startswith("NOTE")]
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as handle:
            handle.write(report)
            if failures:
                handle.write("\n**Findings**\n\n")
                for failure in failures:
                    handle.write(f"- {failure}\n")
            if hard:
                handle.write(
                    "\nRe-pin only if this shift is intentional: see "
                    "`benchmarks/gc_ratchet/README.md`.\n"
                )
    if hard:
        print("gc-ratchet: FAILED", file=sys.stderr)
        for failure in hard:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    for failure in failures:
        print(f"gc-ratchet: {failure}")
    print("gc-ratchet: OK")
    return 0


def cmd_validate(args: argparse.Namespace) -> int:
    """Report artifact defects, at a scope the caller chooses.

    ``--scope all`` (the default, and what a maintainer wants) fails on any
    defect. ``--scope structural`` fails only on defects that void the whole
    artifact, and is what the CI preflight uses.

    The distinction is the #7554 repair. Preflight runs BEFORE the measurement
    step, so anything it fails on costs the entire run's coverage — twelve
    probes that never execute. That price is right for "this artifact is
    unreadable or tampered with" and wrong for "one of its 144 cells is not
    bit-identical". Under ``structural`` the latter is printed loudly and passed
    over, and ``check`` then fails on it *after* the probes have run.

    This is not a hole: ``check`` re-derives the same defect list and reports
    every one of them as a failure, so nothing ``structural`` waves through can
    reach a green job. ``test_structural_preflight_defers_every_defect_it_waves_through``
    asserts exactly that coupling, one planted defect at a time — without it,
    this flag would be indistinguishable from suppression.
    """
    artifact = _load(Path(args.artifact))
    defects = inspect_artifact(artifact)
    fatal = [defect for defect in defects if defect.fatal]
    unfit = [defect for defect in defects if not defect.fatal]

    for defect in unfit:
        print(f"gc-ratchet: {defect.describe()}", file=sys.stderr)
    if fatal:
        raise RatchetError("; ".join(defect.message for defect in fatal))
    if unfit and args.scope == "all":
        print(
            f"gc-ratchet: {args.artifact} has {len(unfit)} unfit cell(s); "
            "re-pin them or record a probe_overrides entry",
            file=sys.stderr,
        )
        return 1
    if unfit:
        print(
            f"gc-ratchet: {len(unfit)} unfit cell(s) deferred to `check` "
            "(--scope structural); they will fail the job there, after the probes run",
            file=sys.stderr,
        )
    print(f"gc-ratchet: {args.artifact} is structurally valid")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="GC retention/memory ratchet")
    sub = parser.add_subparsers(dest="command", required=True)

    measure_cmd = sub.add_parser("measure", help="run the probes and record metrics")
    measure_cmd.add_argument("--perry", required=True)
    measure_cmd.add_argument("--probes-dir", default=str(PROBES_DIR))
    measure_cmd.add_argument("--repeats", type=int, default=7)
    measure_cmd.add_argument("--warmup", type=int, default=1)
    measure_cmd.add_argument("--node", default=None)
    measure_cmd.add_argument("--output", required=True)
    measure_cmd.set_defaults(func=cmd_measure)

    classify_cmd = sub.add_parser(
        "classify",
        help="split each probe's retention into real retention and false-root residue",
    )
    classify_cmd.add_argument("--perry", required=True)
    classify_cmd.add_argument("--probes-dir", default=str(PROBES_DIR))
    classify_cmd.add_argument("--repeats", type=int, default=3)
    classify_cmd.add_argument("--warmup", type=int, default=1)
    classify_cmd.add_argument("--output", default=None)
    classify_cmd.set_defaults(func=cmd_classify)

    assemble_cmd = sub.add_parser("assemble", help="build the pinned baseline artifact")
    assemble_cmd.add_argument("--measurement", required=True)
    assemble_cmd.add_argument("--tolerances", default=str(DEFAULT_TOLERANCES))
    assemble_cmd.add_argument("--perry", required=True)
    assemble_cmd.add_argument("--commit", required=True)
    assemble_cmd.add_argument("--suite", default=None)
    assemble_cmd.add_argument("--notes", default="")
    assemble_cmd.add_argument("--output", default=str(DEFAULT_ARTIFACT))
    assemble_cmd.set_defaults(func=cmd_assemble)

    check_cmd = sub.add_parser("check", help="fail on regression against the baseline")
    check_cmd.add_argument("--artifact", default=str(DEFAULT_ARTIFACT))
    check_cmd.add_argument("--current", required=True)
    check_cmd.add_argument("--profile", choices=PROFILES, default="shared_ci")
    check_cmd.add_argument("--allow-platform-mismatch", action="store_true")
    check_cmd.set_defaults(func=cmd_check)

    validate_cmd = sub.add_parser("validate", help="structural check of the pinned artifact")
    validate_cmd.add_argument("--artifact", default=str(DEFAULT_ARTIFACT))
    validate_cmd.add_argument(
        "--scope",
        choices=("all", "structural"),
        default="all",
        help=(
            "'all' fails on any artifact defect (default; what a maintainer wants). "
            "'structural' fails only on defects that void the whole artifact, leaving "
            "per-cell defects for `check` to report after the probes have run — so one "
            "unfit cell cannot zero the gate's coverage (#7554)."
        ),
    )
    validate_cmd.set_defaults(func=cmd_validate)

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return int(args.func(args))
    except RatchetError as exc:
        print(f"gc-ratchet error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
