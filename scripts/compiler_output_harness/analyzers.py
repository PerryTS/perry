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


# LLVM quotes any identifier containing characters outside its bare-name set --
# notably ``$``, which specialized function names use (``…$spec_i32.exit``). A
# quoted label line starts with ``"``, so a bare-name-only pattern never matches
# it, no new block begins, and the quoted block's body is silently appended to
# the PREVIOUS block. That mis-attributes its calls (perry#9494).
_LABEL_PATTERN = r"^(?:\"([^\"]+)\"|([A-Za-z0-9_.$-]+)):(?:\s|$)"


def extract_blocks(ir: str) -> list[tuple[str, str]]:
    blocks: list[tuple[str, str]] = []
    current_label: str | None = None
    current_lines: list[str] = []
    label_re = re.compile(_LABEL_PATTERN)
    for line in ir.splitlines():
        match = label_re.match(line)
        if match:
            if current_label is not None:
                blocks.append((current_label, "\n".join(current_lines)))
            current_label = match.group(1) or match.group(2)
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
    label_re = re.compile(_LABEL_PATTERN)
    define_re = re.compile(r"^define\b.*@(?:\"([^\"]+)\"|([A-Za-z0-9_.$-]+))\(")
    for line in ir.splitlines():
        define_match = define_re.match(line)
        if define_match:
            if current_label is not None:
                blocks.append(
                    (current_function, current_label, "\n".join(current_lines))
                )
            current_function = define_match.group(1) or define_match.group(2)
            current_label = None
            current_lines = []
            continue
        match = label_re.match(line)
        if match:
            if current_label is not None:
                blocks.append(
                    (current_function, current_label, "\n".join(current_lines))
                )
            current_label = match.group(1) or match.group(2)
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
    # The original copy of a byte-view-versioned loop keeps the generic
    # element accesses (a receiver the admission refused is not a byte view);
    # those run only behind the admission and are reported apart.
    behind_byte_view_version = buffer_slow_path_calls_behind_byte_view_version(ir_after)
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
            # Heap write barriers — the perf-relevant, optimizer-controlled
            # barriers this gate exists to catch. Counted statically because a
            # native region that stores a GC pointer into a heap object needs
            # one; the proof budgets bound how many.
            "write_barriers": after_calls.get("js_write_barrier", 0)
            + after_calls.get("js_write_barrier_slot", 0),
            # GC shadow-stack root-shading barriers (#7088). Before #7088 these
            # lived inside the `js_shadow_slot_bind` / `js_shadow_slot_set`
            # runtime calls and were invisible to this static IR counter; #7088
            # emits the shadow-slot store — and its barrier — inline, so the
            # call site is now visible here. Each is emitted once per rooted
            # pointer-capable local (a structural constant, not an optimizer
            # choice) and is gated behind PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT,
            # so it never fires unless incremental marking is live — meaning it
            # is caught, if it ever regresses at runtime, by write_barriers_traced.
            # Tracked separately so it does not inflate the heap-barrier budget.
            "root_shading_barriers": after_calls.get("js_write_barrier_root_nanbox", 0)
            + after_calls.get("js_write_barrier_root_heap_word", 0),
            "buffer_slow_path_calls": sum(
                count
                for name, count in after_calls.items()
                if any(helper in name for helper in BUFFER_SLOW_PATH_HELPERS)
            )
            - behind_byte_view_version,
            "buffer_slow_path_calls_behind_byte_view_version": behind_byte_view_version,
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
    # A tail-folded vector loop moves bytes with masked intrinsics.
    load_i8 = len(re.findall(r"\bload (?:i8|<\d+ x i8>), ptr\b", body)) + len(
        re.findall(r"@llvm\.masked\.load\.v\d+i8\.", body)
    )
    store_i8 = len(re.findall(r"\bstore (?:i8\b|<\d+ x i8>)", body)) + len(
        re.findall(r"@llvm\.masked\.store\.v\d+i8\.", body)
    )
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


