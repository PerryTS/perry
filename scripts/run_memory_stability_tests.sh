#!/usr/bin/env bash
# Memory-stability regression suite.
#
# Two failure modes this catches that microbenchmarks miss:
#   1. Slow RSS accumulation in long-running programs (a real "2 GB
#      after an hour" leak that a 300 ms bench wouldn't surface).
#   2. Crashes when GC fires aggressively during sensitive ops
#      (parse, recursion, closure init, write barriers).
#
# How it works:
#   - test_memory_*.ts run a sustained allocate-and-discard loop
#     for 100k-200k iterations. RSS must stay under a per-test limit
#     (set ~50% above the current baseline). If a future change
#     pins blocks, leaks the parse-key cache, or breaks tenuring,
#     RSS climbs and the test fails.
#   - test_gc_*.ts force aggressive GC scheduling during sensitive
#     operations. Test passes ⟺ exit code 0 + correct stdout.
#   - PERRY_GC_TRACE=1 JSON lines are parsed for GC acceptance gates:
#     default-env copied-minor must report fallback_reason=none without
#     rebuilding the malloc registry, precise low-pressure runs must not pin bytes,
#     forced policy evacuation must move and release originals cleanly,
#     fallback reasons must remain explicit known values, and representative
#     traced workloads emit a copied-minor fallback evidence report.
#   - targeted low-pressure benchmarks are compiled into $TMPDIR and run
#     under /usr/bin/time:
#       $PERRY compile --no-cache benchmarks/suite/07_object_create.ts -o $TMPDIR/07_object_create
#       $PERRY compile --no-cache benchmarks/suite/12_binary_trees.ts -o $TMPDIR/12_binary_trees
#       $PERRY compile --no-cache benchmarks/suite/bench_gc_pressure.ts -o $TMPDIR/bench_gc_pressure
#     Gates: 07_object_create <= 10 ms / 64 MB RSS,
#            12_binary_trees <= 10 ms / 64 MB RSS,
#            bench_gc_pressure <= 80 ms / 128 MB RSS.
#
# Each test runs under FOUR GC mode combos:
#   - default (generational GC + generated write barriers)
#   - mark-sweep (PERRY_GEN_GC=0 — bisection escape hatch)
#   - explicit generational GC (PERRY_GEN_GC=1)
#   - force-evac+verify (default write barriers + forced evacuation verifier:
#     PERRY_GEN_GC_EVACUATE=1 PERRY_GC_FORCE_EVACUATE=1
#     PERRY_GC_VERIFY_EVACUATION=1)
# so a regression in any mode is caught.
#
# Usage:  scripts/run_memory_stability_tests.sh
# Exit:   0 on all pass, 1 on any failure.

set -euo pipefail

cd "$(dirname "$0")/.."

GC_EVIDENCE_ENABLED=0
GC_EVIDENCE_ROOT="${PERRY_GC_EVIDENCE_DIR:-}"
GC_EVIDENCE_LOG=""
GC_EVIDENCE_TRACE_COUNT=0
GC_EVIDENCE_ARTIFACTS=()
GC_EVIDENCE_TRACE_ARTIFACTS=()
GC_EVIDENCE_TRACE_SUMMARY_ARTIFACTS=()

ensure_output_parent() {
    local path="$1"
    if [[ -n "$path" ]]; then
        mkdir -p "$(dirname "$path")"
    fi
}

record_gc_evidence_artifact() {
    local path="$1"
    if [[ -n "$path" ]]; then
        GC_EVIDENCE_ARTIFACTS+=("$path")
    fi
}

sanitize_gc_evidence_name() {
    local value="$1"
    value=$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')
    value=$(printf '%s' "$value" | sed -E 's/[^a-z0-9._-]+/_/g; s/^_+//; s/_+$//')
    if [[ -z "$value" ]]; then
        value="trace"
    fi
    printf '%s' "$value"
}

