#!/usr/bin/env bash
# JSON polyglot benchmark runner.
#
# Each language is listed TWICE — once with idiomatic / default flags,
# once with aggressive / optimized flags — so a skeptical reader can
# see both the "what most projects ship with" number and the "what's
# achievable with full tuning" number.
#
# Workload: 10k records, ~1 MB blob, parse + stringify, 50 iterations.
#
# Methodology (v0.5.243+):
#   RUNS=11 (default; configurable via $RUNS env var). For each cell
#   we collect every per-run wall-clock ms and emit median, p95,
#   stddev (σ), min, and max — not "best-of-N" — so noise and outlier
#   sensitivity are visible. RSS is captured per-run via /usr/bin/time
#   (peak RSS, not average): macOS BSD `time -l` reports bytes, Linux
#   GNU `time -v` reports kilobytes. We normalize both to bytes and
#   report the max observed peak across runs.
#
# CPU pinning:
#   macOS: taskpolicy -t 0 -l 0 (sets throughput-tier 0 + latency-tier 0
#          — a scheduler HINT toward P-cores on Apple Silicon, NOT
#          strict pinning. Apple does not expose unprivileged hard
#          affinity. -c user-interactive doesn't exist; that flag only
#          accepts downgrade clamps utility/background/maintenance.)
#   Linux: taskset -c 0 (strict pinning to CPU 0).
#   Otherwise: no pinning, with a caveat banner at run start.
#
# Toolchains expected on PATH:
#   perry  (built locally — falls back to ../../target/release/perry)
#   bun, node
#   go, cargo, swiftc, clang++
#   nlohmann/json (brew install nlohmann-json on macOS)
#   kotlinc + gradle (brew install kotlin gradle)
#
# Outputs:
#   RESULTS.md — markdown table sorted by median time

set -uo pipefail
cd "$(dirname "$0")"

PERRY_ROOT=$(cd ../.. && pwd)
PERRY=${PERRY:-../../target/release/perry}
RUNS=${RUNS:-11}
PUBLIC_JSON_OUT="${PUBLIC_BENCH_JSON_OUT:-}"
TMPDIR=$(mktemp -d)
KEEP=${KEEP:-0}
[[ "${1:-}" == "--keep" ]] && KEEP=1
[[ "$KEEP" -eq 0 ]] && trap 'rm -rf "$TMPDIR"' EXIT

NLOHMANN_INCLUDE=$(brew --prefix nlohmann-json 2>/dev/null || echo "")/include

have() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------------------
# Node-side TS stripper. Node measurements run on precompiled .mjs so we
# don't charge Node for `--experimental-strip-types`'s per-launch parse +
# strip cost (Perry compiles AOT and Bun strips natively as part of its
# value prop, so neither pays this; Node otherwise would). The strip is a
# setup step, untimed. esbuild is preferred (fast, pure type stripping);
# tsc works as a fallback.
# ---------------------------------------------------------------------------
NODE_TS_STRIP=""
NODE_TS_STRIP_NOTE=""
if have esbuild; then
    NODE_TS_STRIP="esbuild"
    NODE_TS_STRIP_NOTE="esbuild on PATH"
elif have npx && npx --no-install esbuild --version >/dev/null 2>&1; then
    NODE_TS_STRIP="npx-esbuild"
    NODE_TS_STRIP_NOTE="npx esbuild (project-local)"
elif have tsc; then
    NODE_TS_STRIP="tsc"
    NODE_TS_STRIP_NOTE="tsc on PATH"
fi

# Compile <src.ts> -> <dst.mjs>. Returns 0 on success, 1 if no stripper is
# available or the strip failed. This is a SETUP step — never inside the
# timed window.
precompile_to_mjs() {
    local src="$1"
    local dst="$2"
    case "$NODE_TS_STRIP" in
        esbuild)
            esbuild "$src" --format=esm --platform=neutral --target=esnext \
                --outfile="$dst" --log-level=warning >/dev/null 2>&1
            ;;
        npx-esbuild)
            npx --no-install esbuild "$src" --format=esm --platform=neutral \
                --target=esnext --outfile="$dst" --log-level=warning >/dev/null 2>&1
            ;;
        tsc)
            local d b
            d=$(dirname "$dst")
            b=$(basename "$src" .ts)
            tsc --target esnext --module esnext --moduleResolution bundler \
                --outDir "$d" "$src" >/dev/null 2>&1
            [[ -f "$d/${b}.js" ]] && mv "$d/${b}.js" "$dst"
            ;;
        *)
            return 1
            ;;
    esac
    [[ -f "$dst" ]]
}

