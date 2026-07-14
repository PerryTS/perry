#!/usr/bin/env bash
# Polyglot compute-microbench runner.
#
# 8 microbenches × 9 runtimes (Perry / Rust / C++ / Go / Swift / Java /
# Node / Bun / Hermes (optional) / Python). Each cell is run multiple
# times and reported as median + p95 + σ + min + max — not "best-of-N".
#
# Default RUNS=11 (override via first arg or $RUNS env). RUNS=11 against
# 8 benches × 9 runtimes is ≈ 800 invocations. Python alone may take
# ~10 minutes at RUNS=11 because its tight-loop benches are 5-15 s each.
#
# CPU pinning:
#   macOS: taskpolicy -t 0 -l 0 (P-core SCHEDULER HINT, not strict —
#          Apple does not expose unprivileged hard core affinity).
#   Linux: taskset -c 0 (strict).
#   Otherwise: no pinning, with caveat banner.

set -euo pipefail
cd "$(dirname "$0")"
PERRY_ROOT="../.."
SUITE="../suite"
RUNS=${1:-${RUNS:-11}}
TMPDIR=/tmp/perry_polyglot_bench
mkdir -p "$TMPDIR"
PUBLIC_JSON_OUT="${PUBLIC_BENCH_JSON_OUT:-}"
RAW_SAMPLES_DIR="$TMPDIR/raw_samples"
rm -rf "$RAW_SAMPLES_DIR"
mkdir -p "$RAW_SAMPLES_DIR"

# --- Runtime detection ---
HAS_BUN=0
HAS_SHERMES=0
command -v bun >/dev/null 2>&1 && HAS_BUN=1
command -v shermes >/dev/null 2>&1 && HAS_SHERMES=1

# --- CPU pinning ---
PIN_CMD=()
PIN_NOTE=""
case "$(uname)" in
    Darwin)
        if command -v taskpolicy >/dev/null 2>&1; then
            PIN_CMD=(taskpolicy -t 0 -l 0)
            PIN_NOTE="macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)"
        else
            PIN_NOTE="macOS without taskpolicy — no pinning available"
        fi
        ;;
    Linux)
        if command -v taskset >/dev/null 2>&1; then
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

# Strip TypeScript annotations so Hermes (JS-only) can parse.
strip_types() {
  sed -E \
    -e 's/: (number|string|boolean|any|void)(\[\])?//g' \
    -e 's/\): (number|string|boolean|any|void)(\[\])? \{/) {/g' \
    "$1"
}

# Node-side TS stripper. Node measurements run on precompiled .mjs so we
# don't charge Node for `--experimental-strip-types`'s per-launch parse +
# strip cost (Perry compiles AOT and Bun strips natively; neither pays).
# Setup step, untimed. esbuild preferred; tsc fallback. If nothing's
# available we fall back to --experimental-strip-types and print a banner.
NODE_TS_STRIP=""
NODE_TS_STRIP_NOTE=""
if command -v esbuild >/dev/null 2>&1; then
    NODE_TS_STRIP="esbuild"
    NODE_TS_STRIP_NOTE="esbuild on PATH"
elif command -v npx >/dev/null 2>&1 && npx --no-install esbuild --version >/dev/null 2>&1; then
    NODE_TS_STRIP="npx-esbuild"
    NODE_TS_STRIP_NOTE="npx esbuild (project-local)"
elif command -v tsc >/dev/null 2>&1; then
    NODE_TS_STRIP="tsc"
    NODE_TS_STRIP_NOTE="tsc on PATH"
fi

# Compile <src.ts> -> <dst.mjs>. Returns 0 on success.
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

echo "==============================================================="
echo "Polyglot compute microbenches — Perry v$(grep '^version' $PERRY_ROOT/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
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

