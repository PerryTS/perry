from __future__ import annotations

import json
import re
from typing import Any

from .analyzers import (
    hot_loop_blocks,
    named_hot_regions,
    runtime_call_names,
)
from .common import (
    DYNAMIC_PROPERTY_HELPERS,
    SCHEMA_VERSION,
)
from .spec import WORKLOADS


def target_supports_fma(target: str, clang_args: list[str]) -> bool:
    normalized_target = target.lower()
    normalized_args = " ".join(clang_args).lower()
    if normalized_target.startswith(("aarch64", "arm64")):
        return True
    if not normalized_target.startswith(("x86_64", "amd64", "i386", "i686")):
        return False
    return any(
        marker in normalized_args
        for marker in (
            "+fma",
            "-mfma",
            "haswell",
            "broadwell",
            "skylake",
            "cannonlake",
            "icelake",
            "tigerlake",
            "alderlake",
            "raptorlake",
            "sapphirerapids",
            "x86-64-v3",
            "x86-64-v4",
            "znver",
            "native",
        )
    )


def should_expect_fma(
    *,
    workload: str | None = None,
    fp_contract_mode: str,
    target: str,
    clang_args: list[str],
    expect_fma: str,
    gate_enabled: bool = True,
) -> bool:
    del workload
    if not gate_enabled:
        return False
    if expect_fma == "on":
        return True
    if expect_fma == "off":
        return False
    return fp_contract_mode in {"on", "fast"} and target_supports_fma(target, clang_args)


def vectorization_expectation(
    workload: str,
    vectorization: dict[str, Any],
    workloads: dict[str, Any] = WORKLOADS,
) -> dict[str, Any]:
    expectation = workloads.get(workload, {}).get("vectorization", {})
    min_vectorized = int(expectation.get("min_vectorized_loops", 0))
    allowed = set(expectation.get("allowed_missed_reason_kinds", []))
    observed = set((vectorization.get("missed_reason_kinds") or {}).keys())
    unexpected = sorted(observed - allowed)
    passed = int(vectorization.get("vectorized_count", 0) or 0) >= min_vectorized
    passed = passed and not unexpected
    return {
        "passed": passed,
        "min_vectorized_loops": min_vectorized,
        "vectorized_count": int(vectorization.get("vectorized_count", 0) or 0),
        "scalar_baseline": expectation.get("scalar_baseline", ""),
        "allowed_missed_reason_kinds": sorted(allowed),
        "observed_missed_reason_kinds": vectorization.get("missed_reason_kinds") or {},
        "unexpected_missed_reason_kinds": unexpected,
    }


def runtime_budget_results(
    workload: str,
    runtime_summary: dict[str, Any] | None,
    workloads: dict[str, Any] = WORKLOADS,
) -> list[dict[str, Any]]:
    if runtime_summary is None:
        return []
    budgets = workloads.get(workload, {}).get("runtime_budgets", {})
    results = []
    for field, maximum in sorted(budgets.items()):
        actual = int(runtime_summary.get(field, 0) or 0)
        results.append(
            {
                "field": field,
                "actual": actual,
                "maximum": int(maximum),
                "passed": actual <= int(maximum),
            }
        )
    return results


def _counter_passes(region: dict[str, Any], rule: dict[str, Any]) -> bool:
    for key, minimum in (rule.get("min") or {}).items():
        if int(region.get(key, 0) or 0) < int(minimum):
            return False
    for key, expected in (rule.get("equals") or {}).items():
        if int(region.get(key, 0) or 0) != int(expected):
            return False
    for key, maximum in (rule.get("max") or {}).items():
        if int(region.get(key, 0) or 0) > int(maximum):
            return False
    return True


