from __future__ import annotations

import json
import re
from pathlib import Path
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


TRACE_RUNTIME_BUDGET_FIELDS = {
    "allocations_traced",
    "gc_collections_traced",
    "write_barriers_traced",
}


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
    trace_budget_fields = sorted(set(budgets).intersection(TRACE_RUNTIME_BUDGET_FIELDS))
    if trace_budget_fields and runtime_summary.get("gc_trace_enabled") is not True:
        results.append(
            {
                "field": "gc_trace_enabled",
                "actual": 0,
                "maximum": 1,
                "passed": False,
                "detail": (
                    "PERRY_GC_TRACE was disabled; trace-backed runtime budgets "
                    f"require GC trace data for {trace_budget_fields}"
                ),
            }
        )
    if trace_budget_fields and runtime_summary.get("gc_trace_unavailable") is True:
        results.append(
            {
                "field": "gc_trace_unavailable",
                "actual": 1,
                "maximum": 0,
                "passed": False,
                "detail": (
                    "PERRY_GC_TRACE was requested, but the linked runtime "
                    "reported diagnostics feature disabled"
                ),
            }
        )
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
    allowed_runtime_calls = set(workload_info.get("allowed_hot_loop_runtime_calls", []))
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
            region_allowed_runtime_calls = set(
                region_spec.get("allowed_runtime_calls", allowed_runtime_calls)
            )
            calls = counters.get("runtime_calls", {})
            unexpected_calls = {
                name: count
                for name, count in calls.items()
                if name not in region_allowed_runtime_calls
            }
            add(
                f"named_region_{name}_no_runtime_calls",
                not unexpected_calls,
                f"{name} runtime_calls={json.dumps(calls, sort_keys=True)}"
                + f"; allowed={json.dumps(sorted(region_allowed_runtime_calls))}",
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
    function_fragment = check.get("function_contains")
    if function_fragment:
        function_text = _function_text_containing(text, str(function_fragment))
        if not function_text:
            return False
        text = function_text
    if "equals" in check and text != str(check["equals"]):
        return False
    if "line_equals" in check and str(check["line_equals"]) not in text.splitlines():
        return False
    if "contains" in check and check["contains"] not in text:
        return False
    if "contains_all" in check and not all(part in text for part in check["contains_all"]):
        return False
    if "contains_any" in check and not any(part in text for part in check["contains_any"]):
        return False
    if "regex" in check and not re.search(check["regex"], text):
        return False
    if "regex_any" in check and not any(
        re.search(pattern, text) for pattern in check["regex_any"]
    ):
        return False
    if "regex_all" in check and not all(re.search(pattern, text) for pattern in check["regex_all"]):
        return False
    if "regex_none" in check and any(re.search(pattern, text) for pattern in check["regex_none"]):
        return False
    return True


def _benchmark_run_stdout(run: dict[str, Any]) -> str:
    stdout_path = run.get("stdout_path")
    if stdout_path:
        try:
            return Path(stdout_path).read_text(encoding="utf-8")
        except OSError:
            pass
    first = str(run.get("stdout_first") or "")
    last = str(run.get("stdout_last") or "")
    if last and last != first:
        return first + last
    return first


def _function_text_containing(text: str, fragment: str) -> str:
    matches: list[str] = []
    current: list[str] | None = None
    for line in text.splitlines():
        if line.startswith("define "):
            current = [line]
            continue
        if current is None:
            continue
        current.append(line)
        if line == "}":
            body = "\n".join(current)
            if fragment in body:
                matches.append(body)
            current = None
    return "\n\n".join(matches)


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


def _field_name(value: Any) -> str:
    return _state_name(value) or str(value or "")


def _is_unchecked_native_unknown_bounds(record: dict[str, Any]) -> bool:
    return (
        _access_mode_name(record.get("access_mode")) == "unchecked_native"
        and not _bounds_allows_inbounds(record.get("bounds_state"))
    )


def _is_checked_native_unknown_bounds(record: dict[str, Any]) -> bool:
    return (
        _access_mode_name(record.get("access_mode")) == "checked_native"
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


def _records_for_native_region(
    records: list[dict[str, Any]],
    named_regions: dict[str, Any],
    workload_info: dict[str, Any],
    region: str,
) -> list[dict[str, Any]]:
    region_id = None
    for region_spec in workload_info.get("named_regions", []) or []:
        if region_spec.get("name") == region:
            value = region_spec.get("native_region_id")
            region_id = str(value) if value else None
            break
    if region_id:
        return [r for r in records if r.get("region_id") == region_id]
    return _records_for_region(records, named_regions, region)


def _matches_state(actual: Any, expected: Any, *, state_kind: str) -> bool:
    if expected is None:
        return True
    actual_name = _state_name(actual)
    expected_name = str(expected)
    if expected_name in {"any", "*"}:
        return True
    if expected_name in {"none", ""}:
        return actual_name == ""
    if state_kind == "bounds" and expected_name == "proven_or_guarded":
        return _bounds_allows_inbounds(actual)
    if state_kind == "alias" and expected_name == "no_alias_proven_or_guarded":
        return _alias_allows_noalias(actual)
    return actual_name == expected_name


def _fact_matches(fact: Any, *, kind: Any = None, state: Any = None) -> bool:
    if not isinstance(fact, dict):
        return False
    if kind is not None and str(fact.get("kind") or "") != str(kind):
        return False
    if state is not None and str(fact.get("state") or "") != str(state):
        return False
    return True


def _fact_matches_spec(fact: Any, spec: dict[str, Any], prefix: str) -> bool:
    if not _fact_matches(
        fact,
        kind=spec.get(f"{prefix}_fact_kind"),
        state=spec.get(f"{prefix}_fact_state"),
    ):
        return False
    reason = spec.get(f"{prefix}_fact_reason")
    if reason is not None and _field_name(fact.get("reason")) != str(reason):
        return False
    fact_id_contains = spec.get(f"{prefix}_fact_id_contains")
    if fact_id_contains is not None and str(fact_id_contains) not in str(
        fact.get("fact_id") or ""
    ):
        return False
    return True


def _record_has_fact(
    record: dict[str, Any],
    field: str,
    *,
    kind: Any = None,
    state: Any = None,
) -> bool:
    facts = record.get(field) or []
    if not isinstance(facts, list):
        return False
    return any(_fact_matches(fact, kind=kind, state=state) for fact in facts)


def _record_matches_required(record: dict[str, Any], spec: dict[str, Any]) -> bool:
    exact_fields = (
        "expr_kind",
        "consumer",
        "native_rep_name",
        "region_id",
        "source_function",
        "block_label",
        "function",
        "materialization_reason",
        "fallback_reason",
        "native_value_state",
    )
    for field in exact_fields:
        if field in spec and _field_name(record.get(field)) != str(spec[field]):
            return False
    contains_fields = (
        ("consumer_contains", "consumer"),
        ("expr_kind_contains", "expr_kind"),
        ("function_contains", "function"),
        ("region_id_contains", "region_id"),
        ("notes_contains", "notes"),
    )
    for spec_field, record_field in contains_fields:
        if spec_field not in spec:
            continue
        needle = str(spec[spec_field])
        value = record.get(record_field)
        if isinstance(value, list):
            haystack = " ".join(str(item) for item in value)
        else:
            haystack = str(value or "")
        if needle not in haystack:
            return False
    if "access_mode" in spec and not _matches_state(
        record.get("access_mode"), spec["access_mode"], state_kind="access"
    ):
        return False
    if "bounds_state" in spec and not _matches_state(
        record.get("bounds_state"), spec["bounds_state"], state_kind="bounds"
    ):
        return False
    if "alias_state" in spec and not _matches_state(
        record.get("alias_state"), spec["alias_state"], state_kind="alias"
    ):
        return False
    if "consumed_fact_kind" in spec and not any(
        _fact_matches_spec(fact, spec, "consumed")
        for fact in record.get("consumed_facts", []) or []
    ):
        return False
    if (
        "consumed_fact_state" in spec
        and "consumed_fact_kind" not in spec
        and not any(
            _fact_matches_spec(fact, spec, "consumed")
            for fact in record.get("consumed_facts", []) or []
        )
    ):
        return False
    if "rejected_fact_kind" in spec and not any(
        _fact_matches_spec(fact, spec, "rejected")
        for fact in record.get("rejected_facts", []) or []
    ):
        return False
    if (
        "rejected_fact_state" in spec
        and "rejected_fact_kind" not in spec
        and not any(
            _fact_matches_spec(fact, spec, "rejected")
            for fact in record.get("rejected_facts", []) or []
        )
    ):
        return False
    return True


def generic_native_rep_contract_results(
    workload: str,
    records: list[dict[str, Any]],
    native_rep_artifact_count: int,
    workloads: dict[str, Any] = WORKLOADS,
    named_regions: dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    workload_info = workloads.get(workload, {})
    check_spec = workload_info.get("native_rep_checks") or {}
    if not check_spec:
        return []
    function_fragment = check_spec.get("function_contains")
    if function_fragment:
        needle = str(function_fragment)
        records = [
            r
            for r in records
            if needle in str(r.get("function") or "")
            or needle in str(r.get("source_function") or "")
        ]

    results: list[dict[str, Any]] = []

    def add(name: str, passed: bool, detail: str) -> None:
        results.append({"name": name, "passed": passed, "detail": detail})

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
    checked_unknown_bounds = [
        r for r in records if _is_checked_native_unknown_bounds(r)
    ]
    materialization_records = records
    materialization_regions = [
        str(region) for region in check_spec.get("materialization_regions", []) or []
    ]
    if materialization_regions:
        named_region_map = named_regions or {}
        materialization_records = []
        for region in materialization_regions:
            materialization_records.extend(
                _records_for_native_region(records, named_region_map, workload_info, region)
            )

    allowed_reasons = {str(r) for r in check_spec.get("allow_materialization_reasons", [])}
    unexpected_materializations = [
        r
        for r in materialization_records
        if r.get("materialization_reason")
        and _field_name(r.get("materialization_reason")) not in allowed_reasons
    ]
    dynamic_fallbacks = [r for r in records if _is_dynamic_fallback(r)]
    missing_fallback_reason = [
        r
        for r in dynamic_fallbacks
        if not _field_name(r.get("fallback_reason"))
        or not _field_name(r.get("materialization_reason"))
    ]
    mismatched_fallback_reason = [
        r
        for r in dynamic_fallbacks
        if _field_name(r.get("fallback_reason"))
        and _field_name(r.get("materialization_reason"))
        and _field_name(r.get("fallback_reason"))
        != _field_name(r.get("materialization_reason"))
    ]

    add(
        "native_reps_artifact_present",
        native_rep_artifact_count > 0,
        f"artifacts={native_rep_artifact_count}, records={len(records)}",
    )
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
    add(
        "native_reps_no_checked_unknown_bounds",
        not checked_unknown_bounds,
        json.dumps(checked_unknown_bounds[:5], sort_keys=True),
    )
    add(
        "native_reps_no_unexpected_materialization_reasons",
        not unexpected_materializations,
        "allowed="
        + json.dumps(sorted(allowed_reasons))
        + " scoped_regions="
        + json.dumps(materialization_regions)
        + " unexpected="
        + json.dumps(unexpected_materializations[:5], sort_keys=True),
    )
    add(
        "native_reps_dynamic_fallbacks_have_reasons",
        not missing_fallback_reason,
        json.dumps(missing_fallback_reason[:5], sort_keys=True),
    )
    add(
        "native_reps_dynamic_fallback_reasons_match_materialization",
        not mismatched_fallback_reason,
        json.dumps(mismatched_fallback_reason[:5], sort_keys=True),
    )

    for required in check_spec.get("require_records", []) or []:
        matches = [r for r in records if _record_matches_required(r, required)]
        min_count = int(required.get("min", 1) or 1)
        name = str(required.get("name") or required.get("consumer") or "record")
        add(
            f"native_reps_required_{name}",
            len(matches) >= min_count,
            f"required={json.dumps(required, sort_keys=True)} matches={len(matches)}",
        )

    return results


def native_rep_contract_results(
    workload: str,
    records: list[dict[str, Any]],
    named_regions: dict[str, Any],
    native_rep_artifact_count: int = 0,
    workloads: dict[str, Any] = WORKLOADS,
) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = generic_native_rep_contract_results(
        workload, records, native_rep_artifact_count, workloads, named_regions
    )

    def add(name: str, passed: bool, detail: str) -> None:
        results.append({"name": name, "passed": passed, "detail": detail})

    def expected_region_id(region: str) -> str | None:
        for region_spec in workloads.get(workload, {}).get("named_regions", []) or []:
            if region_spec.get("name") == region:
                value = region_spec.get("native_region_id")
                return str(value) if value else None
        return None

    def records_for_native_region(region: str) -> list[dict[str, Any]]:
        return _records_for_native_region(records, named_regions, workloads.get(workload, {}), region)

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
            consumed_rep_names = {
                r.get("native_rep_name")
                for r in region_records
                if r.get("native_rep_name") in {"i32", "buffer_view", "u8"}
                and _record_has_fact(
                    r, "consumed_facts", kind="representation", state="consumed"
                )
            }
            add(
                f"native_reps_{region}_consumes_representation_facts",
                {"i32", "buffer_view", "u8"}.issubset(consumed_rep_names),
                f"{region} consumed_rep_names={sorted(consumed_rep_names)}",
            )
            consumed_bounds = [
                r
                for r in region_records
                if r.get("native_rep_name") in {"buffer_view", "u8"}
                and _record_has_fact(
                    r, "consumed_facts", kind="bounds", state="consumed"
                )
            ]
            add(
                f"native_reps_{region}_consumes_bounds_facts",
                bool(consumed_bounds),
                f"{region} consumed_bounds_records={len(consumed_bounds)}",
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
        same_consumed_reps = {
            r.get("native_rep_name")
            for r in same_region_records
            if r.get("native_rep_name") in {"buffer_view", "u8"}
            and _record_has_fact(
                r, "consumed_facts", kind="representation", state="consumed"
            )
        }
        same_consumed_bounds = [
            r
            for r in same_region_records
            if r.get("native_rep_name") in {"buffer_view", "u8"}
            and _record_has_fact(r, "consumed_facts", kind="bounds", state="consumed")
        ]
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
        add(
            "native_reps_same_buffer_consumes_representation_facts",
            {"buffer_view", "u8"}.issubset(same_consumed_reps),
            f"same_buffer consumed_rep_names={sorted(same_consumed_reps)}",
        )
        add(
            "native_reps_same_buffer_consumes_bounds_facts",
            bool(same_consumed_bounds),
            f"same_buffer consumed_bounds_records={len(same_consumed_bounds)}",
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
        dynamic_fallback_records = [r for r in records if _is_dynamic_fallback(r)]
        dynamic_fallbacks_missing_reason = [
            r
            for r in dynamic_fallback_records
            if not _field_name(r.get("fallback_reason"))
            or not _field_name(r.get("materialization_reason"))
        ]
        dynamic_fallbacks_missing_rejection = [
            r
            for r in dynamic_fallback_records
            if not (
                _record_has_fact(r, "rejected_facts", kind="bounds", state="missing")
                or _record_has_fact(
                    r, "rejected_facts", kind="alias_noalias", state="missing"
                )
            )
        ]
        dynamic_fallbacks_missing_invalidation = [
            r
            for r in dynamic_fallback_records
            if not _record_has_fact(
                r,
                "rejected_facts",
                kind="materialization_hazard",
                state="invalidated",
            )
        ]
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
        add(
            "native_reps_negative_dynamic_fallbacks_have_reasons",
            not dynamic_fallbacks_missing_reason,
            json.dumps(dynamic_fallbacks_missing_reason[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_dynamic_fallbacks_reject_guards",
            not dynamic_fallbacks_missing_rejection,
            json.dumps(dynamic_fallbacks_missing_rejection[:5], sort_keys=True),
        )
        add(
            "native_reps_negative_dynamic_fallbacks_invalidate_hazards",
            not dynamic_fallbacks_missing_invalidation,
            json.dumps(dynamic_fallbacks_missing_invalidation[:5], sort_keys=True),
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
        bool(workload_info.get("allow_boxed_number_allocations"))
        or "js_boxed_number_new" not in ir_after,
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
        benchmark_runs = list(benchmark.get("runs", []) or [])
        add(
            "benchmark_exit_zero",
            bool(benchmark_runs)
            and all(run.get("exit_code") == 0 for run in benchmark_runs),
            "all benchmark runs exited zero",
        )
    else:
        benchmark_runs = []

    stdout_checks = workload_info.get("stdout_checks", []) or []
    if stdout_checks and not benchmark_runs:
        for check in stdout_checks:
            add(
                check["name"],
                False,
                f"{check.get('detail', check['name'])}: no benchmark stdout captured",
            )
    if benchmark_runs:
        for check in stdout_checks:
            failed_runs = [
                int(run.get("run", index))
                for index, run in enumerate(benchmark_runs, start=1)
                if not _text_check_passes(_benchmark_run_stdout(run), check)
            ]
            add(
                check["name"],
                not failed_runs,
                (
                    f"{check.get('detail', check['name'])}: "
                    f"checked_runs={len(benchmark_runs)} failed_runs={failed_runs}"
                ),
            )

    for budget in runtime_budget_results(workload, runtime_summary, workloads):
        detail = budget.get("detail") or (
            f"{budget['field']} actual={budget['actual']} "
            f"maximum={budget['maximum']}"
        )
        add(
            f"runtime_budget_{budget['field']}",
            bool(budget["passed"]),
            detail,
        )

    for result in named_region_contract_results(workload, named_regions, workloads):
        add(result["name"], bool(result["passed"]), result["detail"])

    for result in native_rep_contract_results(
        workload, native_records, named_regions, len(native_reps or []), workloads
    ):
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
            workload, native_records, named_regions, len(native_reps or []), workloads
        ),
    }