# ---------------------------------------------------------------------------
# CPU pinning detection. Sets PIN_CMD (array, may be empty) and PIN_NOTE.
# ---------------------------------------------------------------------------
PIN_CMD=()
PIN_NOTE=""
case "$(uname)" in
    Darwin)
        if have taskpolicy; then
            PIN_CMD=(taskpolicy -t 0 -l 0)
            PIN_NOTE="macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)"
        else
            PIN_NOTE="macOS without taskpolicy — no pinning available"
        fi
        ;;
    Linux)
        if have taskset; then
            PIN_CMD=(taskset -c 0)
            PIN_NOTE="Linux strict (taskset -c 0)"
        else
            PIN_NOTE="Linux without taskset — no pinning available"
        fi
        ;;
    *)
        PIN_NOTE="Unknown platform $(uname) — no pinning attempted"
        ;;
esac

# ---------------------------------------------------------------------------
# /usr/bin/time selection. macOS ships BSD time (`-l`, RSS in bytes,
# "maximum resident set size" lowercase, value in column 1). Linux ships
# GNU time from coreutils (`-v`, RSS in KB, "Maximum resident set size
# (kbytes): N" with the value after the colon). We normalize both to
# bytes so the rest of run_bench() doesn't care which platform produced
# the line. If /usr/bin/time isn't usable (rare on Linux distros that
# don't install the `time` package — bash's builtin doesn't accept
# -l/-v), we run the workload directly and report 0 RSS.
# ---------------------------------------------------------------------------
TIME_BIN="/usr/bin/time"
TIME_FLAG=""
case "$(uname)" in
    Darwin)
        TIME_FLAG="-l"
        rss_extract() { grep "maximum resident set size" "$1" 2>/dev/null | awk '{print $1}'; }
        ;;
    Linux)
        TIME_FLAG="-v"
        rss_extract() { grep "Maximum resident set size" "$1" 2>/dev/null | awk -F': ' '{print $2 * 1024}'; }
        ;;
    *)
        rss_extract() { echo 0; }
        ;;
esac
if [[ -z "$TIME_FLAG" ]] || [[ ! -x "$TIME_BIN" ]]; then
    TIME_BIN=""
    TIME_FLAG=""
    rss_extract() { echo 0; }
fi

echo "==============================================================="
echo "JSON polyglot benchmark — Perry v$(grep '^version' ../../Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
echo "Hardware: $(uname -srm) on $(hostname -s)"
echo "Runs per cell: $RUNS (median + p95 + σ + min + max reported)"
echo "Pinning strategy: $PIN_NOTE"
if [[ -n "$NODE_TS_STRIP_NOTE" ]]; then
    echo "Node TS strip:   $NODE_TS_STRIP_NOTE (precompile to .mjs, untimed)"
else
    echo "Node TS strip:   none found — falling back to --experimental-strip-types"
    echo "                 (charges Node for runtime TS stripping; install esbuild or tsc)"
fi
echo "==============================================================="
echo

# Results accumulator — TSV: median|label|profile|p95|stddev|min|max|rss_mb
RESULTS_FILE="$TMPDIR/results.tsv"
: > "$RESULTS_FILE"
RAW_RESULTS_FILE="$TMPDIR/public-results.tsv"
: > "$RAW_RESULTS_FILE"

# Compute median, p95, stddev, min, max from a list of integer ms values.
# Reads values one per line on stdin. Outputs single line:
#   "median|p95|stddev|min|max"
# stddev is population stddev (n divisor), one decimal.
compute_stats() {
    awk '
    { v[NR] = $1 + 0 }
    END {
        n = NR
        if (n == 0) { print "0|0|0|0|0"; exit }
        # sort ascending (insertion sort — fine for n=11..30)
        for (i = 2; i <= n; i++) {
            x = v[i]; j = i - 1
            while (j >= 1 && v[j] > x) { v[j+1] = v[j]; j-- }
            v[j+1] = x
        }
        # median
        if (n % 2 == 1) { median = v[(n+1)/2] }
        else { median = (v[n/2] + v[n/2+1]) / 2 }
        # p95: index = ceil(0.95 * n) (1-based)
        p95_idx = int(0.95 * n + 0.99999)
        if (p95_idx > n) p95_idx = n
        if (p95_idx < 1) p95_idx = 1
        p95 = v[p95_idx]
        # mean + stddev (population)
        sum = 0
        for (i = 1; i <= n; i++) sum += v[i]
        mean = sum / n
        ss = 0
        for (i = 1; i <= n; i++) ss += (v[i] - mean) ^ 2
        stddev = sqrt(ss / n)
        printf "%d|%d|%.1f|%d|%d", median, p95, stddev, v[1], v[n]
    }'
}

