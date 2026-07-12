#!/usr/bin/env python3
"""Build and evaluate reproducible Perry benchmark artifacts."""

from __future__ import annotations

import argparse
import json
import math
import statistics
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


SCHEMA_VERSION = 2
RUNTIME_NAMES = ("perry", "node", "bun")
HTTP_WORKLOADS = (
    "http_fastify_minimal",
    "http_fastify_text",
    "http_fastify_parametric",
)


class ArtifactError(ValueError):
    """Raised when benchmark evidence is missing or malformed."""


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise ArtifactError(f"{context}: expected an object")
    return value


def _validate_runtime_metadata(runtimes: Any) -> Mapping[str, Mapping[str, Any]]:
    runtimes = _mapping(runtimes, "runtime metadata")
    for runtime_name in RUNTIME_NAMES:
        if runtime_name not in runtimes:
            raise ArtifactError(f"runtime metadata missing for {runtime_name}")
        metadata = _mapping(runtimes[runtime_name], f"runtime metadata for {runtime_name}")
        if not isinstance(metadata.get("available"), bool):
            raise ArtifactError(f"runtime metadata for {runtime_name} has invalid availability")
        command = metadata.get("command")
        if (
            not isinstance(command, list)
            or not command
            or any(not isinstance(part, str) or not part for part in command)
        ):
            raise ArtifactError(f"runtime metadata for {runtime_name} has no pinned command")
        version = metadata.get("version")
        if metadata["available"] and (not isinstance(version, str) or not version.strip()):
            raise ArtifactError(f"runtime metadata for {runtime_name} has no version")
    if runtimes["perry"]["available"] is not True:
        raise ArtifactError("Perry runtime metadata must be available")
    return runtimes  # type: ignore[return-value]


