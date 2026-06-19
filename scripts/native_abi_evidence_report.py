#!/usr/bin/env python3
"""Build a PR-ready native ABI evidence packet from retained artifacts."""

from __future__ import annotations

import argparse
import json
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional


SCHEMA_VERSION = 1

SCOPE = {
    "summary": (
        "Evidence covers selected native binding descriptors and region-local "
        "native type lowering."
    ),
    "not_covered": (
        "This packet does not claim a general typed function/method/closure "
        "ABI, typed clones, or generic trampoline dispatch."
    ),
}

GATE_MATRIX_SPEC = (
    {
        "area": "native_abi_correctness",
        "label": "Native ABI correctness",
        "evidence": "native_abi_contract and C-layout POD fixtures",
        "gate": "runtime PASS output plus required native-rep ABI tokens",
    },
    {
        "area": "native_region_artifacts",
        "label": "Native-region artifact chain",
        "evidence": "native-abi-proof compiler-output retained HIR/LLVM/object/native-rep artifacts",
        "gate": "required artifacts, structural safety checks, checksum checks, and packet contracts",
    },
    {
        "area": "explain_lowering_accounting",
        "label": "Explain-lowering accounting",
        "evidence": "native-rep records summarized into boxes, conversions, fallbacks, barriers, and typed records",
        "gate": "typed/control material accounting rows must pass quantitative thresholds",
    },
    {
        "area": "runtime_safety",
        "label": "Runtime safety",
        "evidence": "native async runtime tests and GC/rooting checks",
        "gate": "required runtime test names must pass and be present in logs",
    },
    {
        "area": "release_symbols",
        "label": "Release/LTO symbol guard",
        "evidence": "runtime archive symbol sentinel scan",
        "gate": "archive must define all sentinel symbols",
    },
)