# Run a binary RUNS times under the pinning prefix, collect all ms values
# and the worst-case RSS, write a row to RESULTS_FILE plus a stdout line.
#
# Usage: run_bench <workload> <profile> <label> <env-string> <command...>
#   workload: "roundtrip" or "field_access" — used to group rows in RESULTS.md.
run_bench() {
    local workload="$1"; shift
    local profile="$1"; shift
    local label="$1"; shift
    local env_str="$1"; shift
    local samples_file="$TMPDIR/samples.$$"
    local checksums_file="$TMPDIR/checksums.$$"
    : > "$samples_file"
    : > "$checksums_file"
    local worst_rss=0
    local i
    # Build the time-wrapper prefix once per call. Empty when /usr/bin/time
    # isn't available on this platform (workload runs directly, RSS = 0).
    local time_prefix=()
    [[ -n "$TIME_BIN" ]] && time_prefix=("$TIME_BIN" "$TIME_FLAG")
    for i in $(seq 1 "$RUNS"); do
        local stderr_file="$TMPDIR/stderr.$$.$i"
        local out
        if [[ -n "$env_str" && -n "${PIN_CMD[*]-}" ]]; then
            out=$(env $env_str "${PIN_CMD[@]}" "${time_prefix[@]}" "$@" 2>"$stderr_file" || true)
        elif [[ -n "$env_str" ]]; then
            out=$(env $env_str "${time_prefix[@]}" "$@" 2>"$stderr_file" || true)
        elif [[ -n "${PIN_CMD[*]-}" ]]; then
            out=$("${PIN_CMD[@]}" "${time_prefix[@]}" "$@" 2>"$stderr_file" || true)
        else
            out=$("${time_prefix[@]}" "$@" 2>"$stderr_file" || true)
        fi
        local ms
        ms=$(printf '%s\n' "$out" | sed -n 's/^ms:\([0-9]*\)$/\1/p' | head -1)
        local checksum
        checksum=$(printf '%s\n' "$out" | sed -n 's/^checksum:\(.*\)$/\1/p' | head -1)
        local rss_bytes
        rss_bytes=$(rss_extract "$stderr_file")
        rm -f "$stderr_file"
        [[ -z "$ms" ]] && continue
        [[ -z "$rss_bytes" ]] && rss_bytes=0
        echo "$ms" >> "$samples_file"
        [[ -n "$checksum" ]] && echo "$checksum" >> "$checksums_file"
        if [[ "$rss_bytes" -gt "$worst_rss" ]]; then worst_rss="$rss_bytes"; fi
    done
    local sample_count
    sample_count=$(wc -l < "$samples_file" | awk '{print $1}')
    if [[ "$sample_count" -eq 0 ]]; then
        printf "  %-44s  FAILED (no successful runs)\n" "$label"
        rm -f "$samples_file" "$checksums_file"
        return
    fi
    local stats
    stats=$(compute_stats < "$samples_file")
    local raw_samples raw_checksums checksum_count
    raw_samples=$(paste -sd, "$samples_file")
    raw_checksums=$(paste -sd, "$checksums_file")
    checksum_count=$(wc -l < "$checksums_file" | awk '{print $1}')
    rm -f "$samples_file" "$checksums_file"
    local median p95 stddev min max
    IFS='|' read -r median p95 stddev min max <<< "$stats"
    local rss_mb
    rss_mb=$(awk -v b="$worst_rss" 'BEGIN { printf "%d", b/1048576 }')
    printf "  [%-12s] %-44s  median=%-4s p95=%-4s σ=%-4s [%s..%s] ms · %s MB\n" \
        "$workload" "$label ($profile)" "$median" "$p95" "$stddev" "$min" "$max" "$rss_mb"
    printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n" \
        "$workload" "$median" "$label" "$profile" "$p95" "$stddev" "$min" "$max" "$rss_mb" \
        >> "$RESULTS_FILE"
    if [[ "$label" == perry* || "$label" == node* || "$label" == bun* ]]; then
        printf "%s\t%s\t%s\t%s\t%s\t%s\n" \
            "$workload" "$profile" "$label" "$raw_samples" "$raw_checksums" "$checksum_count" \
            >> "$RAW_RESULTS_FILE"
    fi
}

# ---------------------------------------------------------------------------
# Perry — idiomatic = default flags (gen-GC, lazy JSON tape).
#         optimized = same default (Perry doesn't have separate flag tiers
#                     for compiled output; the lazy JSON tape IS the
#                     optimization. Direct path is shown for honesty.)
# ---------------------------------------------------------------------------
echo "=== Perry ==="
if [[ -x "$PERRY" ]]; then
    "$PERRY" compile bench.ts              -o "$TMPDIR/perry_rt"  >/dev/null 2>&1
    "$PERRY" compile bench_field_access.ts -o "$TMPDIR/perry_fa"  >/dev/null 2>&1
    if [[ -x "$TMPDIR/perry_rt" ]]; then
        run_bench "roundtrip"    "optimized" "perry (gen-gc + lazy tape)"  ""                                 "$TMPDIR/perry_rt"
        run_bench "roundtrip"    "idiomatic" "perry (mark-sweep, no lazy)" "PERRY_GEN_GC=0 PERRY_JSON_TAPE=0" "$TMPDIR/perry_rt"
    fi
    if [[ -x "$TMPDIR/perry_fa" ]]; then
        run_bench "field_access" "optimized" "perry (gen-gc + lazy tape)"  ""                                 "$TMPDIR/perry_fa"
        run_bench "field_access" "idiomatic" "perry (mark-sweep, no lazy)" "PERRY_GEN_GC=0 PERRY_JSON_TAPE=0" "$TMPDIR/perry_fa"
    fi