def named_hot_regions(
    workload_info: dict[str, Any],
    ir_after: str,
    claimed: set[tuple[str, str]] | None = None,
) -> dict[str, Any]:
    """Selector-resolved regions. `claimed` blocks belong to a labelled
    region's loop (see `native_loop_regions`) and match no selector: a
    counter-shaped selector would otherwise also pick up a block of that
    loop that happens to have the same shape."""
    claimed = claimed or set()
    blocks = [
        (function, label, block_counter_summary(body))
        for function, label, body in extract_blocks_with_functions(ir_after)
        if (function, label) not in claimed
    ]
    looped: set[tuple[str, str]] | None = None
    if any(region.get("loop_blocks_only") for region in workload_info.get("named_regions", []) or []):
        looped = loop_block_keys(ir_after)
    selected: dict[str, list[tuple[str, str, dict[str, Any]]]] = {}
    assigned: set[tuple[str, str]] = set()
    for region in workload_info.get("named_regions", []) or []:
        name = region["name"]
        for function, label, counters in blocks:
            block_key = (function, label)
            if region.get("exclusive", True) and block_key in assigned:
                continue
            if region.get("loop_blocks_only") and block_key not in (looped or set()):
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


_NANBOX_POINTER_MASK = "281474976710655"


def _cycle_blocks(successors: dict[str, list[str]]) -> set[str]:
    """Blocks on a CFG cycle (members of a non-trivial strongly connected
    component, or a self loop): every block a loop iteration can run."""
    index: dict[str, int] = {}
    low: dict[str, int] = {}
    on_stack: set[str] = set()
    stack: list[str] = []
    cyclic: set[str] = set()
    counter = 0
    for root in successors:
        if root in index:
            continue
        index[root] = low[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        work = [(root, iter(successors[root]))]
        while work:
            node, children = work[-1]
            advanced = False
            for child in children:
                if child not in successors:
                    continue
                if child not in index:
                    index[child] = low[child] = counter
                    counter += 1
                    stack.append(child)
                    on_stack.add(child)
                    work.append((child, iter(successors[child])))
                    advanced = True
                    break
                if child in on_stack:
                    low[node] = min(low[node], index[child])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[node])
            if low[node] == index[node]:
                component = []
                while True:
                    member = stack.pop()
                    on_stack.discard(member)
                    component.append(member)
                    if member == node:
                        break
                if len(component) > 1 or node in successors[node]:
                    cyclic.update(component)
    return cyclic


def loop_header_length_loads(ir: str) -> dict[str, list[str]]:
    """Per function, the loop blocks (blocks on a CFG cycle) of OPTIMIZED IR
    that read a header length word: `load i32` straight from a NaN-box
    payload pointer (`inttoptr (and %v, POINTER_MASK)`, or the null-guarded
    `select` of it). Array, Buffer, TypedArray and string headers keep their
    length at offset 0, so such a load inside a loop re-reads the length every
    iteration. A loop-invariant length is a preheader load, a constant or an
    SSA value (mem2reg of a hoisted slot), none of which is counted."""
    found: dict[str, list[str]] = {}
    define_re = re.compile(r"^define\b.*@(?:\"([^\"]+)\"|([A-Za-z0-9_.$-]+))\(")
    label_re = re.compile(_LABEL_PATTERN)
    def_re = re.compile(r"^\s*(%[-\w.$]+) = (.*)$")
    load_re = re.compile(r"= load i32, ptr (%[-\w.$]+)")
    target_re = re.compile(r"label %(?:\"([^\"]+)\"|([-\w.$]+))")
    payload_re = re.compile(r"and i64 %[-\w.$]+, " + _NANBOX_POINTER_MASK + r"\b")
    cast_re = re.compile(r"inttoptr i64 (%[-\w.$]+) to ptr$")
    guard_re = re.compile(r"select i1 [^,]+, ptr @\S*null_guard\S*, ptr (%[-\w.$]+)$")
    functions: list[tuple[str, list[str]]] = []
    current: tuple[str, list[str]] | None = None
    for line in ir.splitlines():
        match = define_re.match(line)
        if match:
            current = (match.group(1) or match.group(2), [])
            functions.append(current)
        elif current is not None and line.startswith("}"):
            current = None
        elif current is not None:
            current[1].append(line)
    for function, lines in functions:
        defs: dict[str, str] = {}
        blocks: list[tuple[str, list[str]]] = [("", [])]
        for line in lines:
            label = label_re.match(line)
            if label:
                blocks.append((label.group(1) or label.group(2), []))
                continue
            blocks[-1][1].append(line)
            definition = def_re.match(line)
            if definition:
                defs[definition.group(1)] = definition.group(2)
        successors = {
            label: [
                target.group(1) or target.group(2)
                for line in body
                for target in target_re.finditer(line)
            ]
            for label, body in blocks
        }

        def header_pointer(pointer: str) -> bool:
            definition = defs.get(pointer, "")
            cast = cast_re.match(definition)
            if cast:
                return bool(payload_re.match(defs.get(cast.group(1), "")))
            guarded = guard_re.match(definition)
            return bool(guarded) and header_pointer(guarded.group(1))

        looped = _cycle_blocks(successors)
        for label, body in blocks:
            if label not in looped:
                continue
            for line in body:
                load = load_re.search(line)
                if load and header_pointer(load.group(1)):
                    found.setdefault(function, []).append(label)
    return found


