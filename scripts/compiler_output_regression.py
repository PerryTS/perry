#!/usr/bin/env python3
"""Compatibility CLI for the compiler-output regression harness."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from compiler_output_harness.analyzers import (
    benchmark_summary,
    block_counter_summary,
    call_names,
    count_calls_by_name,
    disassemble_object,
    extract_blocks,
    hot_loop_blocks,
    hot_region_counters,
    merge_region_counters,
    named_hot_regions,
    parse_kept_paths,
    parse_target_triple,
    parse_vectorization_remarks,
    percentile,
    region_counters,
    run_benchmark,
    run_perf_stat,
    runtime_call_names,
    runtime_counter_summary,
    structural_counters,
    summarize_gc_trace,
)
from compiler_output_harness.capture import (
    capture,
    compiler_version,
    resolve_benchmark_runs,
    resolve_clang,
    resolve_perry,
    verify_existing,
)
from compiler_output_harness.cli import build_parser, main
from compiler_output_harness.common import (
    DEFAULT_BENCHMARK_RUNS,
    BUFFER_SLOW_PATH_HELPERS,
    DYNAMIC_PROPERTY_HELPERS,
    REPO_ROOT,
    RUNTIME_CALL_PREFIXES,
    SCHEMA_VERSION,
    CommandResult,
    HarnessError,
    relpath,
    run_command,
    utc_now,
    write_text,
)
from compiler_output_harness.spec import (
    DEFAULT_SPEC_PATH,
    SPEC,
    WORKLOADS,
    load_workload_spec,
    validate_workload_spec,
)
from compiler_output_harness.verification import (
    named_region_contract_results,
    runtime_budget_results,
    should_expect_fma,
    target_supports_fma,
    vectorization_expectation,
    verify_artifacts,
)


if __name__ == "__main__":
    raise SystemExit(main())