else
    echo "  Perry binary not found at $PERRY — run 'cargo build --release -p perry' first"
fi

# ---------------------------------------------------------------------------
# Bun — JavaScriptCore JIT.
# ---------------------------------------------------------------------------
echo "=== Bun ==="
if have bun; then
    run_bench "roundtrip"    "idiomatic" "bun (default)" "" bun bench.ts
    run_bench "field_access" "idiomatic" "bun (default)" "" bun bench_field_access.ts
fi

# ---------------------------------------------------------------------------
# Node.js — V8.
#
# Node runs PRECOMPILED .mjs (esbuild/tsc strips TS types as a setup step,
# untimed) instead of `node --experimental-strip-types bench.ts`. Otherwise
# Node would be charged for the runtime TS-stripping cost on every launch
# — work that Perry pays at compile time and Bun pays as part of its
# native TS-runtime value prop. If no stripper is available, we fall back
# to `--experimental-strip-types` and emit a banner so the asymmetry is
# visible. Bun stays on .ts because direct TS execution IS Bun's value
# prop (see methodology box in benchmarks/README.md).
# ---------------------------------------------------------------------------
echo "=== Node.js ==="
if have node; then
    NODE_RT_INPUT=""
    NODE_FA_INPUT=""
    NODE_FALLBACK_FLAGS=()
    if [[ -n "$NODE_TS_STRIP" ]]; then
        precompile_to_mjs bench.ts              "$TMPDIR/bench.mjs"              && NODE_RT_INPUT="$TMPDIR/bench.mjs"
        precompile_to_mjs bench_field_access.ts "$TMPDIR/bench_field_access.mjs" && NODE_FA_INPUT="$TMPDIR/bench_field_access.mjs"
    fi
    if [[ -z "$NODE_RT_INPUT" ]]; then
        NODE_RT_INPUT="bench.ts"
        NODE_FA_INPUT="bench_field_access.ts"
        NODE_FALLBACK_FLAGS=(--experimental-strip-types)
    fi
    run_bench "roundtrip"    "idiomatic" "node (default)"      ""  node ${NODE_FALLBACK_FLAGS[@]+"${NODE_FALLBACK_FLAGS[@]}"} "$NODE_RT_INPUT"
    run_bench "roundtrip"    "optimized" "node --max-old=4096" ""  node ${NODE_FALLBACK_FLAGS[@]+"${NODE_FALLBACK_FLAGS[@]}"} --max-old-space-size=4096 "$NODE_RT_INPUT"
    run_bench "field_access" "idiomatic" "node (default)"      ""  node ${NODE_FALLBACK_FLAGS[@]+"${NODE_FALLBACK_FLAGS[@]}"} "$NODE_FA_INPUT"
    run_bench "field_access" "optimized" "node --max-old=4096" ""  node ${NODE_FALLBACK_FLAGS[@]+"${NODE_FALLBACK_FLAGS[@]}"} --max-old-space-size=4096 "$NODE_FA_INPUT"
fi

# ---------------------------------------------------------------------------
# Go — encoding/json.
# ---------------------------------------------------------------------------
echo "=== Go ==="
if have go; then
    go build -o "$TMPDIR/bench_go_rt_idio"  bench.go              2>&1 | tail -3
    go build -o "$TMPDIR/bench_go_fa_idio"  bench_field_access.go 2>&1 | tail -3
    go build -ldflags="-s -w" -trimpath -o "$TMPDIR/bench_go_rt_opt" bench.go              2>&1 | tail -3
    go build -ldflags="-s -w" -trimpath -o "$TMPDIR/bench_go_fa_opt" bench_field_access.go 2>&1 | tail -3
    [[ -x "$TMPDIR/bench_go_rt_idio" ]] && run_bench "roundtrip"    "idiomatic" "go (encoding/json)"             "" "$TMPDIR/bench_go_rt_idio"
    [[ -x "$TMPDIR/bench_go_rt_opt"  ]] && run_bench "roundtrip"    "optimized" "go -ldflags=\"-s -w\" -trimpath" "" "$TMPDIR/bench_go_rt_opt"
    [[ -x "$TMPDIR/bench_go_fa_idio" ]] && run_bench "field_access" "idiomatic" "go (encoding/json)"             "" "$TMPDIR/bench_go_fa_idio"
    [[ -x "$TMPDIR/bench_go_fa_opt"  ]] && run_bench "field_access" "optimized" "go -ldflags=\"-s -w\" -trimpath" "" "$TMPDIR/bench_go_fa_opt"
