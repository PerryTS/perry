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
GC_STEP_PREFIX = "[gc-step]"

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

PROFILES = ("shared_ci", "pinned_host")


class RatchetError(RuntimeError):
    """Raised when measurement, artifact validation, or comparison fails."""


# ---------------------------------------------------------------------------
# Environment description
# ---------------------------------------------------------------------------


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
        return {"path": str(path), "size": path.stat().st_size, "sha256": hasher.hexdigest()}

    runtime_dir = os.environ.get("PERRY_RUNTIME_DIR")
    search = Path(runtime_dir) if runtime_dir else perry.parent
    out: dict[str, Any] = {"perry": digest(perry), "runtime_dir": str(search)}
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

            for _ in range(warmup):
                run_once([str(binary)])

            samples: dict[str, list[float]] = {metric: [] for metric in SAMPLED_METRICS}
            stdouts: list[str] = []
            for _ in range(repeats):
                run = run_once([str(binary)])
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
                    run_once([str(binary)], extra_env={"PERRY_GC_DIAG": "1"})["stderr"]
                )
                for _ in range(2)
            ]
            if traced[0] != traced[1]:
                differing = sorted(k for k in traced[0] if traced[0][k] != traced[1][k])
                raise RatchetError(
                    f"{name}: GC counters are not deterministic across traced runs "
                    f"({', '.join(differing)}); they cannot be gated"
                )
            if traced[0]["minor_cycles"] < 1:
                raise RatchetError(
                    f"{name}: probe triggered no minor collection — it is not exercising "
                    "the collector and must be rescaled before it can be pinned"
                )

            metrics = {metric: distribution(samples[metric]) for metric in SAMPLED_METRICS}
            for metric in GC_METRICS:
                metrics[metric] = distribution([traced[0][metric], traced[1][metric]])

            results[name] = {
                "stdout": stdouts[0],
                "correctness": _check_against_node(node, source, stdouts[0]),
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


def tolerances_from_json(payload: Mapping[str, Any]) -> dict[str, dict[str, Tolerance]]:
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


def validate_artifact(artifact: Mapping[str, Any]) -> None:
    if artifact.get("schema_version") != SCHEMA_VERSION:
        raise RatchetError(f"unsupported schema_version {artifact.get('schema_version')!r}")
    if artifact.get("kind") != "gc-ratchet-baseline":
        raise RatchetError("artifact is not a gc-ratchet baseline")
    for field in ("commit", "generated_at", "platform"):
        if not isinstance(artifact.get(field), str) or not artifact[field].strip():
            raise RatchetError(f"artifact has an invalid {field}")
    probes = artifact.get("probes")
    if not isinstance(probes, Mapping) or not probes:
        raise RatchetError("artifact records no probes")
    expected = artifact.get("run_config", {}).get("probes")
    if not isinstance(expected, list) or sorted(expected) != sorted(probes):
        raise RatchetError("artifact probe set does not match its run_config")
    tolerances_from_json(artifact.get("tolerances", {}))
    for name, entry in probes.items():
        metrics = entry.get("metrics")
        if not isinstance(metrics, Mapping):
            raise RatchetError(f"{name}: no metrics recorded")
        for metric in ALL_METRICS:
            if metric not in metrics:
                raise RatchetError(f"{name}: baseline is missing {metric}")
            recorded = metrics[metric]
            samples = recorded.get("samples")
            if not isinstance(samples, list) or len(samples) < 2:
                raise RatchetError(f"{name}: {metric} has too few samples")
            if recorded != distribution(samples):
                raise RatchetError(f"{name}: {metric} summary is inconsistent with its samples")
        if entry.get("correctness", {}).get("status") == "fail":
            raise RatchetError(f"{name}: baseline pinned a probe whose output is wrong")
        if metrics["minor_cycles"]["median"] < 1:
            raise RatchetError(f"{name}: baseline pinned a probe that ran no minor collection")


# ---------------------------------------------------------------------------
# Regression check
# ---------------------------------------------------------------------------


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
    """
    validate_artifact(baseline)
    if profile not in PROFILES:
        raise RatchetError(f"unknown profile {profile!r}; expected one of {PROFILES}")
    if current.get("kind") != "gc-ratchet-measurement":
        raise RatchetError("current payload is not a gc-ratchet measurement")

    failures: list[str] = []
    rows: list[Row] = []
    tolerances = tolerances_from_json(baseline["tolerances"])[profile]

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
        if cur_entry.get("correctness", {}).get("status") == "fail":
            failures.append(f"{name}: probe output no longer matches the Node oracle")

        for metric in ALL_METRICS:
            tolerance = tolerances[metric]
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

            if breach:
                status = "REGRESSION" if tolerance.gating else "drift (informational)"
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
        "| Probe | Metric | Baseline | Current | Δ | Allowance | Gating | Status |",
        "|-------|--------|---------:|--------:|---:|----------:|:------:|--------|",
    ]
    for row in rows:
        delta = "-" if row.delta_pct is None else f"{row.delta_pct:+.2f}%"
        lines.append(
            f"| `{row.probe}` | {row.metric} | {row.baseline:,.0f} | {row.current:,.0f} | "
            f"{delta} | {row.allowance:,.0f} | {'yes' if row.gating else 'no'} | {row.status} |"
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
    validate_artifact(_load(Path(args.artifact)))
    print(f"gc-ratchet: {args.artifact} is a valid pinned baseline")
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
