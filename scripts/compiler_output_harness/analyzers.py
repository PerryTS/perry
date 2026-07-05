from __future__ import annotations

import json
import re
import resource
import shutil
import statistics
from collections import Counter
from pathlib import Path
from typing import Any

from .common import (
    ARRAY_SLOW_PATH_HELPERS,
    BUFFER_SLOW_PATH_HELPERS,
    DYNAMIC_PROPERTY_HELPERS,
    RUNTIME_CALL_PREFIXES,
    HarnessError,
    run_command,
)


def parse_kept_paths(log_text: str) -> tuple[list[Path], list[Path], list[Path], list[Path]]:
    ir_paths: list[Path] = []
    object_paths: list[Path] = []
    metadata_paths: list[Path] = []
    native_rep_paths: list[Path] = []
    for line in log_text.splitlines():
        ir_match = re.search(r"kept LLVM IR:\s*(\S+)", line)
        if ir_match:
            ir_paths.append(Path(ir_match.group(1)))
        obj_match = re.search(r"kept object:\s*(\S+)", line)
        if obj_match:
            object_paths.append(Path(obj_match.group(1)))
        meta_match = re.search(r"kept compile metadata:\s*(\S+)", line)
        if meta_match:
            metadata_paths.append(Path(meta_match.group(1)))
        native_match = re.search(r"kept native reps:\s*(\S+)", line)
        if native_match:
            native_rep_paths.append(Path(native_match.group(1)))
    return ir_paths, object_paths, metadata_paths, native_rep_paths


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


def extract_blocks_with_functions(ir: str) -> list[tuple[str, str, str]]:
    blocks: list[tuple[str, str, str]] = []
    current_function = ""
    current_label: str | None = None
    current_lines: list[str] = []
    label_re = re.compile(r"^([A-Za-z0-9_.$-]+):(?:\s|$)")
    define_re = re.compile(r"^define\b.*@([A-Za-z0-9_.$-]+)\(")
    for line in ir.splitlines():
        define_match = define_re.match(line)
        if define_match:
            if current_label is not None:
                blocks.append(
                    (current_function, current_label, "\n".join(current_lines))
                )
            current_function = define_match.group(1)
            current_label = None
            current_lines = []
            continue
        match = label_re.match(line)
        if match:
            if current_label is not None:
                blocks.append(
                    (current_function, current_label, "\n".join(current_lines))
                )
            current_label = match.group(1)
            current_lines = [line]
        elif current_label is not None:
            current_lines.append(line)
    if current_label is not None:
        blocks.append((current_function, current_label, "\n".join(current_lines)))
    return blocks


def call_names(text: str) -> list[str]:
    return [
        match.group(1)
        for match in re.finditer(r"\bcall\b[^\n;]*@([A-Za-z_.$][A-Za-z0-9_.$]*)", text)
    ]


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
            + after_calls.get("js_write_barrier_slot", 0)
            + after_calls.get("js_write_barrier_root_nanbox", 0)
            + after_calls.get("js_write_barrier_root_heap_word", 0),
            "buffer_slow_path_calls": sum(
                count
                for name, count in after_calls.items()
                if any(helper in name for helper in BUFFER_SLOW_PATH_HELPERS)
            ),
            "array_slow_path_calls": sum(
                count
                for name, count in after_calls.items()
                if any(helper in name for helper in ARRAY_SLOW_PATH_HELPERS)
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
    load_i8 = len(re.findall(r"\bload (?:i8|<\d+ x i8>), ptr\b", body))
    store_i8 = len(re.findall(r"\bstore (?:i8\b|<\d+ x i8>)", body))
    load_f64 = len(re.findall(r"\bload double, ptr\b", body))
    store_f64 = len(re.findall(r"\bstore double\b", body))
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
        "load_i8": load_i8,
        "store_i8": store_i8,
        "load_f64": load_f64,
        "store_f64": store_f64,
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
        "load_f64": 0,
        "store_f64": 0,
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
            "load_f64",
            "store_f64",
            "fmul",
            "fadd",
            "mul_i32",
            "xor_i32",
        ):
            merged[key] += int(counters.get(key, 0) or 0)
    merged["runtime_calls"] = dict(sorted(merged["runtime_calls"].items()))
    return merged


def merge_named_region_counters(
    blocks: list[tuple[str, str, dict[str, Any]]],
) -> dict[str, Any]:
    merged = merge_region_counters([(label, counters) for _, label, counters in blocks])
    merged["functions"] = sorted({function for function, _, _ in blocks if function})
    merged["block_keys"] = [
        {"function": function, "label": label} for function, label, _ in blocks
    ]
    return merged


