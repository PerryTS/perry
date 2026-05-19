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
#     and fallback reasons must remain explicit known values.
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

    local mode_specs=(
        "default||"
        "mark-sweep||PERRY_GEN_GC=0"
        "gen-gc-explicit||PERRY_GEN_GC=1"
        "force-evac+verify||PERRY_GEN_GC=1 PERRY_GEN_GC_EVACUATE=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1"
    )

    for spec in "${mode_specs[@]}"; do
        IFS='|' read -r mode_label compile_env_str env_str <<<"$spec"
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
        elif [[ "$LAST_RSS_MB" -gt "$rss_limit_mb" ]]; then
            status="FAIL"
            reason="rss=${LAST_RSS_MB}MB > limit=${rss_limit_mb}MB"
        elif [[ -n "$expect_substr" ]] && ! grep -qF "$expect_substr" "$LAST_STDOUT_FILE"; then
            status="FAIL"
            reason="stdout missing: $expect_substr"
        fi

        printf "  %s [%-18s] %-40s rss=%3dMB / limit=%3dMB %s\n" \
            "$status" "$mode_label" "$(basename "$ts")" \
            "$LAST_RSS_MB" "$rss_limit_mb" "$reason"

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
    else
        printf "  FAIL [gc-trace] %-40s\n" "$label"
        sed 's/^/    /' "$output_file"
        FAIL=$((FAIL + 1))
    fi
}

run_gc_trace_probe() {
    local ts="$TMPDIR/default_copied_minor_churn.ts"
    local bin="$TMPDIR/default_copied_minor_churn"
    local compile_output="$TMPDIR/default_copied_minor_churn_compile.$$.$RANDOM"

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

    if [[ "$LAST_EXIT" -ne 0 ]]; then
        printf "  FAIL [gc-trace] %-40s exit=%d\n" "default copied minor churn" "$LAST_EXIT"
        sed 's/^/    /' "$LAST_STDERR_FILE"
        FAIL=$((FAIL + 1))
        return
    fi
    if ! grep -qF "default_copied_minor_churn:3913788" "$LAST_STDOUT_FILE"; then
        printf "  FAIL [gc-trace] %-40s stdout mismatch\n" "default copied minor churn"
        sed 's/^/    /' "$LAST_STDOUT_FILE"
        FAIL=$((FAIL + 1))
        return
    fi

    assert_gc_trace "default copied minor churn" "$LAST_STDERR_FILE" "copied_minor_default"
}

run_traced_canary() {
    local label="$1"
    local mode="$2"
    shift 2

    run_canary "$label" "$@"
    if [[ "$LAST_CANARY_EXIT" -eq 0 ]]; then
        assert_gc_trace "$label" "$LAST_CANARY_OUTPUT_FILE" "$mode"
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
run_test test-files/test_memory_json_churn.ts      250 "done, checksum=637747500"
run_test test-files/test_memory_string_churn.ts    100 "done, total=9577780"
run_test test-files/test_memory_closure_churn.ts    50 "done, sum=15004649874"

echo ""
echo "=== GC-aggression regression tests (no crash + correct result) ==="
run_test test-files/test_gc_aggressive_forced.ts    50 "done, acc=8022890"
run_test test-files/test_gc_deep_recursion.ts       30 "done, result=320400"

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

if [[ $FAIL -ne 0 ]]; then
    exit 1
fi
