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


def _number(value: Any, context: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ArtifactError(f"{context}: expected a number, got {value!r}")
    value = float(value)
    if not math.isfinite(value):
        raise ArtifactError(f"{context}: expected a finite number")
    return value


def distribution(values: Iterable[Any]) -> dict[str, Any]:
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
) -> dict[str, Any]:
    if requested_samples < 2:
        raise ArtifactError("at least two repeated samples are required")
    for runtime_name in RUNTIME_NAMES:
        if runtime_name not in runtimes:
            raise ArtifactError(f"runtime metadata missing for {runtime_name}")
        metadata = runtimes[runtime_name]
        if not isinstance(metadata.get("command"), list) or not metadata["command"]:
            raise ArtifactError(f"runtime metadata for {runtime_name} has no pinned command")
        if metadata.get("available") and not metadata.get("version"):
            raise ArtifactError(f"runtime metadata for {runtime_name} has no version")

    benchmarks: dict[str, Any] = {}
    for record in records:
        name = str(record.get("name", ""))
        if not name or name in benchmarks:
            raise ArtifactError(f"invalid or duplicate benchmark name: {name!r}")
        runtime_results: dict[str, Any] = {}
        raw_runtimes = record.get("runtimes", {})
        for runtime_name in RUNTIME_NAMES:
            available = bool(runtimes[runtime_name].get("available"))
            raw = raw_runtimes.get(runtime_name)
            if not available:
                if raw:
                    raise ArtifactError(f"{name}: unavailable {runtime_name} unexpectedly has samples")
                continue
            if not isinstance(raw, Mapping):
                raise ArtifactError(f"{name}: {runtime_name} has 0/{requested_samples} samples")
            wall_samples = list(raw.get("wall_ms", []))
            rss_samples = list(raw.get("rss_kb", []))
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
            runtime_results[runtime_name] = {
                "wall_ms": distribution(wall_samples),
                "rss_kb": distribution(rss_samples),
            }

        perry = runtime_results["perry"]
        node = runtime_results.get("node")
        bun = runtime_results.get("bun")
        entry: dict[str, Any] = {
            "runtimes": runtime_results,
            "ratios": {
                "perry_to_node": _runtime_ratio(perry, node) if node else None,
                "perry_to_bun": _runtime_ratio(perry, bun) if bun else None,
            },
            "correctness": dict(record.get("correctness", {})),
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

    return {
        "schema_version": SCHEMA_VERSION,
        "commit": commit,
        "generated_at": generated_at,
        "run_config": {"requested_samples": requested_samples},
        "runtimes": {name: dict(runtimes[name]) for name in RUNTIME_NAMES},
        "benchmarks": benchmarks,
    }


def _legacy_distribution(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    return distribution([value])


def normalize_artifact(payload: Mapping[str, Any]) -> dict[str, Any]:
    if payload.get("schema_version") == SCHEMA_VERSION:
        return dict(payload)
    if "benchmarks" not in payload:
        raise ArtifactError("artifact has no benchmarks object")

    normalized = dict(payload)
    normalized["schema_version"] = 1
    normalized_benchmarks: dict[str, Any] = {}
    for name, old in payload.get("benchmarks", {}).items():
        runtime_results: dict[str, Any] = {}
        for runtime_name in RUNTIME_NAMES:
            wall = _legacy_distribution(old.get(f"{runtime_name}_ms"))
            if wall is None:
                continue
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


def validate_artifact(payload: Mapping[str, Any], *, require_complete: bool = True) -> None:
    if "benchmarks" not in payload or not isinstance(payload["benchmarks"], Mapping):
        raise ArtifactError("artifact has no benchmarks object")
    if not require_complete or payload.get("schema_version") != SCHEMA_VERSION:
        return
    requested = payload.get("run_config", {}).get("requested_samples")
    if not isinstance(requested, int) or requested < 2:
        raise ArtifactError("artifact has invalid requested sample count")
    for name, entry in payload["benchmarks"].items():
        for runtime_name, metadata in payload.get("runtimes", {}).items():
            if not metadata.get("available"):
                continue
            runtime_result = entry.get("runtimes", {}).get(runtime_name)
            if not runtime_result:
                raise ArtifactError(f"{name}: {runtime_name} has 0/{requested} samples")
            for metric_name in ("wall_ms", "rss_kb"):
                metric = runtime_result.get(metric_name, {})
                samples = metric.get("samples", [])
                if metric.get("sample_count") != requested or len(samples) != requested:
                    raise ArtifactError(
                        f"{name}: {runtime_name} has {len(samples)}/{requested} {metric_name} samples"
                    )


def load_artifact(path: str | Path, *, require_complete: bool = True) -> dict[str, Any]:
    try:
        with Path(path).open(encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise ArtifactError(f"could not load {path}: {exc}") from exc
    normalized = normalize_artifact(payload)
    validate_artifact(normalized, require_complete=require_complete)
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


def _metric(entry: Mapping[str, Any], runtime_name: str, metric_name: str) -> Mapping[str, Any] | None:
    return entry.get("runtimes", {}).get(runtime_name, {}).get(metric_name)


def _noise_allowance_ms(base_metric: Mapping[str, Any], current_metric: Mapping[str, Any]) -> float:
    # Integer millisecond timers have a one-tick quantization floor. Above that,
    # use three population standard deviations from this benchmark's own stored
    # samples instead of one global absolute escape hatch.
    dispersions = [float(base_metric.get("stdev", 0)), float(current_metric.get("stdev", 0))]
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


def evaluate_regressions(
    baseline: Mapping[str, Any],
    current: Mapping[str, Any],
    *,
    speed_threshold_pct: float,
    memory_threshold_pct: float,
) -> GateReport:
    baseline = normalize_artifact(baseline)
    current = normalize_artifact(current)
    validate_artifact(current, require_complete=True)
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
        speed_pct = _pct_delta(cur_speed["median"], base_speed["median"])
        memory_pct = _pct_delta(cur_memory["median"], base_memory["median"])
        speed_noise = _noise_allowance_ms(base_speed, cur_speed)
        speed_delta_ms = float(cur_speed["median"]) - float(base_speed["median"])
        memory_delta_kb = float(cur_memory["median"]) - float(base_memory["median"])
        node_ratio_delta = _ratio_delta(base, cur, "node")
        bun_ratio_delta = _ratio_delta(base, cur, "bun")
        peer_corroborates = _peer_corroborates(
            (node_ratio_delta, bun_ratio_delta), speed_threshold_pct
        )

        speed_regression = (
            speed_pct is not None
            and speed_pct > speed_threshold_pct
            and speed_delta_ms > speed_noise
            and peer_corroborates is not False
        )
        speed_improvement = (
            speed_pct is not None
            and speed_pct < -speed_threshold_pct
            and -speed_delta_ms > speed_noise
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
    grouped: dict[str, dict[str, list[Mapping[str, Any]]]] = {}
    for row in payload.get("rows", []):
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
                for metric in required_metrics:
                    if metric not in row:
                        raise ArtifactError(f"{workload}: {runtime_name} sample is missing {metric}")
                    _number(row[metric], f"{workload}: {runtime_name} {metric}")
                if row["rps"] <= 0 or row["success_rate"] < 0.99:
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
    print(f"{'Benchmark':<28} {'Correct':>9} {'Speed':>9} {'Noise':>9} {'RAM':>9} {'P/Node':>9} {'P/Bun':>9} {'Status':>12}")
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
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build")
    build.add_argument("--records", type=Path, required=True)
    build.add_argument("--runtime-metadata", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--runs", type=int, required=True)
    compare = subparsers.add_parser("compare")
    compare.add_argument("baseline", type=Path)
    compare.add_argument("current", type=Path)
    compare.add_argument("--speed-threshold", type=float, required=True)
    compare.add_argument("--memory-threshold", type=float, required=True)
    http = subparsers.add_parser("http-summary")
    http.add_argument("--input", type=Path, required=True)
    http.add_argument("--output", type=Path, required=True)
    http.add_argument("--samples", type=int, required=True)
    http.add_argument("--metadata", type=Path)
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
            )
            args.output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
            return 0
        if args.command == "compare":
            baseline = load_artifact(args.baseline, require_complete=False)
            current = load_artifact(args.current, require_complete=True)
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