def named_region_contract_results(
    workload: str,
    named_regions: dict[str, Any],
    workloads: dict[str, Any] = WORKLOADS,
) -> list[dict[str, Any]]:
    workload_info = workloads.get(workload, {})
    results: list[dict[str, Any]] = []

    def region(name: str) -> dict[str, Any]:
        return named_regions.get(name, {})

    def add(name: str, passed: bool, detail: str) -> None:
        results.append({"name": name, "passed": passed, "detail": detail})

    for region_spec in workload_info.get("named_regions", []) or []:
        name = str(region_spec["name"])
        counters = region(name)
        if region_spec.get("required"):
            add(
                f"named_region_{name}_present",
                bool(counters.get("labels")),
                f"{name} labels={counters.get('labels', [])}",
            )
            if not counters.get("labels"):
                continue
        if region_spec.get("no_runtime_calls"):
            calls = counters.get("runtime_calls", {})
            add(
                f"named_region_{name}_no_runtime_calls",
                not calls,
                f"{name} runtime_calls={json.dumps(calls, sort_keys=True)}",
            )
        if region_spec.get("no_conversions"):
            conversions = {
                key: counters.get(key, 0)
                for key in ("fptosi", "sitofp", "inttoptr", "ptrtoint")
                if counters.get(key, 0)
            }
            add(
                f"named_region_{name}_no_pointer_or_fp_int_conversions",
                not conversions,
                f"{name} conversions={json.dumps(conversions, sort_keys=True)}",
            )
        for rule in region_spec.get("checks", []) or []:
            passed = _counter_passes(counters, rule)
            detail = rule.get("detail") or json.dumps(counters, sort_keys=True)
            add(str(rule["name"]), passed, f"{detail}: {json.dumps(counters, sort_keys=True)}")
    return results


def _text_check_passes(text: str, check: dict[str, Any]) -> bool:
    if "contains" in check and check["contains"] not in text:
        return False
    if "contains_all" in check and not all(part in text for part in check["contains_all"]):
        return False
    if "contains_any" in check and not any(part in text for part in check["contains_any"]):
        return False
    if "regex" in check and not re.search(check["regex"], text):
        return False
    if "regex_all" in check and not all(re.search(pattern, text) for pattern in check["regex_all"]):
        return False
    if "regex_none" in check and any(re.search(pattern, text) for pattern in check["regex_none"]):
        return False
    return True


def _counter_check_passes(counters: dict[str, Any], check: dict[str, Any]) -> tuple[bool, Any]:
    section = check.get("section", "llvm_after")
    counter_set = counters.get(section, {})
    value = counter_set.get(check["counter"], 0)
    if "equals" in check:
        return int(value or 0) == int(check["equals"]), value
    if "max" in check:
        return int(value or 0) <= int(check["max"]), value
    if "min" in check:
        return int(value or 0) >= int(check["min"]), value
    return bool(value), value


def _flatten_native_records(native_reps: list[dict[str, Any]] | None) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for artifact in native_reps or []:
        for record in artifact.get("records", []) or []:
            if isinstance(record, dict):
                records.append(record)
    return records


