#!/usr/bin/env python3
"""Capture and verify Perry compiler-output evidence for CPU benchmarks.

The harness keeps the full chain needed to debug a performance change:

    HIR -> LLVM IR before opt -> LLVM IR after opt -> assembly -> benchmark result

It is intentionally CI-friendly: artifacts live under target/ by default,
structural checks are emitted as JSON, and hardware counters are best-effort
when Linux perf is available.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import resource
import shutil
import statistics
import subprocess
import sys
import time
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_VERSION = 1

WORKLOADS = {
    "image_convolution": {
        "source": "benchmarks/honest_bench/workloads/3_image_convolution/perry/image_conv.ts",
        "kind": "image_convolution",
        "vectorization": {
            "min_vectorized_loops": 0,
            "scalar_baseline": "allowed: current image fixture still has scalar loops",
            "allowed_missed_reason_kinds": {
                "call_instruction",
                "generic_not_vectorized",
                "not_beneficial",
                "uncountable_loop",
                "unknown_trip_count",
                "unsupported_reduction",
            },
        },
        "runtime_budgets": {
            "allocations_traced": 0,
            "gc_collections_traced": 0,
            "write_barriers_static": 0,
            "write_barriers_traced": 0,
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
        },
    },
    "loop_data_dependent": {
        "source": "benchmarks/suite/17_loop_data_dependent.ts",
        "kind": "numeric_loop",
        "vectorization": {
            "min_vectorized_loops": 0,
            "scalar_baseline": "allowed: data-dependent control flow is intentionally scalar",
            "allowed_missed_reason_kinds": {
                "call_instruction",
                "control_flow",
                "generic_not_vectorized",
                "uncountable_loop",
                "unknown_trip_count",
                "unsupported_instruction",
                "unsupported_reduction",
            },
        },
        "runtime_budgets": {
            "allocations_traced": 0,
            "gc_collections_traced": 0,
            "write_barriers_static": 1,
            "write_barriers_traced": 0,
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
        },
    },
    "fma_contract": {
        "source": "benchmarks/compiler_output/fixtures/fma_contract.ts",
        "kind": "fma_contract",
        "vectorization": {
            "min_vectorized_loops": 0,
            "scalar_baseline": "allowed: this fixture exists to prove scalar FP contraction",
            "allowed_missed_reason_kinds": {
                "call_instruction",
                "control_flow",
                "generic_not_vectorized",
                "not_beneficial",
                "uncountable_loop",
                "unknown_trip_count",
                "unsupported_instruction",
                "unsupported_reduction",
            },
        },
        "runtime_budgets": {
            "allocations_traced": 0,
            "gc_collections_traced": 0,
            "write_barriers_static": 0,
            "write_barriers_traced": 0,
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
        },
    },
    "vectorized_buffer_transform": {
        "source": "benchmarks/compiler_output/fixtures/vectorized_buffer_transform.ts",
        "kind": "vectorized_buffer_transform",
        "vectorization": {
            "min_vectorized_loops": 1,
            "scalar_baseline": "",
            "allowed_missed_reason_kinds": {
                "call_instruction",
                "generic_not_vectorized",
                "not_beneficial",
                "uncountable_loop",
                "unknown_trip_count",
                "unsupported_instruction",
                "unsupported_reduction",
            },
        },
        "runtime_budgets": {
            "allocations_traced": 0,
            "gc_collections_traced": 0,
            "write_barriers_static": 0,
            "write_barriers_traced": 0,
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
        },
    },
    "hir_fact_rewrite": {
        "source": "benchmarks/compiler_output/fixtures/hir_fact_rewrite.ts",
        "kind": "hir_fact_rewrite",
        "vectorization": {
            "min_vectorized_loops": 1,
            "scalar_baseline": "",
            "allowed_missed_reason_kinds": {
                "call_instruction",
                "generic_not_vectorized",
                "not_beneficial",
                "uncountable_loop",
                "unknown_trip_count",
                "unsupported_instruction",
                "unsupported_reduction",
            },
        },
        "runtime_budgets": {
            "allocations_traced": 0,
            "gc_collections_traced": 0,
            "write_barriers_static": 0,
            "write_barriers_traced": 0,
            "boxed_number_allocations_static": 0,
            "buffer_slow_path_accesses_static": 0,
        },
    },
}

DEFAULT_BENCHMARK_RUNS = {
    "smoke": 1,
    "standard": 5,
    "release": 15,
}

RUNTIME_CALL_PREFIXES = (
    "js_",
    "perry_runtime_",
)

DYNAMIC_PROPERTY_HELPERS = (
    "js_object_get",
    "js_object_set",
    "js_handle_object_get_property",
    "js_native_call_method",
    "js_get_property",
    "js_set_property",
    "js_dynamic",
)

BUFFER_SLOW_PATH_HELPERS = (
    "js_buffer_get",
    "js_buffer_set",
    "js_typed_array_get",
    "js_typed_array_set",
    "js_uint8array_get",
    "js_uint8array_set",
)


class HarnessError(RuntimeError):
    pass


@dataclass
class CommandResult:
    argv: list[str]
    cwd: str
    exit_code: int
    duration_ms: float
    stdout: str
    stderr: str
    stdout_path: str | None = None
    stderr_path: str | None = None

    def to_json(self) -> dict[str, Any]:
        return {
            "argv": self.argv,
            "cwd": self.cwd,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "stdout_path": self.stdout_path,
            "stderr_path": self.stderr_path,
        }


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def relpath(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def run_command(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    timeout: int,
    stdout_path: Path | None = None,
    stderr_path: Path | None = None,
    check: bool = True,
) -> CommandResult:
    start = time.perf_counter()
    proc = subprocess.run(
        argv,
        cwd=str(cwd),
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )
    duration_ms = (time.perf_counter() - start) * 1000.0
    if stdout_path is not None:
        write_text(stdout_path, proc.stdout)
    if stderr_path is not None:
        write_text(stderr_path, proc.stderr)
    result = CommandResult(
        argv=argv,
        cwd=str(cwd),
        exit_code=proc.returncode,
        duration_ms=duration_ms,
        stdout=proc.stdout,
        stderr=proc.stderr,
        stdout_path=str(stdout_path) if stdout_path is not None else None,
        stderr_path=str(stderr_path) if stderr_path is not None else None,
    )
    if check and proc.returncode != 0:
        raise HarnessError(
            f"command failed with exit {proc.returncode}: {' '.join(argv)}\n"
            f"stderr:\n{proc.stderr[-4000:]}"
        )
    return result


def resolve_perry(arg: str | None) -> list[str]:
    candidate = arg or os.environ.get("PERRY_BIN")
    if candidate:
        path = Path(candidate)
        if path.is_absolute():
            return [str(path)]
        if path.exists() or os.sep in candidate:
            return [str((REPO_ROOT / path).resolve())]
        return [candidate]
    for path in (REPO_ROOT / "target/release/perry", REPO_ROOT / "target/debug/perry"):
        if path.exists():
            return [str(path)]
    return ["cargo", "run", "--quiet", "-p", "perry", "--"]


def resolve_clang(arg: str | None) -> str:
    clang = arg or os.environ.get("CLANG") or shutil.which("clang")
    if not clang:
        raise HarnessError("clang is required to emit optimized IR and assembly")
    return clang


def compiler_version(argv: list[str]) -> str:
    try:
        proc = subprocess.run(
            argv + ["--version"],
            cwd=str(REPO_ROOT),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=20,
        )
        return proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else "unknown"
    except Exception as exc:  # pragma: no cover - defensive metadata only.
        return f"unavailable: {type(exc).__name__}: {exc}"


def parse_kept_paths(log_text: str) -> tuple[list[Path], list[Path]]:
    ir_paths: list[Path] = []
    object_paths: list[Path] = []
    for line in log_text.splitlines():
        ir_match = re.search(r"kept LLVM IR:\s*(\S+)", line)
        if ir_match:
            ir_paths.append(Path(ir_match.group(1)))
        obj_match = re.search(r"kept object:\s*(\S+)", line)
        if obj_match:
            object_paths.append(Path(obj_match.group(1)))
    return ir_paths, object_paths


def parse_target_triple(ir: str) -> str | None:
    match = re.search(r'^target triple = "([^"]+)"', ir, flags=re.MULTILINE)
    return match.group(1) if match else None


def extract_blocks(ir: str) -> list[tuple[str, str]]:
    blocks: list[tuple[str, str]] = []
    current_label: str | None = None
    current_lines: list[str] = []
    label_re = re.compile(r"^([A-Za-z0-9_.$-]+):(?:\s|$)")
    for line in ir.splitlines():
        match = label_re.match(line)
        if match:
            if current_label is not None:
                blocks.append((current_label, "\n".join(current_lines)))
            current_label = match.group(1)
            current_lines = [line]
        elif current_label is not None:
            current_lines.append(line)
    if current_label is not None:
        blocks.append((current_label, "\n".join(current_lines)))
    return blocks


def call_names(text: str) -> list[str]:
    names = []
    for match in re.finditer(r"\bcall\b[^\n;]*@([A-Za-z_.$][A-Za-z0-9_.$]*)", text):
        names.append(match.group(1))
    return names


def count_calls_by_name(text: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for name in call_names(text):
        counts[name] = counts.get(name, 0) + 1
    return dict(sorted(counts.items()))


def runtime_call_names(text: str) -> list[str]:
    return [name for name in call_names(text) if name.startswith(RUNTIME_CALL_PREFIXES)]


def hot_loop_blocks(ir: str) -> list[tuple[str, str]]:
    return [
        (label, body)
        for label, body in extract_blocks(ir)
        if label.startswith(("for.", "while.", "do."))
        and any(part in label for part in (".body", ".latch"))
        and "preheader" not in label
        and "exit" not in label
    ]


def classify_vectorization_reason(line: str) -> str:
    lower = line.lower()
    if "alias" in lower:
        return "aliasing"
    if "call instruction cannot be vectorized" in lower:
        return "call_instruction"
    if "control flow cannot" in lower or "if-conversion" in lower:
        return "control_flow"
    if "could not determine number of loop iterations" in lower:
        return "unknown_trip_count"
    if "unknown trip count" in lower:
        return "unknown_trip_count"
    if "uncountable loop" in lower:
        return "uncountable_loop"
    if "reduction" in lower:
        return "unsupported_reduction"
    if "instruction cannot be vectorized" in lower:
        return "unsupported_instruction"
    if "cost-model" in lower or "not beneficial" in lower:
        return "not_beneficial"
    if "loop not vectorized" in lower:
        return "generic_not_vectorized"
    return "other"


def parse_vectorization_remarks(stderr_text: str) -> dict[str, Any]:
    vectorized = []
    missed = []
    analysis = []
    missed_reason_counts: Counter[str] = Counter()
    missed_reasons = []
    for line in stderr_text.splitlines():
        lower = line.lower()
        if "loop-vectorize" not in lower:
            continue
        if "remark:" in lower and "vectorized loop" in lower:
            vectorized.append(line)
        elif "missed" in lower:
            missed.append(line)
            reason = classify_vectorization_reason(line)
            missed_reason_counts[reason] += 1
            if len(missed_reasons) < 20:
                missed_reasons.append({"kind": reason, "remark": line})
        else:
            analysis.append(line)
            reason = classify_vectorization_reason(line)
            if reason != "other":
                missed_reason_counts[reason] += 1
                if len(missed_reasons) < 20:
                    missed_reasons.append({"kind": reason, "remark": line})
    return {
        "vectorized_count": len(vectorized),
        "missed_count": len(missed),
        "analysis_count": len(analysis),
        "missed_reason_kinds": dict(sorted(missed_reason_counts.items())),
        "missed_reasons": missed_reasons,
        "vectorized": vectorized[:20],
        "missed": missed[:20],
        "analysis": analysis[:20],
    }


def structural_counters(ir_before: str, ir_after: str, assembly: str) -> dict[str, Any]:
    after_calls = count_calls_by_name(ir_after)
    runtime_calls = {
        name: count
        for name, count in after_calls.items()
        if name.startswith(RUNTIME_CALL_PREFIXES)
    }
    return {
        "llvm_before": {
            "line_count": len(ir_before.splitlines()),
            "fptosi": ir_before.count(" fptosi "),
            "sitofp": ir_before.count(" sitofp "),
            "inttoptr": ir_before.count(" inttoptr "),
            "ptrtoint": ir_before.count(" ptrtoint "),
            "runtime_calls": {
                name: count
                for name, count in count_calls_by_name(ir_before).items()
                if name.startswith(RUNTIME_CALL_PREFIXES)
            },
        },
        "llvm_after": {
            "line_count": len(ir_after.splitlines()),
            "getelementptr_inbounds": ir_after.count("getelementptr inbounds"),
            "llvm_assume": ir_after.count("@llvm.assume"),
            "invariant_load_metadata": ir_after.count("!invariant.load"),
            "alias_scope_metadata": ir_after.count("!alias.scope"),
            "noalias_metadata": ir_after.count("!noalias"),
            "fptosi": ir_after.count(" fptosi "),
            "sitofp": ir_after.count(" sitofp "),
            "inttoptr": ir_after.count(" inttoptr "),
            "ptrtoint": ir_after.count(" ptrtoint "),
            "runtime_calls": runtime_calls,
            "boxed_number_allocations": after_calls.get("js_boxed_number_new", 0),
            "write_barriers": after_calls.get("js_write_barrier", 0)
            + after_calls.get("js_write_barrier_slot", 0),
            "buffer_slow_path_calls": sum(
                count
                for name, count in after_calls.items()
                if any(helper in name for helper in BUFFER_SLOW_PATH_HELPERS)
            ),
            "dynamic_property_calls": sum(
                count
                for name, count in after_calls.items()
                if any(helper in name for helper in DYNAMIC_PROPERTY_HELPERS)
            ),
        },
        "assembly": {
            "line_count": len(assembly.splitlines()),
            "call_instructions": len(re.findall(r"\bcallq?\b|\bbl\b", assembly)),
            "fma_instructions": len(
                re.findall(r"\b(vfmadd|vfnmadd|fmadd|fnmadd)\w*", assembly)
            ),
            "simd_register_mentions": len(re.findall(r"\b([xyz]mm\d+|v\d+\.\d)", assembly)),
        },
    }


def block_counter_summary(body: str) -> dict[str, Any]:
    calls = count_calls_by_name(body)
    return {
        "runtime_calls": {
            name: count
            for name, count in calls.items()
            if name.startswith(RUNTIME_CALL_PREFIXES)
        },
        "fptosi": body.count(" fptosi "),
        "sitofp": body.count(" sitofp "),
        "inttoptr": body.count(" inttoptr "),
        "ptrtoint": body.count(" ptrtoint "),
        "load_i8": len(re.findall(r"\bload i8, ptr\b", body)),
        "store_i8": len(re.findall(r"\bstore i8\b", body)),
        "fmul": body.count(" fmul "),
        "fadd": body.count(" fadd "),
        "mul_i32": body.count(" mul i32 "),
        "xor_i32": body.count(" xor i32 "),
    }


def merge_region_counters(
    blocks: list[tuple[str, dict[str, Any]]],
) -> dict[str, Any]:
    merged: dict[str, Any] = {
        "labels": [label for label, _ in blocks],
        "runtime_calls": {},
        "fptosi": 0,
        "sitofp": 0,
        "inttoptr": 0,
        "ptrtoint": 0,
        "load_i8": 0,
        "store_i8": 0,
        "fmul": 0,
        "fadd": 0,
        "mul_i32": 0,
        "xor_i32": 0,
    }
    for _, counters in blocks:
        for name, count in counters.get("runtime_calls", {}).items():
            merged["runtime_calls"][name] = merged["runtime_calls"].get(name, 0) + count
        for key in (
            "fptosi",
            "sitofp",
            "inttoptr",
            "ptrtoint",
            "load_i8",
            "store_i8",
            "fmul",
            "fadd",
            "mul_i32",
            "xor_i32",
        ):
            merged[key] += int(counters.get(key, 0) or 0)
    merged["runtime_calls"] = dict(sorted(merged["runtime_calls"].items()))
    return merged


def hot_region_counters(ir_after: str) -> dict[str, Any]:
    regions: dict[str, Any] = {}
    for label, body in hot_loop_blocks(ir_after):
        regions[label] = block_counter_summary(body)
    return {"hot_loops": regions}


def named_hot_regions(workload: str, ir_after: str) -> dict[str, Any]:
    blocks = [
        (label, block_counter_summary(body))
        for label, body in extract_blocks(ir_after)
    ]
    selected: dict[str, list[tuple[str, dict[str, Any]]]] = {}

    def add(region: str, label: str, counters: dict[str, Any]) -> None:
        selected.setdefault(region, []).append((label, counters))

    for label, counters in blocks:
        if workload == "image_convolution":
            if label.startswith(("for.body.20", "for.body.24", "while.body.28")):
                add("input_generation", label, counters)
            elif counters["load_i8"] >= 3 and counters["store_i8"] >= 1:
                add("blur", label, counters)
            elif (
                label.startswith("for.body.42")
                or (
                    counters["load_i8"] >= 1
                    and counters["xor_i32"] >= 1
                    and counters["mul_i32"] >= 1
                    and counters["store_i8"] == 0
                )
            ):
                add("fnv_hash", label, counters)
        elif workload == "loop_data_dependent":
            if label.startswith("for.body.11") or label.startswith(
                ("arr.merge.16", "arr.merge.21")
            ):
                add("numeric_loop_body", label, counters)
        elif workload == "fma_contract":
            if label.startswith(("for.body", "vector.body")) and (
                counters["fmul"] > 0 or counters["fadd"] > 0
            ):
                add("numeric_loop_body", label, counters)

    return {
        name: merge_region_counters(region_blocks)
        for name, region_blocks in sorted(selected.items())
    }


def region_counters(workload: str, ir_after: str) -> dict[str, Any]:
    regions = hot_region_counters(ir_after)
    regions["named"] = named_hot_regions(workload, ir_after)
    return regions


def runtime_counter_summary(
    benchmark: dict[str, Any] | None, counters: dict[str, Any]
) -> dict[str, Any]:
    after = counters.get("llvm_after", {})
    runtime_calls = after.get("runtime_calls", {})
    gc_collections = 0
    traced_allocations = 0
    traced_write_barriers = 0
    if benchmark is not None:
        for row in benchmark.get("runs", []):
            trace = row.get("gc_trace_summary", {})
            gc_collections += int(trace.get("gc_events", 0) or 0)
            traced_allocations += int(trace.get("malloc_kind_allocations", 0) or 0)
            traced_write_barriers += int(trace.get("write_barrier_calls", 0) or 0)
    return {
        "runtime_calls_static": sum(int(v) for v in runtime_calls.values()),
        "runtime_call_names_static": runtime_calls,
        "allocations_traced": traced_allocations,
        "gc_collections_traced": gc_collections,
        "write_barriers_static": int(after.get("write_barriers", 0) or 0),
        "write_barriers_traced": traced_write_barriers,
        "boxed_number_allocations_static": int(
            after.get("boxed_number_allocations", 0) or 0
        ),
        "buffer_slow_path_accesses_static": int(
            after.get("buffer_slow_path_calls", 0) or 0
        ),
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
    workload: str,
    fp_contract_mode: str,
    target: str,
    clang_args: list[str],
    expect_fma: str,
) -> bool:
    if expect_fma == "on":
        return True
    if expect_fma == "off":
        return False
    return (
        workload == "fma_contract"
        and fp_contract_mode in {"on", "fast"}
        and target_supports_fma(target, clang_args)
    )


def vectorization_expectation(workload: str, vectorization: dict[str, Any]) -> dict[str, Any]:
    expectation = WORKLOADS.get(workload, {}).get("vectorization", {})
    min_vectorized = int(expectation.get("min_vectorized_loops", 0))
    allowed = set(expectation.get("allowed_missed_reason_kinds", set()))
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
    workload: str, runtime_summary: dict[str, Any] | None
) -> list[dict[str, Any]]:
    if runtime_summary is None:
        return []
    budgets = WORKLOADS.get(workload, {}).get("runtime_budgets", {})
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


def named_region_contract_results(
    workload: str, named_regions: dict[str, Any]
) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []

    def region(name: str) -> dict[str, Any]:
        return named_regions.get(name, {})

    def add(name: str, passed: bool, detail: str) -> None:
        results.append({"name": name, "passed": passed, "detail": detail})

    def require_present(name: str) -> bool:
        present = bool(region(name).get("labels"))
        add(
            f"named_region_{name}_present",
            present,
            f"{name} labels={region(name).get('labels', [])}",
        )
        return present

    def require_no_runtime(name: str) -> None:
        calls = region(name).get("runtime_calls", {})
        add(
            f"named_region_{name}_no_runtime_calls",
            not calls,
            f"{name} runtime_calls={json.dumps(calls, sort_keys=True)}",
        )

    def require_no_conversions(name: str) -> None:
        counters = region(name)
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

    if workload == "image_convolution":
        for name in ("input_generation", "blur", "fnv_hash"):
            if require_present(name):
                require_no_runtime(name)
                require_no_conversions(name)

        input_region = region("input_generation")
        if input_region:
            add(
                "named_region_input_generation_byte_writes",
                int(input_region.get("store_i8", 0) or 0) >= 4
                and int(input_region.get("xor_i32", 0) or 0) >= 1,
                "input_generation stores="
                f"{input_region.get('store_i8', 0)} xor_i32={input_region.get('xor_i32', 0)}",
            )

        blur = region("blur")
        if blur:
            add(
                "named_region_blur_byte_load_store",
                int(blur.get("load_i8", 0) or 0) >= 3
                and int(blur.get("store_i8", 0) or 0) >= 1,
                f"blur load_i8={blur.get('load_i8', 0)} store_i8={blur.get('store_i8', 0)}",
            )

        fnv = region("fnv_hash")
        if fnv:
            add(
                "named_region_fnv_i32_hash_shape",
                int(fnv.get("load_i8", 0) or 0) >= 1
                and int(fnv.get("xor_i32", 0) or 0) >= 1
                and int(fnv.get("mul_i32", 0) or 0) >= 1
                and int(fnv.get("store_i8", 0) or 0) == 0,
                "fnv_hash load_i8="
                f"{fnv.get('load_i8', 0)} xor_i32={fnv.get('xor_i32', 0)} "
                f"mul_i32={fnv.get('mul_i32', 0)} store_i8={fnv.get('store_i8', 0)}",
            )

    if workload in {"loop_data_dependent", "fma_contract"}:
        name = "numeric_loop_body"
        if require_present(name):
            require_no_runtime(name)
            numeric = region(name)
            add(
                "named_region_numeric_loop_has_fp_ops",
                int(numeric.get("fmul", 0) or 0) >= 1
                and int(numeric.get("fadd", 0) or 0) >= 1,
                f"numeric_loop_body fmul={numeric.get('fmul', 0)} "
                f"fadd={numeric.get('fadd', 0)}",
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
) -> dict[str, Any]:
    checks: list[dict[str, Any]] = []
    clang_args = clang_args or []
    named_regions = named_hot_regions(workload, ir_after)

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
        ("llvm_after", ir_after),
        ("assembly", assembly),
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

    allowed_hot_loop_runtime = set(
        WORKLOADS.get(workload, {}).get("allowed_hot_loop_runtime_calls", set())
    )
    loop_runtime: dict[str, list[str]] = {}
    unexpected_loop_runtime: dict[str, list[str]] = {}
    loop_fptosi: dict[str, int] = {}
    loop_sitofp: dict[str, int] = {}
    loop_inttoptr: dict[str, int] = {}
    for label, body in hot_loop_blocks(ir_after):
        runtime = sorted(set(runtime_call_names(body)))
        if runtime:
            loop_runtime[label] = runtime
            unexpected = sorted(name for name in runtime if name not in allowed_hot_loop_runtime)
            if unexpected:
                unexpected_loop_runtime[label] = unexpected
        fptosi = body.count(" fptosi ")
        sitofp = body.count(" sitofp ")
        inttoptr = body.count(" inttoptr ")
        if fptosi:
            loop_fptosi[label] = fptosi
        if sitofp:
            loop_sitofp[label] = sitofp
        if inttoptr:
            loop_inttoptr[label] = inttoptr

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

    if workload == "image_convolution":
        add(
            "direct_gep_inbounds",
            "getelementptr inbounds" in ir_after,
            "optimized IR contains direct inbounds GEPs",
        )
        add(
            "llvm_assume_bounds",
            "@llvm.assume" in ir_after,
            "optimized IR preserves emitted bounds assumptions",
        )
        add(
            "invariant_loads",
            "!invariant.load" in ir_after,
            "optimized IR includes invariant-load metadata",
        )
        add(
            "alias_metadata",
            "!alias.scope" in ir_after and "!noalias" in ir_after,
            "optimized IR includes alias.scope and noalias metadata",
        )
        add(
            "hot_loops_no_inttoptr_buffer_fallback",
            not loop_inttoptr,
            "hot loop inttoptr counts: " + json.dumps(loop_inttoptr, sort_keys=True),
        )
        add(
            "fnv_imul_stays_i32",
            bool(re.search(r"\bmul i32\b[^\n]*16777619", ir_after))
            and bool(re.search(r"\bxor i32\b", ir_after)),
            "FNV checksum lowers to i32 xor/mul by 16777619",
        )
        add(
            "byte_load_store_fast_path",
            bool(re.search(r"\bload i8, ptr\b", ir_after))
            and bool(re.search(r"\bstore i8\b", ir_after)),
            "image loops use byte loads and stores",
        )
        add(
            "kernel_coefficients_constant_folded",
            not re.search(r"\bload double\b[^\n]*KERNEL", ir_after)
            and not re.search(r"@js_array_get|@js_handle_array_get", ir_after),
            "blur kernel coefficients are not loaded through dynamic array helpers",
        )
        add(
            "assembly_contains_imul_constant",
            "16777619" in assembly or "01000193" in assembly.lower(),
            "assembly contains the FNV multiply constant",
        )

    if workload == "loop_data_dependent":
        add(
            "numeric_loop_has_fp_ops",
            any(op in ir_after for op in (" fmul ", " fadd ")),
            "numeric loop optimized IR still contains FP arithmetic",
        )

    if workload == "fma_contract":
        add(
            "numeric_loop_has_fp_ops",
            any(op in ir_after for op in (" fmul ", " fadd ")),
            "FMA fixture optimized IR still contains FP arithmetic",
        )
        fma_count = (
            int(counters.get("assembly", {}).get("fma_instructions", 0) or 0)
            if counters is not None
            else len(re.findall(r"\b(vfmadd|vfnmadd|fmadd|fnmadd)\w*", assembly))
        )
        fma_required = should_expect_fma(
            workload=workload,
            fp_contract_mode=fp_contract_mode,
            target=target,
            clang_args=clang_args,
            expect_fma=expect_fma,
        )
        add(
            "fma_instruction_when_contraction_expected",
            (not fma_required) or fma_count > 0,
            "fma_required="
            + json.dumps(fma_required)
            + f", fma_instructions={fma_count}, target={target}, clang_args={clang_args}",
        )
        no_contract_on_fma_target = fp_contract_mode == "off" and target_supports_fma(
            target, clang_args
        )
        add(
            "no_fma_instruction_when_fp_contract_off",
            (not no_contract_on_fma_target) or fma_count == 0,
            "fp_contract_mode="
            + fp_contract_mode
            + f", fma_target={no_contract_on_fma_target}, fma_instructions={fma_count}",
        )

    if workload == "hir_fact_rewrite":
        after = counters.get("llvm_after", {}) if counters is not None else {}
        add(
            "hir_fact_rewrite_direct_gep_inbounds",
            "getelementptr inbounds" in ir_after,
            "rewrite-insensitive buffer index lowering uses direct inbounds GEPs",
        )
        add(
            "hir_fact_rewrite_assume_bounds",
            "@llvm.assume" in ir_after,
            "rewrite-insensitive buffer index lowering preserves bounds assumptions",
        )
        add(
            "hir_fact_rewrite_alias_metadata",
            "!alias.scope" in ir_after and "!noalias" in ir_after,
            "rewrite-insensitive buffer index lowering keeps buffer alias metadata",
        )
        add(
            "hir_fact_rewrite_no_buffer_slow_path",
            int(after.get("buffer_slow_path_calls", 0) or 0) == 0,
            "buffer_slow_path_calls="
            f"{int(after.get('buffer_slow_path_calls', 0) or 0)}",
        )

    if benchmark is not None:
        add(
            "benchmark_exit_zero",
            all(run.get("exit_code") == 0 for run in benchmark.get("runs", [])),
            "all benchmark runs exited zero",
        )

    for budget in runtime_budget_results(workload, runtime_summary):
        add(
            f"runtime_budget_{budget['field']}",
            bool(budget["passed"]),
            (
                f"{budget['field']} actual={budget['actual']} "
                f"maximum={budget['maximum']}"
            ),
        )

    for result in named_region_contract_results(workload, named_regions):
        add(
            result["name"],
            bool(result["passed"]),
            result["detail"],
        )

    vector_expectation = vectorization_expectation(workload, vectorization)
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
        "runtime_budget_results": runtime_budget_results(workload, runtime_summary),
        "named_regions": named_regions,
        "named_region_contract_results": named_region_contract_results(
            workload, named_regions
        ),
    }


def summarize_gc_trace(stderr_text: str) -> dict[str, Any]:
    events = []
    for line in stderr_text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("event") == "gc" or "gc_kind" in event or "collection_kind" in event:
            events.append(event)
    write_barrier_calls = 0
    allocations = 0
    for event in events:
        wb = event.get("write_barrier")
        if isinstance(wb, dict):
            write_barrier_calls += int(wb.get("calls", 0) or 0)
        for row in event.get("malloc_kinds", []) or []:
            if isinstance(row, dict):
                allocations += int(row.get("allocated_count", 0) or 0)
    return {
        "gc_events": len(events),
        "write_barrier_calls": write_barrier_calls,
        "malloc_kind_allocations": allocations,
    }


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    rank = (len(ordered) - 1) * pct
    low = int(rank)
    high = min(low + 1, len(ordered) - 1)
    weight = rank - low
    return ordered[low] * (1.0 - weight) + ordered[high] * weight


def benchmark_summary(rows: list[dict[str, Any]], benchmark_mode: str) -> dict[str, Any]:
    wall = [float(row["wall_ms"]) for row in rows if row["exit_code"] == 0]
    return {
        "runs": rows,
        "benchmark_mode": benchmark_mode,
        "successful_runs": len(wall),
        "median_wall_ms": statistics.median(wall) if wall else None,
        "mean_wall_ms": statistics.mean(wall) if wall else None,
        "stddev_wall_ms": statistics.stdev(wall) if len(wall) > 1 else 0.0 if wall else None,
        "p95_wall_ms": percentile(wall, 0.95) if wall else None,
        "min_wall_ms": min(wall) if wall else None,
        "max_wall_ms": max(wall) if wall else None,
        "stat_quality": "timing" if len(wall) >= 5 else "smoke",
    }


def resolve_benchmark_runs(args: argparse.Namespace) -> int:
    if args.runs is not None:
        runs = int(args.runs)
    else:
        runs = DEFAULT_BENCHMARK_RUNS[args.benchmark_mode]
    if runs < 1:
        raise HarnessError("--runs must be at least 1")
    return runs


def run_benchmark(
    binary: Path,
    *,
    out_dir: Path,
    runs: int,
    timeout: int,
    enable_gc_trace: bool,
    benchmark_mode: str,
) -> dict[str, Any]:
    rows = []
    for idx in range(1, runs + 1):
        stdout_path = out_dir / f"benchmark-run-{idx}.stdout"
        stderr_path = out_dir / f"benchmark-run-{idx}.stderr"
        before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        env = os.environ.copy()
        if enable_gc_trace:
            env["PERRY_GC_TRACE"] = "1"
        result = run_command(
            [str(binary)],
            cwd=out_dir,
            env=env,
            timeout=timeout,
            stdout_path=stdout_path,
            stderr_path=stderr_path,
            check=False,
        )
        after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        rows.append(
            {
                "run": idx,
                "exit_code": result.exit_code,
                "wall_ms": result.duration_ms,
                "max_rss_kb_delta": max(0, after - before),
                "stdout_path": str(stdout_path),
                "stderr_path": str(stderr_path),
                "stdout_first": result.stdout[:240],
                "stdout_last": result.stdout[-240:],
                "gc_trace_summary": summarize_gc_trace(result.stderr),
            }
        )
    return benchmark_summary(rows, benchmark_mode)


def run_perf_stat(binary: Path, *, out_dir: Path, timeout: int) -> dict[str, Any]:
    perf = shutil.which("perf")
    if not perf:
        return {"available": False, "reason": "perf not found"}
    events = "instructions,cycles,branches,branch-misses,cache-references,cache-misses"
    stderr_path = out_dir / "perf-stat.stderr"
    stdout_path = out_dir / "perf-stat.stdout"
    result = run_command(
        [perf, "stat", "-x,", "-e", events, str(binary)],
        cwd=out_dir,
        timeout=timeout,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
        check=False,
    )
    counters: dict[str, int] = {}
    for line in result.stderr.splitlines():
        parts = line.split(",")
        if len(parts) < 3:
            continue
        value, _, event_name = parts[:3]
        value = value.strip().replace(",", "")
        event_name = event_name.strip()
        if value and value not in {"<not counted>", "<not supported>"}:
            try:
                counters[event_name] = int(float(value))
            except ValueError:
                pass
    return {
        "available": result.exit_code == 0,
        "exit_code": result.exit_code,
        "stdout_path": str(stdout_path),
        "stderr_path": str(stderr_path),
        "counters": counters,
        "reason": "" if result.exit_code == 0 else result.stderr[-500:],
    }


def capture(args: argparse.Namespace) -> int:
    if args.workload not in WORKLOADS:
        raise HarnessError(f"unknown workload {args.workload!r}")

    workload_info = WORKLOADS[args.workload]
    source = (REPO_ROOT / workload_info["source"]).resolve()
    if not source.exists():
        raise HarnessError(f"source not found: {source}")

    out_dir = (
        Path(args.out_dir)
        if args.out_dir
        else REPO_ROOT / "target/compiler-output-regression" / args.workload
    )
    if not out_dir.is_absolute():
        out_dir = REPO_ROOT / out_dir
    out_dir = out_dir.resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    perry = resolve_perry(args.perry)
    clang = resolve_clang(args.clang)
    clang_args = list(args.clang_arg or [])
    runs = resolve_benchmark_runs(args)
    binary = (out_dir / args.workload).resolve()

    commands: dict[str, Any] = {}

    hir_stdout = out_dir / "hir.txt"
    hir_stderr = out_dir / "hir.stderr"
    hir_cmd = perry + [
        "compile",
        str(source),
        "-o",
        str(out_dir / "hir-probe.o"),
        "--print-hir",
        "--no-link",
        "--no-cache",
    ]
    if args.fast_math:
        hir_cmd.append("--fast-math")
    if args.fp_contract:
        hir_cmd.append(f"--fp-contract={args.fp_contract}")
    commands["hir"] = run_command(
        hir_cmd,
        cwd=out_dir,
        env={**os.environ, "PERRY_NO_CACHE": "1"},
        timeout=args.compile_timeout,
        stdout_path=hir_stdout,
        stderr_path=hir_stderr,
    ).to_json()

    compile_stdout = out_dir / "compile.stdout"
    compile_stderr = out_dir / "compile.stderr"
    compile_cmd = perry + [
        "compile",
        str(source),
        "-o",
        str(binary),
        "--no-cache",
    ]
    if args.fast_math:
        compile_cmd.append("--fast-math")
    if args.fp_contract:
        compile_cmd.append(f"--fp-contract={args.fp_contract}")
    commands["compile"] = run_command(
        compile_cmd,
        cwd=out_dir,
        env={**os.environ, "PERRY_LLVM_KEEP_IR": "1", "PERRY_NO_CACHE": "1"},
        timeout=args.compile_timeout,
        stdout_path=compile_stdout,
        stderr_path=compile_stderr,
    ).to_json()

    kept_irs, kept_objects = parse_kept_paths(
        compile_stdout.read_text(encoding="utf-8")
        + "\n"
        + compile_stderr.read_text(encoding="utf-8")
    )
    if not kept_irs:
        raise HarnessError(
            "PERRY_LLVM_KEEP_IR did not report a retained LLVM IR path"
        )
    primary_ir = kept_irs[0]
    ir_before_path = out_dir / "llvm-before-opt.ll"
    shutil.copyfile(primary_ir, ir_before_path)
    for index, path in enumerate(kept_irs[1:], start=1):
        shutil.copyfile(path, out_dir / f"llvm-before-opt-{index}.ll")
    for index, path in enumerate(kept_objects):
        if path.exists():
            shutil.copyfile(path, out_dir / f"object-{index}.o")

    ir_before = ir_before_path.read_text(encoding="utf-8")
    target = args.target or parse_target_triple(ir_before) or "x86_64-unknown-linux-gnu"

    ir_after_path = out_dir / "llvm-after-opt.ll"
    opt_remarks_path = out_dir / "llvm-vectorization-remarks.stderr"
    opt_cmd = [
        clang,
        "-S",
        "-emit-llvm",
        "-O3",
        "-fno-math-errno",
        "-Rpass=loop-vectorize",
        "-Rpass-missed=loop-vectorize",
        "-Rpass-analysis=loop-vectorize",
        *clang_args,
        "-target",
        target,
        str(ir_before_path),
        "-o",
        str(ir_after_path),
    ]
    commands["llvm_after_opt"] = run_command(
        opt_cmd,
        cwd=out_dir,
        timeout=args.compile_timeout,
        stderr_path=opt_remarks_path,
    ).to_json()

    asm_path = out_dir / "assembly.s"
    asm_cmd = [
        clang,
        "-S",
        "-O3",
        "-fno-math-errno",
        *clang_args,
        "-target",
        target,
        str(ir_before_path),
        "-o",
        str(asm_path),
    ]
    commands["assembly"] = run_command(
        asm_cmd,
        cwd=out_dir,
        timeout=args.compile_timeout,
    ).to_json()

    benchmark = None
    if not args.skip_run:
        benchmark = run_benchmark(
            binary,
            out_dir=out_dir,
            runs=runs,
            timeout=args.run_timeout,
            enable_gc_trace=not args.no_gc_trace,
            benchmark_mode=args.benchmark_mode,
        )

    perf_stat = None
    if not args.skip_run and args.perf_counters != "off":
        perf_stat = run_perf_stat(binary, out_dir=out_dir, timeout=args.run_timeout)
        if args.perf_counters == "on" and not perf_stat.get("available"):
            raise HarnessError(f"perf stat unavailable: {perf_stat.get('reason')}")

    ir_after = ir_after_path.read_text(encoding="utf-8")
    assembly = asm_path.read_text(encoding="utf-8")
    vectorization = parse_vectorization_remarks(
        opt_remarks_path.read_text(encoding="utf-8")
        if opt_remarks_path.exists()
        else ""
    )
    counters = structural_counters(ir_before, ir_after, assembly)
    regions = region_counters(args.workload, ir_after)
    runtime_summary = runtime_counter_summary(benchmark, counters)
    fp_contract_mode = (
        args.fp_contract if args.fp_contract else ("fast" if args.fast_math else "off")
    )
    verification = verify_artifacts(
        workload=args.workload,
        ir_before=ir_before,
        ir_after=ir_after,
        assembly=assembly,
        benchmark=benchmark,
        vectorization=vectorization,
        counters=counters,
        runtime_summary=runtime_summary,
        fp_contract_mode=fp_contract_mode,
        target=target,
        clang_args=clang_args,
        expect_fma=args.expect_fma,
    )

    manifest = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": utc_now(),
        "workload": args.workload,
        "workload_kind": workload_info["kind"],
        "source": relpath(source),
        "target": target,
        "fp_modes": {
            "fast_math": bool(args.fast_math),
            "fp_contract": fp_contract_mode,
        },
        "clang_args": clang_args,
        "benchmark_settings": {
            "benchmark_mode": args.benchmark_mode,
            "runs": runs,
            "user_supplied_runs": args.runs is not None,
        },
        "tool_versions": {
            "perry": compiler_version(perry),
            "clang": compiler_version([clang]),
        },
        "commands": commands,
        "artifacts": {
            "hir": str(hir_stdout),
            "llvm_before_opt": str(ir_before_path),
            "llvm_after_opt": str(ir_after_path),
            "assembly": str(asm_path),
            "vectorization_remarks": str(opt_remarks_path),
            "binary": str(binary),
        },
        "benchmark": benchmark,
        "perf_stat": perf_stat,
        "vectorization_remarks": vectorization,
        "counters": counters,
        "regions": regions,
        "runtime_counter_summary": runtime_summary,
        "verification": verification,
    }

    manifest_path = out_dir / "manifest.json"
    verification_path = out_dir / "structural-report.json"
    write_text(manifest_path, json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    write_text(
        verification_path,
        json.dumps(verification, indent=2, sort_keys=True) + "\n",
    )

    if args.print_summary:
        print(json.dumps({"manifest": str(manifest_path), **verification}, indent=2))

    if args.gate and verification["status"] != "pass":
        return 1
    return 0


def verify_existing(args: argparse.Namespace) -> int:
    root = Path(args.artifact_dir)
    before = root / "llvm-before-opt.ll"
    after = root / "llvm-after-opt.ll"
    asm = root / "assembly.s"
    missing = [str(path) for path in (before, after, asm) if not path.exists()]
    if missing:
        raise HarnessError(f"missing artifacts: {', '.join(missing)}")
    remarks = root / "llvm-vectorization-remarks.stderr"
    vectorization = parse_vectorization_remarks(
        remarks.read_text(encoding="utf-8") if remarks.exists() else ""
    )
    ir_before = before.read_text(encoding="utf-8")
    ir_after = after.read_text(encoding="utf-8")
    assembly = asm.read_text(encoding="utf-8")
    counters = structural_counters(ir_before, ir_after, assembly)
    runtime_summary = runtime_counter_summary(None, counters)
    report = verify_artifacts(
        workload=args.workload,
        ir_before=ir_before,
        ir_after=ir_after,
        assembly=assembly,
        benchmark=None,
        vectorization=vectorization,
        counters=counters,
        runtime_summary=runtime_summary,
        fp_contract_mode=args.fp_contract or "off",
        target=args.target or parse_target_triple(ir_before) or "",
        clang_args=list(args.clang_arg or []),
        expect_fma=args.expect_fma,
    )
    output = root / "structural-report.json"
    write_text(output, json.dumps(report, indent=2, sort_keys=True) + "\n")
    if args.print_summary:
        print(json.dumps(report, indent=2))
    return 1 if args.gate and report["status"] != "pass" else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    capture_p = sub.add_parser("capture", help="compile, retain artifacts, and verify")
    capture_p.add_argument("--workload", choices=sorted(WORKLOADS), default="image_convolution")
    capture_p.add_argument("--out-dir")
    capture_p.add_argument("--perry")
    capture_p.add_argument("--clang")
    capture_p.add_argument("--target")
    capture_p.add_argument(
        "--clang-arg",
        action="append",
        help="extra clang argument for optimized IR and assembly emission",
    )
    capture_p.add_argument("--runs", type=int)
    capture_p.add_argument(
        "--benchmark-mode",
        choices=sorted(DEFAULT_BENCHMARK_RUNS),
        default="smoke",
        help="default run-count profile when --runs is omitted",
    )
    capture_p.add_argument("--compile-timeout", type=int, default=300)
    capture_p.add_argument("--run-timeout", type=int, default=300)
    capture_p.add_argument("--skip-run", action="store_true")
    capture_p.add_argument("--no-gc-trace", action="store_true")
    capture_p.add_argument("--fast-math", action="store_true")
    capture_p.add_argument("--fp-contract", choices=("off", "on", "fast"))
    capture_p.add_argument(
        "--expect-fma",
        choices=("auto", "off", "on"),
        default="auto",
        help="gate FMA assembly for the fma_contract workload",
    )
    capture_p.add_argument("--perf-counters", choices=("auto", "off", "on"), default="auto")
    capture_p.add_argument("--gate", action="store_true")
    capture_p.add_argument("--print-summary", action="store_true")
    capture_p.set_defaults(func=capture)

    verify_p = sub.add_parser("verify", help="verify an existing artifact directory")
    verify_p.add_argument("--workload", choices=sorted(WORKLOADS), default="image_convolution")
    verify_p.add_argument("--artifact-dir", required=True)
    verify_p.add_argument("--target")
    verify_p.add_argument("--clang-arg", action="append")
    verify_p.add_argument("--fp-contract", choices=("off", "on", "fast"))
    verify_p.add_argument("--expect-fma", choices=("auto", "off", "on"), default="auto")
    verify_p.add_argument("--gate", action="store_true")
    verify_p.add_argument("--print-summary", action="store_true")
    verify_p.set_defaults(func=verify_existing)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except HarnessError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