def region_counters(
    workload: str, ir_after: str, workloads: dict[str, Any] | None = None
) -> dict[str, Any]:
    if workloads is None:
        from .spec import WORKLOADS

        workloads = WORKLOADS
    regions = hot_region_counters(ir_after)
    workload_info = workloads.get(workload, {})
    loop_regions = native_loop_regions(workload_info, ir_after)
    claimed = {
        (key["function"], key["label"])
        for region in loop_regions.values()
        for key in region.get("block_keys", [])
    }
    regions["named"] = named_hot_regions(workload_info, ir_after, claimed)
    regions["named"].update(loop_regions)
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
        "root_shading_barriers_static": int(after.get("root_shading_barriers", 0) or 0),
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


# ---------------------------------------------------------------------------
# Loop identity in optimized IR.
#
# Codegen gives the latch of every labelled loop an `!llvm.loop` loop ID that
# lists `!{!"perry.region", !"<region id>"}`, and the latch of the original
# (fallback) copy of a byte-view-versioned loop `!{!"perry.byte_view.fallback"}`.
# The admission branch that picks between the copies carries
# `!perry.byte_view.version`. LLVM keeps unknown loop-ID operands through
# rotation, unrolling and vectorization, so a region is found in optimized IR
# by its loop ID, not by a block label. Anything that loses the ID loses the
# loop: the region is then absent and a required region fails (fail closed).
# ---------------------------------------------------------------------------

_DEFINE_RE = re.compile(r"^define\b.*@(?:\"([^\"]+)\"|([A-Za-z0-9_.$-]+))\(")
_TARGET_RE = re.compile(r"label %(?:\"([^\"]+)\"|([-\w.$]+))")
_MD_DEF_RE = re.compile(r"^!(\d+) = (?:distinct )?!\{(.*)\}\s*$")
_MD_REF_RE = re.compile(r"!(\d+)")
_MD_STRING_RE = re.compile(r'!"((?:[^"\\]|\\[0-9A-Fa-f]{2})*)"')
_LOOP_ATTACHMENT_RE = re.compile(r"!llvm\.loop !(\d+)")
_BYTE_VIEW_VERSION_RE = re.compile(r"!perry\.byte_view\.version\b")
_REGION_TAG = "perry.region"
_FALLBACK_TAG = "perry.byte_view.fallback"


def _md_string(text: str) -> str:
    return re.sub(r"\\([0-9A-Fa-f]{2})", lambda m: chr(int(m.group(1), 16)), text)


def _metadata_nodes(ir: str) -> dict[str, str]:
    return {
        match.group(1): match.group(2)
        for match in (_MD_DEF_RE.match(line) for line in ir.splitlines())
        if match
    }