def _state_name(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and value:
        return next(iter(value.keys()))
    return ""


def _bounds_allows_inbounds(value: Any) -> bool:
    return _state_name(value) in {"proven", "guarded"}


def _alias_allows_noalias(value: Any) -> bool:
    return _state_name(value) in {"no_alias_proven", "no_alias_guarded"}


def _access_mode_name(value: Any) -> str:
    return _state_name(value)


def _is_unchecked_native_unknown_bounds(record: dict[str, Any]) -> bool:
    return (
        _access_mode_name(record.get("access_mode")) == "unchecked_native"
        and not _bounds_allows_inbounds(record.get("bounds_state"))
    )


def _is_dynamic_fallback(record: dict[str, Any]) -> bool:
    return _access_mode_name(record.get("access_mode")) == "dynamic_fallback"


def _records_for_region(
    records: list[dict[str, Any]], named_regions: dict[str, Any], region: str
) -> list[dict[str, Any]]:
    region_info = (named_regions.get(region, {}) or {})
    block_keys = {
        (entry.get("function") or "", entry.get("label") or "")
        for entry in region_info.get("block_keys", []) or []
        if isinstance(entry, dict)
    }
    if block_keys:
        return [
            record
            for record in records
            if (record.get("function") or "", record.get("block_label") or "")
            in block_keys
        ]
    labels = set(region_info.get("labels", []) or [])
    return [record for record in records if record.get("block_label") in labels]


def native_rep_contract_results(
    workload: str,
    records: list[dict[str, Any]],
    named_regions: dict[str, Any],
) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []

    def add(name: str, passed: bool, detail: str) -> None:
        results.append({"name": name, "passed": passed, "detail": detail})

    def expected_region_id(region: str) -> str | None:
        for region_spec in WORKLOADS.get(workload, {}).get("named_regions", []) or []:
            if region_spec.get("name") == region:
                value = region_spec.get("native_region_id")
                return str(value) if value else None
        return None

    def records_for_native_region(region: str) -> list[dict[str, Any]]:
        region_id = expected_region_id(region)
        if region_id:
            return [r for r in records if r.get("region_id") == region_id]
        return _records_for_region(records, named_regions, region)

    unsafe_inbounds = [
        r
        for r in records
        if r.get("emitted_inbounds") and not _bounds_allows_inbounds(r.get("bounds_state"))
    ]
    unsafe_noalias = [
        r
        for r in records
        if r.get("emitted_noalias") and not _alias_allows_noalias(r.get("alias_state"))
    ]
    unchecked_unknown_bounds = [
        r for r in records if _is_unchecked_native_unknown_bounds(r)
    ]
    if workload.startswith("h1_"):
        add("native_reps_artifact_present", bool(records), f"records={len(records)}")
        add(
            "native_reps_no_unsafe_inbounds_claims",
            not unsafe_inbounds,
            json.dumps(unsafe_inbounds[:5], sort_keys=True),
        )
        add(
            "native_reps_no_unsafe_noalias_claims",
            not unsafe_noalias,
            json.dumps(unsafe_noalias[:5], sort_keys=True),
        )
        add(
            "native_reps_no_unchecked_unknown_bounds",
            not unchecked_unknown_bounds,
            json.dumps(unchecked_unknown_bounds[:5], sort_keys=True),
        )

    if workload == "h1_native_rep_equivalence":
        for region in ("direct_bounded", "local_cast", "helper_index"):
            region_records = records_for_native_region(region)
            rep_names = {r.get("native_rep_name") for r in region_records}
            consumers = " ".join(str(r.get("consumer", "")) for r in region_records)
            materializations = [r for r in region_records if r.get("materialization_reason")]
            add(
                f"native_reps_{region}_has_i32_index",
                "i32" in rep_names,
                f"{region} reps={sorted(rep_names)}",
            )
            add(
                f"native_reps_{region}_has_buffer_view",
                "buffer_view" in rep_names,
                f"{region} reps={sorted(rep_names)}",
            )
            add(
                f"native_reps_{region}_has_u8_conversion",
                "u8_load_zext_i32" in consumers and "u8_store_trunc_i32" in consumers,
                f"{region} consumers={consumers}",
            )
            add(
                f"native_reps_{region}_no_materialization",
                not materializations,
                f"{region} materializations={json.dumps(materializations[:5], sort_keys=True)}",
            )
            bounded = [
                r
                for r in region_records
                if r.get("native_rep_name") in {"buffer_view", "u8"}
                and _bounds_allows_inbounds(r.get("bounds_state"))
            ]
            add(
                f"native_reps_{region}_bounds_proven_or_guarded",
                bool(bounded),
                f"{region} bounded_records={len(bounded)}",
            )

        same_region_records = records_for_native_region("same_buffer")
        if not same_region_records:
            same_region_records = [
                r
                for r in records
                if "incInPlace" in str(r.get("function", ""))
                and r.get("block_label") == "for.body.2"
            ]
        same_records = [
            r
            for r in same_region_records
            if r.get("native_rep_name") in {"buffer_view", "u8"}
            and _state_name(r.get("alias_state")) in {"unknown", "may_alias", ""}
            and not r.get("emitted_noalias")
        ]
        same_reps = {r.get("native_rep_name") for r in same_records}
        same_noalias = [r for r in same_region_records if r.get("emitted_noalias")]
        add(
            "native_reps_same_buffer_has_raw_buffer_view",
            "buffer_view" in same_reps and "u8" in same_reps,
            f"same_buffer reps={sorted(same_reps)}",
        )
        add(
            "native_reps_same_buffer_denies_noalias",
            not same_noalias,
            json.dumps(same_noalias[:5], sort_keys=True),
        )

    if workload == "h1_buffer_alias_negative":
        def records_in_function(fragment: str) -> list[dict[str, Any]]:
            return [
                r
                for r in records
                if fragment in str(r.get("function", ""))
            ]

        def denied_alias(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
            return [
                r
                for r in rows
                if r.get("native_rep_name") in {"buffer_view", "u8"}
                and _state_name(r.get("alias_state")) in {"unknown", "may_alias", ""}
                and not r.get("emitted_noalias")
            ]

        def denied_bounds(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
            return [
                r
                for r in rows
                if r.get("native_rep_name") in {"buffer_view", "u8", "i32", "js_value"}
                and _state_name(r.get("bounds_state")) in {"unknown", ""}
                and _is_dynamic_fallback(r)
            ]

        def fallback_access(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
            return [
                r
                for r in rows
                if r.get("native_rep_name") in {"buffer_view", "u8", "i32", "js_value"}
                and _is_dynamic_fallback(r)
            ]

        def unchecked_native_access(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
            return [
                r
                for r in rows
                if _access_mode_name(r.get("access_mode")) == "unchecked_native"
            ]

        def fallback_buffer_access(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
            return [
                r
                for r in rows
                if _is_dynamic_fallback(r)
                and (
                    str(r.get("expr_kind", "")).startswith(("BufferIndex", "Uint8Array"))
                    or "slow_path" in str(r.get("consumer", ""))
                )
            ]

        denied_noalias = [
            r
            for r in records
            if r.get("native_rep_name") in {"buffer_view", "u8"}
            and _state_name(r.get("alias_state")) in {"unknown", "may_alias", ""}
            and not r.get("emitted_noalias")
        ]
        denied_inbounds = [
            r
            for r in records
            if r.get("native_rep_name") in {"buffer_view", "u8", "i32", "js_value"}
            and _state_name(r.get("bounds_state")) in {"unknown", ""}
            and _is_dynamic_fallback(r)
        ]
        reasons = {
            _state_name(r.get("materialization_reason"))
            or str(r.get("materialization_reason") or "")
            for r in records
            if r.get("materialization_reason")
        }
        add(
            "native_reps_negative_denies_unsafe_noalias",
            bool(denied_noalias),
            f"denied_noalias_records={len(denied_noalias)}",
        )
        add(
            "native_reps_negative_denies_unsafe_inbounds",
            bool(denied_inbounds),
            f"denied_inbounds_records={len(denied_inbounds)}",
        )
        add(
            "native_reps_negative_reports_boundary_reason",
            bool(reasons),
            f"materialization_reasons={sorted(reasons)}",
        )
        alias_local_records = records_for_native_region("alias_local")
        reassignment_records = records_for_native_region("reassignment_region")
        unknown_call_escape_records = records_for_native_region("unknown_call_escape")
        length_mismatch_records = records_for_native_region("length_mismatch")
        mutated_for_records = records_for_native_region("mutated_for_index")
        mutated_while_records = records_for_native_region("mutated_while_index")
        stale_native_alias_records = records_for_native_region("stale_native_alias")
        stale_allocation_length_records = records_for_native_region("stale_allocation_length")
        array_buffer_view_records = records_for_native_region("array_buffer_views")
        length_mismatch_unchecked_unknown = [
            r for r in length_mismatch_records if _is_unchecked_native_unknown_bounds(r)
        ]
        length_mismatch_dynamic_fallback_accesses = fallback_buffer_access(length_mismatch_records)
        mutated_for_unchecked_native = unchecked_native_access(mutated_for_records)
        mutated_while_unchecked_native = unchecked_native_access(mutated_while_records)
        mutated_for_dynamic_fallback_accesses = fallback_buffer_access(mutated_for_records)
        mutated_while_dynamic_fallback_accesses = fallback_buffer_access(mutated_while_records)
        stale_native_alias_unsafe_native = [
            r
            for r in stale_native_alias_records
            if _access_mode_name(r.get("access_mode")) == "unchecked_native"
            or r.get("emitted_inbounds")
            or r.get("emitted_noalias")
        ]
        stale_allocation_length_unsafe_native = [
            r
            for r in stale_allocation_length_records
            if _access_mode_name(r.get("access_mode")) == "unchecked_native"
            or r.get("emitted_inbounds")
            or r.get("emitted_noalias")
        ]
        stale_native_alias_dynamic_fallback_accesses = fallback_buffer_access(
            stale_native_alias_records
        )
        stale_allocation_length_dynamic_fallback_accesses = fallback_buffer_access(
            stale_allocation_length_records
        )
        array_buffer_view_unsafe_noalias = [
            r
            for r in array_buffer_view_records
            if r.get("native_rep_name") in {"buffer_view", "u8"}
            and (
                r.get("emitted_noalias")
                or _state_name(r.get("alias_state"))
                in {"no_alias_proven", "no_alias_guarded"}
            )
        ]
        hazard_checks = {
            "alias_local": bool(denied_alias(alias_local_records))
            or bool(fallback_access(alias_local_records))
            or any(
                _state_name(r.get("materialization_reason")) == "unknown_alias"
                for r in alias_local_records
            ),
            "reassignment": bool(denied_bounds(reassignment_records))
            or any(
                _state_name(r.get("materialization_reason")) == "reassignment"
                for r in records
            ),
            "unknown_call_escape": bool(denied_alias(unknown_call_escape_records))
            or any(
                _state_name(r.get("materialization_reason")) == "unknown_call_escape"
                for r in records
            ),
            "closure_capture": any(
                _state_name(r.get("materialization_reason")) == "closure_capture"
                for r in records
            ),
            "shared_backing": bool(denied_bounds(records_in_function("sharedBacking")))
            or not unchecked_native_access(records_in_function("sharedBacking")),
            "length_mismatch": not length_mismatch_unchecked_unknown
            and bool(length_mismatch_dynamic_fallback_accesses),
            "mutated_for_index": not mutated_for_unchecked_native
            and bool(mutated_for_dynamic_fallback_accesses),
            "mutated_while_index": not mutated_while_unchecked_native
            and bool(mutated_while_dynamic_fallback_accesses),
            "stale_native_alias": not stale_native_alias_unsafe_native
            and bool(stale_native_alias_dynamic_fallback_accesses),
            "stale_allocation_length": not stale_allocation_length_unsafe_native
            and bool(stale_allocation_length_dynamic_fallback_accesses),
            "array_buffer_views": not array_buffer_view_unsafe_noalias
            and (
                bool(denied_alias(array_buffer_view_records))
                or bool(fallback_buffer_access(array_buffer_view_records))
            ),
        }
        for hazard, passed in hazard_checks.items():
            add(
                f"native_reps_negative_{hazard}_access_denied",
                passed,
                json.dumps(hazard_checks, sort_keys=True),
            )
        add(
            "native_reps_negative_length_mismatch_no_unchecked_unknown",
            not length_mismatch_unchecked_unknown,
            json.dumps(length_mismatch_unchecked_unknown[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_length_mismatch_has_dynamic_fallback",
            bool(length_mismatch_dynamic_fallback_accesses),
            json.dumps(length_mismatch_dynamic_fallback_accesses[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_mutated_for_index_no_unchecked_native",
            not mutated_for_unchecked_native,
            json.dumps(mutated_for_unchecked_native[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_mutated_for_index_has_dynamic_fallback",
            bool(mutated_for_dynamic_fallback_accesses),
            json.dumps(mutated_for_dynamic_fallback_accesses[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_mutated_while_index_no_unchecked_native",
            not mutated_while_unchecked_native,
            json.dumps(mutated_while_unchecked_native[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_mutated_while_index_has_dynamic_fallback",
            bool(mutated_while_dynamic_fallback_accesses),
            json.dumps(mutated_while_dynamic_fallback_accesses[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_stale_native_alias_no_unchecked_or_native_claims",
            not stale_native_alias_unsafe_native,
            json.dumps(stale_native_alias_unsafe_native[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_stale_native_alias_has_dynamic_fallback",
            bool(stale_native_alias_dynamic_fallback_accesses),
            json.dumps(stale_native_alias_dynamic_fallback_accesses[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_stale_allocation_length_no_unchecked_or_native_claims",
            not stale_allocation_length_unsafe_native,
            json.dumps(stale_allocation_length_unsafe_native[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_stale_allocation_length_has_dynamic_fallback",
            bool(stale_allocation_length_dynamic_fallback_accesses),
            json.dumps(stale_allocation_length_dynamic_fallback_accesses[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_array_buffer_views_denies_noalias",
            not array_buffer_view_unsafe_noalias,
            json.dumps(array_buffer_view_unsafe_noalias[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_array_buffer_views_has_safe_access",
            bool(denied_alias(array_buffer_view_records))
            or bool(fallback_buffer_access(array_buffer_view_records)),
            json.dumps(array_buffer_view_records[:5], sort_keys=True),
        )

    return results


def verify_artifacts(
    *,
    workload: str,
    ir_before: str,
    ir_after: str,
    assembly: str,
    benchmark: dict[str, Any] | None,
    vectorization: dict[str, Any],
    counters: dict[str, Any] | None = None,
    runtime_summary: dict[str, Any] | None = None,
    fp_contract_mode: str = "off",
    target: str = "",
    clang_args: list[str] | None = None,
    expect_fma: str = "auto",
    native_reps: list[dict[str, Any]] | None = None,
    workloads: dict[str, Any] = WORKLOADS,
) -> dict[str, Any]:
    checks: list[dict[str, Any]] = []
    clang_args = clang_args or []
    workload_info = workloads.get(workload, {})
    named_regions = named_hot_regions(workload_info, ir_after)
    counters = counters or {}
    native_records = _flatten_native_records(native_reps)

    def add(name: str, passed: bool, detail: str, severity: str = "error") -> None:
        checks.append(
            {
                "name": name,
                "status": "pass" if passed else "fail",
                "severity": severity,
                "detail": detail,
            }
        )

    for label, text in (
        ("llvm_before", ir_before),
        ("llvm_after_analysis", ir_after),
        ("object_disassembly", assembly),
    ):
        add(f"{label}_present", bool(text.strip()), f"{label} is non-empty")

    add(
        "no_dynamic_property_runtime",
        bool(workload_info.get("allow_dynamic_property_runtime"))
        or not any(helper in ir_after for helper in DYNAMIC_PROPERTY_HELPERS),
        "optimized IR has no dynamic property helper calls",
    )
    add(
        "no_boxed_number_allocations",
        "js_boxed_number_new" not in ir_after,
        "optimized IR has no boxed-number allocation helper",
    )

    allowed_hot_loop_runtime = set(workload_info.get("allowed_hot_loop_runtime_calls", []))
    loop_runtime: dict[str, list[str]] = {}
    unexpected_loop_runtime: dict[str, list[str]] = {}
    loop_fptosi: dict[str, int] = {}
    loop_sitofp: dict[str, int] = {}
    loop_counters: dict[str, dict[str, int]] = {}
    for label, body in hot_loop_blocks(ir_after):
        runtime = sorted(set(runtime_call_names(body)))
        if runtime:
            loop_runtime[label] = runtime
            unexpected = sorted(name for name in runtime if name not in allowed_hot_loop_runtime)
            if unexpected:
                unexpected_loop_runtime[label] = unexpected
        summary = {
            "fptosi": body.count(" fptosi "),
            "sitofp": body.count(" sitofp "),
            "inttoptr": body.count(" inttoptr "),
        }
        loop_counters[label] = summary
        if summary["fptosi"]:
            loop_fptosi[label] = summary["fptosi"]
        if summary["sitofp"]:
            loop_sitofp[label] = summary["sitofp"]

    add(
        "hot_loops_no_runtime_calls",
        not unexpected_loop_runtime,
        "hot loop runtime calls: "
        + json.dumps(loop_runtime, sort_keys=True)
        + "; allowed="
        + json.dumps(sorted(allowed_hot_loop_runtime)),
    )
    add(
        "hot_loops_no_repeated_fptosi",
        bool(workload_info.get("allow_hot_loop_conversions")) or not loop_fptosi,
        "hot loop fptosi counts: " + json.dumps(loop_fptosi, sort_keys=True),
    )
    add(
        "hot_loops_no_sitofp",
        bool(workload_info.get("allow_hot_loop_conversions")) or not loop_sitofp,
        "hot loop sitofp counts: " + json.dumps(loop_sitofp, sort_keys=True),
    )

    for check in workload_info.get("hot_loop_checks", []) or []:
        observed = {
            label: values.get(check["counter"], 0)
            for label, values in loop_counters.items()
            if values.get(check["counter"], 0) != int(check.get("equals", 0))
        }
        if "equals" in check:
            passed = not observed
        else:
            passed = True
        add(check["name"], passed, f"{check.get('detail', check['name'])}: {observed}")

    for check in workload_info.get("ir_checks", []) or []:
        section = check.get("section", "llvm_after")
        text = ir_before if section == "llvm_before" else ir_after
        add(
            check["name"],
            _text_check_passes(text, check),
            check.get("detail", check["name"]),
        )

    for check in workload_info.get("assembly_checks", []) or []:
        add(
            check["name"],
            _text_check_passes(assembly, check),
            check.get("detail", check["name"]),
        )

    for check in workload_info.get("counter_checks", []) or []:
        passed, value = _counter_check_passes(counters, check)
        add(
            check["name"],
            passed,
            f"{check.get('detail', check['name'])}={value}",
        )

    fma_gate = workload_info.get("fma_gate") or {}
    if fma_gate.get("enabled"):
        fma_count = int(counters.get("assembly", {}).get("fma_instructions", 0) or 0)
        if not counters.get("assembly"):
            fma_count = len(re.findall(r"\b(vfmadd|vfnmadd|fmadd|fnmadd)\w*", assembly))
        fma_required = should_expect_fma(
            workload=workload,
            fp_contract_mode=fp_contract_mode,
            target=target,
            clang_args=clang_args,
            expect_fma=expect_fma,
            gate_enabled=True,
        )
        add(
            str(
                fma_gate.get(
                    "expected_check_name", "fma_instruction_when_contraction_expected"
                )
            ),
            (not fma_required) or fma_count > 0,
            "fma_required="
            + json.dumps(fma_required)
            + f", fma_instructions={fma_count}, target={target}, clang_args={clang_args}",
        )
        no_contract_on_fma_target = fp_contract_mode == "off" and target_supports_fma(
            target, clang_args
        )
        add(
            str(
                fma_gate.get(
                    "forbidden_check_name", "no_fma_instruction_when_fp_contract_off"
                )
            ),
            (not no_contract_on_fma_target) or fma_count == 0,
            "fp_contract_mode="
            + fp_contract_mode
            + f", fma_target={no_contract_on_fma_target}, fma_instructions={fma_count}",
        )

    if benchmark is not None:
        add(
            "benchmark_exit_zero",
            all(run.get("exit_code") == 0 for run in benchmark.get("runs", [])),
            "all benchmark runs exited zero",
        )

    for budget in runtime_budget_results(workload, runtime_summary, workloads):
        add(
            f"runtime_budget_{budget['field']}",
            bool(budget["passed"]),
            (
                f"{budget['field']} actual={budget['actual']} "
                f"maximum={budget['maximum']}"
            ),
        )

    for result in named_region_contract_results(workload, named_regions, workloads):
        add(result["name"], bool(result["passed"]), result["detail"])

    for result in native_rep_contract_results(workload, native_records, named_regions):
        add(result["name"], bool(result["passed"]), result["detail"])

    vector_expectation = vectorization_expectation(workload, vectorization, workloads)
    add(
        "vectorization_expectation",
        bool(vector_expectation["passed"]),
        json.dumps(vector_expectation, sort_keys=True),
    )

    errors = [c for c in checks if c["severity"] == "error" and c["status"] != "pass"]
    return {
        "schema_version": SCHEMA_VERSION,
        "status": "pass" if not errors else "fail",
        "checks": checks,
        "errors": [f"{c['name']}: {c['detail']}" for c in errors],
        "vectorization_expectation": vector_expectation,
        "runtime_budget_results": runtime_budget_results(workload, runtime_summary, workloads),
        "named_regions": named_regions,
        "named_region_contract_results": named_region_contract_results(
            workload, named_regions, workloads
        ),
        "native_rep_contract_results": native_rep_contract_results(
            workload, native_records, named_regions
        ),
    }
