#!/usr/bin/env python3
"""Assemble, render, and verify Perry's public Node/Bun evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import shutil
import statistics
import subprocess
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from benchmarks.benchmark_gate import ArtifactError, load_artifact, validate_artifact


DEFAULT_ARTIFACT = ROOT / "benchmarks/results/public-node-bun-v1.json"
README = ROOT / "README.md"
SUITE_RESULTS = ROOT / "benchmarks/suite/results/RESULTS.md"
README_START = "<!-- public-node-bun:start -->"
README_END = "<!-- public-node-bun:end -->"
SOURCE_PATHS = (
    "Cargo.toml",
    "benchmarks/suite/*.ts",
    "benchmarks/json_polyglot/*.ts",
    "benchmarks/app-patterns/kernels/*.ts",
)
HARNESS_PATHS = (
    "benchmarks/public_baseline.py",
    "benchmarks/run_public_baseline.sh",
    "benchmarks/compare.sh",
    "benchmarks/verify_benchmark_output.py",
    "benchmarks/benchmark_gate.py",
    "benchmarks/polyglot/run_all.sh",
    "benchmarks/json_polyglot/run.sh",
    "benchmarks/app-patterns/run.sh",
    "benchmarks/honest_bench/run.sh",
    "benchmarks/honest_bench/harness",
    "benchmarks/honest_bench/scripts",
    "benchmarks/honest_bench/workloads",
    "benchmarks/honest_bench/results/expected.json",
)
RUNTIMES = ("perry", "node", "bun")
PINNED_VERSIONS = {"node": "v22.23.1", "bun": "1.3.14"}
REFRESH_COMMAND = "./benchmarks/run_public_baseline.sh"
EXPECTED_COMPONENT_BENCHMARKS = {
    "polyglot": {
        "fibonacci", "loop_overhead", "loop_data_dependent", "array_write",
        "array_read", "math_intensive", "object_create", "nested_loops", "accumulate",
    },
    "json_polyglot": {"roundtrip", "field_access"},
    "app_patterns": {
        "buffer_transcode", "date_format_parse", "json_parse_1mb", "json_stringify_1mb",
        "map_1m", "object_deep_clone", "promise_all_chains", "regex_replace",
        "string_concat_csv", "string_split_map_join", "string_template_interp",
    },
    "honest_bench": {"image_convolution", "json_pipeline_small", "json_pipeline_full"},
}
EXPECTED_SUITE_BENCHMARKS = {
    "02_loop_overhead", "03_array_write", "04_array_read", "05_fibonacci",
    "06_math_intensive", "07_object_create", "08_string_concat", "09_method_calls",
    "10_nested_loops", "11_prime_sieve", "12_binary_trees", "13_factorial",
    "14_closure", "15_mandelbrot", "16_matrix_multiply", "bench_gc_pressure",
    "bench_json_roundtrip", "bench_object_property", "bench_int_arithmetic",
    "bench_buffer_readwrite", "bench_array_grow", "bench_string_heavy",
    "bench_numeric_array_numeric", "bench_numeric_array_downgrade",
}


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def tracked_fingerprint(paths: Iterable[str]) -> str:
    names = _git("ls-files", "-z", "--", *paths).split("\0")
    digest = hashlib.sha256()
    for name in sorted(filter(None, names)):
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update((ROOT / name).read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def utc_z_timestamp(value: Any) -> str:
    """Normalize an aware ISO-8601 timestamp to UTC with a Z suffix."""
    try:
        parsed = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    except ValueError as exc:
        raise ArtifactError(f"invalid timestamp: {value!r}") from exc
    if parsed.tzinfo is None:
        raise ArtifactError(f"timestamp is missing a UTC offset: {value!r}")
    return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def distribution(samples: Iterable[float]) -> dict[str, Any]:
    values = list(samples)
    if not values:
        raise ArtifactError("cannot summarize an empty sample set")
    ordered = sorted(values)
    p95_index = max(0, math.ceil(0.95 * len(ordered)) - 1)
    return {
        "samples": values,
        "sample_count": len(values),
        "median": statistics.median(values),
        "p95": ordered[p95_index],
        "min": ordered[0],
        "max": ordered[-1],
        "stdev": statistics.pstdev(values),
    }


def _load(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ArtifactError(f"could not load {path}: {exc}") from exc


def _validate_component(component: Mapping[str, Any], suite: str) -> None:
    if component.get("schema_version") != 1 or component.get("suite") != suite:
        raise ArtifactError(f"{suite}: unsupported component schema")
    requested = component.get("run_config", {}).get("requested_samples")
    if not isinstance(requested, int) or requested < 2:
        raise ArtifactError(f"{suite}: invalid requested sample count")
    commands = component.get("commands", {})
    runtime_metadata = component.get("runtime_metadata", {})
    for runtime in RUNTIMES:
        if not commands.get(runtime):
            raise ArtifactError(f"{suite}: missing resolved {runtime} command")
        metadata = runtime_metadata.get(runtime, {})
        if not metadata.get("version") or not Path(
            str(metadata.get("resolved_executable", ""))
        ).is_absolute():
            raise ArtifactError(f"{suite}: incomplete {runtime} runtime metadata")
    actual_benchmarks = set(component.get("benchmarks", {}))
    expected_benchmarks = EXPECTED_COMPONENT_BENCHMARKS[suite]
    if actual_benchmarks != expected_benchmarks:
        missing = sorted(expected_benchmarks - actual_benchmarks)
        extra = sorted(actual_benchmarks - expected_benchmarks)
        raise ArtifactError(f"{suite}: workload set mismatch; missing={missing}, extra={extra}")
    for name, entry in component.get("benchmarks", {}).items():
        correctness = entry.get("correctness", {})
        if correctness.get("status") != "pass":
            raise ArtifactError(f"{suite}/{name}: correctness did not pass")
        for runtime in RUNTIMES:
            runtime_result = entry.get("runtimes", {}).get(runtime)
            if not runtime_result:
                raise ArtifactError(f"{suite}/{name}: missing {runtime} results")
            samples = runtime_result.get("wall_ms", {}).get("samples", [])
            if len(samples) != requested:
                raise ArtifactError(
                    f"{suite}/{name}: {runtime} has {len(samples)}/{requested} samples"
                )


def _validate_suite(suite: Mapping[str, Any]) -> None:
    validate_artifact(suite)
    suite_names = set(suite.get("benchmarks", {}))
    if suite_names != EXPECTED_SUITE_BENCHMARKS:
        raise ArtifactError(
            "suite: workload set mismatch; "
            f"missing={sorted(EXPECTED_SUITE_BENCHMARKS - suite_names)}, "
            f"extra={sorted(suite_names - EXPECTED_SUITE_BENCHMARKS)}"
        )
    for name, entry in suite.get("benchmarks", {}).items():
        if entry.get("correctness", {}).get("status") != "pass":
            raise ArtifactError(f"suite/{name}: correctness did not pass")


def normalize_honest(results: Mapping[str, Any], metadata: Mapping[str, Any]) -> dict[str, Any]:
    measured = metadata.get("harness", {}).get("measured")
    if not isinstance(measured, int) or measured < 2:
        raise ArtifactError("honest_bench: invalid measured sample count")
    grouped: dict[tuple[str, str], list[Mapping[str, Any]]] = defaultdict(list)
    for row in results.get("rows", []):
        if row.get("language") in RUNTIMES:
            grouped[(row["workload"], row["language"])].append(row)
    workloads = sorted(EXPECTED_COMPONENT_BENCHMARKS["honest_bench"])
    benchmarks: dict[str, Any] = {}
    for workload in workloads:
        runtime_results: dict[str, Any] = {}
        for runtime in RUNTIMES:
            rows = sorted(grouped.get((workload, runtime), []), key=lambda row: row["run"])
            if len(rows) != measured:
                raise ArtifactError(
                    f"honest_bench/{workload}: {runtime} has {len(rows)}/{measured} samples"
                )
            if any(row.get("exit_code") != 0 or row.get("output_match") is not True for row in rows):
                raise ArtifactError(f"honest_bench/{workload}: {runtime} correctness failed")
            runtime_results[runtime] = {
                "wall_ms": distribution(float(row["wall_ms"]) for row in rows),
                "rss_kb": distribution(float(row["max_rss_kb"]) for row in rows),
            }
            commands = {tuple(row.get("command", [])) for row in rows}
            if len(commands) != 1 or not next(iter(commands), ()):
                raise ArtifactError(f"honest_bench/{workload}: {runtime} command is incomplete")
            measured_command = list(commands.pop())
            if not Path(measured_command[0]).is_absolute():
                raise ArtifactError(f"honest_bench/{workload}: {runtime} command is not resolved")
            runtime_results[runtime]["measured_command"] = measured_command
        benchmarks[workload] = {
            "correctness": {"status": "pass", "reference": "bun"},
            "runtimes": runtime_results,
        }
    component = {
        "schema_version": 1,
        "suite": "honest_bench",
        "commit": metadata.get("commit"),
        "generated_at": utc_z_timestamp(metadata.get("generated_at")),
        "run_config": {
            "warmup": metadata.get("harness", {}).get("warmup"),
            "requested_samples": measured,
        },
        "commands": metadata.get("commands", {}),
        "runtime_metadata": {
            runtime: {
                "version": metadata.get("toolchains", {}).get(runtime),
                "resolved_executable": metadata.get("executables", {}).get(runtime),
            }
            for runtime in RUNTIMES
        },
        "benchmarks": benchmarks,
    }
    _validate_component(component, "honest_bench")
    return component


def assemble(
    suite_path: Path,
    polyglot_path: Path,
    json_path: Path,
    app_path: Path,
    honest_results_path: Path,
    honest_metadata_path: Path,
) -> dict[str, Any]:
    suite = load_artifact(suite_path)
    _validate_suite(suite)
    suite.setdefault("run_config", {})["warmup"] = (
        "benchmark-defined internal warmup; five complete process samples"
    )
    components = {
        "suite": suite,
        "polyglot": _load(polyglot_path),
        "json_polyglot": _load(json_path),
        "app_patterns": _load(app_path),
        "honest_bench": normalize_honest(
            _load(honest_results_path), _load(honest_metadata_path)
        ),
    }
    for name in ("polyglot", "json_polyglot", "app_patterns"):
        _validate_component(components[name], name)

    full_commit = _git("rev-parse", "HEAD")
    commits = {str(component.get("commit") or "") for component in components.values()}
    if "" in commits or any(
        not full_commit.startswith(commit) and not commit.startswith(full_commit)
        for commit in commits
    ):
        raise ArtifactError(f"component commits do not match HEAD {full_commit}: {sorted(commits)}")

    runtimes = json.loads(json.dumps(suite.get("runtimes", {})))
    for runtime in RUNTIMES:
        metadata = runtimes.get(runtime, {})
        if not metadata.get("available") or not metadata.get("version") or not metadata.get("command"):
            raise ArtifactError(f"suite: {runtime} metadata is incomplete")
        if runtime in PINNED_VERSIONS and metadata["version"] != PINNED_VERSIONS[runtime]:
            raise ArtifactError(
                f"suite: expected {runtime} {PINNED_VERSIONS[runtime]}, found {metadata['version']}"
            )
        executable = metadata["command"][0]
        if runtime == "perry":
            executable = metadata.get("compile_command", [str(ROOT / "target/release/perry")])[0]
        resolved = shutil.which(executable) or str((ROOT / executable).resolve())
        metadata["resolved_executable"] = resolved

    for component_name, component in components.items():
        if component_name == "suite":
            continue
        for runtime in RUNTIMES:
            component_runtime = component["runtime_metadata"][runtime]
            if component_runtime["version"] != runtimes[runtime]["version"]:
                raise ArtifactError(
                    f"{component_name}: {runtime} version does not match suite metadata"
                )

    honest_metadata = _load(honest_metadata_path)
    toolchains = honest_metadata.get("toolchains", {})
    for runtime in RUNTIMES:
        if toolchains.get(runtime) != runtimes[runtime]["version"]:
            raise ArtifactError(
                f"honest_bench: {runtime} version {toolchains.get(runtime)!r} does not match "
                f"suite version {runtimes[runtime]['version']!r}"
            )
    host = honest_metadata.get("host", {})
    for field in ("os_version", "kernel", "arch", "cpu", "ncpu", "ram_gb"):
        if host.get(field) in (None, "", 0):
            raise ArtifactError(f"host metadata is missing {field}")
    return {
        "schema_version": 1,
        "kind": "perry-public-node-bun-baseline",
        "commit": full_commit,
        "perry_version": runtimes["perry"]["version"],
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "freshness": {
            "source_fingerprint": tracked_fingerprint(SOURCE_PATHS),
            "harness_fingerprint": tracked_fingerprint(HARNESS_PATHS),
        },
        "host": host,
        "runtimes": runtimes,
        "policy": {
            "publishable": True,
            "quiet_host": {
                "metric": "aggregate CPU active percentage",
                "maximum_percent": 25.0,
                "consecutive_seconds": 60,
                "checked_before_each_component": True,
            },
            "requirements": [
                "all required runtimes available",
                "all requested raw samples present",
                "all correctness checks passed",
                "all components measured at one commit",
            ],
        },
        "components": components,
    }


HEADLINE_SUITE = (
    ("13_factorial", "factorial", "Modular accumulation"),
    ("09_method_calls", "method_calls", "Class method dispatch"),
    ("14_closure", "closure", "Closure creation and invocation"),
    ("12_binary_trees", "binary_trees", "Tree allocation and traversal"),
    ("08_string_concat", "string_concat", "String append loop"),
    ("11_prime_sieve", "prime_sieve", "Sieve of Eratosthenes"),
    ("15_mandelbrot", "mandelbrot", "Complex-number iteration"),
    ("16_matrix_multiply", "matrix_multiply", "Matrix multiplication"),
)


def _median(entry: Mapping[str, Any], runtime: str) -> float:
    return float(entry["runtimes"][runtime]["wall_ms"]["median"])


def _fmt_ms(value: float) -> str:
    return f"{value:g} ms"


def _outcome(perry: float, node: float, bun: float) -> str:
    if perry < node and perry < bun:
        return "win vs both"
    if perry > node and perry > bun:
        return "loss vs both"
    if perry == node and perry == bun:
        return "tie"
    return "mixed"


def readme_block(artifact: Mapping[str, Any]) -> str:
    suite = artifact["components"]["suite"]["benchmarks"]
    json_suite = artifact["components"]["json_polyglot"]["benchmarks"]
    rows: list[tuple[str, Mapping[str, Any], str]] = [
        (display, suite[key], description) for key, display, description in HEADLINE_SUITE
    ]
    rows.append(("json_roundtrip", json_suite["roundtrip"], "Parse and stringify ~1 MB JSON"))
    lines = [
        README_START,
        f"Generated from [`benchmarks/results/public-node-bun-v1.json`](benchmarks/results/public-node-bun-v1.json) at Perry commit `{artifact['commit'][:12]}`.",
        "Lower wall-clock median is better; every row includes complete raw samples and passed correctness checks.",
        "",
        "| Benchmark | Perry | Node.js | Bun | Result | What it tests |",
        "|---|---:|---:|---:|---|---|",
    ]
    for name, entry, description in rows:
        perry, node, bun = (_median(entry, runtime) for runtime in RUNTIMES)
        lines.append(
            f"| {name} | {_fmt_ms(perry)} | {_fmt_ms(node)} | {_fmt_ms(bun)} | "
            f"{_outcome(perry, node, bun)} | {description} |"
        )
    lines.extend(["", README_END])
    return "\n".join(lines)


def suite_results(artifact: Mapping[str, Any]) -> str:
    component = artifact["components"]["suite"]
    runtimes = artifact["runtimes"]
    requested = component["run_config"]["requested_samples"]
    lines = [
        "# suite/ Node and Bun Results (generated)",
        "",
        f"Evidence: [`public-node-bun-v1.json`](../../results/public-node-bun-v1.json) · commit `{artifact['commit']}`",
        f"Perry: `{artifact['perry_version']}` · Node: `{runtimes['node']['version']}` · Bun: `{runtimes['bun']['version']}`",
        f"Policy: {requested} measured samples per runtime and benchmark; incomplete or incorrect rows are rejected.",
        "",
        "| Benchmark | Perry median | Node median | Bun median | Result |",
        "|---|---:|---:|---:|---|",
    ]
    wins = losses = mixed = 0
    for name, entry in component["benchmarks"].items():
        perry, node, bun = (_median(entry, runtime) for runtime in RUNTIMES)
        outcome = _outcome(perry, node, bun)
        wins += outcome == "win vs both"
        losses += outcome == "loss vs both"
        mixed += outcome not in ("win vs both", "loss vs both")
        lines.append(
            f"| {name} | {_fmt_ms(perry)} | {_fmt_ms(node)} | {_fmt_ms(bun)} | {outcome} |"
        )
    lines.extend(
        [
            "",
            "## Summary",
            "",
            f"- Wins versus both peers: **{wins}**",
            f"- Losses versus both peers: **{losses}**",
            f"- Mixed or tied rows: **{mixed}**",
            "",
            "> Historical note: the former v0.5.908 single-run commentary is archived in Git history and is not current evidence.",
            "",
        ]
    )
    return "\n".join(lines)


def _replace_block(text: str, block: str) -> str:
    if README_START not in text or README_END not in text:
        raise ArtifactError("README generated markers are missing")
    before, remainder = text.split(README_START, 1)
    _, after = remainder.split(README_END, 1)
    return before + block + after


def render(artifact: Mapping[str, Any]) -> None:
    README.write_text(
        _replace_block(README.read_text(encoding="utf-8"), readme_block(artifact)),
        encoding="utf-8",
    )
    SUITE_RESULTS.write_text(suite_results(artifact), encoding="utf-8")


def validate_public(artifact: Mapping[str, Any], max_age_days: int) -> None:
    if artifact.get("schema_version") != 1 or not artifact.get("policy", {}).get("publishable"):
        raise ArtifactError("public artifact is not publishable schema version 1")
    generated = datetime.fromisoformat(str(artifact["generated_at"]).replace("Z", "+00:00"))
    age = datetime.now(timezone.utc) - generated
    if age.total_seconds() < -300:
        raise ArtifactError("public artifact timestamp is in the future")
    if age.days > max_age_days:
        raise ArtifactError(
            f"public artifact is stale ({age.days} days old); regenerate it with {REFRESH_COMMAND}"
        )
    freshness = artifact.get("freshness", {})
    if freshness.get("source_fingerprint") != tracked_fingerprint(SOURCE_PATHS):
        raise ArtifactError(
            f"public artifact benchmark inputs changed; regenerate it with {REFRESH_COMMAND}"
        )
    if freshness.get("harness_fingerprint") != tracked_fingerprint(HARNESS_PATHS):
        raise ArtifactError(
            f"public artifact benchmark harness changed; regenerate it with {REFRESH_COMMAND}"
        )
    for name in ("polyglot", "json_polyglot", "app_patterns", "honest_bench"):
        _validate_component(artifact["components"][name], name)
    _validate_suite(artifact["components"]["suite"])


def check(artifact: Mapping[str, Any], max_age_days: int) -> None:
    validate_public(artifact, max_age_days)
    expected_readme = _replace_block(README.read_text(encoding="utf-8"), readme_block(artifact))
    if expected_readme != README.read_text(encoding="utf-8"):
        raise ArtifactError("README Node/Bun table has drifted from the public artifact")
    if suite_results(artifact) != SUITE_RESULTS.read_text(encoding="utf-8"):
        raise ArtifactError("suite RESULTS.md has drifted from the public artifact")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    assemble_parser = sub.add_parser("assemble")
    assemble_parser.add_argument("--suite", type=Path, required=True)
    assemble_parser.add_argument("--polyglot", type=Path, required=True)
    assemble_parser.add_argument("--json-polyglot", type=Path, required=True)
    assemble_parser.add_argument("--app-patterns", type=Path, required=True)
    assemble_parser.add_argument("--honest-results", type=Path, required=True)
    assemble_parser.add_argument("--honest-metadata", type=Path, required=True)
    assemble_parser.add_argument("--output", type=Path, default=DEFAULT_ARTIFACT)
    render_parser = sub.add_parser("render")
    render_parser.add_argument("--artifact", type=Path, default=DEFAULT_ARTIFACT)
    check_parser = sub.add_parser("check")
    check_parser.add_argument("--artifact", type=Path, default=DEFAULT_ARTIFACT)
    check_parser.add_argument("--max-age-days", type=int, default=45)
    args = parser.parse_args(argv)
    try:
        if args.command == "assemble":
            artifact = assemble(
                args.suite,
                args.polyglot,
                args.json_polyglot,
                args.app_patterns,
                args.honest_results,
                args.honest_metadata,
            )
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
        elif args.command == "render":
            artifact = _load(args.artifact)
            validate_public(artifact, 10_000)
            render(artifact)
        else:
            check(_load(args.artifact), args.max_age_days)
        return 0
    except (ArtifactError, KeyError, OSError, subprocess.CalledProcessError) as exc:
        print(f"public baseline error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