def _loop_id_tags(nodes: dict[str, str], loop_id: str) -> tuple[set[str], bool]:
    """(region ids, byte-view fallback?) a loop ID lists. The ID must be a
    loop ID (first operand is itself); its operands are tuples whose first
    string names the tag."""
    body = nodes.get(loop_id)
    if body is None:
        return set(), False
    refs = _MD_REF_RE.findall(body.split(",", 1)[0])
    if refs != [loop_id]:
        return set(), False
    regions: set[str] = set()
    fallback = False
    for operand in _MD_REF_RE.findall(body)[1:]:
        strings = [_md_string(s) for s in _MD_STRING_RE.findall(nodes.get(operand, ""))]
        if len(strings) == 2 and strings[0] == _REGION_TAG:
            regions.add(strings[1])
        elif strings == [_FALLBACK_TAG]:
            fallback = True
    return regions, fallback


def _ir_functions(ir: str) -> list[tuple[str, list[tuple[str, list[str]]]]]:
    """Every defined function as (name, [(block label, lines)]). The entry
    block keeps its implicit label as ""."""
    label_re = re.compile(_LABEL_PATTERN)
    functions: list[tuple[str, list[tuple[str, list[str]]]]] = []
    blocks: list[tuple[str, list[str]]] | None = None
    for line in ir.splitlines():
        match = _DEFINE_RE.match(line)
        if match:
            blocks = [("", [])]
            functions.append((match.group(1) or match.group(2), blocks))
            continue
        if blocks is None:
            continue
        if line.startswith("}"):
            blocks = None
            continue
        label = label_re.match(line)
        if label:
            if blocks == [("", [])]:
                blocks.clear()
            blocks.append((label.group(1) or label.group(2), [line]))
            continue
        blocks[-1][1].append(line)
    return functions


def _terminator(lines: list[str]) -> str:
    """The block's terminator text (a `switch` spans several lines)."""
    for index in range(len(lines) - 1, -1, -1):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith(";"):
            continue
        if stripped.startswith("]"):
            start = index
            while start > 0 and "switch " not in lines[start] and "indirectbr " not in lines[start]:
                start -= 1
            return "\n".join(lines[start : index + 1])
        if stripped.startswith(("to label", "unwind ")):
            return "\n".join(lines[max(index - 1, 0) : index + 1])
        return stripped
    return ""


class _FunctionCfg:
    def __init__(self, name: str, blocks: list[tuple[str, list[str]]]):
        self.name = name
        self.blocks = blocks
        self.terminators = {label: _terminator(lines) for label, lines in blocks}
        self.successors = {
            label: list(
                dict.fromkeys(
                    m.group(1) or m.group(2) for m in _TARGET_RE.finditer(self.terminators[label])
                )
            )
            for label, _ in blocks
        }
        self.predecessors: dict[str, list[str]] = {label: [] for label, _ in blocks}
        for label, successors in self.successors.items():
            for successor in successors:
                if successor in self.predecessors:
                    self.predecessors[successor].append(label)
        self.idom = self._dominator_tree()

    def _dominator_tree(self) -> dict[str, str]:
        if not self.blocks:
            return {}
        entry = self.blocks[0][0]
        order: list[str] = []
        seen = {entry}
        stack = [(entry, iter(self.successors.get(entry, [])))]
        while stack:
            node, children = stack[-1]
            for child in children:
                if child in self.successors and child not in seen:
                    seen.add(child)
                    stack.append((child, iter(self.successors[child])))
                    break
            else:
                stack.pop()
                order.append(node)
        order.reverse()
        rank = {label: index for index, label in enumerate(order)}
        idom = {entry: entry}

        def intersect(a: str, b: str) -> str:
            while a != b:
                while rank[a] > rank[b]:
                    a = idom[a]
                while rank[b] > rank[a]:
                    b = idom[b]
            return a

        changed = True
        while changed:
            changed = False
            for node in order[1:]:
                done = [p for p in self.predecessors[node] if p in idom]
                if not done:
                    continue
                new = done[0]
                for pred in done[1:]:
                    new = intersect(pred, new)
                if idom.get(node) != new:
                    idom[node] = new
                    changed = True
        return idom

    def dominates(self, a: str, b: str) -> bool:
        if b not in self.idom or a not in self.idom:
            return False
        while True:
            if a == b:
                return True
            parent = self.idom[b]
            if parent == b:
                return False
            b = parent

    def natural_loop(self, latch: str, header: str) -> set[str]:
        """Blocks of the loop of back edge latch -> header (header dominates
        latch), or empty when the edge is not a back edge."""
        if not self.dominates(header, latch):
            return set()
        body = {header, latch}
        work = [latch]
        while work:
            node = work.pop()
            if node == header:
                continue
            for pred in self.predecessors.get(node, []):
                if pred not in body and pred in self.idom:
                    body.add(pred)
                    work.append(pred)
        return body

    def edge_dominated(self, source: str, target: str) -> set[str]:
        """Blocks reached only through the edge source -> target: `target`
        dominates them and every other way into `target` is a back edge from
        inside it. Empty when the edge is not the only way in."""
        if self.successors.get(source, []).count(target) != 1:
            return set()
        for pred in self.predecessors.get(target, []):
            if pred != source and not self.dominates(target, pred):
                return set()
        return {label for label, _ in self.blocks if self.dominates(target, label)}