def _number(value: Any, context: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ArtifactError(f"{context}: expected a number, got {value!r}")
    value = float(value)
    if not math.isfinite(value):
        raise ArtifactError(f"{context}: expected a finite number")
    return value


def distribution(values: Iterable[Any]) -> dict[str, Any]:
    """Return raw samples and deterministic summary statistics."""
    samples = [_number(value, "sample") for value in values]
    if not samples:
        raise ArtifactError("distribution has no samples")
    ordered = sorted(samples)
    median = statistics.median(ordered)
    deviations = [abs(value - median) for value in ordered]
    p95_index = max(0, math.ceil(len(ordered) * 0.95) - 1)
    return {
        "samples": [_clean_number(value) for value in samples],
        "sample_count": len(samples),
        "median": _clean_number(median),
        "p95": _clean_number(ordered[p95_index]),
        "min": _clean_number(ordered[0]),
        "max": _clean_number(ordered[-1]),
        "mad": _clean_number(statistics.median(deviations)),
        "stdev": _clean_number(statistics.pstdev(ordered)),
    }


def _clean_number(value: float) -> int | float:
    return int(value) if float(value).is_integer() else round(float(value), 6)


def _ratio(numerator: Any, denominator: Any) -> float | None:
    if numerator is None or denominator in (None, 0):
        return None
    return round(float(numerator) / float(denominator), 6)


def _runtime_ratio(perry: Mapping[str, Any], peer: Mapping[str, Any]) -> dict[str, Any]:
    return {
        "wall_time": _ratio(perry["wall_ms"]["median"], peer["wall_ms"]["median"]),
        "rss": _ratio(perry["rss_kb"]["median"], peer["rss_kb"]["median"]),
    }


def build_artifact(
    *,
    records: Sequence[Mapping[str, Any]],
    requested_samples: int,
    runtimes: Mapping[str, Mapping[str, Any]],
    commit: str,
    generated_at: str,
    expected_benchmarks: Sequence[str] | None = None,
) -> dict[str, Any]:
    """Build one complete schema-v2 benchmark artifact."""
    if isinstance(requested_samples, bool) or not isinstance(requested_samples, int) or requested_samples < 2:
        raise ArtifactError("at least two repeated samples are required")
    runtimes = _validate_runtime_metadata(runtimes)
    if expected_benchmarks is not None:
        if (
            isinstance(expected_benchmarks, (str, bytes))
            or not expected_benchmarks
            or any(not isinstance(name, str) or not name for name in expected_benchmarks)
            or len(set(expected_benchmarks)) != len(expected_benchmarks)
        ):
            raise ArtifactError("expected benchmark names are empty, duplicated, or invalid")

    benchmarks: dict[str, Any] = {}
    for record in records:
        record = _mapping(record, "benchmark record")
        name = record.get("name")
        if not isinstance(name, str) or not name or name in benchmarks:
            raise ArtifactError(f"invalid or duplicate benchmark name: {name!r}")
        runtime_results: dict[str, Any] = {}
        raw_runtimes = _mapping(record.get("runtimes"), f"{name}: runtimes")
        for runtime_name in RUNTIME_NAMES:
            available = runtimes[runtime_name]["available"]
            raw = raw_runtimes.get(runtime_name)
            if not available:
                if raw:
                    raise ArtifactError(f"{name}: unavailable {runtime_name} unexpectedly has samples")
                continue
            if not isinstance(raw, Mapping):
                raise ArtifactError(f"{name}: {runtime_name} has 0/{requested_samples} samples")
            wall_samples = raw.get("wall_ms")
            rss_samples = raw.get("rss_kb")
            if not isinstance(wall_samples, list) or not isinstance(rss_samples, list):
                raise ArtifactError(f"{name}: {runtime_name} samples must be arrays")
            if len(wall_samples) != requested_samples:
                raise ArtifactError(
                    f"{name}: {runtime_name} has {len(wall_samples)}/{requested_samples} wall samples"
                )
            if len(rss_samples) != requested_samples:
                raise ArtifactError(
                    f"{name}: {runtime_name} has {len(rss_samples)}/{requested_samples} RSS samples"
                )
            if any(_number(value, f"{name}: {runtime_name} RSS sample") <= 0 for value in rss_samples):
                raise ArtifactError(f"{name}: {runtime_name} has an invalid zero RSS sample")
            if any(_number(value, f"{name}: {runtime_name} wall sample") < 0 for value in wall_samples):
                raise ArtifactError(f"{name}: {runtime_name} has an invalid negative wall sample")
            runtime_results[runtime_name] = {
                "wall_ms": distribution(wall_samples),
                "rss_kb": distribution(rss_samples),
            }

        perry = runtime_results["perry"]
        node = runtime_results.get("node")
        bun = runtime_results.get("bun")
        correctness = _mapping(record.get("correctness"), f"{name}: correctness")
        if correctness.get("status") not in ("pass", "fail", "unchecked"):
            raise ArtifactError(f"{name}: correctness has an invalid status")
        entry: dict[str, Any] = {
            "runtimes": runtime_results,
            "ratios": {
                "perry_to_node": _runtime_ratio(perry, node) if node else None,
                "perry_to_bun": _runtime_ratio(perry, bun) if bun else None,
            },
            "correctness": dict(correctness),
            # Compatibility fields retained for existing artifact consumers.
            "perry_ms": perry["wall_ms"]["median"],
            "perry_rss_kb": perry["rss_kb"]["median"],
        }
        for peer_name, peer in (("node", node), ("bun", bun)):
            if peer:
                entry[f"{peer_name}_ms"] = peer["wall_ms"]["median"]
                entry[f"{peer_name}_rss_kb"] = peer["rss_kb"]["median"]
        if node:
            entry["speed_ratio"] = entry["ratios"]["perry_to_node"]["wall_time"]
            entry["memory_ratio"] = entry["ratios"]["perry_to_node"]["rss"]
        benchmarks[name] = entry

    if not benchmarks:
        raise ArtifactError("artifact contains no benchmark records")
    expected = list(expected_benchmarks) if expected_benchmarks is not None else list(benchmarks)
    missing = sorted(set(expected) - set(benchmarks))
    unexpected = sorted(set(benchmarks) - set(expected))
    if missing or unexpected:
        raise ArtifactError(
            f"benchmark set mismatch: missing={missing or 'none'}, unexpected={unexpected or 'none'}"
        )

    artifact = {
        "schema_version": SCHEMA_VERSION,
        "commit": commit,
        "generated_at": generated_at,
        "run_config": {
            "requested_samples": requested_samples,
            "expected_benchmarks": expected,
        },
        "runtimes": {name: dict(runtimes[name]) for name in RUNTIME_NAMES},
        "benchmarks": benchmarks,
    }
    validate_artifact(artifact)
    return artifact


def _legacy_distribution(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    return distribution([value])


def normalize_artifact(payload: Mapping[str, Any]) -> dict[str, Any]:
    """Normalize a supported legacy or schema-v2 artifact for comparison."""
    payload = _mapping(payload, "artifact")
    schema_version = payload.get("schema_version")
    if schema_version == SCHEMA_VERSION:
        return dict(payload)
    if schema_version not in (None, 1):
        raise ArtifactError(f"unsupported benchmark artifact schema: {schema_version!r}")
    if "benchmarks" not in payload:
        raise ArtifactError("artifact has no benchmarks object")
    legacy_benchmarks = _mapping(payload["benchmarks"], "legacy benchmarks")
    if not legacy_benchmarks:
        raise ArtifactError("artifact contains no benchmarks")

    normalized = dict(payload)
    normalized["schema_version"] = 1
    normalized_benchmarks: dict[str, Any] = {}
    for name, old in legacy_benchmarks.items():
        old = _mapping(old, f"legacy benchmark {name}")
        runtime_results: dict[str, Any] = {}
        for runtime_name in RUNTIME_NAMES:
            wall = _legacy_distribution(old.get(f"{runtime_name}_ms"))
            if wall is None:
                continue
            if wall["median"] < 0:
                raise ArtifactError(f"legacy benchmark {name} has a negative {runtime_name} timing")
            rss = _legacy_distribution(old.get(f"{runtime_name}_rss_kb", 0))
            runtime_results[runtime_name] = {"wall_ms": wall, "rss_kb": rss}
        perry = runtime_results.get("perry")
        if not perry:
            raise ArtifactError(f"legacy benchmark {name} has no Perry timing")
        node = runtime_results.get("node")
        bun = runtime_results.get("bun")
        entry = dict(old)
        entry["runtimes"] = runtime_results
        entry["ratios"] = {
            "perry_to_node": _runtime_ratio(perry, node) if node else None,
            "perry_to_bun": _runtime_ratio(perry, bun) if bun else None,
        }
        normalized_benchmarks[name] = entry
    normalized["benchmarks"] = normalized_benchmarks
    return normalized


def validate_artifact(payload: Mapping[str, Any]) -> None:
    """Validate schema structure, completeness, and derived distributions."""
    if "benchmarks" not in payload or not isinstance(payload["benchmarks"], Mapping):
        raise ArtifactError("artifact has no benchmarks object")
    if not payload["benchmarks"]:
        raise ArtifactError("artifact contains no benchmarks")
    if payload.get("schema_version") != SCHEMA_VERSION:
        return
    for field in ("commit", "generated_at"):
        if not isinstance(payload.get(field), str) or not payload[field].strip():
            raise ArtifactError(f"artifact has invalid {field}")
    runtimes = _validate_runtime_metadata(payload.get("runtimes"))
    run_config = _mapping(payload.get("run_config"), "run_config")
    requested = run_config.get("requested_samples")
    if not isinstance(requested, int) or requested < 2:
        raise ArtifactError("artifact has invalid requested sample count")
    expected_benchmarks = run_config.get("expected_benchmarks")
    if (
        not isinstance(expected_benchmarks, list)
        or not expected_benchmarks
        or any(not isinstance(name, str) or not name for name in expected_benchmarks)
        or len(set(expected_benchmarks)) != len(expected_benchmarks)
    ):
        raise ArtifactError("artifact has invalid expected benchmark names")
    if set(expected_benchmarks) != set(payload["benchmarks"]):
        missing = sorted(set(expected_benchmarks) - set(payload["benchmarks"]))
        unexpected = sorted(set(payload["benchmarks"]) - set(expected_benchmarks))
        raise ArtifactError(
            f"benchmark set mismatch: missing={missing or 'none'}, unexpected={unexpected or 'none'}"
        )
    for name, entry in payload["benchmarks"].items():
        entry = _mapping(entry, f"benchmark {name}")
        runtime_results = _mapping(entry.get("runtimes"), f"{name}: runtimes")
        for runtime_name in RUNTIME_NAMES:
            metadata = runtimes[runtime_name]
            if not metadata["available"]:
                if runtime_name in runtime_results:
                    raise ArtifactError(f"{name}: unavailable {runtime_name} unexpectedly has samples")
                continue
            runtime_result = runtime_results.get(runtime_name)
            if not runtime_result:
                raise ArtifactError(f"{name}: {runtime_name} has 0/{requested} samples")
            runtime_result = _mapping(runtime_result, f"{name}: {runtime_name}")
            for metric_name in ("wall_ms", "rss_kb"):
                metric = _mapping(runtime_result.get(metric_name), f"{name}: {runtime_name} {metric_name}")
                samples = metric.get("samples", [])
                if not isinstance(samples, list):
                    raise ArtifactError(f"{name}: {runtime_name} {metric_name} samples are invalid")
                if metric.get("sample_count") != requested or len(samples) != requested:
                    raise ArtifactError(
                        f"{name}: {runtime_name} has {len(samples)}/{requested} {metric_name} samples"
                    )
                recalculated = distribution(samples)
                for field, value in recalculated.items():
                    if metric.get(field) != value:
                        raise ArtifactError(
                            f"{name}: {runtime_name} {metric_name} has inconsistent {field}"
                        )
                if metric_name == "rss_kb" and any(
                    _number(value, f"{name}: {runtime_name} RSS sample") <= 0 for value in samples
                ):
                    raise ArtifactError(f"{name}: {runtime_name} has an invalid zero RSS sample")
                if metric_name == "wall_ms" and any(
                    _number(value, f"{name}: {runtime_name} wall sample") < 0 for value in samples
                ):
                    raise ArtifactError(f"{name}: {runtime_name} has a negative wall sample")
        correctness = _mapping(entry.get("correctness"), f"{name}: correctness")
        correctness_status = correctness.get("status")
        if correctness_status not in ("pass", "fail", "unchecked"):
            raise ArtifactError(f"{name}: correctness has an invalid status")
        if correctness_status == "unchecked" and any(
            runtimes[peer]["available"] for peer in ("node", "bun")
        ):
            raise ArtifactError(f"{name}: correctness is unchecked despite an available peer")
        reference = correctness.get("reference")
        if correctness_status in ("pass", "fail") and reference not in ("node", "bun"):
            raise ArtifactError(f"{name}: correctness has an invalid reference")
        if correctness_status == "unchecked" and reference not in ("none", "node", "bun"):
            raise ArtifactError(f"{name}: correctness has an invalid reference")
        perry_result = runtime_results["perry"]
        ratios = _mapping(entry.get("ratios"), f"{name}: ratios")
        for peer in ("node", "bun"):
            peer_result = runtime_results.get(peer)
            expected_ratio = _runtime_ratio(perry_result, peer_result) if peer_result else None
            if ratios.get(f"perry_to_{peer}") != expected_ratio:
                raise ArtifactError(f"{name}: Perry-to-{peer} ratio is inconsistent")
        compatibility_values = {
            "perry_ms": perry_result["wall_ms"]["median"],
            "perry_rss_kb": perry_result["rss_kb"]["median"],
        }
        for peer in ("node", "bun"):
            if peer in runtime_results:
                compatibility_values[f"{peer}_ms"] = runtime_results[peer]["wall_ms"]["median"]
                compatibility_values[f"{peer}_rss_kb"] = runtime_results[peer]["rss_kb"]["median"]
        if "node" in runtime_results:
            compatibility_values["speed_ratio"] = ratios["perry_to_node"]["wall_time"]
            compatibility_values["memory_ratio"] = ratios["perry_to_node"]["rss"]
        for field, value in compatibility_values.items():
            if entry.get(field) != value:
                raise ArtifactError(f"{name}: compatibility field {field} is inconsistent")


def load_artifact(path: str | Path) -> dict[str, Any]:
    """Load, normalize, and validate a benchmark artifact from disk."""
    try:
        with Path(path).open(encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise ArtifactError(f"could not load {path}: {exc}") from exc
    normalized = normalize_artifact(payload)
    validate_artifact(normalized)
    return normalized


@dataclass(frozen=True)
class ComparisonRow:
    name: str
    correctness: str
    speed_delta_pct: float | None
    memory_delta_pct: float | None
    speed_noise_ms: float
    node_ratio_delta_pct: float | None
    bun_ratio_delta_pct: float | None
    status: str


@dataclass(frozen=True)
class GateReport:
    rows: list[ComparisonRow]
    regressions: list[ComparisonRow]
    improvements: list[ComparisonRow]
    correctness_failures: list[str]


def _pct_delta(current: Any, baseline: Any) -> float | None:
    if current is None or baseline in (None, 0):
        return None
    return (float(current) - float(baseline)) / float(baseline) * 100.0


def _speed_pct_delta(current: Any, baseline: Any) -> float | None:
    if current is None or baseline is None:
        return None
    current_value = float(current)
    baseline_value = float(baseline)
    if baseline_value == 0:
        # Internal suite timers have 1 ms resolution. Treat a zero baseline as
        # the bottom of that first tick rather than making the row ungateable.
        return 0.0 if current_value == 0 else current_value * 100.0
    return (current_value - baseline_value) / baseline_value * 100.0


def _metric(entry: Mapping[str, Any], runtime_name: str, metric_name: str) -> Mapping[str, Any] | None:
    return entry.get("runtimes", {}).get(runtime_name, {}).get(metric_name)


def _noise_allowance_ms(base_metric: Mapping[str, Any], current_metric: Mapping[str, Any]) -> float:
    # Integer millisecond timers have a one-tick quantization floor. Above that,
    # use three robust sigma estimates (MAD × 1.4826) from this benchmark's own
    # stored samples. A single outlier must not hide a stable median shift.
    dispersions = [
        float(base_metric.get("mad", 0)) * 1.4826,
        float(current_metric.get("mad", 0)) * 1.4826,
    ]
    return max(1.0, 3.0 * max(dispersions))


def _ratio_delta(base: Mapping[str, Any], current: Mapping[str, Any], peer: str) -> float | None:
    base_ratio = base.get("ratios", {}).get(f"perry_to_{peer}")
    current_ratio = current.get("ratios", {}).get(f"perry_to_{peer}")
    if not base_ratio or not current_ratio:
        return None
    return _pct_delta(current_ratio.get("wall_time"), base_ratio.get("wall_time"))


def _peer_corroborates(delta_values: Sequence[float | None], threshold_pct: float) -> bool | None:
    available = [value for value in delta_values if value is not None]
    if not available:
        return None
    # Peer-relative trends control for runner-wide drift. Requiring half of the
    # headline threshold keeps this a corroborating signal rather than a second
    # equally blunt gate.
    return statistics.median(available) > threshold_pct / 2.0


def _peer_metadata_matches(
    baseline: Mapping[str, Any], current: Mapping[str, Any], peer: str
) -> bool:
    base_metadata = baseline.get("runtimes", {}).get(peer)
    current_metadata = current.get("runtimes", {}).get(peer)
    if not isinstance(base_metadata, Mapping) or not isinstance(current_metadata, Mapping):
        return False
    return (
        base_metadata.get("available") is True
        and current_metadata.get("available") is True
        and base_metadata.get("version") == current_metadata.get("version")
        and base_metadata.get("command") == current_metadata.get("command")
    )


def evaluate_regressions(
    baseline: Mapping[str, Any],
    current: Mapping[str, Any],
    *,
    speed_threshold_pct: float,
    memory_threshold_pct: float,
) -> GateReport:
    """Compare Perry medians using percentage, noise, memory, and peer signals."""
    for name, value in (
        ("speed threshold", speed_threshold_pct),
        ("memory threshold", memory_threshold_pct),
    ):
        if _number(value, name) <= 0:
            raise ArtifactError(f"{name} must be a positive finite number")
    baseline = normalize_artifact(baseline)
    current = normalize_artifact(current)
    validate_artifact(baseline)
    validate_artifact(current)
    rows: list[ComparisonRow] = []
    regressions: list[ComparisonRow] = []
    improvements: list[ComparisonRow] = []
    correctness_failures: list[str] = []

    for name, cur in current["benchmarks"].items():
        correctness = cur.get("correctness", {})
        correctness_status = correctness.get("status", "unchecked")
        if correctness_status == "fail":
            correctness_failures.append(f"{name}: {correctness.get('reason', 'semantic output mismatch')}")
            rows.append(ComparisonRow(name, "fail", None, None, 0, None, None, "INVALID"))
            continue
        base = baseline.get("benchmarks", {}).get(name)
        if not base:
            rows.append(ComparisonRow(name, correctness_status, None, None, 0, None, None, "new"))
            continue

        base_speed = _metric(base, "perry", "wall_ms")
        cur_speed = _metric(cur, "perry", "wall_ms")
        base_memory = _metric(base, "perry", "rss_kb")
        cur_memory = _metric(cur, "perry", "rss_kb")
        if not base_speed or not cur_speed or not base_memory or not cur_memory:
            raise ArtifactError(f"{name}: Perry speed or RSS distribution missing")
        speed_pct = _speed_pct_delta(cur_speed["median"], base_speed["median"])
        memory_pct = _pct_delta(cur_memory["median"], base_memory["median"])
        speed_noise = _noise_allowance_ms(base_speed, cur_speed)
        speed_delta_ms = float(cur_speed["median"]) - float(base_speed["median"])
        memory_delta_kb = float(cur_memory["median"]) - float(base_memory["median"])
        node_ratio_delta = (
            _ratio_delta(base, cur, "node")
            if _peer_metadata_matches(baseline, current, "node")
            else None
        )
        bun_ratio_delta = (
            _ratio_delta(base, cur, "bun")
            if _peer_metadata_matches(baseline, current, "bun")
            else None
        )
        peer_corroborates = _peer_corroborates(
            (node_ratio_delta, bun_ratio_delta), speed_threshold_pct
        )

        speed_regression = (
            speed_pct is not None
            and speed_pct > speed_threshold_pct
            and speed_delta_ms >= speed_noise
            and peer_corroborates is not False
        )
        speed_improvement = (
            speed_pct is not None
            and speed_pct < -speed_threshold_pct
            and -speed_delta_ms >= speed_noise
        )
        # Memory retains a small OS accounting floor; finding #7 concerns timed
        # regions, not RSS page accounting.
        memory_regression = (
            memory_pct is not None
            and memory_pct > memory_threshold_pct
            and memory_delta_kb >= 4096
        )
        memory_improvement = (
            memory_pct is not None
            and memory_pct < -memory_threshold_pct
            and -memory_delta_kb >= 4096
        )
        status = "REGRESSION" if speed_regression or memory_regression else (
            "improved" if speed_improvement or memory_improvement else "ok"
        )
        row = ComparisonRow(
            name=name,
            correctness=correctness_status,
            speed_delta_pct=speed_pct,
            memory_delta_pct=memory_pct,
            speed_noise_ms=speed_noise,
            node_ratio_delta_pct=node_ratio_delta,
            bun_ratio_delta_pct=bun_ratio_delta,
            status=status,
        )
        rows.append(row)
        if status == "REGRESSION":
            regressions.append(row)
        elif status == "improved":
            improvements.append(row)

    return GateReport(rows, regressions, improvements, correctness_failures)


def summarize_http(
    payload: Mapping[str, Any],
    *,
    expected_samples: int,
    expected_runtimes: Sequence[str] = RUNTIME_NAMES,
    expected_workloads: Sequence[str] = HTTP_WORKLOADS,
    metadata: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Validate and summarize fixed-load Fastify HTTP samples."""
    if isinstance(expected_samples, bool) or not isinstance(expected_samples, int) or expected_samples < 1:
        raise ArtifactError("HTTP expected sample count must be a positive integer")
    if (
        "perry" not in expected_runtimes
        or len(set(expected_runtimes)) != len(expected_runtimes)
        or any(runtime not in RUNTIME_NAMES for runtime in expected_runtimes)
    ):
        raise ArtifactError("HTTP expected runtimes must include Perry exactly once")
    if (
        not expected_workloads
        or len(set(expected_workloads)) != len(expected_workloads)
        or any(not isinstance(workload, str) or not workload for workload in expected_workloads)
    ):
        raise ArtifactError("HTTP expected workloads are empty or duplicated")
    payload = _mapping(payload, "HTTP artifact")
    raw_rows = payload.get("rows")
    if not isinstance(raw_rows, list):
        raise ArtifactError("HTTP artifact rows must be an array")
    grouped: dict[str, dict[str, list[Mapping[str, Any]]]] = {}
    for index, row in enumerate(raw_rows, 1):
        row = _mapping(row, f"HTTP row {index}")
        workload = str(row.get("workload", ""))
        language = str(row.get("language", ""))
        if not workload.startswith("http_fastify_") or language not in expected_runtimes:
            continue
        grouped.setdefault(workload, {}).setdefault(language, []).append(row)
    missing_workloads = sorted(set(expected_workloads) - set(grouped))
    if missing_workloads:
        raise ArtifactError(f"HTTP artifact is missing workloads: {', '.join(missing_workloads)}")

    workloads: dict[str, Any] = {}
    for workload in expected_workloads:
        runtime_rows = grouped[workload]
        summaries: dict[str, Any] = {}
        for runtime_name in expected_runtimes:
            rows = runtime_rows.get(runtime_name, [])
            if len(rows) != expected_samples:
                raise ArtifactError(
                    f"{workload}: {runtime_name} has {len(rows)}/{expected_samples} HTTP samples"
                )
            run_numbers = {row.get("run") for row in rows}
            expected_run_numbers = set(range(1, expected_samples + 1))
            if run_numbers != expected_run_numbers:
                raise ArtifactError(
                    f"{workload}: {runtime_name} HTTP run indexes are incomplete or duplicated"
                )
            failed = [row for row in rows if row.get("exit_code") != 0]
            if failed:
                raise ArtifactError(f"{workload}: {runtime_name} has {len(failed)} failed HTTP samples")
            required_metrics = ("rps", "p50_ms", "p95_ms", "p99_ms", "success_rate")
            for row in rows:
                values: dict[str, float] = {}
                for metric in required_metrics:
                    if metric not in row:
                        raise ArtifactError(f"{workload}: {runtime_name} sample is missing {metric}")
                    values[metric] = _number(row[metric], f"{workload}: {runtime_name} {metric}")
                if (
                    values["rps"] <= 0
                    or not 0.99 <= values["success_rate"] <= 1.0
                    or min(values["p50_ms"], values["p95_ms"], values["p99_ms"]) < 0
                    or not values["p50_ms"] <= values["p95_ms"] <= values["p99_ms"]
                ):
                    raise ArtifactError(f"{workload}: {runtime_name} has an unhealthy HTTP sample")
            summaries[runtime_name] = {
                metric: distribution(row[metric] for row in rows)
                for metric in required_metrics
            }
        perry_rps = summaries["perry"]["rps"]["median"]
        workloads[workload] = {
            "runtimes": summaries,
            "ratios": {
                f"perry_to_{peer}_rps": _ratio(perry_rps, summaries[peer]["rps"]["median"])
                for peer in expected_runtimes
                if peer != "perry"
            },
        }
    if metadata is not None:
        metadata = _mapping(metadata, "HTTP metadata")
        toolchains = _mapping(metadata.get("toolchains"), "HTTP metadata toolchains")
        commands = _mapping(metadata.get("commands"), "HTTP metadata commands")
        for toolchain in ("perry", "node", "bun", "oha"):
            version = toolchains.get(toolchain)
            if not isinstance(version, str) or not version.strip() or version.startswith("error:"):
                raise ArtifactError(f"HTTP metadata has no usable {toolchain} version")
        for command_name in ("perry_http", "perry_http_compile", "node_http", "bun_http", "oha"):
            command = commands.get(command_name)
            if (
                not isinstance(command, list)
                or not command
                or any(not isinstance(part, str) or not part for part in command)
            ):
                raise ArtifactError(f"HTTP metadata has no pinned {command_name} command")
    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "run_config": {"requested_samples": expected_samples},
        "metadata": dict(metadata or {}),
        "workloads": workloads,
    }


def _format_pct(value: float | None) -> str:
    return "-" if value is None else f"{value:+.1f}%"


def _print_report(report: GateReport, baseline: Mapping[str, Any], current: Mapping[str, Any]) -> None:
    print(f"Baseline commit: {baseline.get('commit', '?')} | Current commit: {current.get('commit', '?')}")
    print()
    print(
        f"{'Benchmark':<28} {'Correct':>9} {'Speed':>9} {'Noise':>9} "
        f"{'RAM':>9} {'P/Node':>9} {'P/Bun':>9} {'Status':>12}"
    )
    print("-" * 102)
    for row in report.rows:
        print(
            f"{row.name.replace('_', ' '):<28} {row.correctness:>9} "
            f"{_format_pct(row.speed_delta_pct):>9} {row.speed_noise_ms:>8.1f}ms "
            f"{_format_pct(row.memory_delta_pct):>9} {_format_pct(row.node_ratio_delta_pct):>9} "
            f"{_format_pct(row.bun_ratio_delta_pct):>9} {row.status:>12}"
        )
    print()
    if report.correctness_failures:
        print(f"{len(report.correctness_failures)} CORRECTNESS FAILURE(S):")
        for failure in report.correctness_failures:
            print(f"  - {failure}")
    elif report.regressions:
        print(f"{len(report.regressions)} REGRESSION(S):")
        for row in report.regressions:
            print(
                f"  - {row.name}: speed {_format_pct(row.speed_delta_pct)}, "
                f"RAM {_format_pct(row.memory_delta_pct)}, "
                f"noise allowance {row.speed_noise_ms:.1f}ms"
            )
    elif report.improvements:
        print(f"{len(report.improvements)} improvement(s), no regressions")
    else:
        print("No significant changes")


def _read_json_lines(path: Path) -> list[dict[str, Any]]:
    records = []
    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            if not line.strip():
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise ArtifactError(f"{path}:{line_number}: {exc}") from exc
    return records


def _git_commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def main(argv: Sequence[str] | None = None) -> int:
    """Run artifact build, comparison, or HTTP summary commands."""
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build")
    build.add_argument("--records", type=Path, required=True)
    build.add_argument("--runtime-metadata", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--runs", type=int, required=True)
    build.add_argument("--expected-benchmarks", required=True)
    compare = subparsers.add_parser("compare")
    compare.add_argument("baseline", type=Path)
    compare.add_argument("current", type=Path)
    compare.add_argument("--speed-threshold", type=float, required=True)
    compare.add_argument("--memory-threshold", type=float, required=True)
    http = subparsers.add_parser("http-summary")
    http.add_argument("--input", type=Path, required=True)
    http.add_argument("--output", type=Path, required=True)
    http.add_argument("--samples", type=int, required=True)
    http.add_argument("--metadata", type=Path, required=True)
    args = parser.parse_args(argv)

    try:
        if args.command == "build":
            runtimes = json.loads(args.runtime_metadata.read_text(encoding="utf-8"))
            artifact = build_artifact(
                records=_read_json_lines(args.records),
                requested_samples=args.runs,
                runtimes=runtimes,
                commit=_git_commit(),
                generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                expected_benchmarks=args.expected_benchmarks.split(","),
            )
            args.output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
            return 0
        if args.command == "compare":
            baseline = load_artifact(args.baseline)
            current = load_artifact(args.current)
            report = evaluate_regressions(
                baseline,
                current,
                speed_threshold_pct=args.speed_threshold,
                memory_threshold_pct=args.memory_threshold,
            )
            _print_report(report, baseline, current)
            return 1 if report.correctness_failures or report.regressions else 0
        payload = json.loads(args.input.read_text(encoding="utf-8"))
        metadata = json.loads(args.metadata.read_text(encoding="utf-8")) if args.metadata else None
        summary = summarize_http(payload, expected_samples=args.samples, metadata=metadata)
        args.output.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        return 0
    except (ArtifactError, OSError, json.JSONDecodeError, subprocess.CalledProcessError) as exc:
        print(f"invalid benchmark artifact: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