echo "=== Building ==="
cargo build --release --manifest-path="$PERRY_ROOT/Cargo.toml" -p perry -q 2>/dev/null
PERRY="$PERRY_ROOT/target/release/perry"
for f in "$SUITE"/*.ts; do
  name=$(basename "$f" .ts)
  $PERRY compile "$f" -o "$TMPDIR/perry_${name}" -q 2>/dev/null || true
done
echo "  Perry: done"
g++ -O3 -std=c++17 bench.cpp -o "$TMPDIR/bench_cpp" 2>/dev/null && echo "  C++: done"
rustc -O bench.rs -o "$TMPDIR/bench_rs" 2>/dev/null && echo "  Rust: done"
swiftc -O bench.swift -o "$TMPDIR/bench_swift" 2>/dev/null && echo "  Swift: done"
go build -o "$TMPDIR/bench_go" bench.go 2>/dev/null && echo "  Go: done"
javac -d "$TMPDIR" bench.java 2>/dev/null && echo "  Java: done"
echo "  Python: (interpreted)"

if [ $HAS_SHERMES -eq 1 ]; then
  for bk in "05_fibonacci" "02_loop_overhead" "03_array_write" "04_array_read" "06_math_intensive" "07_object_create" "10_nested_loops" "13_factorial"; do
    js_file="$TMPDIR/shermes_${bk}.js"
    strip_types "$SUITE/${bk}.ts" > "$js_file"
    shermes -typed -O -o "$TMPDIR/shermes_${bk}" "$js_file" 2>/dev/null || \
      shermes -O -o "$TMPDIR/shermes_${bk}" "$js_file" 2>/dev/null || true
  done
  echo "  Hermes: done"
fi

echo ""
echo "=== Running ($RUNS runs/cell, median + p95 + σ reported) ==="

get_time() { echo "$1" | grep -oE "${2}:[0-9]+" | head -1 | grep -oE '[0-9]+$'; }

# Compute median, p95, stddev, min, max from stdin (one int per line).
# Output: median|p95|stddev|min|max
compute_stats() {
    awk '
    { v[NR] = $1 + 0 }
    END {
        n = NR
        if (n == 0) { print "0|0|0|0|0"; exit }
        for (i = 2; i <= n; i++) {
            x = v[i]; j = i - 1
            while (j >= 1 && v[j] > x) { v[j+1] = v[j]; j-- }
            v[j+1] = x
        }
        if (n % 2 == 1) { median = v[(n+1)/2] }
        else { median = (v[n/2] + v[n/2+1]) / 2 }
        p95_idx = int(0.95 * n + 0.99999)
        if (p95_idx > n) p95_idx = n
        if (p95_idx < 1) p95_idx = 1
        p95 = v[p95_idx]
        sum = 0
        for (i = 1; i <= n; i++) sum += v[i]
        mean = sum / n
        ss = 0
        for (i = 1; i <= n; i++) ss += (v[i] - mean) ^ 2
        stddev = sqrt(ss / n)
        printf "%d|%d|%.1f|%d|%d", median, p95, stddev, v[1], v[n]
    }'
}

# Run a command RUNS times under the pinning prefix, parse "${key}:N" out
# of stdout per run, return "median|p95|stddev|min|max" via stdout.
# Outputs "-|-|-|-|-" if no successful runs.
stats_of() {
    local cmd="$1" key="$2" runtime="$3" bench="$4"
    local samples="$TMPDIR/.samples.$$"
    local outputs="$TMPDIR/.outputs.$$"
    : > "$samples"
    : > "$outputs"
    local i
    for i in $(seq 1 $RUNS); do
        local out
        if [[ -n "${PIN_CMD[*]-}" ]]; then
            out=$("${PIN_CMD[@]}" $cmd 2>/dev/null) || true
        else
            out=$($cmd 2>/dev/null) || true
        fi
        local t
        t=$(get_time "$out" "$key")
        if [[ -n "$t" ]]; then
            echo "$t" >> "$samples"
            printf '%s\n' "$out" \
                | sed -E "/^${key}:[0-9]+$/d" \
                | shasum -a 256 \
                | awk '{print $1}' >> "$outputs"
        fi
    done
    if [[ ! -s "$samples" ]]; then
        rm -f "$samples"
        rm -f "$outputs"
        echo "-|-|-|-|-"
        return
    fi
    local result
    result=$(compute_stats < "$samples")
    mkdir -p "$RAW_SAMPLES_DIR/$runtime"
    cp "$samples" "$RAW_SAMPLES_DIR/$runtime/$bench.txt"
    cp "$outputs" "$RAW_SAMPLES_DIR/$runtime/$bench.outputs"
    rm -f "$samples" "$outputs"
    echo "$result"
}

# Run each language across all benches. Produces TSV file
# `$TMPDIR/results_<lang>.tsv` with `bench<TAB>median|p95|stddev|min|max`.
run_lang() {
    local lang="$1" cmd="$2"
    local results="$TMPDIR/results_${lang}.tsv"
    : > "$results"
    for bk in "fibonacci:fibonacci" "loop_overhead:loop_overhead" "loop_data_dependent:loop_data_dependent" "array_write:array_write" "array_read:array_read" "math_intensive:math_intensive" "object_create:object_create" "nested_loops:nested_loops" "accumulate:accumulate"; do
        IFS=: read -r bench key <<< "$bk"
        local stats
        stats=$(stats_of "$cmd" "$key" "$lang" "$bench")
        printf "%s\t%s\n" "$bench" "$stats" >> "$results"
    done
    echo "  $lang: done"
}

# Perry (per-bench binary)
: > "$TMPDIR/results_perry.tsv"
for bk in "fibonacci:05_fibonacci:fibonacci" "loop_overhead:02_loop_overhead:loop_overhead" "loop_data_dependent:17_loop_data_dependent:loop_data_dependent" "array_write:03_array_write:array_write" "array_read:04_array_read:array_read" "math_intensive:06_math_intensive:math_intensive" "object_create:07_object_create:object_create" "nested_loops:10_nested_loops:nested_loops" "accumulate:13_factorial:accumulate"; do
    IFS=: read -r bench ts key <<< "$bk"
    stats=$(stats_of "$TMPDIR/perry_${ts}" "$key" "perry" "$bench")
    printf "%s\t%s\n" "$bench" "$stats" >> "$TMPDIR/results_perry.tsv"
done
echo "  Perry: done"

# Node — runs PRECOMPILED .mjs (esbuild/tsc strips TS as a setup step,
# untimed). Without precompile, Node would be charged for
# --experimental-strip-types' per-launch TS-stripping cost — work that
# Perry pays at compile time and Bun pays as part of its native TS-runtime
# value prop. Falls back to --experimental-strip-types if no stripper is
# available; the banner at script start makes the asymmetry visible.
: > "$TMPDIR/results_node.tsv"
for bk in "fibonacci:05_fibonacci:fibonacci" "loop_overhead:02_loop_overhead:loop_overhead" "loop_data_dependent:17_loop_data_dependent:loop_data_dependent" "array_write:03_array_write:array_write" "array_read:04_array_read:array_read" "math_intensive:06_math_intensive:math_intensive" "object_create:07_object_create:object_create" "nested_loops:10_nested_loops:nested_loops" "accumulate:13_factorial:accumulate"; do
    IFS=: read -r bench ts key <<< "$bk"
    node_input=""
    if [[ -n "$NODE_TS_STRIP" ]]; then
        precompile_to_mjs "$SUITE/${ts}.ts" "$TMPDIR/node_${ts}.mjs" && node_input="$TMPDIR/node_${ts}.mjs"
    fi
    if [[ -n "$node_input" ]]; then
        stats=$(stats_of "node $node_input" "$key" "node" "$bench")
    else
        stats=$(stats_of "node --experimental-strip-types $SUITE/${ts}.ts" "$key" "node" "$bench")
    fi
    printf "%s\t%s\n" "$bench" "$stats" >> "$TMPDIR/results_node.tsv"
done
echo "  Node: done"

# Bun
: > "$TMPDIR/results_bun.tsv"
if [ $HAS_BUN -eq 1 ]; then
    for bk in "fibonacci:05_fibonacci:fibonacci" "loop_overhead:02_loop_overhead:loop_overhead" "loop_data_dependent:17_loop_data_dependent:loop_data_dependent" "array_write:03_array_write:array_write" "array_read:04_array_read:array_read" "math_intensive:06_math_intensive:math_intensive" "object_create:07_object_create:object_create" "nested_loops:10_nested_loops:nested_loops" "accumulate:13_factorial:accumulate"; do
        IFS=: read -r bench ts key <<< "$bk"
        stats=$(stats_of "bun run $SUITE/${ts}.ts" "$key" "bun" "$bench")
        printf "%s\t%s\n" "$bench" "$stats" >> "$TMPDIR/results_bun.tsv"
    done
    echo "  Bun: done"
else
    for bench in fibonacci loop_overhead loop_data_dependent array_write array_read math_intensive object_create nested_loops accumulate; do
        printf "%s\t-|-|-|-|-\n" "$bench" >> "$TMPDIR/results_bun.tsv"
    done
    echo "  Bun: skipped (not installed)"
fi

# Hermes
: > "$TMPDIR/results_hermes.tsv"
if [ $HAS_SHERMES -eq 1 ]; then
    for bk in "fibonacci:05_fibonacci:fibonacci" "loop_overhead:02_loop_overhead:loop_overhead" "loop_data_dependent:17_loop_data_dependent:loop_data_dependent" "array_write:03_array_write:array_write" "array_read:04_array_read:array_read" "math_intensive:06_math_intensive:math_intensive" "object_create:07_object_create:object_create" "nested_loops:10_nested_loops:nested_loops" "accumulate:13_factorial:accumulate"; do
        IFS=: read -r bench ts key <<< "$bk"
        if [ -x "$TMPDIR/shermes_${ts}" ]; then
            stats=$(stats_of "$TMPDIR/shermes_${ts}" "$key" "hermes" "$bench")
        else
            stats="-|-|-|-|-"
        fi
        printf "%s\t%s\n" "$bench" "$stats" >> "$TMPDIR/results_hermes.tsv"
    done
    echo "  Hermes: done"
else
    for bench in fibonacci loop_overhead loop_data_dependent array_write array_read math_intensive object_create nested_loops accumulate; do
        printf "%s\t-|-|-|-|-\n" "$bench" >> "$TMPDIR/results_hermes.tsv"
    done
    echo "  Hermes: skipped (not installed)"
fi

# Polyglot single-binary languages
run_lang "rust"   "$TMPDIR/bench_rs"
run_lang "cpp"    "$TMPDIR/bench_cpp"
run_lang "swift"  "$TMPDIR/bench_swift"
run_lang "go"     "$TMPDIR/bench_go"
run_lang "java"   "java -cp $TMPDIR bench"
run_lang "python" "python3 bench.py"

# Lookup helpers
field() { echo "$1" | cut -d'|' -f"$2"; }
median_of() { grep "^${2}\b" "$TMPDIR/results_${1}.tsv" | cut -f2 | cut -d'|' -f1; }
stats_str() {
    local lang="$1" bench="$2"
    local s
    s=$(grep "^${bench}\b" "$TMPDIR/results_${lang}.tsv" | cut -f2)
    [[ -z "$s" || "$s" == "-|-|-|-|-" ]] && { echo "-"; return; }
    local median p95 stddev mn mx
    IFS='|' read -r median p95 stddev mn mx <<< "$s"
    echo "${median} (p95: ${p95}, σ: ${stddev}, min: ${mn}, max: ${mx})"
}

# ---------------------------------------------------------------------------
# Write RESULTS_AUTO.md and print to stdout. The hand-curated
# RESULTS.md / RESULTS_OPT.md alongside this script keep their
# commentary; this file is the raw output table that gets pasted in.
# ---------------------------------------------------------------------------
{
    echo "# Polyglot Compute-Microbench Results (auto-generated)"
    echo
    echo "**Runs per cell:** $RUNS · **Pinning:** $PIN_NOTE"
    echo "**Hardware:** $(uname -srm) on $(hostname -s) · **Date:** $(date -u +%Y-%m-%d)"
    echo "**Perry version:** v$(grep '^version' $PERRY_ROOT/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
    echo
    echo "Headline = median wall-clock ms. Lower is better."
    echo
    printf "| %-19s | %5s | %5s | %5s | %5s | %5s | %5s | %5s | %5s | %6s | %7s |\n" \
        "Benchmark" "Perry" "Rust" "C++" "Go" "Swift" "Java" "Node" "Bun" "Hermes" "Python"
    echo "|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|"
    for bench in fibonacci loop_overhead loop_data_dependent array_write array_read math_intensive object_create nested_loops accumulate; do
        printf "| %-19s | %5s | %5s | %5s | %5s | %5s | %5s | %5s | %5s | %6s | %7s |\n" \
            "$bench" \
            "$(median_of perry  $bench || echo -)" \
            "$(median_of rust   $bench || echo -)" \
            "$(median_of cpp    $bench || echo -)" \
            "$(median_of go     $bench || echo -)" \
            "$(median_of swift  $bench || echo -)" \
            "$(median_of java   $bench || echo -)" \
            "$(median_of node   $bench || echo -)" \
            "$(median_of bun    $bench || echo -)" \
            "$(median_of hermes $bench || echo -)" \
            "$(median_of python $bench || echo -)"
    done
    echo
    echo "## Per-cell full stats"
    echo
    echo "Format: median (p95: X, σ: S, min: Y, max: Z) ms"
    echo
    echo "| Benchmark | Runtime | Stats (ms) |"
    echo "|---|---|---|"
    for bench in fibonacci loop_overhead loop_data_dependent array_write array_read math_intensive object_create nested_loops accumulate; do
        for lang in perry rust cpp go swift java node bun hermes python; do
            echo "| $bench | $lang | $(stats_str $lang $bench) |"
        done
    done
} | tee RESULTS_AUTO.md

echo ""
echo "Wrote $(pwd)/RESULTS_AUTO.md"

if [[ -n "$PUBLIC_JSON_OUT" ]]; then
    mkdir -p "$(dirname "$PUBLIC_JSON_OUT")"
    PYTHONPATH="$PERRY_ROOT" python3 - "$RAW_SAMPLES_DIR" "$PUBLIC_JSON_OUT" "$PERRY_ROOT" "$RUNS" "$PIN_NOTE" <<'PY'
import json, subprocess, sys
import shutil
from datetime import datetime, timezone
from pathlib import Path
from benchmarks.public_baseline import distribution

raw_dir, output_path, root, requested, pinning = sys.argv[1:]
raw_dir = Path(raw_dir)
requested = int(requested)
benchmarks = {}
for sample_path in sorted((raw_dir / "perry").glob("*.txt")):
    name = sample_path.stem
    runtime_results = {}
    for runtime in ("perry", "node", "bun"):
        path = raw_dir / runtime / f"{name}.txt"
        outputs_path = raw_dir / runtime / f"{name}.outputs"
        samples = [float(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []
        outputs = outputs_path.read_text().splitlines() if outputs_path.exists() else []
        if len(samples) != requested:
            raise SystemExit(f"polyglot/{name}: {runtime} has {len(samples)}/{requested} samples")
        if len(outputs) != requested or len(set(outputs)) != 1:
            raise SystemExit(f"polyglot/{name}: {runtime} correctness evidence is incomplete or unstable")
        runtime_results[runtime] = {"wall_ms": distribution(samples)}
        runtime_results[runtime]["output_sha256"] = outputs[0]
    hashes = {runtime_results[runtime]["output_sha256"] for runtime in ("perry", "node", "bun")}
    if len(hashes) != 1:
        raise SystemExit(f"polyglot/{name}: Perry/Node/Bun outputs differ")
    benchmarks[name] = {
        "correctness": {"status": "pass", "reference": "node+bun", "output_sha256": hashes.pop()},
        "runtimes": runtime_results,
    }
component = {
    "schema_version": 1,
    "suite": "polyglot",
    "commit": subprocess.check_output(["git", "-C", root, "rev-parse", "HEAD"], text=True).strip(),
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "run_config": {
        "warmup": "none beyond benchmark-specific setup",
        "requested_samples": requested,
        "pinning": pinning,
    },
    "commands": {
        "perry": [root + "/target/release/perry", "compile", "<source.ts>", "-o", "<binary>"],
        "node": [shutil.which("node"), "<precompiled.mjs>"],
        "bun": [shutil.which("bun"), "run", "<source.ts>"],
        "runner": ["benchmarks/polyglot/run_all.sh", str(requested)],
    },
    "runtime_metadata": {
        "perry": {
            "version": subprocess.check_output([root + "/target/release/perry", "--version"], text=True).strip(),
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