def _tagged_loops(cfg: _FunctionCfg, nodes: dict[str, str]):
    """(regions, fallback?, loop blocks) for every back edge whose latch
    carries a perry loop ID."""
    for latch, terminator in cfg.terminators.items():
        attachment = _LOOP_ATTACHMENT_RE.search(terminator)
        if not attachment:
            continue
        regions, fallback = _loop_id_tags(nodes, attachment.group(1))
        if not regions and not fallback:
            continue
        for header in cfg.successors.get(latch, []):
            body = cfg.natural_loop(latch, header)
            if body:
                yield regions, fallback, body


def _byte_view_fallback_blocks(cfg: _FunctionCfg, nodes: dict[str, str]) -> set[str]:
    """Blocks of the original copy of a byte-view-versioned loop that run only
    behind its admission. For a branch V marked `!perry.byte_view.version`
    with successors F and S: S is the fallback side when the blocks S
    dominates hold a fallback-marked loop and the blocks F dominates hold
    none, and no way into S starts inside F's blocks (the fast copy never
    falls into the original copy). The excluded blocks are those S
    dominates. A way into S other than V is the refused admission threaded
    around V by LLVM (JumpThreading clones V's block where the admission is
    known false); it still enters only S. Nothing is excluded without a
    marked V, so removing the admission branch counts the whole original
    copy, and a block of the fast copy is never excluded."""
    fallback_loops = [body for _, fallback, body in _tagged_loops(cfg, nodes) if fallback]
    if not fallback_loops:
        return set()
    labels = [label for label, _ in cfg.blocks]
    excluded: set[str] = set()
    for source, terminator in cfg.terminators.items():
        if not _BYTE_VIEW_VERSION_RE.search(terminator) or not terminator.startswith("br i1 "):
            continue
        successors = cfg.successors.get(source, [])
        if len(successors) != 2:
            continue
        sides = [
            {label for label in labels if cfg.dominates(successor, label)}
            for successor in successors
        ]
        holds = [any(loop <= side for loop in fallback_loops) for side in sides]
        if holds.count(True) != 1:
            continue
        fallback = holds.index(True)
        fast_side = sides[1 - fallback]
        if any(loop & fast_side for loop in fallback_loops):
            continue
        if any(pred in fast_side for pred in cfg.predecessors.get(successors[fallback], [])):
            continue
        excluded |= sides[fallback]
    return excluded


def byte_view_fallback_blocks(ir: str) -> dict[str, set[str]]:
    nodes = _metadata_nodes(ir)
    found: dict[str, set[str]] = {}
    for name, blocks in _ir_functions(ir):
        if not any(_BYTE_VIEW_VERSION_RE.search(line) for _, lines in blocks for line in lines):
            continue
        excluded = _byte_view_fallback_blocks(_FunctionCfg(name, blocks), nodes)
        if excluded:
            found[name] = excluded
    return found