fi

# ---------------------------------------------------------------------------
# Rust serde_json
# ---------------------------------------------------------------------------
echo "=== Rust ==="
if have cargo; then
    cargo build --release --target-dir "$TMPDIR/cargo_target" --quiet 2>&1 | tail -5
    cargo build --profile release-aggressive --target-dir "$TMPDIR/cargo_target" --quiet 2>&1 | tail -5
    [[ -x "$TMPDIR/cargo_target/release/bench"                            ]] && run_bench "roundtrip"    "idiomatic" "rust serde_json"             "" "$TMPDIR/cargo_target/release/bench"
    [[ -x "$TMPDIR/cargo_target/release/bench_field_access"               ]] && run_bench "field_access" "idiomatic" "rust serde_json"             "" "$TMPDIR/cargo_target/release/bench_field_access"
    [[ -x "$TMPDIR/cargo_target/release-aggressive/bench"                 ]] && run_bench "roundtrip"    "optimized" "rust serde_json (LTO+1cgu)" "" "$TMPDIR/cargo_target/release-aggressive/bench"
    [[ -x "$TMPDIR/cargo_target/release-aggressive/bench_field_access"    ]] && run_bench "field_access" "optimized" "rust serde_json (LTO+1cgu)" "" "$TMPDIR/cargo_target/release-aggressive/bench_field_access"
fi

# ---------------------------------------------------------------------------
# Swift
# ---------------------------------------------------------------------------
echo "=== Swift ==="
if have swiftc; then
    swiftc -O      bench.swift              -o "$TMPDIR/bench_swift_rt_idio" 2>&1 | tail -3
    swiftc -O      bench_field_access.swift -o "$TMPDIR/bench_swift_fa_idio" 2>&1 | tail -3
    swiftc -O -wmo bench.swift              -o "$TMPDIR/bench_swift_rt_opt"  2>&1 | tail -3
    swiftc -O -wmo bench_field_access.swift -o "$TMPDIR/bench_swift_fa_opt"  2>&1 | tail -3
    [[ -x "$TMPDIR/bench_swift_rt_idio" ]] && run_bench "roundtrip"    "idiomatic" "swift -O (Foundation)"     "" "$TMPDIR/bench_swift_rt_idio"
    [[ -x "$TMPDIR/bench_swift_rt_opt"  ]] && run_bench "roundtrip"    "optimized" "swift -O -wmo (Foundation)" "" "$TMPDIR/bench_swift_rt_opt"
    [[ -x "$TMPDIR/bench_swift_fa_idio" ]] && run_bench "field_access" "idiomatic" "swift -O (Foundation)"     "" "$TMPDIR/bench_swift_fa_idio"
    [[ -x "$TMPDIR/bench_swift_fa_opt"  ]] && run_bench "field_access" "optimized" "swift -O -wmo (Foundation)" "" "$TMPDIR/bench_swift_fa_opt"
fi

# ---------------------------------------------------------------------------
# Kotlin (kotlinx.serialization on JVM)
# ---------------------------------------------------------------------------
echo "=== Kotlin ==="
GRADLE_LIB=$(brew --prefix gradle 2>/dev/null)/libexec/lib
KOTLINC_LIB=$(brew --prefix kotlin 2>/dev/null)/libexec/lib
if have kotlinc && [[ -d "$GRADLE_LIB" ]] && [[ -d "$KOTLINC_LIB" ]]; then
    SERIALIZATION_CP="$GRADLE_LIB/kotlinx-serialization-core-jvm-1.9.0.jar:$GRADLE_LIB/kotlinx-serialization-json-jvm-1.9.0.jar"
    PLUGIN="$KOTLINC_LIB/kotlinx-serialization-compiler-plugin.jar"
    kotlinc -Xplugin="$PLUGIN" -classpath "$SERIALIZATION_CP" -include-runtime -d "$TMPDIR/bench_kt_rt.jar" bench.kt              2>&1 | tail -3
    kotlinc -Xplugin="$PLUGIN" -classpath "$SERIALIZATION_CP" -include-runtime -d "$TMPDIR/bench_kt_fa.jar" bench_field_access.kt 2>&1 | tail -3
    if [[ -f "$TMPDIR/bench_kt_rt.jar" ]]; then
        run_bench "roundtrip" "idiomatic" "kotlin (kotlinx.serialization)" "" \
            java                  -cp "$TMPDIR/bench_kt_rt.jar:$SERIALIZATION_CP" BenchKt
        run_bench "roundtrip" "optimized" "kotlin -server -Xmx512m"        "" \
            java -server -Xmx512m -cp "$TMPDIR/bench_kt_rt.jar:$SERIALIZATION_CP" BenchKt
    fi
    if [[ -f "$TMPDIR/bench_kt_fa.jar" ]]; then
        run_bench "field_access" "idiomatic" "kotlin (kotlinx.serialization)" "" \
            java                  -cp "$TMPDIR/bench_kt_fa.jar:$SERIALIZATION_CP" Bench_field_accessKt
        run_bench "field_access" "optimized" "kotlin -server -Xmx512m"        "" \
            java -server -Xmx512m -cp "$TMPDIR/bench_kt_fa.jar:$SERIALIZATION_CP" Bench_field_accessKt
    fi