REQUIRED_CORRECTNESS = {
    "native_abi_contract": {
        "label": "Selected native ABI contract",
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

REQUIRED_RELEASE_SYMBOL_TOKENS = (
    "defines all",
    "sentinel symbols",
)
REQUIRED_RELEASE_SENTINEL_COUNT = 101
REQUIRED_RELEASE_FINGERPRINT_FIELDS = (
    "runtime_archive_sha256",
    "runtime_source_digest",
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

REQUIRED_PACKET_STDOUT_CHECKS = {
    "native_abi_packet_typed": "native_abi_packet_typed_checksum",
    "native_abi_packet_control": "native_abi_packet_control_checksum",
}

DELTA_FIELDS = (
    "boxed_number_allocations_static",
    "buffer_slow_path_accesses_static",
    "array_slow_path_accesses_static",
    "allocations_traced",
    "write_barriers_static",
    "write_barriers_traced",
    "runtime_calls_static",
)

REQUIRED_IMPROVEMENT_FIELDS = (
    "boxed_number_allocations_static",
    "buffer_slow_path_accesses_static",
    "array_slow_path_accesses_static",
    "allocations_traced",
)

MATERIAL_REDUCTION_THRESHOLDS = {
    "allocations_traced": 95.0,
    "write_barriers_static": 75.0,
    "write_barriers_traced": 95.0,
    "runtime_calls_static": 25.0,
}

MATERIAL_ELIMINATION_FIELDS = (
    "boxed_number_allocations_static",
    "buffer_slow_path_accesses_static",
    "array_slow_path_accesses_static",
)

MATERIAL_SPEEDUP_THRESHOLDS = {
    "median_wall_ms": 2.0,
    "p95_wall_ms": 1.5,
}

MATERIAL_REQUIRED_STAT_QUALITY = "timing"

MATERIAL_ACCOUNTING_CONTRACT = (
    {
        "field": "boxed_number_allocations_static",
        "category": "boxes",
        "source": "optimized IR helper counter",
        "typed_max": 0,
        "control_min": 1,
        "reduction_min": 100.0,
        "proves": "typed packet avoids boxed Number allocation helpers",
    },
    {
        "field": "buffer_slow_path_accesses_static",
        "category": "helpers",
        "source": "optimized IR helper counter",
        "typed_max": 0,
        "control_min": 1,
        "reduction_min": 100.0,
        "proves": "typed packet avoids Buffer slow-path helpers",
    },
    {
        "field": "array_slow_path_accesses_static",
        "category": "helpers",
        "source": "optimized IR helper counter",
        "typed_max": 0,
        "control_min": 1,
        "reduction_min": 100.0,
        "proves": "typed packet avoids typed-array/Uint8Array slow-path helpers",
    },
    {
        "field": "runtime_calls_static",
        "category": "helpers",
        "source": "optimized IR runtime-call counter",
        "control_min": 1,
        "reduction_min": MATERIAL_REDUCTION_THRESHOLDS["runtime_calls_static"],
        "proves": "typed packet removes representative runtime helper call sites",
    },
    {
        "field": "allocations_traced",
        "category": "allocations",
        "source": "GC trace allocation counter",
        "control_min": 1,
        "reduction_min": MATERIAL_REDUCTION_THRESHOLDS["allocations_traced"],
        "proves": "typed packet removes representative traced runtime allocations",
    },
    {
        "field": "write_barriers_static",
        "category": "barriers",
        "source": "optimized IR write-barrier counter",
        "control_min": 1,
        "reduction_min": MATERIAL_REDUCTION_THRESHOLDS["write_barriers_static"],
        "proves": "typed packet removes representative static write-barrier helper sites",
    },
    {
        "field": "write_barriers_traced",
        "category": "barriers",
        "source": "GC trace write-barrier counter",
        "control_min": 1,
        "reduction_min": MATERIAL_REDUCTION_THRESHOLDS["write_barriers_traced"],
        "proves": "typed packet removes representative runtime write-barrier traffic",
    },
    {
        "field": "median_wall_ms",
        "category": "benchmark",
        "source": "packet timing",
        "speedup_min": MATERIAL_SPEEDUP_THRESHOLDS["median_wall_ms"],
        "proves": "typed packet has material median wall-time speedup",
    },
    {
        "field": "p95_wall_ms",
        "category": "benchmark",
        "source": "packet timing",
        "speedup_min": MATERIAL_SPEEDUP_THRESHOLDS["p95_wall_ms"],
        "proves": "typed packet keeps tail latency materially faster",
    },
)

MATERIAL_CONTRACTS = {
    "reductions": MATERIAL_REDUCTION_THRESHOLDS,
    "eliminations": {field: 0 for field in MATERIAL_ELIMINATION_FIELDS},
    "speedups": MATERIAL_SPEEDUP_THRESHOLDS,
    "stat_quality": MATERIAL_REQUIRED_STAT_QUALITY,
}

PACKET_WORKLOAD_CONTRACTS: dict[str, dict[str, Any]] = {
    "native_abi_packet_typed": {
        "source": "benchmarks/compiler_output/fixtures/native_abi_packet_typed.ts",
        "kind": "native_abi_packet_typed",
        "zero_static_fields": (
            "boxed_number_allocations_static",
            "buffer_slow_path_accesses_static",
            "array_slow_path_accesses_static",
        ),
        "required_native_records": (
            {
                "name": "typed_unchecked_buffer_view",
                "native_rep_name": "buffer_view",
                "consumer_contains": "BufferView",
                "access_mode": "unchecked_native",
                "bounds_state": "proven_or_guarded",
            },
            {
                "name": "typed_unchecked_u8_access",
                "native_rep_name": "u8",
                "consumer_contains": "u8_",
                "access_mode": "unchecked_native",
                "bounds_state": "proven_or_guarded",
            },
        ),
    },
    "native_abi_packet_control": {
        "source": "benchmarks/compiler_output/fixtures/native_abi_packet_control.ts",
        "kind": "native_abi_packet_control",
        "positive_static_fields": (
            "boxed_number_allocations_static",
            "buffer_slow_path_accesses_static",
            "array_slow_path_accesses_static",
            "write_barriers_static",
            "runtime_calls_static",
        ),
    },
}


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
        return {
            "control": control,
            "typed": typed,
            "delta": None,
            "delta_pct": None,
            "reduction_pct": None,
            "speedup": None,
        }
    delta = typed - control
    pct = None if control == 0 else (delta / control) * 100.0
    reduction_pct = None if control == 0 else ((control - typed) / control) * 100.0
    speedup = None if typed <= 0 else control / typed
    return {
        "control": control,
        "typed": typed,
        "delta": delta,
        "delta_pct": None if pct is None else round(pct, 1),
        "reduction_pct": None if reduction_pct is None else round(reduction_pct, 1),
        "speedup": None if speedup is None else round(speedup, 3),
    }


def release_sentinel_counts(log: str) -> list[int]:
    counts: list[int] = []
    for match in re.finditer(r"defines all\s+(\d+)\s+sentinel symbols", log):
        try:
            counts.append(int(match.group(1)))
        except ValueError:
            continue
    return counts


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


def state_name(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and value:
        return str(next(iter(value.keys())))
    return str(value)


def bounds_allows_inbounds(value: Any) -> bool:
    return state_name(value) in {"proven", "guarded"}


def native_rep_records(
    manifest: dict[str, Any],
    artifact_root: Path,
) -> tuple[list[dict[str, Any]], int]:
    retained = nested(manifest, "artifacts", "native_reps", default=[])
    if not isinstance(retained, list):
        return ([], 0)
    records: list[dict[str, Any]] = []
    artifact_count = 0
    for row in retained:
        if not isinstance(row, dict):
            continue
        path = resolve_path(row.get("native_reps_artifact"), artifact_root)
        if not path or not path.exists():
            continue
        artifact_count += 1
        artifact = load_json(path, {})
        artifact_records = artifact.get("records", []) if isinstance(artifact, dict) else []
        for record in artifact_records:
            if isinstance(record, dict):
                records.append(record)
    return (records, artifact_count)


def packet_record_matches(record: dict[str, Any], required: dict[str, Any]) -> bool:
    if "native_rep_name" in required and state_name(record.get("native_rep_name")) != str(
        required["native_rep_name"]
    ):
        return False
    if "native_value_state" in required and state_name(record.get("native_value_state")) != str(
        required["native_value_state"]
    ):
        return False
    if "consumer_contains" in required and str(required["consumer_contains"]) not in str(
        record.get("consumer") or ""
    ):
        return False
    if "access_mode" in required and state_name(record.get("access_mode")) != str(
        required["access_mode"]
    ):
        return False
    if "materialization_reason" in required and state_name(
        record.get("materialization_reason")
    ) != str(required["materialization_reason"]):
        return False
    if "fallback_reason" in required and state_name(record.get("fallback_reason")) != str(
        required["fallback_reason"]
    ):
        return False
    if required.get("bounds_state") == "proven_or_guarded" and not bounds_allows_inbounds(
        record.get("bounds_state")
    ):
        return False
    return True


def count_key(counts: dict[str, int], value: Any) -> None:
    key = state_name(value)
    if key:
        counts[key] = counts.get(key, 0) + 1


def record_notes(record: dict[str, Any]) -> list[str]:
    notes = record.get("notes", [])
    if not isinstance(notes, list):
        return []
    return [str(note) for note in notes if isinstance(note, str)]


def transition_op(record: dict[str, Any], field: str) -> str:
    value = record.get(field)
    if not isinstance(value, dict):
        return ""
    return state_name(value.get("op"))


def transition_to_rep(record: dict[str, Any]) -> str:
    value = record.get("native_abi_transition")
    if not isinstance(value, dict):
        return ""
    return state_name(value.get("to_native_rep"))


def is_unbox_or_coercion_op(op: str) -> bool:
    return op in {
        "js_value_to_bits",
        "bits_to_js_value",
        "signed_int_to_float",
        "unsigned_int_to_float",
        "float_extend",
    }


def explain_lowering_accounting(
    records: list[dict[str, Any]],
    runtime_summary: Any,
) -> dict[str, Any]:
    native_rep_counts: dict[str, int] = {}
    native_value_state_counts: dict[str, int] = {}
    access_mode_counts: dict[str, int] = {}
    materialization_reason_counts: dict[str, int] = {}
    fallback_reason_counts: dict[str, int] = {}
    boxes = 0
    unboxes_or_coercions = 0
    dynamic_fallbacks = 0
    barrier_eliminations = 0
    barrier_emissions = 0
    typed_native_records = 0
    js_value_bits_records = 0

    for record in records:
        native_rep = state_name(record.get("native_rep_name"))
        native_value_state = state_name(record.get("native_value_state"))
        access_mode = state_name(record.get("access_mode"))
        materialization_reason = state_name(record.get("materialization_reason"))
        fallback_reason = state_name(record.get("fallback_reason"))
        notes = record_notes(record)
        notes_text = ";".join(notes)
        consumer = str(record.get("consumer") or "")
        expr_kind = str(record.get("expr_kind") or "")

        count_key(native_rep_counts, native_rep)
        count_key(native_value_state_counts, native_value_state)
        count_key(access_mode_counts, access_mode)
        count_key(materialization_reason_counts, materialization_reason)
        count_key(fallback_reason_counts, fallback_reason)

        if native_rep and native_rep != "js_value":
            typed_native_records += 1
        if native_rep == "js_value_bits":
            js_value_bits_records += 1

        if materialization_reason or transition_to_rep(record) == "js_value":
            boxes += 1
        for op in (
            transition_op(record, "native_abi_transition"),
            transition_op(record, "scalar_conversion"),
        ):
            if is_unbox_or_coercion_op(op):
                unboxes_or_coercions += 1

        if access_mode == "dynamic_fallback" or native_value_state == "dynamic_fallback" or fallback_reason:
            dynamic_fallbacks += 1

        if (
            "barrier=elided" in notes_text
            or "barrier_eliminated" in notes_text
            or "write_barrier=0" in notes_text
            or "without_barrier" in notes_text
        ):
            barrier_eliminations += 1
        elif (
            "barrier=emitted" in notes_text
            or "write_barrier=1" in notes_text
            or consumer == "write_barrier.child_bits"
            or "write_barrier_slot" in consumer
            or "write_barrier_root" in consumer
            or expr_kind == "WriteBarrier"
        ):
            barrier_emissions += 1

    summary = runtime_summary if isinstance(runtime_summary, dict) else {}
    return {
        "record_count": len(records),
        "typed_native_records": typed_native_records,
        "js_value_bits_records": js_value_bits_records,
        "boxes_inserted": boxes,
        "unboxes_or_coercions": unboxes_or_coercions,
        "dynamic_fallbacks": dynamic_fallbacks,
        "barrier_eliminations": barrier_eliminations,
        "barrier_emissions": barrier_emissions,
        "native_rep_counts": dict(sorted(native_rep_counts.items())),
        "native_value_state_counts": dict(sorted(native_value_state_counts.items())),
        "access_mode_counts": dict(sorted(access_mode_counts.items())),
        "materialization_reason_counts": dict(sorted(materialization_reason_counts.items())),
        "fallback_reason_counts": dict(sorted(fallback_reason_counts.items())),
        "runtime_counter_summary": {
            field: int_value(summary.get(field))
            for field in DELTA_FIELDS
            if field in summary
        },
    }


def packet_workload_contract(
    workload: str,
    manifest: dict[str, Any],
    artifact_root: Path,
) -> dict[str, Any]:
    contract = PACKET_WORKLOAD_CONTRACTS.get(workload)
    if not contract:
        return {"status": "skipped"}

    errors: list[str] = []
    if manifest.get("workload") != workload:
        errors.append(
            f"manifest workload must be {workload!r} (got {manifest.get('workload')!r})"
        )
    expected_kind = contract["kind"]
    if manifest.get("workload_kind") != expected_kind:
        errors.append(
            f"manifest workload_kind must be {expected_kind!r} "
            f"(got {manifest.get('workload_kind')!r})"
        )
    expected_source = contract["source"]
    if manifest.get("source") != expected_source:
        errors.append(
            f"manifest source must be {expected_source!r} (got {manifest.get('source')!r})"
        )

    summary = manifest.get("runtime_counter_summary", {})
    if not isinstance(summary, dict):
        summary = {}
    static_counter_checks: list[dict[str, Any]] = []
    for field in contract.get("zero_static_fields", ()) or ():
        value = number_value(summary.get(field))
        passed = value == 0
        static_counter_checks.append(
            {"field": field, "expected": "zero", "actual": value, "passed": passed}
        )
        if not passed:
            errors.append(f"{field} must be zero for {workload} (got {value})")
    for field in contract.get("positive_static_fields", ()) or ():
        value = number_value(summary.get(field))
        passed = value is not None and value > 0
        static_counter_checks.append(
            {"field": field, "expected": "positive", "actual": value, "passed": passed}
        )
        if not passed:
            errors.append(f"{field} must be positive for {workload} (got {value})")

    records, artifact_count = native_rep_records(manifest, artifact_root)
    required_record_checks: list[dict[str, Any]] = []
    for required in contract.get("required_native_records", ()) or ():
        matches = [record for record in records if packet_record_matches(record, required)]
        min_count = int(required.get("min", 1) or 1)
        passed = len(matches) >= min_count
        required_record_checks.append(
            {
                "name": required.get("name", "native_record"),
                "required": required,
                "matches": len(matches),
                "min": min_count,
                "passed": passed,
            }
        )
        if not passed:
            errors.append(
                f"required native-rep record {required.get('name', 'native_record')!r} "
                f"matched {len(matches)} records, expected at least {min_count}"
            )

    return {
        "status": "fail" if errors else "pass",
        "expected_source": expected_source,
        "expected_kind": expected_kind,
        "manifest_source": manifest.get("source"),
        "manifest_workload_kind": manifest.get("workload_kind"),
        "native_rep_artifacts": artifact_count,
        "native_rep_records": len(records),
        "static_counter_checks": static_counter_checks,
        "required_native_records": required_record_checks,
        "errors": errors,
    }


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
        required_stdout_name = REQUIRED_PACKET_STDOUT_CHECKS.get(name)
        stdout_checks = []
        missing_stdout_checks = []
        failing_stdout_checks = []
        if required_stdout_name:
            stdout_checks = [
                check
                for check in structural.get("checks", []) or []
                if isinstance(check, dict) and check.get("name") == required_stdout_name
            ]
            failing_stdout_checks = [
                check for check in stdout_checks if check.get("status") != "pass"
            ]
            if not stdout_checks:
                missing_stdout_checks.append(required_stdout_name)
        packet_contract = packet_workload_contract(name, manifest, artifact_dir)
        workload_status = "pass"
        if row.get("status") != "pass" or structural.get("status") != "pass":
            workload_status = "fail"
        if missing_artifacts or failing_safety:
            workload_status = "fail"
        if missing_stdout_checks or failing_stdout_checks:
            workload_status = "fail"
        if packet_contract.get("status") == "fail":
            workload_status = "fail"
        workload_errors = list(row.get("errors") or []) + list(structural.get("errors") or [])
        if packet_contract.get("status") == "fail":
            workload_errors.extend(
                f"packet_contract: {error}" for error in packet_contract.get("errors", [])
            )
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
            "stdout_checks": stdout_checks,
            "missing_stdout_checks": missing_stdout_checks,
            "failing_stdout_checks": failing_stdout_checks,
            "packet_contract": packet_contract,
            "explain_lowering_accounting": explain_lowering_accounting(
                native_rep_records(manifest, artifact_dir)[0],
                manifest.get("runtime_counter_summary", {}),
            ),
            "runtime_counter_summary": manifest.get("runtime_counter_summary", {}),
            "benchmark": manifest.get("benchmark", {}),
            "errors": workload_errors,
        }
        if gate and workload_status != "pass":
            errors.append(
                f"compiler-output:{name}: {workload_status}; "
                f"{workloads[name]['errors'] or missing_artifacts or missing_stdout_checks or failing_stdout_checks}"
            )

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


def material_accounting_rows(fields: dict[str, dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for spec in MATERIAL_ACCOUNTING_CONTRACT:
        field = str(spec["field"])
        delta = fields.get(field, {})
        control = delta.get("control")
        typed = delta.get("typed")
        failures: list[str] = []
        thresholds: dict[str, Any] = {}

        if control is None or typed is None:
            failures.append("control and typed values are required")
        else:
            if "control_min" in spec:
                control_min = float(spec["control_min"])
                thresholds["control_min"] = control_min
                if control < control_min:
                    failures.append(
                        f"control baseline must be >= {control_min:g} (observed={control})"
                    )
            if "typed_max" in spec:
                typed_max = float(spec["typed_max"])
                thresholds["typed_max"] = typed_max
                if typed > typed_max:
                    failures.append(
                        f"typed value must be <= {typed_max:g} (observed={typed})"
                    )
            if "reduction_min" in spec:
                reduction_min = float(spec["reduction_min"])
                thresholds["reduction_min_pct"] = reduction_min
                if control <= 0:
                    failures.append(
                        f"positive control baseline required for reduction (control={control})"
                    )
                else:
                    raw_reduction = ((control - typed) / control) * 100.0
                    if raw_reduction < reduction_min:
                        failures.append(
                            f"reduction must be >= {reduction_min:g}% "
                            f"(observed={raw_reduction:.1f}%)"
                        )
            if "speedup_min" in spec:
                speedup_min = float(spec["speedup_min"])
                thresholds["speedup_min"] = speedup_min
                if control <= 0 or typed <= 0:
                    failures.append(
                        "positive timing values required for speedup "
                        f"(control={control}, typed={typed})"
                    )
                else:
                    raw_speedup = control / typed
                    if raw_speedup < speedup_min:
                        failures.append(
                            f"speedup must be >= {speedup_min:g}x "
                            f"(observed={raw_speedup:.3f}x)"
                        )

        rows.append(
            {
                "field": field,
                "category": spec["category"],
                "source": spec["source"],
                "proves": spec["proves"],
                "status": "fail" if failures else "pass",
                "thresholds": thresholds,
                "control": control,
                "typed": typed,
                "delta": delta.get("delta"),
                "delta_pct": delta.get("delta_pct"),
                "reduction_pct": delta.get("reduction_pct"),
                "speedup": delta.get("speedup"),
                "failures": failures,
            }
        )
    return rows


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
    fields["p95_wall_ms"] = ratio_delta(
        number_value(nested(control, "benchmark", "p95_wall_ms")),
        number_value(nested(typed, "benchmark", "p95_wall_ms")),
    )
    missing = [name for name, delta in fields.items() if delta["typed"] is None or delta["control"] is None]
    if gate and missing:
        errors.append(f"benchmark deltas missing values: {missing}")

    material_failures: list[str] = []
    material_passes: list[str] = []
    benchmark_stat_quality = {
        "typed": nested(typed, "benchmark", "stat_quality"),
        "control": nested(control, "benchmark", "stat_quality"),
    }
    for role, quality in benchmark_stat_quality.items():
        if quality != MATERIAL_REQUIRED_STAT_QUALITY:
            material_failures.append(
                f"{role} benchmark stat_quality must be {MATERIAL_REQUIRED_STAT_QUALITY!r} "
                f"to prove p95 speedup (observed={quality!r})"
            )

    for field, minimum in MATERIAL_REDUCTION_THRESHOLDS.items():
        delta = fields.get(field, {})
        control_value = delta.get("control")
        typed_value = delta.get("typed")
        reduction_pct = delta.get("reduction_pct")
        if control_value is None or typed_value is None:
            continue
        if control_value <= 0:
            material_failures.append(
                f"{field}: control baseline must be positive to prove >={minimum:.0f}% reduction "
                f"(control={control_value}, typed={typed_value})"
            )
            continue
        raw_reduction_pct = ((control_value - typed_value) / control_value) * 100.0
        if raw_reduction_pct < minimum:
            material_failures.append(
                f"{field}: reduction must be >={minimum:.0f}% "
                f"(control={control_value}, typed={typed_value}, reduction_pct={reduction_pct})"
            )
        else:
            material_passes.append(field)

    for field in MATERIAL_ELIMINATION_FIELDS:
        delta = fields.get(field, {})
        control_value = delta.get("control")
        typed_value = delta.get("typed")
        if control_value is None or typed_value is None:
            continue
        if control_value <= 0:
            material_failures.append(
                f"{field}: control baseline must be positive to prove elimination "
                f"(control={control_value}, typed={typed_value})"
            )
        elif typed_value != 0:
            material_failures.append(
                f"{field}: typed value must be 0 for 100% elimination "
                f"(control={control_value}, typed={typed_value})"
            )
        else:
            material_passes.append(field)

    for field, minimum in MATERIAL_SPEEDUP_THRESHOLDS.items():
        delta = fields.get(field, {})
        control_value = delta.get("control")
        typed_value = delta.get("typed")
        speedup = delta.get("speedup")
        if control_value is None or typed_value is None:
            continue
        if control_value <= 0 or typed_value <= 0:
            material_failures.append(
                f"{field}: positive wall-time values are required to prove >={minimum:g}x speedup "
                f"(control={control_value}, typed={typed_value})"
            )
            continue
        raw_speedup = control_value / typed_value
        if raw_speedup < minimum:
            material_failures.append(
                f"{field}: speedup must be >={minimum:g}x "
                f"(control={control_value}, typed={typed_value}, speedup={speedup})"
            )
        else:
            material_passes.append(field)

    non_improving = []
    zero_baseline_required_fields = []
    positive_required_improvements = []
    for field in REQUIRED_IMPROVEMENT_FIELDS:
        delta = fields.get(field, {})
        control_value = delta.get("control")
        typed_value = delta.get("typed")
        if control_value is None or typed_value is None:
            continue
        if control_value > 0:
            if typed_value < control_value:
                positive_required_improvements.append(field)
                continue
            non_improving.append(
                f"{field}: typed must be lower than control "
                f"(control={control_value}, typed={typed_value})"
            )
        elif typed_value > control_value:
            non_improving.append(
                f"{field}: typed must not exceed zero-baseline control "
                f"(control={control_value}, typed={typed_value})"
            )
        else:
            zero_baseline_required_fields.append(field)
    if gate and not positive_required_improvements:
        non_improving.append(
            "at least one required improvement field must have a positive control "
            "baseline and a lower typed value"
        )
    material_accounting = material_accounting_rows(fields)
    accounting_failures = [
        f"{row['field']}: {failure}"
        for row in material_accounting
        for failure in row.get("failures", [])
    ]
    material_failures.extend(accounting_failures)
    if gate and non_improving:
        errors.append(f"benchmark deltas missing required improvements: {non_improving}")
    if gate and material_failures:
        errors.append(f"benchmark deltas miss material performance gate: {material_failures}")
    return {
        "status": "pass" if not missing and not non_improving and not material_failures else "fail",
        "typed_workload": "native_abi_packet_typed",
        "control_workload": "native_abi_packet_control",
        "required_improvement_fields": list(REQUIRED_IMPROVEMENT_FIELDS),
        "material_contracts": MATERIAL_CONTRACTS,
        "material_reduction_thresholds": MATERIAL_REDUCTION_THRESHOLDS,
        "material_elimination_fields": list(MATERIAL_ELIMINATION_FIELDS),
        "material_speedup_thresholds": MATERIAL_SPEEDUP_THRESHOLDS,
        "material_required_stat_quality": MATERIAL_REQUIRED_STAT_QUALITY,
        "benchmark_stat_quality": benchmark_stat_quality,
        "material_passes": material_passes,
        "material_failures": material_failures,
        "positive_required_improvements": positive_required_improvements,
        "zero_baseline_required_fields": zero_baseline_required_fields,
        "missing_values": missing,
        "non_improving_required_fields": non_improving,
        "material_accounting": material_accounting,
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


def release_symbol_summary(
    root: Path,
    metadata: dict[str, Any],
    errors: list[str],
    *,
    gate: bool,
) -> dict[str, Any]:
    command = command_entry(metadata, "release", "runtime_symbols")
    status = command_status(metadata, "release", "runtime_symbols")
    log_path = resolve_path(command.get("log"), root)
    log = read_text(log_path) if log_path else ""
    missing_tokens = [token for token in REQUIRED_RELEASE_SYMBOL_TOKENS if token not in log]
    archive = metadata.get("runtime_archive", "") if isinstance(metadata, dict) else ""
    fingerprints = {
        key: metadata.get(key, "") if isinstance(metadata, dict) else ""
        for key in REQUIRED_RELEASE_FINGERPRINT_FIELDS
    }
    missing_fingerprints = [key for key, value in fingerprints.items() if not value]
    sentinel_counts = release_sentinel_counts(log)
    stale_symbol_count = [
        count for count in sentinel_counts if count < REQUIRED_RELEASE_SENTINEL_COUNT
    ]
    passed = (
        status == "pass"
        and bool(log)
        and not missing_tokens
        and bool(sentinel_counts)
        and not stale_symbol_count
        and not missing_fingerprints
    )
    if gate and status != "pass":
        errors.append(f"release:runtime_symbols: command status is {status}")
    if gate and not log:
        errors.append("release:runtime_symbols: symbol guard log is missing")
    if gate and missing_tokens:
        errors.append(
            "release:runtime_symbols: expected proof tokens missing from log: "
            f"{missing_tokens}"
        )
    if gate and not sentinel_counts:
        errors.append("release:runtime_symbols: sentinel count proof is missing from log")
    if gate and stale_symbol_count:
        errors.append(
            "release:runtime_symbols: sentinel count is below current guard set "
            f"(required={REQUIRED_RELEASE_SENTINEL_COUNT}, observed={sentinel_counts})"
        )
    if gate and missing_fingerprints:
        errors.append(
            "release:runtime_symbols: archive/source freshness fingerprints missing: "
            f"{missing_fingerprints}"
        )
    return {
        "status": "pass" if passed else "fail",
        "command": command,
        "runtime_archive": archive,
        "log": str(log_path) if log_path else "",
        "required_tokens": list(REQUIRED_RELEASE_SYMBOL_TOKENS),
        "missing_tokens": missing_tokens,
        "required_sentinel_count": REQUIRED_RELEASE_SENTINEL_COUNT,
        "sentinel_counts": sentinel_counts,
        "stale_symbol_counts": stale_symbol_count,
        "fingerprints": fingerprints,
        "missing_fingerprints": missing_fingerprints,
    }


def all_rows_pass(rows: Any) -> bool:
    if not isinstance(rows, dict) or not rows:
        return False
    return all(isinstance(row, dict) and row.get("status") == "pass" for row in rows.values())


def compiler_matrix_status(compiler: dict[str, Any]) -> str:
    workloads = compiler.get("workloads", {})
    if compiler.get("status") != "pass" or not isinstance(workloads, dict) or not workloads:
        return "fail"
    return "pass" if all(row.get("status") == "pass" for row in workloads.values()) else "fail"


def gate_matrix_summary(
    correctness: dict[str, Any],
    compiler: dict[str, Any],
    runtime: dict[str, Any],
    release_symbols: dict[str, Any],
    deltas: dict[str, Any],
) -> list[dict[str, Any]]:
    status_by_area = {
        "native_abi_correctness": "pass" if all_rows_pass(correctness) else "fail",
        "native_region_artifacts": compiler_matrix_status(compiler),
        "explain_lowering_accounting": "pass" if deltas.get("status") == "pass" else "fail",
        "runtime_safety": "pass" if runtime.get("status") == "pass" else "fail",
        "release_symbols": "pass" if release_symbols.get("status") == "pass" else "fail",
    }
    return [
        {
            **row,
            "status": status_by_area.get(str(row["area"]), "fail"),
        }
        for row in GATE_MATRIX_SPEC
    ]


def build_packet(root: Path, metadata_path: Path, repo_root: Path, *, gate: bool) -> dict[str, Any]:
    metadata = load_json(metadata_path, {})
    errors: list[str] = []
    warnings: list[str] = []

    correctness = correctness_summary(root, metadata, errors, gate=gate)
    compiler = compiler_output_summary(root, metadata, errors, warnings, gate=gate)
    runtime = runtime_safety_summary(root, metadata, errors, gate=gate)
    release_symbols = release_symbol_summary(root, metadata, errors, gate=gate)
    deltas = benchmark_deltas(compiler, errors, gate=gate)
    gate_matrix = gate_matrix_summary(correctness, compiler, runtime, release_symbols, deltas)

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
        "scope": SCOPE,
        "gate_matrix": gate_matrix,
        "correctness": correctness,
        "native_call_lowering": compiler,
        "gc_root_safety": runtime,
        "release_symbol_guard": release_symbols,
        "benchmark_deltas": deltas,
    }
    return packet


def markdown_for_packet(packet: dict[str, Any], repo_root: Path) -> str:
    status = str(packet.get("status", "missing")).upper()
    lines = [
        f"# Selected Native / Region-Local Evidence Packet: {status}",
        "",
        f"- Generated: `{packet.get('generated_at', '')}`",
        f"- Root: `{packet.get('root', '')}`",
        f"- Gate: `{packet.get('gate')}`",
    ]
    scope = packet.get("scope", {})
    if isinstance(scope, dict):
        lines.append("")
        lines.append("## Scope")
        lines.append(f"- {scope.get('summary', SCOPE['summary'])}")
        lines.append(f"- {scope.get('not_covered', SCOPE['not_covered'])}")
    if packet.get("errors"):
        lines.append("")
        lines.append("## Gate Failures")
        lines.extend(f"- {error}" for error in packet["errors"])

    lines.append("")
    lines.append("## Gate Matrix")
    lines.append("| Area | Status | Gate | Evidence |")
    lines.append("|---|---:|---|---|")
    for row in packet.get("gate_matrix", []):
        lines.append(
            f"| {row.get('label', row.get('area', ''))} | `{row.get('status', 'missing')}` | "
            f"{row.get('gate', '')} | {row.get('evidence', '')} |"
        )

    lines.append("")
    lines.append("## Correctness Fixtures")
    for name, row in packet.get("correctness", {}).items():
        lines.append(
            f"- `{name}`: `{row.get('status')}`; native-reps={row.get('native_reps_artifact_count')}; "
            f"dir=`{row.get('evidence_dir')}`"
        )

    lines.append("")
    lines.append("## Selected Native / Region-Local Lowering")
    lowering = packet.get("native_call_lowering", {})
    lines.append(f"- Suite: `{lowering.get('status', 'missing')}` report=`{lowering.get('suite_report', '')}`")
    for name, row in lowering.get("workloads", {}).items():
        contract = row.get("packet_contract", {})
        explain = row.get("explain_lowering_accounting", {})
        lines.append(
            f"- `{name}`: `{row.get('status')}`; missing_artifacts={len(row.get('missing_artifacts', []))}; "
            f"safety_failures={len(row.get('failing_safety_checks', []))}; "
            f"stdout_missing={len(row.get('missing_stdout_checks', []))}; "
            f"stdout_failures={len(row.get('failing_stdout_checks', []))}; "
            f"packet_contract=`{contract.get('status', 'skipped')}`; "
            f"explain_records={explain.get('record_count', 0)}; "
            f"boxes={explain.get('boxes_inserted', 0)}; "
            f"dynamic_fallbacks={explain.get('dynamic_fallbacks', 0)}"
        )

    lines.append("")
    lines.append("## GC / Root Safety")
    safety = packet.get("gc_root_safety", {})
    lines.append(
        f"- Native async runtime tests: `{safety.get('status', 'missing')}`; "
        f"observed={len(safety.get('observed_tests', []))}/{len(safety.get('required_tests', []))}"
    )

    lines.append("")
    lines.append("## Release / LTO Symbol Guard")
    symbols = packet.get("release_symbol_guard", {})
    fingerprints = symbols.get("fingerprints", {})
    lines.append(
        f"- Runtime symbol guard: `{symbols.get('status', 'missing')}`; "
        f"archive=`{symbols.get('runtime_archive', '')}`; "
        f"sentinels={symbols.get('sentinel_counts', [])}/{symbols.get('required_sentinel_count', '')}; "
        f"missing_tokens={symbols.get('missing_tokens', [])}; "
        f"archive_sha256=`{fingerprints.get('runtime_archive_sha256', '')}`; "
        f"source_digest=`{fingerprints.get('runtime_source_digest', '')}`"
    )

    lines.append("")
    lines.append("## Packet Deltas")
    deltas = packet.get("benchmark_deltas", {})
    material_status = "pass" if deltas.get("status") == "pass" else "fail"
    lines.append(f"- Material gate: `{material_status}`")
    lines.append(
        f"- Timing quality: typed=`{deltas.get('benchmark_stat_quality', {}).get('typed')}` "
        f"control=`{deltas.get('benchmark_stat_quality', {}).get('control')}` "
        f"required=`{deltas.get('material_required_stat_quality')}`"
    )
    contracts = deltas.get("material_contracts", {})
    if contracts:
        lines.append(
            f"- Contract: reductions={contracts.get('reductions', {})}; "
            f"eliminations={contracts.get('eliminations', {})}; "
            f"speedups={contracts.get('speedups', {})}; "
            f"stat_quality=`{contracts.get('stat_quality', '')}`"
        )
    if deltas.get("material_failures"):
        lines.extend(f"  - {failure}" for failure in deltas.get("material_failures", []))
    if deltas.get("missing_values"):
        lines.append(f"  - missing_values={deltas.get('missing_values')}")

    lines.append("")
    lines.append("## Material Accounting")
    lines.append("| Field | Category | Status | Control | Typed | Reduction | Speedup | Thresholds |")
    lines.append("|---|---|---:|---:|---:|---:|---:|---|")
    for row in deltas.get("material_accounting", []):
        thresholds = ", ".join(f"{key}={value}" for key, value in row.get("thresholds", {}).items())
        lines.append(
            f"| `{row.get('field')}` | {row.get('category')} | `{row.get('status')}` | "
            f"{row.get('control')} | {row.get('typed')} | {row.get('reduction_pct')} | "
            f"{row.get('speedup')} | {thresholds} |"
        )

    lines.append("")
    lines.append("## Raw Deltas")
    for field, delta in deltas.get("fields", {}).items():
        lines.append(
            f"- `{field}`: control={delta.get('control')} typed={delta.get('typed')} "
            f"delta={delta.get('delta')} delta_pct={delta.get('delta_pct')} "
            f"reduction_pct={delta.get('reduction_pct')} speedup={delta.get('speedup')}"
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