def buffer_slow_path_calls_behind_byte_view_version(ir: str) -> int:
    """Buffer slow-path calls in the original copy of a byte-view-versioned
    loop that only the refused admission reaches (see
    `_byte_view_fallback_blocks`)."""
    excluded = byte_view_fallback_blocks(ir)
    if not excluded:
        return 0
    total = 0
    for name, blocks in _ir_functions(ir):
        labels = excluded.get(name)
        if not labels:
            continue
        for label, lines in blocks:
            if label in labels:
                total += sum(
                    1
                    for callee in call_names("\n".join(lines))
                    if any(helper in callee for helper in BUFFER_SLOW_PATH_HELPERS)
                )
    return total


def loop_tagged_regions(ir: str) -> dict[str, list[tuple[str, str, str]]]:
    """Region id -> the (function, label, body) blocks of every loop whose
    loop ID names that region (all copies: versioned, specialized, inlined),
    minus the original copy behind a byte-view admission (its generic
    accesses are the semantic fallback, not the region)."""
    nodes = _metadata_nodes(ir)
    found: dict[str, list[tuple[str, str, str]]] = {}
    for name, blocks in _ir_functions(ir):
        if not any(_LOOP_ATTACHMENT_RE.search(line) for _, lines in blocks for line in lines):
            continue
        cfg = _FunctionCfg(name, blocks)
        excluded = _byte_view_fallback_blocks(cfg, nodes)
        members: dict[str, set[str]] = {}
        for regions, _, body in _tagged_loops(cfg, nodes):
            for region in regions:
                members.setdefault(region, set()).update(body - excluded)
        for region, labels in members.items():
            for label, lines in blocks:
                if label in labels:
                    found.setdefault(region, []).append((name, label, "\n".join(lines)))
    return found


def native_loop_regions(workload_info: dict[str, Any], ir: str) -> dict[str, Any]:
    """Resolve each selector-less named region that names a `native_region_id`
    from the loop that region labels, by the loop ID codegen gave its latch
    (see `loop_tagged_regions`). A region whose loop ID did not survive is
    absent."""
    wanted: dict[str, str] = {}
    for region in workload_info.get("named_regions", []) or []:
        region_id = region.get("native_region_id")
        if region_id and not region.get("selectors"):
            wanted[str(region_id)] = str(region["name"])
    if not wanted:
        return {}
    tagged = loop_tagged_regions(ir)
    return {
        name: merge_named_region_counters(
            [
                (function, label, block_counter_summary(_without_poll_root_launders(body)))
                for function, label, body in tagged[region_id]
            ]
        )
        for region_id, name in wanted.items()
        if tagged.get(region_id)
    }


_ROOT_LAUNDER_RE = re.compile(r'= (?:tail )?call i64 asm "", "=r,0"\(')


def _without_poll_root_launders(body: str) -> str:
    """A block that runs the loop's GC safepoint poll reloads the frame's
    roots through launder asm (`asm "", "=r,0"`); LLVM folds a root it knows
    is a constant (a not-yet-initialized local is `undefined`) into an
    `inttoptr` constant operand. Those operands are the poll's root protocol,
    which the region allows (`js_gc_loop_safepoint`), not a conversion of the
    loop's own values."""
    if "@js_gc_loop_safepoint(" not in body:
        return body
    return "\n".join(line for line in body.splitlines() if not _ROOT_LAUNDER_RE.search(line))


def loop_block_keys(ir: str) -> set[tuple[str, str]]:
    """(function, label) of every block on a CFG cycle: the blocks a loop
    iteration can run."""
    keys: set[tuple[str, str]] = set()
    for name, blocks in _ir_functions(ir):
        cfg_successors = {
            label: [m.group(1) or m.group(2) for m in _TARGET_RE.finditer(_terminator(lines))]
            for label, lines in blocks
        }
        keys.update((name, label) for label in _cycle_blocks(cfg_successors))
    return keys
