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
    workloads: dict[str, Any] = WORKLOADS,
) -> dict[str, Any]:
    checks: list[dict[str, Any]] = []
    clang_args = clang_args or []
    workload_info = workloads.get(workload, {})
    named_regions = named_hot_regions(workload_info, ir_after)
    counters = counters or {}

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
        not any(helper in ir_after for helper in DYNAMIC_PROPERTY_HELPERS),
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
        not loop_fptosi,
        "hot loop fptosi counts: " + json.dumps(loop_fptosi, sort_keys=True),
    )
    add(
        "hot_loops_no_sitofp",
        not loop_sitofp,
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
        add(
            check["name"],
            _text_check_passes(ir_after, check),
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
    }