def _selector_matches(
    function: str, label: str, counters: dict[str, Any], selector: dict[str, Any]
) -> bool:
    function_any = selector.get("function_any")
    if function_any and function not in set(function_any):
        return False
    function_contains = selector.get("function_contains")
    if function_contains and function_contains not in function:
        return False
    function_regex = selector.get("function_regex")
    if function_regex and not re.search(function_regex, function):
        return False
    labels = selector.get("label_any")
    if labels and label not in set(labels):
        return False
    prefixes = selector.get("label_prefix_any")
    if prefixes and not any(label.startswith(prefix) for prefix in prefixes):
        return False
    for key, minimum in (selector.get("counter_min") or {}).items():
        if int(counters.get(key, 0) or 0) < int(minimum):
            return False
    for key, expected in (selector.get("counter_equals") or {}).items():
        if int(counters.get(key, 0) or 0) != int(expected):
            return False
    any_min = selector.get("counter_any_min") or {}
    if any_min and not any(
        int(counters.get(key, 0) or 0) >= int(minimum)
        for key, minimum in any_min.items()
    ):
        return False
    return True


def hot_region_counters(ir_after: str) -> dict[str, Any]:
    regions: dict[str, Any] = {}
    for label, body in hot_loop_blocks(ir_after):
        regions[label] = block_counter_summary(body)
    return {"hot_loops": regions}


def named_hot_regions(workload_info: dict[str, Any], ir_after: str) -> dict[str, Any]:
    blocks = [
        (function, label, block_counter_summary(body))
        for function, label, body in extract_blocks_with_functions(ir_after)
    ]
    selected: dict[str, list[tuple[str, str, dict[str, Any]]]] = {}
    assigned: set[tuple[str, str]] = set()
    for region in workload_info.get("named_regions", []) or []:
        name = region["name"]
        for function, label, counters in blocks:
            block_key = (function, label)
            if region.get("exclusive", True) and block_key in assigned:
                continue
            if any(
                _selector_matches(function, label, counters, selector)
                for selector in region.get("selectors", []) or []
            ):
                selected.setdefault(name, []).append((function, label, counters))
                if region.get("exclusive", True):
                    assigned.add(block_key)
    return {
        name: merge_named_region_counters(region_blocks)
        for name, region_blocks in selected.items()
    }


def region_counters(
    workload: str, ir_after: str, workloads: dict[str, Any] | None = None
) -> dict[str, Any]:
    if workloads is None:
        from .spec import WORKLOADS

        workloads = WORKLOADS
    regions = hot_region_counters(ir_after)
    regions["named"] = named_hot_regions(workloads.get(workload, {}), ir_after)
    return regions


def runtime_counter_summary(
    benchmark: dict[str, Any] | None, counters: dict[str, Any]
) -> dict[str, Any]:
    after = counters.get("llvm_after", {})
    runtime_calls = after.get("runtime_calls", {})
    gc_collections = 0
    traced_allocations = 0
    traced_write_barriers = 0
    gc_trace_unavailable = False
    gc_trace_enabled: bool | None = None
    if benchmark is not None:
        if isinstance(benchmark.get("gc_trace_enabled"), bool):
            gc_trace_enabled = bool(benchmark["gc_trace_enabled"])
        for row in benchmark.get("runs", []):
            if isinstance(row.get("gc_trace_enabled"), bool):
                row_trace_enabled = bool(row["gc_trace_enabled"])
                gc_trace_enabled = (
                    row_trace_enabled
                    if gc_trace_enabled is None
                    else gc_trace_enabled and row_trace_enabled
            )
            trace = row.get("gc_trace_summary", {})
            gc_trace_unavailable = gc_trace_unavailable or bool(
                trace.get("diagnostics_disabled")
            )
            gc_collections += int(trace.get("gc_events", 0) or 0)
            traced_allocations += int(trace.get("malloc_kind_allocations", 0) or 0)
            traced_write_barriers += int(trace.get("write_barrier_calls", 0) or 0)
    return {
        "gc_trace_enabled": gc_trace_enabled,
        "gc_trace_unavailable": gc_trace_unavailable,
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
        "array_slow_path_accesses_static": int(
            after.get("array_slow_path_calls", 0) or 0
        ),
    }


def summarize_gc_trace(stderr_text: str) -> dict[str, Any]:
    events = []
    diagnostics_disabled = False
    for line in stderr_text.splitlines():
        line = line.strip()
        if "diagnostics feature disabled" in line:
            diagnostics_disabled = True
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
        "diagnostics_disabled": diagnostics_disabled,
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
        import os

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
                "gc_trace_enabled": bool(enable_gc_trace),
                "gc_trace_summary": summarize_gc_trace(result.stderr),
            }
        )
    summary = benchmark_summary(rows, benchmark_mode)
    summary["gc_trace_enabled"] = bool(enable_gc_trace)
    return summary


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


def resolve_objdump() -> str:
    import os

    explicit = os.environ.get("LLVM_OBJDUMP") or os.environ.get("OBJDUMP")
    if explicit:
        return explicit
    for name in ("llvm-objdump", "objdump"):
        found = shutil.which(name)
        if found:
            return found
    raise HarnessError("llvm-objdump or objdump is required to disassemble retained objects")


def disassemble_object(
    object_path: Path, *, output_path: Path, cwd: Path, timeout: int
) -> dict[str, Any]:
    objdump = resolve_objdump()
    result = run_command(
        [objdump, "-d", str(object_path)],
        cwd=cwd,
        timeout=timeout,
        stdout_path=output_path,
        check=True,
    )
    return result.to_json()