else
    echo "  kotlinc / gradle JARs not found — skipping (brew install kotlin gradle)"
fi

# ---------------------------------------------------------------------------
# C++ (nlohmann/json)
# ---------------------------------------------------------------------------
echo "=== C++ (nlohmann/json — popular default) ==="
if have clang++ && [[ -d "$NLOHMANN_INCLUDE" ]]; then
    clang++ -std=c++17 -O2          -I"$NLOHMANN_INCLUDE" bench.cpp              -o "$TMPDIR/bench_cpp_rt_idio" 2>&1 | tail -3
    clang++ -std=c++17 -O2          -I"$NLOHMANN_INCLUDE" bench_field_access.cpp -o "$TMPDIR/bench_cpp_fa_idio" 2>&1 | tail -3
    clang++ -std=c++17 -O3 -flto    -I"$NLOHMANN_INCLUDE" bench.cpp              -o "$TMPDIR/bench_cpp_rt_opt"  2>&1 | tail -3
    clang++ -std=c++17 -O3 -flto    -I"$NLOHMANN_INCLUDE" bench_field_access.cpp -o "$TMPDIR/bench_cpp_fa_opt"  2>&1 | tail -3
    [[ -x "$TMPDIR/bench_cpp_rt_idio" ]] && run_bench "roundtrip"    "idiomatic" "c++ -O2 (nlohmann/json)"      "" "$TMPDIR/bench_cpp_rt_idio"
    [[ -x "$TMPDIR/bench_cpp_rt_opt"  ]] && run_bench "roundtrip"    "optimized" "c++ -O3 -flto (nlohmann/json)" "" "$TMPDIR/bench_cpp_rt_opt"
    [[ -x "$TMPDIR/bench_cpp_fa_idio" ]] && run_bench "field_access" "idiomatic" "c++ -O2 (nlohmann/json)"      "" "$TMPDIR/bench_cpp_fa_idio"
    [[ -x "$TMPDIR/bench_cpp_fa_opt"  ]] && run_bench "field_access" "optimized" "c++ -O3 -flto (nlohmann/json)" "" "$TMPDIR/bench_cpp_fa_opt"
fi

# ---------------------------------------------------------------------------
# C++ (simdjson — SIMD-accelerated parse-throughput reference)
#   Listed alongside nlohmann/json so a reader can see both:
#   "what most C++ projects ship with" AND "the C++ ceiling".
#   simdjson 4.x uses ondemand for parse + raw_json() bytes for stringify
#   (the unmutated-parse fast path, analogous to Perry's lazy tape).
#   See bench_simdjson.cpp file header for the full rationale.
# ---------------------------------------------------------------------------
echo "=== C++ (simdjson — SIMD reference) ==="
SIMDJSON_PREFIX=$(brew --prefix simdjson 2>/dev/null || echo "")
SIMDJSON_INCLUDE_DIR="$SIMDJSON_PREFIX/include"
SIMDJSON_LIB_DIR="$SIMDJSON_PREFIX/lib"
if have clang++ && [[ -d "$SIMDJSON_INCLUDE_DIR" ]]; then
    clang++ -std=c++17 -O2       -I"$SIMDJSON_INCLUDE_DIR" -L"$SIMDJSON_LIB_DIR" bench_simdjson.cpp              -lsimdjson -o "$TMPDIR/bench_simdjson_rt_idio" 2>&1 | tail -3
    clang++ -std=c++17 -O2       -I"$SIMDJSON_INCLUDE_DIR" -L"$SIMDJSON_LIB_DIR" bench_field_access_simdjson.cpp -lsimdjson -o "$TMPDIR/bench_simdjson_fa_idio" 2>&1 | tail -3
    clang++ -std=c++17 -O3 -flto -I"$SIMDJSON_INCLUDE_DIR" -L"$SIMDJSON_LIB_DIR" bench_simdjson.cpp              -lsimdjson -o "$TMPDIR/bench_simdjson_rt_opt"  2>&1 | tail -3
    clang++ -std=c++17 -O3 -flto -I"$SIMDJSON_INCLUDE_DIR" -L"$SIMDJSON_LIB_DIR" bench_field_access_simdjson.cpp -lsimdjson -o "$TMPDIR/bench_simdjson_fa_opt"  2>&1 | tail -3
    [[ -x "$TMPDIR/bench_simdjson_rt_idio" ]] && run_bench "roundtrip"    "idiomatic" "c++ -O2 (simdjson)"      "" "$TMPDIR/bench_simdjson_rt_idio"
    [[ -x "$TMPDIR/bench_simdjson_rt_opt"  ]] && run_bench "roundtrip"    "optimized" "c++ -O3 -flto (simdjson)" "" "$TMPDIR/bench_simdjson_rt_opt"
    [[ -x "$TMPDIR/bench_simdjson_fa_idio" ]] && run_bench "field_access" "idiomatic" "c++ -O2 (simdjson)"      "" "$TMPDIR/bench_simdjson_fa_idio"
    [[ -x "$TMPDIR/bench_simdjson_fa_opt"  ]] && run_bench "field_access" "optimized" "c++ -O3 -flto (simdjson)" "" "$TMPDIR/bench_simdjson_fa_opt"