init_gc_evidence_outputs() {
    if [[ -n "$GC_EVIDENCE_ROOT" ]]; then
        GC_EVIDENCE_ENABLED=1
        mkdir -p \
            "$GC_EVIDENCE_ROOT/reports" \
            "$GC_EVIDENCE_ROOT/logs" \
            "$GC_EVIDENCE_ROOT/traces" \
            "$GC_EVIDENCE_ROOT/trace-summaries"

        if [[ -z "${PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT:-}" ]]; then
            PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT="$GC_EVIDENCE_ROOT/reports/copied_minor_fallback_report.json"
        fi
        if [[ -z "${PERRY_TARGET_COLLECTOR_GATES_OUT:-}" ]]; then
            PERRY_TARGET_COLLECTOR_GATES_OUT="$GC_EVIDENCE_ROOT/reports/target_collector_gates_report.json"
        fi
        if [[ -z "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
            PERRY_TEST_SUMMARY_OUT="$GC_EVIDENCE_ROOT/reports/memory_stability_summary.json"
        fi

        GC_EVIDENCE_LOG="$GC_EVIDENCE_ROOT/logs/memory-stability.log"
        : >"$GC_EVIDENCE_LOG"
        record_gc_evidence_artifact "$GC_EVIDENCE_LOG"
        exec > >(tee -a "$GC_EVIDENCE_LOG") 2>&1
    fi

    ensure_output_parent "${PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT:-}"
    ensure_output_parent "${PERRY_TARGET_COLLECTOR_GATES_OUT:-}"
    ensure_output_parent "${PERRY_TEST_SUMMARY_OUT:-}"

    record_gc_evidence_artifact "${PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT:-}"
    record_gc_evidence_artifact "${PERRY_TARGET_COLLECTOR_GATES_OUT:-}"
    record_gc_evidence_artifact "${PERRY_TEST_SUMMARY_OUT:-}"
}

copy_gc_trace_evidence() {
    local group="$1"
    local label="$2"
    local trace_file="$3"
    LAST_EVIDENCE_TRACE_FILE=""

    if [[ "$GC_EVIDENCE_ENABLED" -ne 1 || -z "$trace_file" || ! -f "$trace_file" ]]; then
        return
    fi

    local safe_group safe_label dest_dir dest
    safe_group=$(sanitize_gc_evidence_name "$group")
    safe_label=$(sanitize_gc_evidence_name "$label")
    GC_EVIDENCE_TRACE_COUNT=$((GC_EVIDENCE_TRACE_COUNT + 1))
    dest_dir="$GC_EVIDENCE_ROOT/traces/$safe_group"
    mkdir -p "$dest_dir"
    dest=$(printf '%s/%03d_%s.log' "$dest_dir" "$GC_EVIDENCE_TRACE_COUNT" "$safe_label")

    if cp "$trace_file" "$dest"; then
        LAST_EVIDENCE_TRACE_FILE="$dest"
        GC_EVIDENCE_TRACE_ARTIFACTS+=("$dest")
        record_gc_evidence_artifact "$dest"
    else
        printf "  WARN [gc-evidence] failed to copy trace %s -> %s\n" "$trace_file" "$dest"
    fi
}

write_gc_trace_summary() {
    local group="$1"
    local label="$2"
    local assertion_mode="$3"
    local status="$4"
    local trace_file="$5"
    local copied_trace_file="$6"
    LAST_EVIDENCE_TRACE_SUMMARY_FILE=""

    if [[ "$GC_EVIDENCE_ENABLED" -ne 1 || -z "$trace_file" || ! -f "$trace_file" ]]; then
        return
    fi

    local safe_group safe_label dest_dir dest
    safe_group=$(sanitize_gc_evidence_name "$group")
    safe_label=$(sanitize_gc_evidence_name "$label")
    dest_dir="$GC_EVIDENCE_ROOT/trace-summaries/$safe_group"
    mkdir -p "$dest_dir"
    dest=$(printf '%s/%03d_%s.json' "$dest_dir" "$GC_EVIDENCE_TRACE_COUNT" "$safe_label")

    if "$PYTHON" - \
        "$label" "$assertion_mode" "$status" "$trace_file" "$copied_trace_file" "$dest" <<'PY'; then
import json
import sys
from pathlib import Path

label = sys.argv[1]
assertion_mode = sys.argv[2]
status = sys.argv[3]
trace_file = Path(sys.argv[4])
copied_trace_file = sys.argv[5]
out = Path(sys.argv[6])

known_fallback_reasons = (
    "none",
    "copy_only_roots",
    "barriers_inactive",
    "conservative_stack",
    "malloc_registry_unavailable",
    "pinned_young_root",
    "pinned_young_dirty_slot",
    "pinned_young_transitive",
    "not_attempted",
)
fallback_reason_counts = {reason: 0 for reason in known_fallback_reasons}


def nested(obj, *path, default=None):
    cur = obj
    for key in path:
        if not isinstance(cur, dict):
            return default
        cur = cur.get(key, default)
    return cur


def non_negative_int(value):
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        return 0
    return value


gc_cycle_count = 0
copied_bytes = 0
promoted_bytes = 0
moved_bytes = 0
old_page_moved_bytes = 0

with trace_file.open("r", encoding="utf-8", errors="replace") as fh:
    for line in fh:
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict) or event.get("event") != "gc_cycle":
            continue

        gc_cycle_count += 1
        copying_nursery = nested(event, "copying_nursery", default={})
        if not isinstance(copying_nursery, dict):
            copying_nursery = {}
        reason = copying_nursery.get("fallback_reason")
        if isinstance(reason, str):
            fallback_reason_counts.setdefault(reason, 0)
            fallback_reason_counts[reason] += 1
        else:
            fallback_reason_counts.setdefault("_missing", 0)
            fallback_reason_counts["_missing"] += 1

        copied_bytes += non_negative_int(copying_nursery.get("copied_bytes"))
        promoted_bytes += non_negative_int(copying_nursery.get("promoted_bytes"))
        moved_bytes += non_negative_int(nested(event, "evacuation", "moved_bytes", default=0))
        old_page_moved_bytes += non_negative_int(
            nested(event, "evacuation", "old_page_moved_bytes", default=0)
        )

summary = {
    "schema_version": 1,
    "workload_label": label,
    "assertion_mode": assertion_mode,
    "status": status,
    "source_trace_path": str(trace_file),
    "trace_path": copied_trace_file or str(trace_file),
    "gc_cycle_count": gc_cycle_count,
    "fallback_reason_counts": fallback_reason_counts,
    "copied_bytes": copied_bytes,
    "promoted_bytes": promoted_bytes,
    "moved_bytes": moved_bytes,
    "old_page_moved_bytes": old_page_moved_bytes,
    "byte_totals": {
        "copied_bytes": copied_bytes,
        "promoted_bytes": promoted_bytes,
        "moved_bytes": moved_bytes,
        "old_page_moved_bytes": old_page_moved_bytes,
    },
}

with out.open("w", encoding="utf-8") as fh:
    json.dump(summary, fh, indent=2)
    fh.write("\n")
PY
        LAST_EVIDENCE_TRACE_SUMMARY_FILE="$dest"
        GC_EVIDENCE_TRACE_SUMMARY_ARTIFACTS+=("$dest")
        record_gc_evidence_artifact "$dest"
    else
        printf "  WARN [gc-evidence] failed to summarize trace %s\n" "$trace_file"
    fi
}

record_gc_trace_evidence() {
    local group="$1"
    local label="$2"
    local assertion_mode="$3"
    local status="$4"
    local trace_file="$5"

    if [[ "$GC_EVIDENCE_ENABLED" -ne 1 || -z "$trace_file" || ! -f "$trace_file" ]]; then
        return
    fi

    copy_gc_trace_evidence "$group" "$label" "$trace_file"
    write_gc_trace_summary \
        "$group" "$label" "$assertion_mode" "$status" \
        "$trace_file" "$LAST_EVIDENCE_TRACE_FILE"
}

print_gc_evidence_artifacts() {
    if [[ "$GC_EVIDENCE_ENABLED" -ne 1 && ${#GC_EVIDENCE_ARTIFACTS[@]} -eq 0 ]]; then
        return
    fi

    echo ""
    echo "=== GC evidence artifacts ==="
    if [[ "$GC_EVIDENCE_ENABLED" -eq 1 ]]; then
        echo "  root: $GC_EVIDENCE_ROOT"
    fi
    if [[ -n "$GC_EVIDENCE_LOG" ]]; then
        echo "  log: $GC_EVIDENCE_LOG"
    fi
    if [[ -n "${PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT:-}" ]]; then
        echo "  copied-minor report: $PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT"
    fi
    if [[ -n "${PERRY_TARGET_COLLECTOR_GATES_OUT:-}" ]]; then
        echo "  target-collector report: $PERRY_TARGET_COLLECTOR_GATES_OUT"
    fi
    if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
        echo "  summary: $PERRY_TEST_SUMMARY_OUT"
    fi
    if [[ ${#GC_EVIDENCE_TRACE_ARTIFACTS[@]} -gt 0 ]]; then
        echo "  traces:"
        printf '    %s\n' "${GC_EVIDENCE_TRACE_ARTIFACTS[@]}"
    fi
    if [[ ${#GC_EVIDENCE_TRACE_SUMMARY_ARTIFACTS[@]} -gt 0 ]]; then
        echo "  trace summaries:"
        printf '    %s\n' "${GC_EVIDENCE_TRACE_SUMMARY_ARTIFACTS[@]}"
    fi
}

init_gc_evidence_outputs

cargo build --release -p perry-runtime -p perry-stdlib -p perry --quiet

PERRY=./target/release/perry
PYTHON=${PYTHON:-python3}
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Globals set by run_one. Bash makes it painful to return multiple
# values cleanly; globals beat parsing a single-line string.
LAST_RSS_MB=0
LAST_EXIT=0
LAST_STDOUT_FILE=""
LAST_STDERR_FILE=""
LAST_CANARY_EXIT=0
LAST_CANARY_OUTPUT_FILE=""
LAST_GC_TRACE_FILE=""
LAST_TRACE_ASSERT_STATUS=""
LAST_EVIDENCE_TRACE_FILE=""
LAST_EVIDENCE_TRACE_SUMMARY_FILE=""

# Run a compiled binary under /usr/bin/time. Cross-platform RSS read
# (macOS reports bytes, Linux reports KB).
run_one() {
    local bin="$1"
    shift  # remaining args are env VAR=val pairs

    LAST_STDOUT_FILE="$TMPDIR/stdout.$$.$RANDOM"
    LAST_STDERR_FILE="$TMPDIR/stderr.$$.$RANDOM"
    LAST_EXIT=0

    if [[ "$(uname)" == "Darwin" ]]; then
        env "$@" /usr/bin/time -l "$bin" >"$LAST_STDOUT_FILE" 2>"$LAST_STDERR_FILE" \
            || LAST_EXIT=$?
        local b
        b=$(awk '/maximum resident set size/ {print $1}' "$LAST_STDERR_FILE")
        b=${b:-0}
        LAST_RSS_MB=$((b / 1024 / 1024))
    else
        env "$@" /usr/bin/time -v "$bin" >"$LAST_STDOUT_FILE" 2>"$LAST_STDERR_FILE" \
            || LAST_EXIT=$?
        local kb
        kb=$(awk '/Maximum resident set size/ {print $NF}' "$LAST_STDERR_FILE")
        kb=${kb:-0}
        LAST_RSS_MB=$((kb / 1024))
    fi
}

# Compile once per GC mode. Generated write barriers are on by default;
# PERRY_WRITE_BARRIERS=0/off/false is the benchmark/debug escape hatch
# that suppresses barrier emission at compile time and disables runtime
# exact helper barriers.
PASS=0
FAIL=0

run_test() {
    local ts="$1"
    local rss_limit_mb="$2"
    local expect_substr="$3"
    local force_verify_rss_limit_mb="${4:-$rss_limit_mb}"

    local mode_specs=(
        "default||"
        "mark-sweep||PERRY_GEN_GC=0"
        "gen-gc-explicit||PERRY_GEN_GC=1"
        "force-evac+verify||PERRY_GEN_GC=1 PERRY_GEN_GC_EVACUATE=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1"
    )

    for spec in "${mode_specs[@]}"; do
        IFS='|' read -r mode_label compile_env_str env_str <<<"$spec"
        local effective_rss_limit_mb="$rss_limit_mb"
        if [[ "$mode_label" == "force-evac+verify" ]]; then
            effective_rss_limit_mb="$force_verify_rss_limit_mb"
        fi
        local bin="$TMPDIR/$(basename "${ts%.ts}")_${mode_label//[^A-Za-z0-9_]/_}"

        local compile_env_args=()
        if [[ -n "$compile_env_str" ]]; then
            # shellcheck disable=SC2206
            compile_env_args=($compile_env_str)
        fi
        if ! env "${compile_env_args[@]+"${compile_env_args[@]}"}" \
            $PERRY compile --no-cache "$ts" -o "$bin" >/dev/null 2>&1; then
            printf "  FAIL [%-18s] %-40s compile failed\n" "$mode_label" "$(basename "$ts")"
            FAIL=$((FAIL + 1))
            continue
        fi

        # Split env_str on spaces into argv tokens (an empty string
        # gives env zero args, which is fine).
        local env_args=()
        if [[ -n "$env_str" ]]; then
            # shellcheck disable=SC2206
            env_args=($env_str)
        fi

        # `"${env_args[@]+"${env_args[@]}"}"` is the safe-expand
        # idiom under `set -u`: empty array → no args, non-empty →
        # quoted expansion.
        run_one "$bin" "${env_args[@]+"${env_args[@]}"}"

        local status="PASS"
        local reason=""

        if [[ "$LAST_EXIT" -ne 0 ]]; then
            status="FAIL"
            reason="exit=$LAST_EXIT"
        elif [[ "$LAST_RSS_MB" -gt "$effective_rss_limit_mb" ]]; then
            status="FAIL"
            reason="rss=${LAST_RSS_MB}MB > limit=${effective_rss_limit_mb}MB"
        elif [[ -n "$expect_substr" ]] && ! grep -qF "$expect_substr" "$LAST_STDOUT_FILE"; then
            status="FAIL"
            reason="stdout missing: $expect_substr"
        fi

        printf "  %s [%-18s] %-40s rss=%3dMB / limit=%3dMB %s\n" \
            "$status" "$mode_label" "$(basename "$ts")" \
            "$LAST_RSS_MB" "$effective_rss_limit_mb" "$reason"

        if [[ "$status" == "PASS" ]]; then
            PASS=$((PASS + 1))
        else
            FAIL=$((FAIL + 1))
        fi
    done
}

run_canary() {
    local label="$1"
    shift

    LAST_CANARY_OUTPUT_FILE="$TMPDIR/canary.$$.$RANDOM"
    LAST_CANARY_EXIT=0

    "$@" >"$LAST_CANARY_OUTPUT_FILE" 2>&1 || LAST_CANARY_EXIT=$?

    if [[ "$LAST_CANARY_EXIT" -eq 0 ]]; then
        printf "  PASS [canary] %-40s\n" "$label"
        PASS=$((PASS + 1))
    else
        printf "  FAIL [canary] %-40s exit=%d\n" "$label" "$LAST_CANARY_EXIT"
        sed 's/^/    /' "$LAST_CANARY_OUTPUT_FILE"
        FAIL=$((FAIL + 1))
    fi
}

assert_gc_trace() {
    local label="$1"
    local trace_file="$2"
    local mode="$3"
    local output_file="$TMPDIR/gc_trace_assert.$$.$RANDOM"
    LAST_TRACE_ASSERT_STATUS="fail"

    if "$PYTHON" - "$mode" "$trace_file" >"$output_file" 2>&1 <<'PY'; then
import json
import sys

mode = sys.argv[1]
trace_path = sys.argv[2]

allowed_fallback_reasons = {
    "none",
    "copy_only_roots",
    "barriers_inactive",
    "conservative_stack",
    "malloc_registry_unavailable",
    "pinned_young_root",
    "pinned_young_dirty_slot",
    "pinned_young_transitive",
    "not_attempted",
}


def nested(obj, *path, default=None):
    cur = obj
    for key in path:
        if not isinstance(cur, dict):
            return default
        cur = cur.get(key, default)
    return cur


cycles = []
with open(trace_path, "r", encoding="utf-8", errors="replace") as fh:
    for line in fh:
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("event") == "gc_cycle":
            cycles.append(event)

errors = []
if not cycles:
    errors.append("no gc_cycle JSON events found")

for idx, cycle in enumerate(cycles):
    reason = nested(cycle, "copying_nursery", "fallback_reason")
    eligible = nested(cycle, "copying_nursery", "eligible")
    shadow_roots = cycle.get("shadow_roots")
    root_sources = cycle.get("root_sources")
    layout_scans = cycle.get("layout_scans")
    if reason not in allowed_fallback_reasons:
        errors.append(f"cycle {idx}: unexpected fallback_reason={reason!r}")
    if not isinstance(eligible, bool):
        errors.append(f"cycle {idx}: copying_nursery.eligible={eligible!r}, want bool")
    elif reason == "none" and eligible is not True:
        errors.append(f"cycle {idx}: eligible={eligible!r} with fallback_reason='none'")
    elif reason != "none" and eligible is not False:
        errors.append(f"cycle {idx}: eligible={eligible!r} with fallback_reason={reason!r}")
    if not isinstance(shadow_roots, dict):
        errors.append(f"cycle {idx}: shadow_roots missing or not an object")
    else:
        for field in ("slots_scanned", "nonzero_slots", "pointer_roots", "rewritten_slots"):
            value = shadow_roots.get(field)
            if not isinstance(value, int) or value < 0:
                errors.append(f"cycle {idx}: shadow_roots.{field}={value!r}, want non-negative int")
        slots = shadow_roots.get("slots_scanned", -1)
        nonzero = shadow_roots.get("nonzero_slots", -1)
        pointers = shadow_roots.get("pointer_roots", -1)
        rewritten = shadow_roots.get("rewritten_slots", -1)
        if isinstance(slots, int) and isinstance(nonzero, int) and nonzero > slots:
            errors.append(f"cycle {idx}: shadow_roots.nonzero_slots={nonzero} > slots_scanned={slots}")
        if isinstance(nonzero, int) and isinstance(pointers, int) and pointers > nonzero:
            errors.append(f"cycle {idx}: shadow_roots.pointer_roots={pointers} > nonzero_slots={nonzero}")
        if isinstance(pointers, int) and isinstance(rewritten, int) and rewritten > pointers:
            errors.append(f"cycle {idx}: shadow_roots.rewritten_slots={rewritten} > pointer_roots={pointers}")
    if not isinstance(root_sources, dict):
        errors.append(f"cycle {idx}: root_sources missing or not an object")
    else:
        for source in (
            "compiled_shadow",
            "module_globals",
            "runtime_handles",
            "runtime_mutable_scanners",
            "ffi_mutable_scanners",
        ):
            stats = root_sources.get(source)
            if not isinstance(stats, dict):
                errors.append(f"cycle {idx}: root_sources.{source} missing or not an object")
                continue
            for field in (
                "registered_scanners",
                "slots_scanned",
                "nonzero_slots",
                "pointer_roots",
                "rewritten_slots",
            ):
                value = stats.get(field)
                if not isinstance(value, int) or value < 0:
                    errors.append(
                        f"cycle {idx}: root_sources.{source}.{field}={value!r}, "
                        "want non-negative int"
                    )
            slots = stats.get("slots_scanned", -1)
            nonzero = stats.get("nonzero_slots", -1)
            pointers = stats.get("pointer_roots", -1)
            rewritten = stats.get("rewritten_slots", -1)
            if isinstance(slots, int) and isinstance(nonzero, int) and nonzero > slots:
                errors.append(
                    f"cycle {idx}: root_sources.{source}.nonzero_slots={nonzero} "
                    f"> slots_scanned={slots}"
                )
            if isinstance(nonzero, int) and isinstance(pointers, int) and pointers > nonzero:
                errors.append(
                    f"cycle {idx}: root_sources.{source}.pointer_roots={pointers} "
                    f"> nonzero_slots={nonzero}"
                )
            if isinstance(pointers, int) and isinstance(rewritten, int) and rewritten > pointers:
                errors.append(
                    f"cycle {idx}: root_sources.{source}.rewritten_slots={rewritten} "
                    f"> pointer_roots={pointers}"
                )
        native = root_sources.get("native_stack_fallback")
        if not isinstance(native, dict):
            errors.append(f"cycle {idx}: root_sources.native_stack_fallback missing or not an object")
        else:
            decision = native.get("decision")
            if decision not in ("scan", "skip_disabled", "skip_shadow_stack_active"):
                errors.append(
                    f"cycle {idx}: root_sources.native_stack_fallback.decision={decision!r}"
                )
            if not isinstance(native.get("scanned"), bool):
                errors.append(
                    f"cycle {idx}: root_sources.native_stack_fallback.scanned="
                    f"{native.get('scanned')!r}, want bool"
                )
            for field in (
                "roots_found",
                "pinned_roots",
                "pinned_bytes",
                "compiled_frame_pinned_roots",
                "compiled_frame_pinned_bytes",
            ):
                value = native.get(field)
                if not isinstance(value, int) or value < 0:
                    errors.append(
                        f"cycle {idx}: root_sources.native_stack_fallback.{field}="
                        f"{value!r}, want non-negative int"
                    )
    if not isinstance(layout_scans, dict):
        errors.append(f"cycle {idx}: layout_scans missing or not an object")
    else:
        for field in (
            "pointer_slots_read",
            "masked_pointer_slots_read",
            "unknown_layout_slots_read",
            "pointer_free_ranges_skipped",
            "pointer_free_slots_skipped",
            "raw_numeric_array_ranges_skipped",
            "raw_numeric_array_slots_skipped",
            "raw_numeric_object_field_ranges_skipped",
            "raw_numeric_object_field_slots_skipped",
        ):
            value = layout_scans.get(field)
            if not isinstance(value, int) or value < 0:
                errors.append(f"cycle {idx}: layout_scans.{field}={value!r}, want non-negative int")
        pointer_slots = layout_scans.get("pointer_slots_read", -1)
        masked_slots = layout_scans.get("masked_pointer_slots_read", -1)
        unknown_slots = layout_scans.get("unknown_layout_slots_read", -1)
        pointer_free_ranges = layout_scans.get("pointer_free_ranges_skipped", -1)
        pointer_free_slots = layout_scans.get("pointer_free_slots_skipped", -1)
        if (
            isinstance(pointer_slots, int)
            and isinstance(masked_slots, int)
            and isinstance(unknown_slots, int)
            and masked_slots + unknown_slots > pointer_slots
        ):
            errors.append(
                f"cycle {idx}: layout_scans masked+unknown={masked_slots + unknown_slots} "
                f"> pointer_slots_read={pointer_slots}"
            )
        if (
            isinstance(pointer_free_ranges, int)
            and isinstance(pointer_free_slots, int)
            and pointer_free_ranges > 0
            and pointer_free_slots == 0
        ):
            errors.append(f"cycle {idx}: pointer-free ranges skipped but slots skipped is zero")

if mode in ("copied_minor_precise", "copied_minor_default"):
    for idx, cycle in enumerate(cycles):
        if cycle.get("collection_kind") != "minor":
            errors.append(f"cycle {idx}: collection_kind={cycle.get('collection_kind')!r}, want 'minor'")
        reason = nested(cycle, "copying_nursery", "fallback_reason")
        eligible = nested(cycle, "copying_nursery", "eligible")
        rebuilds = nested(cycle, "copying_nursery", "malloc_registry_rebuilds", default=-1)
        conservative_pinned_bytes = cycle.get("conservative_pinned_bytes", -1)
        legacy_pinned_bytes = nested(
            cycle, "legacy_copy_only_scanner_pinned", "bytes", default=-1
        )
        native_pinned_bytes = nested(
            cycle, "root_sources", "native_stack_fallback", "pinned_bytes", default=-1
        )
        compiled_frame_pinned_bytes = nested(
            cycle,
            "root_sources",
            "native_stack_fallback",
            "compiled_frame_pinned_bytes",
            default=-1,
        )
        if reason != "none":
            errors.append(f"cycle {idx}: fallback_reason={reason!r}, want 'none'")
        if eligible is not True:
            errors.append(f"cycle {idx}: eligible={eligible!r}, want true")
        if rebuilds != 0:
            errors.append(f"cycle {idx}: malloc_registry_rebuilds={rebuilds}, want 0")
        if conservative_pinned_bytes != 0:
            errors.append(
                f"cycle {idx}: conservative_pinned_bytes={conservative_pinned_bytes}, want 0"
            )
        if legacy_pinned_bytes != 0:
            errors.append(
                f"cycle {idx}: legacy_copy_only_scanner_pinned.bytes={legacy_pinned_bytes}, want 0"
            )
        if native_pinned_bytes != 0:
            errors.append(
                f"cycle {idx}: root_sources.native_stack_fallback.pinned_bytes="
                f"{native_pinned_bytes}, want 0"
            )
        if compiled_frame_pinned_bytes != 0:
            errors.append(
                f"cycle {idx}: root_sources.native_stack_fallback."
                f"compiled_frame_pinned_bytes={compiled_frame_pinned_bytes}, want 0"
            )
    copied_productive = [
        cycle
        for cycle in cycles
        if nested(cycle, "copying_nursery", "copied_objects", default=0)
        + nested(cycle, "copying_nursery", "promoted_objects", default=0)
        > 0
    ]
    if not copied_productive:
        errors.append("no copied-minor trace copied or promoted any object")
    nonzero_shadow_roots = [
        nested(cycle, "shadow_roots", "nonzero_slots", default=0) for cycle in cycles
    ]
    if not nonzero_shadow_roots:
        errors.append("copied-minor trace did not report shadow_roots.nonzero_slots")
    elif nonzero_shadow_roots[-1] > nonzero_shadow_roots[0]:
        errors.append(
            "shadow_roots.nonzero_slots grew across copied-minor probe: "
            f"{nonzero_shadow_roots}"
        )
    mutable_source_evidence = [
        sum(
            nested(cycle, "root_sources", source, "pointer_roots", default=0)
            + nested(cycle, "root_sources", source, "rewritten_slots", default=0)
            for source in (
                "compiled_shadow",
                "module_globals",
                "runtime_handles",
                "runtime_mutable_scanners",
                "ffi_mutable_scanners",
            )
        )
        for cycle in cycles
    ]
    if not mutable_source_evidence or max(mutable_source_evidence) == 0:
        errors.append("copied-minor trace did not report mutable root source evidence")
elif mode == "evacuation_productive":
    productive = [
        cycle
        for cycle in cycles
        if nested(cycle, "evacuation_policy", "enabled") is True
        and nested(cycle, "evacuation", "moved_bytes", default=0) > 0
    ]
    if not productive:
        errors.append("no policy-enabled evacuation moved bytes")
    for idx, cycle in enumerate(productive):
        moved_bytes = nested(cycle, "evacuation", "moved_bytes", default=-1)
        released_bytes = nested(cycle, "evacuation", "released_original_bytes", default=-2)
        moved_objects = nested(cycle, "evacuation", "moved_objects", default=-1)
        released_objects = nested(cycle, "evacuation", "released_original_objects", default=-2)
        retained_bytes = nested(cycle, "evacuation", "retained_forwarded_stub_bytes", default=-1)
        retained_objects = nested(
            cycle, "evacuation", "retained_forwarded_stub_objects", default=-1
        )
        sweep_retained_bytes = nested(cycle, "sweep", "retained_forwarded_stub_bytes", default=-1)
        sweep_retained_objects = nested(
            cycle, "sweep", "retained_forwarded_stub_objects", default=-1
        )
        if moved_bytes != released_bytes:
            errors.append(
                f"productive evacuation {idx}: moved_bytes={moved_bytes}, "
                f"released_original_bytes={released_bytes}"
            )
        if moved_objects != released_objects:
            errors.append(
                f"productive evacuation {idx}: moved_objects={moved_objects}, "
                f"released_original_objects={released_objects}"
            )
        if retained_bytes != 0 or retained_objects != 0:
            errors.append(
                f"productive evacuation {idx}: retained forwarding stubs "
                f"bytes={retained_bytes} objects={retained_objects}"
            )
        if sweep_retained_bytes != 0 or sweep_retained_objects != 0:
            errors.append(
                f"productive evacuation {idx}: sweep retained forwarding stubs "
                f"bytes={sweep_retained_bytes} objects={sweep_retained_objects}"
            )
elif mode == "barriers_inactive":
    matches = [
        cycle
        for cycle in cycles
        if nested(cycle, "copying_nursery", "fallback_reason") == "barriers_inactive"
        and nested(cycle, "evacuation_policy", "reason") == "barriers_inactive"
    ]
    if not matches:
        errors.append("no trace reported barriers_inactive for copying and evacuation policy")
    for idx, cycle in enumerate(matches):
        if nested(cycle, "copying_nursery", "eligible") is not False:
            errors.append(f"barriers-inactive trace {idx}: copied-minor unexpectedly eligible")
        if nested(cycle, "evacuation_policy", "enabled") is not False:
            errors.append(f"barriers-inactive trace {idx}: evacuation policy unexpectedly enabled")
        if nested(cycle, "evacuation", "moved_bytes", default=-1) != 0:
            errors.append(f"barriers-inactive trace {idx}: evacuation moved bytes")
elif mode != "fallback_reasons":
    errors.append(f"unknown assertion mode {mode!r}")

if errors:
    print("\n".join(errors))
    sys.exit(1)

print(f"validated {len(cycles)} gc_cycle event(s)")
PY
        local detail
        detail=$(tr '\n' ' ' <"$output_file" | sed 's/[[:space:]]*$//')
        printf "  PASS [gc-trace] %-40s %s\n" "$label" "$detail"
        PASS=$((PASS + 1))
        LAST_TRACE_ASSERT_STATUS="pass"
    else
        printf "  FAIL [gc-trace] %-40s\n" "$label"
        sed 's/^/    /' "$output_file"
        FAIL=$((FAIL + 1))
        LAST_TRACE_ASSERT_STATUS="fail"
    fi
}

run_gc_trace_probe() {
    local ts="$TMPDIR/default_copied_minor_churn.ts"
    local bin="$TMPDIR/default_copied_minor_churn"
    local compile_output="$TMPDIR/default_copied_minor_churn_compile.$$.$RANDOM"
    LAST_GC_TRACE_FILE=""

    cat >"$ts" <<'EOF'
declare function gc(): void;

function smallBlob(i: number): string {
  return JSON.stringify({ id: i, name: "small_" + i, value: i * 7 });
}

function largeBlob(i: number): string {
  const items: any[] = [];
  for (let j = 0; j < 18; j++) {
    items.push({
      id: i * 18 + j,
      name: "item_" + j,
      nested: { x: j, y: j * 2 },
    });
  }
  return JSON.stringify(items);
}

function churnBatch(base: number): number {
  let checksum = 0;
  for (let k = 0; k < 64; k++) {
    const i = base + k;
    const s: any = JSON.parse(smallBlob(i));
    const l: any = JSON.parse(largeBlob(i));
    const shortText = "s" + (i % 9);
    const name = "record_" + i + "_value_" + (i * 3);
    const obj: any = { id: i, left: s, right: l[0], n: shortText.length + name.length };
    checksum += s.id + l.length + l[0].id + obj.n;
  }
  return checksum;
}

function copiedProbe(i: number): number {
  const live: any[] = [];
  live.push(i);
  live.push(i + 1);
  live.push(i + 2);
  gc();
  return i + 3;
}

function main(): number {
  let checksum = 0;
  for (let batch = 0; batch < 10; batch++) {
    checksum += churnBatch(batch * 64);
    checksum += copiedProbe(batch * 64);
  }
  return checksum;
}

const result = main();
console.log("default_copied_minor_churn:" + result);
EOF

    if ! $PERRY compile --no-cache "$ts" -o "$bin" >"$compile_output" 2>&1; then
        printf "  FAIL [gc-trace] %-40s compile failed\n" "default copied minor churn"
        sed 's/^/    /' "$compile_output"
        FAIL=$((FAIL + 1))
        return
    fi

    run_one "$bin" PERRY_GC_TRACE=1
    LAST_GC_TRACE_FILE="$LAST_STDERR_FILE"

    if [[ "$LAST_EXIT" -ne 0 ]]; then
        printf "  FAIL [gc-trace] %-40s exit=%d\n" "default copied minor churn" "$LAST_EXIT"
        sed 's/^/    /' "$LAST_STDERR_FILE"
        record_gc_trace_evidence \
            "gc-trace" "default copied minor churn" "copied_minor_default" "fail" \
            "$LAST_GC_TRACE_FILE"
        FAIL=$((FAIL + 1))
        return
    fi
    if ! grep -qF "default_copied_minor_churn:3913788" "$LAST_STDOUT_FILE"; then
        printf "  FAIL [gc-trace] %-40s stdout mismatch\n" "default copied minor churn"
        sed 's/^/    /' "$LAST_STDOUT_FILE"
        record_gc_trace_evidence \
            "gc-trace" "default copied minor churn" "copied_minor_default" "fail" \
            "$LAST_GC_TRACE_FILE"
        FAIL=$((FAIL + 1))
        return
    fi

    assert_gc_trace "default copied minor churn" "$LAST_STDERR_FILE" "copied_minor_default"
    record_gc_trace_evidence \
        "gc-trace" "default copied minor churn" "copied_minor_default" \
        "$LAST_TRACE_ASSERT_STATUS" "$LAST_GC_TRACE_FILE"
}

write_copied_minor_fallback_workloads() {
    local out_dir="$1"

    cat >"$out_dir/json_roundtrip.ts" <<'EOF'
declare function gc(): void;

function payload(i: number): string {
  const items: any[] = [];
  for (let j = 0; j < 24; j++) {
    items.push({
      id: i * 24 + j,
      name: "item_" + j + "_for_" + i,
      nested: { x: j, y: j * 3 },
    });
  }
  return JSON.stringify({
    id: i,
    route: "/api/items/" + i,
    ok: (i % 2) === 0,
    items,
  });
}

let checksum = 0;
for (let batch = 0; batch < 8; batch++) {
  for (let k = 0; k < 80; k++) {
    const i = batch * 80 + k;
    const parsed: any = JSON.parse(payload(i));
    const reshaped = JSON.stringify({
      id: parsed.id,
      first: parsed.items[0].name,
      count: parsed.items.length,
      lastY: parsed.items[23].nested.y,
    });
    const again: any = JSON.parse(reshaped);
    checksum += again.id + again.count + again.first.length + again.lastY;
  }
  gc();
}

console.log("json_roundtrip:" + checksum);
EOF

    cat >"$out_dir/string_churn.ts" <<'EOF'
declare function gc(): void;

function label(i: number): string {
  return "request_" + i + "_tenant_" + (i % 17) + "_segment_" + (i % 5);
}

let total = 0;
for (let batch = 0; batch < 10; batch++) {
  for (let k = 0; k < 400; k++) {
    const i = batch * 400 + k;
    const shortText = "s" + (i % 9);
    const mediumText = "field_" + i;
    const longText = label(i) + "_payload_" + (i * 13);
    const combined = shortText + "|" + mediumText + "|" + longText;
    total += shortText.length + mediumText.length + longText.length + combined.length;
    if ((i % 4) === 0) {
      total += ("copy_" + combined).length;
    }
  }
  gc();
}

console.log("string_churn:" + total);
EOF

    cat >"$out_dir/object_property_churn.ts" <<'EOF'
declare function gc(): void;

function makeRecord(i: number): any {
  return {
    id: i,
    name: "record_" + i,
    status: (i % 3) === 0 ? "open" : "closed",
    nested: { count: i * 3, tag: "tag_" + (i % 11) },
    a: i + 1,
    b: i + 2,
  };
}

let checksum = 0;
for (let batch = 0; batch < 10; batch++) {
  for (let k = 0; k < 300; k++) {
    const i = batch * 300 + k;
    const record: any = makeRecord(i);
    const slot = "slot_" + (i % 4);
    record.lastSeen = "tick_" + i;
    record.score = record.a + record.b + record.nested.count;
    record[slot] = record.score + i;
    const copy: any = {
      id: record.id,
      name: record.name,
      score: record.score,
      slotValue: record[slot],
      tag: record.nested.tag,
    };
    checksum += copy.id + copy.name.length + copy.score + copy.slotValue + copy.tag.length;
  }
  gc();
}

console.log("object_property_churn:" + checksum);
EOF

    cat >"$out_dir/mixed_request_shaping.ts" <<'EOF'
declare function gc(): void;

function makeRequest(i: number): any {
  return {
    method: (i % 2) === 0 ? "GET" : "POST",
    url: "/v1/accounts/" + (i % 31) + "/events/" + i,
    headers: {
      requestId: "req_" + i,
      tenant: "tenant_" + (i % 7),
    },
    body: JSON.stringify({
      id: i,
      amount: i * 9,
      labels: ["alpha_" + i, "beta_" + (i % 13), "stable"],
    }),
  };
}

function handle(req: any): any {
  const body: any = JSON.parse(req.body);
  const responseBody = {
    ok: true,
    id: body.id,
    route: req.url,
    method: req.method,
    labels: body.labels,
    tenant: req.headers.tenant,
  };
  return {
    status: 200 + (body.id % 3),
    requestId: req.headers.requestId,
    body: JSON.stringify(responseBody),
  };
}

let checksum = 0;
for (let batch = 0; batch < 8; batch++) {
  for (let k = 0; k < 96; k++) {
    const i = batch * 96 + k;
    const response: any = handle(makeRequest(i));
    const parsed: any = JSON.parse(response.body);
    checksum += response.status + response.requestId.length + parsed.id;
    checksum += parsed.route.length + parsed.labels.length + parsed.tenant.length;
  }
  gc();
}

console.log("mixed_request_shaping:" + checksum);
EOF

    cat >"$out_dir/map_set_churn.ts" <<'EOF'
declare function gc(): void;

let checksum = 0;
for (let batch = 0; batch < 8; batch++) {
  const retained = new Map<number, Set<any>>();
  for (let i = 0; i < 160; i++) {
    const map = new Map<any, any>();
    const set = new Set<any>();
    for (let j = 0; j < 12; j++) {
      const key = "key_" + batch + "_" + i + "_" + j;
      map.set(j, key);
      map.set(key, i + j);
      set.add(j);
      set.add(key);
    }
    const gotText: any = map.get(3);
    const gotNumber: any = map.get("key_" + batch + "_" + i + "_7");
    checksum += gotText.length;
    checksum += gotNumber;
    checksum += set.has("key_" + batch + "_" + i + "_5") ? 17 : 0;
    checksum += set.size;
    if ((i % 40) === 0) {
      retained.set(i, set);
    }
  }
  checksum += retained.size;
  gc();
}

console.log("map_set_churn:" + checksum);
EOF

    cat >"$out_dir/promise_churn.ts" <<'EOF'
declare function gc(): void;

let checksum = 0;
let rootedPromiseValues: number[] | null = null;

async function churn(batch: number): Promise<void> {
  const tasks: Promise<number>[] = [];
  for (let i = 0; i < 160; i++) {
    const base = batch * 1000 + i;
    tasks.push(Promise.resolve(base)
      .then((v: number): number => v + 1)
      .then((v: number): number => v * 2));
  }
  await Promise.all(tasks).then((values: number[]): void => {
    rootedPromiseValues = values;
    gc();
    values = rootedPromiseValues as number[];
    for (let i = 0; i < values.length; i++) {
      checksum += values[i] + (i % 7);
    }
    rootedPromiseValues = null;
  });
}

for (let batch = 0; batch < 8; batch++) {
  await churn(batch);
}

console.log("promise_churn:" + checksum);
EOF
}

run_copied_minor_fallback_workload() {
    local name="$1"
    local ts="$2"
    local bin="$TMPDIR/${name}_copied_minor_fallback"
    local compile_output="$TMPDIR/${name}_copied_minor_fallback_compile.$$.$RANDOM"
    LAST_GC_TRACE_FILE=""

    if ! $PERRY compile --no-cache "$ts" -o "$bin" >"$compile_output" 2>&1; then
        printf "  FAIL [gc-trace] %-40s compile failed\n" "$name"
        sed 's/^/    /' "$compile_output"
        FAIL=$((FAIL + 1))
        return 1
    fi

    run_one "$bin" PERRY_GC_TRACE=1
    LAST_GC_TRACE_FILE="$LAST_STDERR_FILE"

    if [[ "$LAST_EXIT" -ne 0 ]]; then
        printf "  FAIL [gc-trace] %-40s exit=%d\n" "$name" "$LAST_EXIT"
        sed 's/^/    /' "$LAST_STDERR_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi
    if ! grep -q "^${name}:" "$LAST_STDOUT_FILE"; then
        printf "  FAIL [gc-trace] %-40s stdout missing workload marker\n" "$name"
        sed 's/^/    /' "$LAST_STDOUT_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi

    printf "  PASS [gc-trace] %-40s trace=%s\n" "$name" "$LAST_STDERR_FILE"
    PASS=$((PASS + 1))
    return 0
}

run_copied_minor_fallback_report() {
    local workloads_dir="$TMPDIR/copied_minor_fallback_workloads"
    mkdir -p "$workloads_dir"
    write_copied_minor_fallback_workloads "$workloads_dir"

    local workload_specs=(
        "json_roundtrip:$workloads_dir/json_roundtrip.ts"
        "string_churn:$workloads_dir/string_churn.ts"
        "object_property_churn:$workloads_dir/object_property_churn.ts"
        "mixed_request_shaping:$workloads_dir/mixed_request_shaping.ts"
        "map_set_churn:$workloads_dir/map_set_churn.ts"
        "promise_churn:$workloads_dir/promise_churn.ts"
    )

    local report_args=()
    local trace_names=()
    local trace_files=()
    local workload_failed=0
    local spec
    for spec in "${workload_specs[@]}"; do
        local name="${spec%%:*}"
        local ts="${spec#*:}"
        if run_copied_minor_fallback_workload "$name" "$ts"; then
            report_args+=(--workload "$name=$LAST_STDERR_FILE")
            trace_names+=("$name")
            trace_files+=("$LAST_STDERR_FILE")
        else
            workload_failed=1
            record_gc_trace_evidence \
                "copied-minor-fallback" "$name" "strict_fallback_evidence" "fail" \
                "$LAST_GC_TRACE_FILE"
        fi
    done

    if [[ "$workload_failed" -ne 0 ]]; then
        local i
        for i in "${!trace_names[@]}"; do
            record_gc_trace_evidence \
                "copied-minor-fallback" "${trace_names[$i]}" \
                "strict_fallback_evidence" "blocked" "${trace_files[$i]}"
        done
        return
    fi

    local report_out="${PERRY_COPIED_MINOR_FALLBACK_REPORT_OUT:-$TMPDIR/copied_minor_fallback_report.json}"
    local parser_output="$TMPDIR/copied_minor_fallback_report_parser.$$.$RANDOM"
    local report_status="fail"
    if "$PYTHON" scripts/copied_minor_fallback_report.py \
        "${report_args[@]}" \
        --strict-fallback-evidence \
        --out "$report_out" >"$parser_output" 2>&1; then
        report_status="pass"
        local top_summary
        top_summary=$("$PYTHON" - "$report_out" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    report = json.load(fh)
top = report.get("top_remaining_reason")
if top is None:
    print("top_remaining_reason=none")
else:
    print(f"top_remaining_reason={top['reason']} count={top['count']}")
PY
)
        printf "  PASS [gc-trace] %-40s %s report=%s\n" \
            "copied-minor fallback evidence" "$top_summary" "$report_out"
        PASS=$((PASS + 1))
    else
        printf "  FAIL [gc-trace] %-40s\n" "copied-minor fallback evidence"
        sed 's/^/    /' "$parser_output"
        if [[ -f "$report_out" ]]; then
            printf "    report=%s\n" "$report_out"
        fi
        FAIL=$((FAIL + 1))
    fi

    local i
    for i in "${!trace_names[@]}"; do
        record_gc_trace_evidence \
            "copied-minor-fallback" "${trace_names[$i]}" \
            "strict_fallback_evidence" "$report_status" "${trace_files[$i]}"
    done
}

write_target_collector_gate_workloads() {
    local out_dir="$1"

    cat >"$out_dir/default_copying.ts" <<'EOF'
declare function gc(): void;

let total = 0;
let keep: any[] = [];
for (let batch = 0; batch < 8; batch++) {
  keep = [];
  for (let i = 0; i < 64; i++) {
    const child: any = { value: batch * 100 + i, next: { score: i * 3 + batch } };
    keep.push(child);
  }
  total += keep.length + keep[0].value + keep[63].next.score;
  gc();
}

console.log("default_copying:" + total);
EOF

    cat >"$out_dir/string_heavy.ts" <<'EOF'
declare function gc(): void;

let total = 0;
let keep: string[] = [];
for (let batch = 0; batch < 4; batch++) {
  keep = [];
  let text = "heap-string-seed-" + batch + "-abcdefghijklmnopqrstuvwxyz";
  for (let i = 0; i < 21; i++) {
    text = text + "|" + batch + ":" + i + ":payload";
    if ((i % 3) === 0) {
      keep.push(text);
    }
    total += text.length + keep.length;
  }
  gc();
  for (let i = 0; i < keep.length; i++) {
    total += keep[i].length % 97;
  }
  total -= keep.length;
}

console.log("string_heavy:" + total);
EOF

    cat >"$out_dir/closure_heavy.ts" <<'EOF'
declare function gc(): void;

function makeAdder(base: number, bias: any): (x: number) => number {
  return (x: number): number => base + bias.value + x;
}

let total = 0;
let keep: ((x: number) => number)[] = [];
for (let batch = 0; batch < 5; batch++) {
  keep = [];
  for (let i = 0; i < 140; i++) {
    const base = batch * 100 + i;
    const bias: any = { value: (base % 13) + batch };
    keep.push(makeAdder(base, bias));
  }
  gc();
  for (let i = 0; i < keep.length; i++) {
    total += keep[i](i);
  }
  total += batch * 27;
}

console.log("closure_heavy:" + total);
EOF

    cat >"$out_dir/async_promise_closures.ts" <<'EOF'
declare function gc(): void;

let rootedWorkers: (() => Promise<number>)[] | null = null;

async function main(): Promise<number> {
  let total = 0;
  for (let batch = 0; batch < 4; batch++) {
    let workers: (() => Promise<number>)[] = [];
    for (let i = 0; i < 30; i++) {
      const captured = batch * 20 + i;
      const worker = async (): Promise<number> => {
        const first = captured + 1;
        const second = first * 2 + batch;
        return second + captured;
      };
      workers.push(worker);
    }
    rootedWorkers = workers;
    gc();
    workers = rootedWorkers as (() => Promise<number>)[];
    const tasks: Promise<number>[] = [];
    for (let i = 0; i < workers.length; i++) {
      tasks.push(workers[i]());
    }
    rootedWorkers = null;
    const results = await Promise.all(tasks);
    for (let i = 0; i < results.length; i++) {
      total += results[i] + i;
    }
    total += batch * 60;
  }
  return total;
}

const result = await main();
gc();
console.log("async_promise_closures:" + result);
EOF

    cat >"$out_dir/large_object_barriers.ts" <<'EOF'
declare function gc(): void;

let checksum = 0;
let holders: any[] = [];
for (let batch = 0; batch < 4; batch++) {
  const child: any = { value: batch + 10 };
  const parent: any[] = [];
  for (let i = 0; i < 5000; i++) {
    parent.push(i + batch);
  }
  parent[123] = child;
  holders = [parent];
  gc();
  checksum += holders[0][123].value + holders[0].length + holders[0][4000];
}

console.log("large_object_barriers:" + checksum);
EOF
}

run_target_collector_gate_workload() {
    local name="$1"
    local ts="$2"
    local expected_result="$3"
    shift 3
    local bin="$TMPDIR/${name}_target_collector_gate"
    local compile_output="$TMPDIR/${name}_target_collector_gate_compile.$$.$RANDOM"
    LAST_GC_TRACE_FILE=""

    if ! $PERRY compile --no-cache "$ts" -o "$bin" >"$compile_output" 2>&1; then
        printf "  FAIL [target-gc] %-40s compile failed\n" "$name"
        sed 's/^/    /' "$compile_output"
        FAIL=$((FAIL + 1))
        return 1
    fi

    run_one "$bin" PERRY_GC_TRACE=1 "$@"
    LAST_GC_TRACE_FILE="$LAST_STDERR_FILE"

    if [[ "$LAST_EXIT" -ne 0 ]]; then
        printf "  FAIL [target-gc] %-40s exit=%d\n" "$name" "$LAST_EXIT"
        sed 's/^/    /' "$LAST_STDERR_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi
    local expected_output="${name}:${expected_result}"
    local actual_output
    actual_output=$(while IFS= read -r line; do
        if [[ "$line" == "${name}:"* ]]; then
            printf "%s\n" "$line"
        fi
    done <"$LAST_STDOUT_FILE")

    if [[ -z "$actual_output" ]]; then
        printf "  FAIL [target-gc] %-40s stdout missing workload marker\n" "$name"
        sed 's/^/    /' "$LAST_STDOUT_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi
    if [[ "$actual_output" != "$expected_output" ]]; then
        printf "  FAIL [target-gc] %-40s stdout mismatch\n" "$name"
        printf "    expected: %s\n" "$expected_output"
        printf "    stdout:\n"
        sed 's/^/      /' "$LAST_STDOUT_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi

    printf "  PASS [target-gc] %-40s trace=%s\n" "$name" "$LAST_STDERR_FILE"
    PASS=$((PASS + 1))
    return 0
}

run_target_collector_old_page_trace() {
    local label="old_page_forced_defrag"
    LAST_CANARY_OUTPUT_FILE="$TMPDIR/${label}.$$.$RANDOM"
    LAST_CANARY_EXIT=0

    env PERRY_GC_TRACE=1 PERRY_GC_FORCE_EVACUATE=1 \
        cargo test -p perry-runtime --release \
        test_old_page_defrag_target_gate_emits_trace -- --nocapture \
        >"$LAST_CANARY_OUTPUT_FILE" 2>&1 || LAST_CANARY_EXIT=$?

    if [[ "$LAST_CANARY_EXIT" -eq 0 ]]; then
        printf "  PASS [target-gc] %-40s trace=%s\n" "$label" "$LAST_CANARY_OUTPUT_FILE"
        PASS=$((PASS + 1))
        return 0
    fi

    printf "  FAIL [target-gc] %-40s exit=%d\n" "$label" "$LAST_CANARY_EXIT"
    sed 's/^/    /' "$LAST_CANARY_OUTPUT_FILE"
    FAIL=$((FAIL + 1))
    return 1
}

run_target_collector_pointer_free_trace() {
    local label="pointer_free_numeric"
    LAST_CANARY_OUTPUT_FILE="$TMPDIR/${label}.$$.$RANDOM"
    LAST_CANARY_EXIT=0

    env PERRY_GC_TRACE=1 \
        cargo test -p perry-runtime --release \
        test_pointer_free_target_gate_emits_trace -- --nocapture \
        >"$LAST_CANARY_OUTPUT_FILE" 2>&1 || LAST_CANARY_EXIT=$?

    if [[ "$LAST_CANARY_EXIT" -eq 0 ]]; then
        printf "  PASS [target-gc] %-40s trace=%s\n" "$label" "$LAST_CANARY_OUTPUT_FILE"
        PASS=$((PASS + 1))
        return 0
    fi

    printf "  FAIL [target-gc] %-40s exit=%d\n" "$label" "$LAST_CANARY_EXIT"
    sed 's/^/    /' "$LAST_CANARY_OUTPUT_FILE"
    FAIL=$((FAIL + 1))
    return 1
}

run_raw_numeric_object_fields_codegen_semantics() {
    local label="raw numeric object field semantics"
    local bin="$TMPDIR/raw_numeric_object_fields_codegen_semantics"
    local compile_output="$TMPDIR/raw_numeric_object_fields_compile.$$.$RANDOM"
    local expected_output="$TMPDIR/raw_numeric_object_fields_expected.$$.$RANDOM"
    local diff_output="$TMPDIR/raw_numeric_object_fields_diff.$$.$RANDOM"

    if ! $PERRY compile --no-cache tests/raw_numeric_object_fields.ts -o "$bin" \
        >"$compile_output" 2>&1; then
        printf "  FAIL [target-gc] %-40s compile failed\n" "$label"
        sed 's/^/    /' "$compile_output"
        FAIL=$((FAIL + 1))
        return 1
    fi

    run_one "$bin"

    if [[ "$LAST_EXIT" -ne 0 ]]; then
        printf "  FAIL [target-gc] %-40s exit=%d\n" "$label" "$LAST_EXIT"
        sed 's/^/    /' "$LAST_STDERR_FILE"
        FAIL=$((FAIL + 1))
        return 1
    fi

    printf '%s\n' \
        '{"x":3.5,"y":4.25,"negZero":0,"nan":null,"wide":2147483648.5}' \
        'true' \
        'true' \
        '2147483648.5' \
        '7.75' \
        '{"x":3.5,"y":4.25,"negZero":0,"nan":null,"wide":2147483648.5}' \
        '6.25' \
        '{"x":{"label":"callback"},"y":6.25,"negZero":0,"nan":null,"wide":2147483648.5}' \
        'callback' \
        '{"value":7.75}' \
        '{"value":{"label":"boxed"}}' \
        '3.75' \
        '{"value":{"label":"class-transition"},"other":"boxed"}' \
        'class-transition' \
        '{"value":"abc","other":"boxed"}' \
        'abc' \
        'frozen-write-error' \
        '9.5' \
        'sealed-extra-error' \
        '{"value":12.5}' \
        'prevent-extra-error' \
        '{"value":13.5}' \
        '44' \
        '15.5' >"$expected_output"

    if ! diff -u "$expected_output" "$LAST_STDOUT_FILE" >"$diff_output"; then
        printf "  FAIL [target-gc] %-40s stdout mismatch\n" "$label"
        sed 's/^/    /' "$diff_output"
        FAIL=$((FAIL + 1))
        return 1
    fi

    printf "  PASS [target-gc] %-40s\n" "$label"
    PASS=$((PASS + 1))
    return 0
}

run_target_collector_architecture_gates() {
    local workloads_dir="$TMPDIR/target_collector_gate_workloads"
    mkdir -p "$workloads_dir"
    write_target_collector_gate_workloads "$workloads_dir"

    local workload_specs=(
        "default_copying|4852|$workloads_dir/default_copying.ts"
        "string_heavy|17028|$workloads_dir/string_heavy.ts"
        "closure_heavy|243150|$workloads_dir/closure_heavy.ts"
        "async_promise_closures|18540|$workloads_dir/async_promise_closures.ts|PERRY_GEN_GC_EVACUATE=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1"
        "large_object_barriers|36052|$workloads_dir/large_object_barriers.ts"
        "raw_numeric_layouts|71|benchmarks/compiler_output/fixtures/raw_numeric_layout_smoke.ts"
    )

    local report_args=()
    local trace_names=()
    local trace_files=()
    local workload_failed=0
    local spec
    for spec in "${workload_specs[@]}"; do
        local name expected_result ts runtime_env_str
        IFS='|' read -r name expected_result ts runtime_env_str <<<"$spec"
        local runtime_env_args=()
        if [[ -n "$runtime_env_str" ]]; then
            # shellcheck disable=SC2206
            runtime_env_args=($runtime_env_str)
        fi
        if run_target_collector_gate_workload \
            "$name" "$ts" "$expected_result" \
            "${runtime_env_args[@]+"${runtime_env_args[@]}"}"; then
            report_args+=(--workload "$name=$LAST_STDERR_FILE")
            trace_names+=("$name")
            trace_files+=("$LAST_STDERR_FILE")
        else
            workload_failed=1
            record_gc_trace_evidence \
                "target-collector" "$name" "target_collector_gates" "fail" \
                "$LAST_GC_TRACE_FILE"
        fi
    done

    if run_target_collector_pointer_free_trace; then
        report_args+=(--workload "pointer_free_numeric=$LAST_CANARY_OUTPUT_FILE")
        trace_names+=("pointer_free_numeric")
        trace_files+=("$LAST_CANARY_OUTPUT_FILE")
    else
        workload_failed=1
        record_gc_trace_evidence \
            "target-collector" "pointer_free_numeric" "target_collector_gates" "fail" \
            "$LAST_CANARY_OUTPUT_FILE"
    fi

    if run_target_collector_old_page_trace; then
        report_args+=(--workload "old_page_forced_defrag=$LAST_CANARY_OUTPUT_FILE")
        trace_names+=("old_page_forced_defrag")
        trace_files+=("$LAST_CANARY_OUTPUT_FILE")
    else
        workload_failed=1
        record_gc_trace_evidence \
            "target-collector" "old_page_forced_defrag" "target_collector_gates" "fail" \
            "$LAST_CANARY_OUTPUT_FILE"
    fi

    if [[ "$workload_failed" -ne 0 ]]; then
        local i
        for i in "${!trace_names[@]}"; do
            record_gc_trace_evidence \
                "target-collector" "${trace_names[$i]}" \
                "target_collector_gates" "blocked" "${trace_files[$i]}"
        done
        return
    fi

    local report_out="${PERRY_TARGET_COLLECTOR_GATES_OUT:-$TMPDIR/target_collector_gates_report.json}"
    local parser_output="$TMPDIR/target_collector_gates_parser.$$.$RANDOM"
    local report_status="fail"
    if "$PYTHON" scripts/copied_minor_fallback_report.py \
        "${report_args[@]}" \
        --target-collector-gates \
        --out "$report_out" >"$parser_output" 2>&1; then
        report_status="pass"
        local gate_summary
        gate_summary=$("$PYTHON" - "$report_out" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    report = json.load(fh)
summary = report["summary"]
copying = summary["copying_nursery"]
layout = summary["layout_scans"]
old_page = summary["old_page_accounting"]
print(
    "copied_or_promoted="
    f"{copying['copied_objects'] + copying['promoted_objects']} "
    f"pointer_free_skipped={layout['pointer_free_slots_skipped']} "
    f"large_excluded={copying['large_excluded_objects']} "
    f"old_page_moved_bytes={old_page['old_page_moved_bytes']} "
    f"old_page_dead_bytes={old_page['dead_bytes']} "
    f"old_page_reusable_bytes={old_page['reusable_bytes']} "
    f"old_page_returned_bytes={old_page['returned_bytes']}"
)
PY
)
        printf "  PASS [target-gc] %-40s %s report=%s\n" \
            "architecture stress gates" "$gate_summary" "$report_out"
        PASS=$((PASS + 1))
    else
        printf "  FAIL [target-gc] %-40s\n" "architecture stress gates"
        sed 's/^/    /' "$parser_output"
        if [[ -f "$report_out" ]]; then
            printf "    report=%s\n" "$report_out"
        fi
        FAIL=$((FAIL + 1))
    fi

    local i
    for i in "${!trace_names[@]}"; do
        record_gc_trace_evidence \
            "target-collector" "${trace_names[$i]}" \
            "target_collector_gates" "$report_status" "${trace_files[$i]}"
    done
}

run_traced_canary() {
    local label="$1"
    local mode="$2"
    shift 2

    run_canary "$label" "$@"
    if [[ "$LAST_CANARY_EXIT" -eq 0 ]]; then
        assert_gc_trace "$label" "$LAST_CANARY_OUTPUT_FILE" "$mode"
        record_gc_trace_evidence \
            "traced-canary" "$label" "$mode" "$LAST_TRACE_ASSERT_STATUS" \
            "$LAST_CANARY_OUTPUT_FILE"
    else
        record_gc_trace_evidence \
            "traced-canary" "$label" "$mode" "fail" "$LAST_CANARY_OUTPUT_FILE"
    fi
}

run_benchmark_gate() {
    local ts="$1"
    local time_limit_ms="$2"
    local rss_limit_mb="$3"
    local name
    name=$(basename "${ts%.ts}")
    local bin="$TMPDIR/$name"
    local compile_output="$TMPDIR/${name}_compile.$$.$RANDOM"

    if ! $PERRY compile --no-cache "$ts" -o "$bin" >"$compile_output" 2>&1; then
        printf "  FAIL [bench] %-28s compile failed\n" "$name"
        sed 's/^/    /' "$compile_output"
        FAIL=$((FAIL + 1))
        return
    fi

    run_one "$bin"

    local timing
    timing=$(awk -F: '/^[[:alnum:]_]+:[0-9]+$/ {print $1 ":" $2; exit}' "$LAST_STDOUT_FILE")
    local elapsed_ms=""
    local timing_label=""
    if [[ -n "$timing" ]]; then
        timing_label="${timing%%:*}"
        elapsed_ms="${timing##*:}"
    fi

    local status="PASS"
    local reason=""
    if [[ "$LAST_EXIT" -ne 0 ]]; then
        status="FAIL"
        reason="exit=$LAST_EXIT"
    elif [[ -z "$elapsed_ms" ]]; then
        status="FAIL"
        reason="stdout missing benchmark timing"
    elif [[ "$elapsed_ms" -gt "$time_limit_ms" ]]; then
        status="FAIL"
        reason="time=${elapsed_ms}ms > limit=${time_limit_ms}ms"
    elif [[ "$LAST_RSS_MB" -gt "$rss_limit_mb" ]]; then
        status="FAIL"
        reason="rss=${LAST_RSS_MB}MB > limit=${rss_limit_mb}MB"
    fi

    printf "  %s [bench] %-28s %-16s time=%3sms / limit=%3sms rss=%3dMB / limit=%3dMB %s\n" \
        "$status" "$name" "$timing_label" "${elapsed_ms:-NA}" "$time_limit_ms" \
        "$LAST_RSS_MB" "$rss_limit_mb" "$reason"

    if [[ "$status" == "PASS" ]]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Memory-leak regression tests (RSS plateau under sustained alloc) ==="
# Limits ~50-70% above measured baseline on macOS arm64. CI runners
# may differ slightly; loosen a limit here rather than in the .ts.
run_test test-files/test_memory_long_lived_loop.ts 100 "done, lastId=199999"
# JSON churn — widened to 290 / 315 MB for the GC rework window under
# #1090. Observed Ubuntu CI on the v0.5.1024 release-packages run:
# default 268 MB, gen-gc 268 MB, force-evac+verify 288 MB — the same
# Linux glibc + RSS-accounting gap that prompted Ralph's prior 200→250
# bump in v0.5.842 (f95ef059). 290/315 was already set in v0.5.1022
# (#1286 / 4fcfddb9) but PR #1324 ("Port GC checkpoint runtime work for
# #1090", e933b893) reverted the ceiling back to 250/275 while writing
# new comments referencing macOS arm64 baselines that don't apply on
# Ubuntu CI. Restore the wider ceiling here; revisit + tighten when
# #1090 closes.
run_test test-files/test_memory_json_churn.ts      290 "done, checksum=637747500" 315
run_test test-files/test_memory_string_churn.ts    100 "done, total=9577780"
run_test test-files/test_memory_closure_churn.ts    50 "done, sum=15004649874"
# #1790: 1,000,000 create-and-discard class-EXPRESSION objects
# (OBJECT_TYPE_CLASS). Per-evaluation class objects with no subclass must NOT
# be retained by any side-table — RSS must plateau (measured ~106 MB macOS
# arm64 across all four modes; limit set with headroom for Linux CI's higher
# RSS accounting, mirroring the json_churn note above).
run_test test-files/test_memory_class_object_churn.ts 200 "done, kept=999999 hasSink=true" 230

echo ""
echo "=== GC-aggression regression tests (no crash + correct result) ==="
run_test test-files/test_gc_aggressive_forced.ts    50 "done, acc=8022890"
run_test test-files/test_gc_deep_recursion.ts       30 "done, result=320400"
# #1790: `class Sub extends make(...)` parent class object survives forced
# evacuation — inherited mixed pointer/scalar static fields + static methods
# (incl. a 2-level chain) must still resolve via the rewritten side-table ptr.
run_test test-files/test_gc_class_object_forced.ts  50 "done: alpha|alpha|7|7|d:alpha:7|mid|d:mid:3"

echo ""
echo "=== Server GC unsafe-zone regression tests ==="
run_canary "issue #1425 Fastify/ws manual gc" \
    tests/test_issue_1425_gc_unsafe_zones.sh

echo ""
echo "=== Forced-evacuation verifier canaries ==="
run_canary "evacuation verifier surfaces" \
    cargo test -p perry-runtime --release test_evacuation_verify
run_canary "barriers inactive force-evac gate" \
    env PERRY_WRITE_BARRIERS=0 PERRY_GC_FORCE_EVACUATE=1 \
    cargo test -p perry-runtime --release test_forced_evacuation_barriers_inactive_does_not_forward_candidate
run_canary "old parent remembers young child" \
    env PERRY_GC_FORCE_EVACUATE=1 \
    cargo test -p perry-runtime --release test_evacuated_old_parent_re_remembers_young_child_canary

echo ""
echo "=== GC acceptance telemetry (PERRY_GC_TRACE=1 JSON gates) ==="
run_gc_trace_probe
run_traced_canary "barriers inactive telemetry" "barriers_inactive" \
    env PERRY_GC_TRACE=1 PERRY_WRITE_BARRIERS=0 PERRY_GC_FORCE_EVACUATE=1 \
    cargo test -p perry-runtime --release test_forced_evacuation_barriers_inactive_does_not_forward_candidate -- --nocapture
run_traced_canary "productive evacuation telemetry" "evacuation_productive" \
    env PERRY_GC_TRACE=1 PERRY_GC_FORCE_EVACUATE=1 \
    cargo test -p perry-runtime --release test_evacuated_old_parent_re_remembers_young_child_canary -- --nocapture

echo ""
echo "=== Target collector architecture gates ==="
run_canary "copying minor rewrites" \
    cargo test -p perry-runtime --release test_copying_minor_rewrites
run_canary "malloc kind telemetry" \
    cargo test -p perry-runtime --release test_malloc_kind_telemetry
run_canary "old page accounting/defrag" \
    cargo test -p perry-runtime --release test_old_page_
run_canary "layout mask canaries" \
    cargo test -p perry-runtime --release test_layout_mask
run_canary "typed shape descriptor canaries" \
    cargo test -p perry-runtime --release test_typed_shape_descriptor
run_canary "unboxed object canaries" \
    cargo test -p perry-runtime --release test_unboxed_object
run_canary "typed/unboxed codegen layout installers" \
    cargo test -p perry-codegen --release --test typed_shape_descriptor --test typed_shape_descriptors -- --test-threads=1
run_raw_numeric_object_fields_codegen_semantics
run_canary "managed string allocation" \
    cargo test -p perry-runtime --release test_small_js_string_alloc_uses_managed_nursery_page
run_canary "managed closure allocation" \
    cargo test -p perry-runtime --release test_small_js_closure_alloc_uses_managed_nursery_page
run_target_collector_architecture_gates

echo ""
echo "=== Copied-minor fallback evidence report ==="
run_copied_minor_fallback_report

echo ""
echo "=== Targeted low-pressure benchmark gates ==="
echo "  Commands: $PERRY compile --no-cache <benchmark.ts> -o \$TMPDIR/<name>; /usr/bin/time <binary>"
echo "  Thresholds: 07_object_create <= 10ms/64MB, 12_binary_trees <= 10ms/64MB, bench_gc_pressure <= 80ms/128MB"
run_benchmark_gate benchmarks/suite/07_object_create.ts 10 64
run_benchmark_gate benchmarks/suite/12_binary_trees.ts 10 64
run_benchmark_gate benchmarks/suite/bench_gc_pressure.ts 80 128

echo ""
echo "=== Summary ==="
echo "  PASS: $PASS"
echo "  FAIL: $FAIL"

# release_sweep.sh hook — see comment in run_parity_tests.sh.
if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_memory_stability_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

print_gc_evidence_artifacts

if [[ $FAIL -ne 0 ]]; then
    exit 1
fi
