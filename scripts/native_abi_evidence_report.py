#!/usr/bin/env python3
"""Build a PR-ready native ABI evidence packet from retained artifacts."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional


SCHEMA_VERSION = 1

REQUIRED_CORRECTNESS = {
    "native_abi_contract": {
        "label": "Native ABI contract",
        "dir": "native-abi-contract",
        "stdout": "PASS",
        "tokens": (
            '"native_rep_name": "u32"',
            '"native_rep_name": "u64"',
            '"native_rep_name": "usize"',
            '"native_rep_name": "f32"',
            '"native_rep_name": "buffer_len"',
            '"native_rep_name": "native_handle"',
            '"native_rep_name": "promise_boundary"',
            '"native_rep_name": "pod_record"',
            '"op": "native_handle_box"',
            '"op": "promise_box"',
        ),
    },
    "c_layout_pod_records": {
        "label": "C-layout POD records",
        "dir": "c-layout-pod-records",
        "stdout": "read=7,1.5,2.25,4",
        "tokens": (
            '"native_rep_name": "pod_record"',
            '"pod_layouts"',
            '"packing": "c"',
            '"materialization_reason": "pod_dynamic_mutation"',
        ),
    },
}

REQUIRED_RUNTIME_TESTS = (
    "resolves_once_and_duplicate_returns_status",
    "main_thread_token_wrong_thread_rejects",
    "main_thread_token_wrong_thread_cancel_rejects_instead_of_cancelling",
    "reject_cleanup_disposes_attached_handles_but_success_keeps_them_live",
    "test_native_async_completion_token_roots_survive_copied_minor_gc",
)

REQUIRED_COMPILER_ARTIFACTS = (
    "hir",
    "llvm_before_opt",
    "llvm_after_opt_analysis",
    "object_disassembly",
)

SAFETY_CHECK_NAMES = (
    "native_reps_no_unsafe_inbounds_claims",
    "native_reps_no_unsafe_noalias_claims",
    "native_reps_no_unchecked_unknown_bounds",
    "native_reps_no_checked_unknown_bounds",
    "native_reps_no_unexpected_materialization_reasons",
)

DELTA_FIELDS = (
    "boxed_number_allocations_static",
    "buffer_slow_path_accesses_static",
    "allocations_traced",
    "write_barriers_static",
    "runtime_calls_static",
)

REQUIRED_IMPROVEMENT_FIELDS = (
    "boxed_number_allocations_static",
    "buffer_slow_path_accesses_static",
    "allocations_traced",
)


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def load_json(path: Path, default: Any = None) -> Any:
    if not path.exists():
        return default
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def nested(obj: Any, *keys: str, default: Any = None) -> Any:
    cur = obj
    for key in keys:
        if not isinstance(cur, dict):
            return default
        cur = cur.get(key, default)
    return cur


def int_value(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    return 0


def number_value(value: Any) -> Optional[float]:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    return None


def resolve_path(path_value: Any, root: Path) -> Optional[Path]:
    if not isinstance(path_value, str) or not path_value:
        return None
    path = Path(path_value)
    if path.is_absolute():
        return path
    return root / path


def path_exists(path_value: Any, root: Path) -> bool:
    path = resolve_path(path_value, root)
    return bool(path and path.exists())


def command_entry(metadata: dict[str, Any], label: str, name: str) -> dict[str, Any]:
    entry = nested(metadata, "commands", label, name, default={})
    return entry if isinstance(entry, dict) else {}


def command_status(metadata: dict[str, Any], label: str, name: str) -> str:
    entry = command_entry(metadata, label, name)
    status = entry.get("status")
    if isinstance(status, str):
        return status
    code = entry.get("exit_code")
    if isinstance(code, int):
        return "pass" if code == 0 else "fail"
    return "missing"


def rel(path: Path, root: Path) -> str:
    try:
        return str(path.resolve().relative_to(root.resolve()))
    except Exception:
        return str(path)


def ratio_delta(control: Optional[float], typed: Optional[float]) -> dict[str, Any]:
    if control is None or typed is None:
        return {"control": control, "typed": typed, "delta": None, "delta_pct": None}
    pct = None if control == 0 else ((typed - control) / control) * 100.0
    return {
        "control": control,
        "typed": typed,
        "delta": typed - control,
        "delta_pct": None if pct is None else round(pct, 1),
    }


def native_reps_text(evidence_dir: Path) -> str:
    text = read_text(evidence_dir / "native-reps.txt")
    if text:
        return text
    chunks = []
    for path in sorted((evidence_dir / "native-reps").glob("*.json")):
        chunks.append(read_text(path))
    return "\n".join(chunks)


def correctness_summary(
    root: Path,
    metadata: dict[str, Any],
    errors: list[str],
    *,
    gate: bool,
) -> dict[str, Any]:
    base = root / "correctness"
    result: dict[str, Any] = {}
    for name, spec in REQUIRED_CORRECTNESS.items():
        evidence_dir = base / str(spec["dir"])
        stdout = read_text(evidence_dir / "runtime.stdout")
        reps = native_reps_text(evidence_dir)
        missing_tokens = [token for token in spec["tokens"] if token not in reps]
        command = command_entry(metadata, "correctness", name)
        status = command_status(metadata, "correctness", name)
        passed = (
            status == "pass"
            and bool(stdout)
            and str(spec["stdout"]) in stdout
            and not missing_tokens
        )
        result[name] = {
            "label": spec["label"],
            "status": "pass" if passed else "fail",
            "command": command,
            "evidence_dir": str(evidence_dir),
            "compile_log_present": (evidence_dir / "compile.log").exists(),
            "runtime_stdout_present": bool(stdout),
            "native_reps_artifact_count": len(list((evidence_dir / "native-reps").glob("*.json"))),
            "missing_tokens": missing_tokens,
        }
        if gate and status != "pass":
            errors.append(f"correctness:{name}: command status is {status}")
        if gate and not stdout:
            errors.append(f"correctness:{name}: runtime stdout evidence is missing")
        if gate and missing_tokens:
            errors.append(f"correctness:{name}: native-reps tokens missing: {missing_tokens}")
    return result


def artifact_path_from_manifest(
    manifest: dict[str, Any],
    key: str,
    artifact_root: Path,
) -> tuple[str, bool]:
    value = nested(manifest, "artifacts", key)
    if isinstance(value, dict):
        value = value.get("path")
    path = resolve_path(value, artifact_root) if value else None
    return (str(path) if path else "", bool(path and path.exists()))


def retained_objects_ok(manifest: dict[str, Any], artifact_root: Path) -> bool:
    retained = nested(manifest, "artifacts", "retained_objects", default=[])
    if not isinstance(retained, list) or not retained:
        return False
    for row in retained:
        if not isinstance(row, dict):
            return False
        if not path_exists(row.get("object_artifact"), artifact_root):
            return False
        if not path_exists(row.get("compile_plan_artifact"), artifact_root):
            return False
    return True


def native_reps_ok(manifest: dict[str, Any], artifact_root: Path) -> bool:
    retained = nested(manifest, "artifacts", "native_reps", default=[])
    if not isinstance(retained, list) or not retained:
        return False
    return all(
        isinstance(row, dict) and path_exists(row.get("native_reps_artifact"), artifact_root)
        for row in retained
    )


def compiler_output_summary(
    root: Path,
    metadata: dict[str, Any],
    errors: list[str],
    warnings: list[str],
    *,
    gate: bool,
) -> dict[str, Any]:
    suite_root = root / "compiler-output" / "native-abi-proof"
    suite_report_path = suite_root / "suite-report.json"
    suite_report = load_json(suite_report_path, {})
    status = command_status(metadata, "packet", "compiler_output")
    if gate and status != "pass":
        errors.append(f"packet: compiler_output command status is {status}")
    elif status not in ("pass", "skipped"):
        warnings.append(f"packet: compiler_output command status is {status}")
    if gate and not suite_report:
        errors.append("compiler-output native-abi-proof suite report is missing")

    workloads: dict[str, Any] = {}
    rows = suite_report.get("workloads") if isinstance(suite_report, dict) else []
    if not isinstance(rows, list):
        rows = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        name = str(row.get("workload") or "")
        artifact_dir = resolve_path(row.get("artifact_dir"), root) or (suite_root / name)
        manifest_path = artifact_dir / "manifest.json"
        report_path = artifact_dir / "structural-report.json"
        manifest = load_json(manifest_path, {})
        structural = load_json(report_path, {})
        artifacts = {
            key: artifact_path_from_manifest(manifest, key, artifact_dir)
            for key in REQUIRED_COMPILER_ARTIFACTS
        }
        missing_artifacts = [
            key for key, (_path, exists) in artifacts.items() if not exists
        ]
        if not retained_objects_ok(manifest, artifact_dir):
            missing_artifacts.append("retained_objects_or_compile_plan")
        if not native_reps_ok(manifest, artifact_dir):
            missing_artifacts.append("native_reps")
        safety_checks = [
            check
            for check in structural.get("checks", []) or []
            if isinstance(check, dict)
            and any(str(check.get("name", "")).endswith(name) for name in SAFETY_CHECK_NAMES)
        ]
        failing_safety = [
            check for check in safety_checks if check.get("status") != "pass"
        ]
        workload_status = "pass"
        if row.get("status") != "pass" or structural.get("status") != "pass":
            workload_status = "fail"
        if missing_artifacts or failing_safety:
            workload_status = "fail"
        workloads[name] = {
            "status": workload_status,
            "suite_status": row.get("status"),
            "exit_code": row.get("exit_code"),
            "artifact_dir": str(artifact_dir),
            "manifest": str(manifest_path),
            "structural_report": str(report_path),
            "missing_artifacts": missing_artifacts,
            "safety_checks": safety_checks,
            "failing_safety_checks": failing_safety,
            "runtime_counter_summary": manifest.get("runtime_counter_summary", {}),
            "benchmark": manifest.get("benchmark", {}),
            "errors": list(row.get("errors") or []) + list(structural.get("errors") or []),
        }
        if gate and workload_status != "pass":
            errors.append(f"compiler-output:{name}: {workload_status}; {workloads[name]['errors'] or missing_artifacts}")

    required = {"native_abi_packet_typed", "native_abi_packet_control"}
    missing_required = sorted(required - set(workloads))
    if gate and missing_required:
        errors.append(f"compiler-output: required packet workloads missing: {missing_required}")

    return {
        "status": suite_report.get("status", "missing") if isinstance(suite_report, dict) else "missing",
        "command": command_entry(metadata, "packet", "compiler_output"),
        "suite_report": str(suite_report_path),
        "workloads": workloads,
        "failed_workloads": suite_report.get("failed_workloads", []) if isinstance(suite_report, dict) else [],
    }


def benchmark_deltas(compiler: dict[str, Any], errors: list[str], *, gate: bool) -> dict[str, Any]:
    workloads = compiler.get("workloads", {})
    typed = workloads.get("native_abi_packet_typed", {})
    control = workloads.get("native_abi_packet_control", {})
    if not typed or not control:
        if gate:
            errors.append("benchmark deltas require native_abi_packet_typed and native_abi_packet_control")
        return {"status": "missing", "fields": {}}

    fields: dict[str, Any] = {}
    typed_summary = typed.get("runtime_counter_summary", {})
    control_summary = control.get("runtime_counter_summary", {})
    for field in DELTA_FIELDS:
        fields[field] = ratio_delta(
            number_value(control_summary.get(field)),
            number_value(typed_summary.get(field)),
        )

    fields["median_wall_ms"] = ratio_delta(
        number_value(nested(control, "benchmark", "median_wall_ms")),
        number_value(nested(typed, "benchmark", "median_wall_ms")),
    )
    fields["mean_wall_ms"] = ratio_delta(
        number_value(nested(control, "benchmark", "mean_wall_ms")),
        number_value(nested(typed, "benchmark", "mean_wall_ms")),
    )
    missing = [name for name, delta in fields.items() if delta["typed"] is None or delta["control"] is None]
    if gate and missing:
        errors.append(f"benchmark deltas missing values: {missing}")
    non_improving = []
    for field in REQUIRED_IMPROVEMENT_FIELDS:
        delta = fields.get(field, {})
        control_value = delta.get("control")
        typed_value = delta.get("typed")
        if control_value is None or typed_value is None:
            continue
        if control_value <= 0:
            non_improving.append(
                f"{field}: control must be positive to prove a reduction "
                f"(control={control_value}, typed={typed_value})"
            )
        elif typed_value >= control_value:
            non_improving.append(
                f"{field}: typed must be lower than control "
                f"(control={control_value}, typed={typed_value})"
            )
    if gate and non_improving:
        errors.append(f"benchmark deltas missing required improvements: {non_improving}")
    return {
        "status": "pass" if not missing and not non_improving else "fail",
        "typed_workload": "native_abi_packet_typed",
        "control_workload": "native_abi_packet_control",
        "required_improvement_fields": list(REQUIRED_IMPROVEMENT_FIELDS),
        "missing_values": missing,
        "non_improving_required_fields": non_improving,
        "fields": fields,
    }


def runtime_safety_summary(
    root: Path,
    metadata: dict[str, Any],
    errors: list[str],
    *,
    gate: bool,
) -> dict[str, Any]:
    command = command_entry(metadata, "runtime", "native_async")
    status = command_status(metadata, "runtime", "native_async")
    log_path = resolve_path(command.get("log"), root)
    log = read_text(log_path) if log_path else ""
    observed = [name for name in REQUIRED_RUNTIME_TESTS if name in log]
    missing = [name for name in REQUIRED_RUNTIME_TESTS if name not in observed]
    if gate and status != "pass":
        errors.append(f"runtime:native_async: command status is {status}")
    if gate and missing:
        errors.append(f"runtime:native_async: expected test names missing from log: {missing}")
    return {
        "status": "pass" if status == "pass" and not missing else "fail",
        "command": command,
        "log": str(log_path) if log_path else "",
        "required_tests": list(REQUIRED_RUNTIME_TESTS),
        "observed_tests": observed,
        "missing_tests": missing,
    }


def build_packet(root: Path, metadata_path: Path, repo_root: Path, *, gate: bool) -> dict[str, Any]:
    metadata = load_json(metadata_path, {})
    errors: list[str] = []
    warnings: list[str] = []

    correctness = correctness_summary(root, metadata, errors, gate=gate)
    compiler = compiler_output_summary(root, metadata, errors, warnings, gate=gate)
    runtime = runtime_safety_summary(root, metadata, errors, gate=gate)
    deltas = benchmark_deltas(compiler, errors, gate=gate)

    commands = metadata.get("commands", {}) if isinstance(metadata, dict) else {}
    packet = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": utc_now(),
        "status": "fail" if errors else "pass",
        "gate": gate,
        "root": str(root),
        "metadata": str(metadata_path),
        "errors": errors,
        "warnings": warnings,
        "tool_versions": metadata.get("tool_versions", {}) if isinstance(metadata, dict) else {},
        "commands": commands,
        "artifact_verification": {
            "correctness_dirs": {
                name: row["evidence_dir"] for name, row in correctness.items()
            },
            "compiler_suite_report": compiler.get("suite_report"),
        },
        "correctness": correctness,
        "native_call_lowering": compiler,
        "gc_root_safety": runtime,
        "benchmark_deltas": deltas,
    }
    return packet


def markdown_for_packet(packet: dict[str, Any], repo_root: Path) -> str:
    status = str(packet.get("status", "missing")).upper()
    lines = [
        f"# Native ABI Evidence Packet: {status}",
        "",
        f"- Generated: `{packet.get('generated_at', '')}`",
        f"- Root: `{packet.get('root', '')}`",
        f"- Gate: `{packet.get('gate')}`",
    ]
    if packet.get("errors"):
        lines.append("")
        lines.append("## Gate Failures")
        lines.extend(f"- {error}" for error in packet["errors"])

    lines.append("")
    lines.append("## Correctness Fixtures")
    for name, row in packet.get("correctness", {}).items():
        lines.append(
            f"- `{name}`: `{row.get('status')}`; native-reps={row.get('native_reps_artifact_count')}; "
            f"dir=`{row.get('evidence_dir')}`"
        )

    lines.append("")
    lines.append("## Native Call Lowering")
    lowering = packet.get("native_call_lowering", {})
    lines.append(f"- Suite: `{lowering.get('status', 'missing')}` report=`{lowering.get('suite_report', '')}`")
    for name, row in lowering.get("workloads", {}).items():
        lines.append(
            f"- `{name}`: `{row.get('status')}`; missing_artifacts={len(row.get('missing_artifacts', []))}; "
            f"safety_failures={len(row.get('failing_safety_checks', []))}"
        )

    lines.append("")
    lines.append("## GC / Root Safety")
    safety = packet.get("gc_root_safety", {})
    lines.append(
        f"- Native async runtime tests: `{safety.get('status', 'missing')}`; "
        f"observed={len(safety.get('observed_tests', []))}/{len(safety.get('required_tests', []))}"
    )

    lines.append("")
    lines.append("## Packet Deltas")
    deltas = packet.get("benchmark_deltas", {})
    for field, delta in deltas.get("fields", {}).items():
        lines.append(
            f"- `{field}`: control={delta.get('control')} typed={delta.get('typed')} "
            f"delta={delta.get('delta')} delta_pct={delta.get('delta_pct')}"
        )

    return "\n".join(lines) + "\n"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--metadata")
    parser.add_argument("--repo-root", default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument("--json-out")
    parser.add_argument("--md-out")
    parser.add_argument("--gate", action="store_true")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    root = Path(args.root).resolve()
    repo_root = Path(args.repo_root).resolve()
    metadata_path = Path(args.metadata).resolve() if args.metadata else root / "metadata.json"
    json_out = Path(args.json_out).resolve() if args.json_out else root / "native-abi-evidence.json"
    md_out = Path(args.md_out).resolve() if args.md_out else root / "native-abi-evidence.md"

    packet = build_packet(root, metadata_path, repo_root, gate=args.gate)
    write_json(json_out, packet)
    md_out.parent.mkdir(parents=True, exist_ok=True)
    md_out.write_text(markdown_for_packet(packet, repo_root), encoding="utf-8")
    return 1 if packet["status"] == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