else
    echo "  simdjson not found — install via 'brew install simdjson'"
fi

# ---------------------------------------------------------------------------
# AssemblyScript (json-as) — the TS-to-native peer.
#   AS is a TypeScript-like language that compiles to WebAssembly.
#   Run via wasmtime. json-as generates type-specialized (de)serializers
#   at compile time via a transform — same approach as Rust serde and
#   Kotlin kotlinx.serialization, no runtime reflection. AS is strictly
#   typed (no `any`) so the workload uses concrete `Item`/`Nested`
#   classes — see asconfig.json + assembly/bench.ts. This makes the AS
#   row closer in shape to the Rust/Kotlin typed-struct rows than to
#   the dynamic-typing JS rows; documented in benchmarks/README.md's
#   "Honest disclaimers" section.
# ---------------------------------------------------------------------------
echo "=== AssemblyScript + json-as + wasmtime ==="
AS_DIR="$(pwd)/as_workspace"
if have wasmtime && [[ -d "$AS_DIR/node_modules" ]]; then
    (
        cd "$AS_DIR"
        npx --no-install asc assembly/bench.ts              --target release --outFile build/bench_rt.wasm 2>&1 | tail -3
        npx --no-install asc assembly/bench_field_access.ts --target release --outFile build/bench_fa.wasm 2>&1 | tail -3
    )
    [[ -f "$AS_DIR/build/bench_rt.wasm" ]] && run_bench "roundtrip"    "idiomatic" "assemblyscript+json-as (wasmtime)" "" wasmtime "$AS_DIR/build/bench_rt.wasm"
    [[ -f "$AS_DIR/build/bench_fa.wasm" ]] && run_bench "field_access" "idiomatic" "assemblyscript+json-as (wasmtime)" "" wasmtime "$AS_DIR/build/bench_fa.wasm"
else
    echo "  AssemblyScript / wasmtime not set up — see as_workspace/README"
fi

# ---------------------------------------------------------------------------
# Write RESULTS.md (sorted by median time, ascending)
# ---------------------------------------------------------------------------
{
    echo "# JSON Polyglot Benchmark Results"
    echo
    echo "**Runs per cell:** $RUNS · **Pinning:** $PIN_NOTE"
    echo "**Hardware:** $(uname -srm) on $(hostname -s)."
    echo "**Date:** $(date -u +%Y-%m-%d)."
    echo
    echo "Two workloads, each language listed twice (idiomatic / optimized flag profile)."
    echo "Median wall-clock time is the headline number; p95, σ (population stddev),"
    echo "min, and max are reported per cell so noise is visible. Lower is better."
    echo
    echo "## JSON validate-and-roundtrip"
    echo
    echo "Per iteration: parse → stringify → discard. The unmutated parse lets"
    echo "Perry's lazy tape (v0.5.204+) memcpy the original blob bytes for"
    echo "stringify, which is why Perry's headline number on this workload is so"
    echo "low — the lazy path can avoid materializing the parse tree entirely."
    echo "10k records, ~1 MB blob, 50 iterations per run."
    echo
    echo "| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |"
    echo "|---|---|---:|---:|---:|---:|---:|---:|"
    awk -F'\t' '$1 == "roundtrip"' "$RESULTS_FILE" \
        | sort -t$'\t' -k2 -n \
        | while IFS=$'\t' read -r workload median label profile p95 stddev mn mx rss; do
            printf "| %s | %s | %s | %s | %s | %s | %s | %s |\n" \
                "$label" "$profile" "$median" "$p95" "$stddev" "$mn" "$mx" "$rss"
        done
    echo
    echo "## JSON parse-and-iterate"
    echo
    echo "Per iteration: parse → sum every record's nested.x (touches every element)"
    echo "→ stringify. The full-tree iteration FORCES Perry's lazy tape to"
    echo "materialize, so this is the honest comparison for workloads that touch"
    echo "JSON content. 10k records, ~1 MB blob, 50 iterations per run."
    echo
    echo "| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |"
    echo "|---|---|---:|---:|---:|---:|---:|---:|"
    awk -F'\t' '$1 == "field_access"' "$RESULTS_FILE" \
        | sort -t$'\t' -k2 -n \
        | while IFS=$'\t' read -r workload median label profile p95 stddev mn mx rss; do
            printf "| %s | %s | %s | %s | %s | %s | %s | %s |\n" \
                "$label" "$profile" "$median" "$p95" "$stddev" "$mn" "$mx" "$rss"
        done
} > RESULTS.md

echo
echo "Wrote $(pwd)/RESULTS.md"
cat RESULTS.md

if [[ -n "$PUBLIC_JSON_OUT" ]]; then
    mkdir -p "$(dirname "$PUBLIC_JSON_OUT")"
    PYTHONPATH="$(cd "$PERRY_ROOT" && pwd)" python3 - "$RAW_RESULTS_FILE" "$PUBLIC_JSON_OUT" "$(cd "$PERRY_ROOT" && pwd)" "$RUNS" "$PIN_NOTE" <<'PY'
import json, shutil, subprocess, sys
from datetime import datetime, timezone
from pathlib import Path
from benchmarks.public_baseline import distribution

raw_path, output_path, root, requested, pinning = sys.argv[1:]
requested = int(requested)
selected = {}
for line in open(raw_path):
    workload, profile, label, raw, raw_checksums, checksum_count = line.rstrip("\n").split("\t")
    runtime = label.split()[0]
    wanted = ((runtime == "perry" and profile == "optimized") or
              (runtime in ("node", "bun") and profile == "idiomatic"))
    if not wanted or (workload, runtime) in selected:
        continue
    samples = [float(value) for value in raw.split(",") if value]
    checksums = [value for value in raw_checksums.split(",") if value]
    if len(samples) != requested or int(checksum_count) != requested or len(checksums) != requested:
        raise SystemExit(f"json_polyglot/{workload}: {runtime} raw samples are incomplete")
    if len(set(checksums)) != 1:
        raise SystemExit(f"json_polyglot/{workload}: {runtime} checksum changed across samples")
    selected[(workload, runtime)] = (samples, checksums[0])

benchmarks = {}
for workload in ("roundtrip", "field_access"):
    checksums = {selected[(workload, runtime)][1] for runtime in ("perry", "node", "bun")}
    if len(checksums) != 1:
        raise SystemExit(f"json_polyglot/{workload}: checksum mismatch {sorted(checksums)}")
    benchmarks[workload] = {
        "correctness": {"status": "pass", "reference": "node+bun", "checksum": checksums.pop()},
        "runtimes": {
            runtime: {"wall_ms": distribution(selected[(workload, runtime)][0])}
            for runtime in ("perry", "node", "bun")
        },
    }
component = {
    "schema_version": 1,
    "suite": "json_polyglot",
    "commit": subprocess.check_output(["git", "-C", root, "rev-parse", "HEAD"], text=True).strip(),
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "run_config": {
        "warmup": "3 untimed iterations inside each benchmark process",
        "requested_samples": requested,
        "pinning": pinning,
    },
    "commands": {
        "perry": [root + "/target/release/perry", "compile", "bench.ts", "-o", "<binary>"],
        "node": [shutil.which("node"), "<precompiled.mjs>"],
        "bun": [shutil.which("bun"), "bench.ts"],
        "runner": ["benchmarks/json_polyglot/run.sh"],
    },
    "runtime_metadata": {
        "perry": {
            "version": subprocess.check_output([str(Path(root + "/target/release/perry")), "--version"], text=True).strip(),
            "resolved_executable": str(Path(root + "/target/release/perry").resolve()),
        },
        "node": {
            "version": subprocess.check_output(["node", "--version"], text=True).strip(),
            "resolved_executable": shutil.which("node"),
        },
        "bun": {
            "version": subprocess.check_output(["bun", "--version"], text=True).strip(),
            "resolved_executable": shutil.which("bun"),
        },
    },
    "benchmarks": benchmarks,
}
Path(output_path).write_text(json.dumps(component, indent=2) + "\n")
PY
    echo "Machine-readable results: $PUBLIC_JSON_OUT"
fi
