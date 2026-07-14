#!/usr/bin/env bash
# Top-level driver: build all implementations, run all workloads, write
# results/results.json and results/metadata.json. Exits non-zero if any build
# fails; workload run failures are recorded as exit_code in the JSON but
# don't abort the suite (unless --strict-output is set and a row had an
# output mismatch — see #441).
#
# Flags:
#   --strict-output   Fail the run if any measured row had output_match=false.
#                     Default: log + continue (so the report can surface
#                     "perf-only" + "correctness" regressions separately).
#
# Env var overrides (all optional):
#   HONEST_BENCH_WARMUP=5
#   HONEST_BENCH_MEASURED=20
#   HONEST_BENCH_SKIP_BUILD=1            — skip (re)building the toolchains
#   HONEST_BENCH_ONLY=3                  — comma-separated workload ids to run
#                                          (1=json, 3=image_convolution,
#                                           4=http_fastify; 2 TBD)
#   HONEST_BENCH_REFRESH_EXPECTED=1      — rebuild results/expected.json from
#                                          a fresh Bun run (output semantics
#                                          intentionally changed)
#
# Layout assumes cwd = benchmarks/honest_bench/.

set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(pwd)"
PERRY_ROOT="$(cd ../.. && pwd)"
RESULTS_DIR="$ROOT/results"
mkdir -p "$RESULTS_DIR"
ONLY="${HONEST_BENCH_ONLY:-1,3}"

STRICT_OUTPUT=0
for arg in "$@"; do
  case "$arg" in
    --strict-output) STRICT_OUTPUT=1 ;;
    -h|--help)
      sed -n '2,30p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

# ------------------------------ 1. metadata -----------------------------------
echo "--- capturing metadata"
python3 - <<PY > "$RESULTS_DIR/metadata.json"
import json, subprocess, datetime, platform, os, shutil
def run(cmd):
    try:
        completed = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        return completed.stdout.strip() if completed.returncode == 0 else ""
    except Exception:
        return ""

def system_profiler_hardware():
    if platform.system() != "Darwin":
        return {}
    try:
        raw = run(["system_profiler", "SPHardwareDataType", "-json"])
        return json.loads(raw).get("SPHardwareDataType", [{}])[0] if raw else {}
    except (IndexError, json.JSONDecodeError):
        return {}

hardware = system_profiler_hardware()
cpu = run(["sysctl", "-n", "machdep.cpu.brand_string"]) or hardware.get("chip_type") or platform.processor()
ncpu = run(["sysctl", "-n", "hw.ncpu"]) or str(os.cpu_count() or "")
ram_bytes = run(["sysctl", "-n", "hw.memsize"])
try:
    ram_gb = round(int(ram_bytes) / (1024**3), 2)
except (TypeError, ValueError):
    try:
        ram_gb = round(os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE") / (1024**3), 2)
    except (AttributeError, OSError, ValueError):
        ram_gb = 0.0
meta = {
    "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "commit": run(["git", "-C", "$PERRY_ROOT", "rev-parse", "HEAD"]),
    "host": {
        "os_version": run(["sw_vers", "-productVersion"]),
        "kernel":     run(["uname", "-a"]),
        "arch":       platform.machine(),
        "cpu":        cpu,
        "ncpu":       ncpu,
        "ram_gb":     ram_gb,
    },
    "toolchains": {
        "rustc": run(["rustc", "--version"]),
        "cargo": run(["cargo", "--version"]),
        "zig":   run(["zig", "version"]),
        "python": run(["python3", "--version"]),
        "node": run(["node", "--version"]),
        "bun": run(["bun", "--version"]),
        "oha": run([os.environ.get("HONEST_BENCH_OHA", "/tmp/oha"), "--version"]),
        "perry": run([os.path.join("$PERRY_ROOT", "target/release/perry"), "--version"]) or "(local build)",
    },
    "executables": {
        "perry": os.path.realpath(os.path.join("$PERRY_ROOT", "target/release/perry")),
        "node": shutil.which("node"),
        "bun": shutil.which("bun"),
    },
    "harness": {
        "warmup":   int(os.environ.get("HONEST_BENCH_WARMUP", 5)),
        "measured": int(os.environ.get("HONEST_BENCH_MEASURED", 20)),
        "http_duration": os.environ.get("HONEST_BENCH_HTTP_DURATION", "15s"),
        "http_warmup_duration": os.environ.get("HONEST_BENCH_HTTP_WARMUP_DUR", "5s"),
        "http_concurrency": int(os.environ.get("HONEST_BENCH_HTTP_CONC", 10)),
    },
    "commands": {
        "perry": ["<compiled-workload>"],
        "perry_compile": [os.path.join("$PERRY_ROOT", "target/release/perry"), "<source.ts>", "-o", "<compiled-workload>"],
        "node": [shutil.which("node"), "--experimental-strip-types", "<source.ts>"],
        "bun": [shutil.which("bun"), "run", "<source.ts>"],
        "perry_http": ["<compiled-kernel>"],
        "perry_http_compile": [os.path.join("$PERRY_ROOT", "target/release/perry"), "<kernel.ts>", "-o", "<compiled-kernel>"],
        "node_http": ["node", "--import", "tsx", "<kernel.ts>"],
        "bun_http": ["bun", "run", "<kernel.ts>"],
        "oha": [os.environ.get("HONEST_BENCH_OHA", "/tmp/oha"), "-z", os.environ.get("HONEST_BENCH_HTTP_DURATION", "15s"), "-c", os.environ.get("HONEST_BENCH_HTTP_CONC", "10"), "-r", "0", "--no-tui", "--output-format", "json", "<url>"],
    },
}
print(json.dumps(meta, indent=2))
PY

# ------------------------------ 2. build --------------------------------------
if [[ -z "${HONEST_BENCH_SKIP_BUILD:-}" ]]; then
  if [[ ",${HONEST_BENCH_ONLY:-1,3}," == *,3,* ]]; then
    echo "--- building Rust image_conv"
    (cd "$ROOT/workloads/3_image_convolution/rust" && cargo build --release >/dev/null)
    echo "--- building Zig image_conv"
    (cd "$ROOT/workloads/3_image_convolution/zig" && ./build.sh >/dev/null)
    echo "--- building Perry image_conv"
    (cd "$PERRY_ROOT" && target/release/perry "$ROOT/workloads/3_image_convolution/perry/image_conv.ts" \
          -o "$ROOT/workloads/3_image_convolution/perry/image_conv" 2>&1 | tail -2)
  fi

  if [[ ",${HONEST_BENCH_ONLY:-1,3}," == *,1,* ]]; then
    echo "--- building Rust json_pipeline"
    (cd "$ROOT/workloads/1_json_pipeline/rust" && cargo build --release >/dev/null)
    echo "--- building Zig json_pipeline"
    (cd "$ROOT/workloads/1_json_pipeline/zig" && ./build.sh >/dev/null)
    echo "--- building Perry json_pipeline"
    (cd "$PERRY_ROOT" && target/release/perry "$ROOT/workloads/1_json_pipeline/perry/json_pipeline.ts" \
          -o "$ROOT/workloads/1_json_pipeline/perry/json_pipeline" 2>&1 | tail -2)
  fi

  # Workload 4 (http_fastify) is opt-in — only build it when selected, since it
  # compiles three Perry kernels and needs a one-time npm install of fastify.
  if [[ ",${HONEST_BENCH_ONLY:-1,3}," == *,4,* ]]; then
    HTTP_KERNEL_DIR="$ROOT/workloads/4_http_fastify"
    for kernel in minimal text parametric; do
      echo "--- building Perry http_fastify/$kernel"
      (cd "$PERRY_ROOT" && target/release/perry "$HTTP_KERNEL_DIR/perry/$kernel.ts" \
            -o "$HTTP_KERNEL_DIR/perry/$kernel" 2>&1 | tail -2)
    done
    echo "--- npm ci fastify (http_fastify/node)"
    (cd "$HTTP_KERNEL_DIR/node" && npm ci --ignore-scripts --no-audit --no-fund --silent)
  fi
fi

# ------------------------------ 3. fixtures -----------------------------------
if [[ ",$ONLY," == *,1,* && ! -f "$ROOT/assets/input.json" ]]; then
  echo "--- generating JSON fixture (one-time)"
  python3 scripts/gen_json.py
fi
if [[ ",$ONLY," == *,1,* && ! -f "$ROOT/assets/input_small.json" ]]; then
  # 100-record cut of the full fixture — all three languages run this
  python3 -c "
import json
with open('$ROOT/assets/input.json') as f: full = json.load(f)
with open('$ROOT/assets/input_small.json', 'w') as f:
    json.dump(full[:100], f, separators=(',',':'))
"
fi

# ------------------------------ 3.5 reference capture (#441) ------------------
# Bun is the truth source for output correctness. We capture canonical tokens
# (hash=…, checksum=…, records_in=…, dims=…) + the sha256 of any output file
# once, cache to results/expected.json, and use that to gate every measured
# run. Updated only when output semantics intentionally change (set
# HONEST_BENCH_REFRESH_EXPECTED=1 to refresh).
EXPECTED="$RESULTS_DIR/expected.json"
NODE_FLAGS="--experimental-strip-types --disable-warning=MODULE_TYPELESS_PACKAGE_JSON"
NODE_IMG="$ROOT/workloads/3_image_convolution/node/image_conv.ts"
NODE_JSON="$ROOT/workloads/1_json_pipeline/node/json_pipeline.ts"

if [[ ! -f "$EXPECTED" || -n "${HONEST_BENCH_REFRESH_EXPECTED:-}" ]]; then
  echo "--- capturing Bun reference outputs → $EXPECTED"
  python3 - "$ROOT" "$ONLY" <<'PY' > "$EXPECTED"
import json, os, subprocess, sys
ROOT, only_str = sys.argv[1], sys.argv[2]
ONLY = set(only_str.split(","))
HARNESS = os.path.join(ROOT, "harness", "capture_expected.py")
NODE_IMG  = os.path.join(ROOT, "workloads", "3_image_convolution", "node", "image_conv.ts")
NODE_JSON = os.path.join(ROOT, "workloads", "1_json_pipeline", "node", "json_pipeline.ts")
SMALL_IN  = os.path.join(ROOT, "assets", "input_small.json")
FULL_IN   = os.path.join(ROOT, "assets", "input.json")

def capture(workload, *args):
    r = subprocess.run(["python3", HARNESS, workload, *args],
                       capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(f"capture failed for {workload}:\n{r.stderr}")
        sys.exit(2)
    return json.loads(r.stdout)

out = {}
if "3" in ONLY:
    out["image_convolution"] = capture("image_convolution", "--", "bun", "run", NODE_IMG)
if "1" in ONLY:
    out["json_pipeline_small"] = capture(
        "json_pipeline_small", "--output-file=/tmp/out_bun.json",
        "--", "bun", "run", NODE_JSON, SMALL_IN, "/tmp/out_bun.json")
    out["json_pipeline_full"] = capture(
        "json_pipeline_full",  "--output-file=/tmp/out_bun.json",
        "--", "bun", "run", NODE_JSON, FULL_IN,  "/tmp/out_bun.json")

print(json.dumps(out, indent=2, sort_keys=True))
PY
fi

export HONEST_BENCH_EXPECTED_JSON="$EXPECTED"
export HONEST_BENCH_CHECK_OUTPUT="$ROOT/harness/check_output.py"

# ------------------------------ 4. run ----------------------------------------
RESULTS="$RESULTS_DIR/results.json"
# Clear & start a JSON array — we append raw rows and close it at the end.
: > "$RESULTS.rows"

run_one() {
  local workload="$1" lang="$2" bin="$3"; shift 3
  echo "--- running $workload / $lang"
  # For workloads that produce an output file, pass its path through the
  # environment so the harness can sha256 it against the cached Bun reference.
  # By the call sites' convention, the last positional is the output path.
  local out_file=""
  if [[ "$workload" == json_pipeline* && $# -ge 1 ]]; then
    out_file="${@: -1}"
  fi
  HONEST_BENCH_OUTPUT_FILE="$out_file" \
    bash "$ROOT/harness/run_bench.sh" "$workload" "$lang" "$bin" "$@" >> "$RESULTS.rows"
}

if [[ ",$ONLY," == *,3,* ]]; then
  echo "=== workload 3: image convolution ==="
  run_one image_convolution rust  "$ROOT/workloads/3_image_convolution/rust/target/release/image_conv"
  run_one image_convolution zig   "$ROOT/workloads/3_image_convolution/zig/zig-out/bin/image_conv"
  run_one image_convolution perry "$ROOT/workloads/3_image_convolution/perry/image_conv"
  run_one image_convolution node  "node" $NODE_FLAGS "$NODE_IMG"
  run_one image_convolution bun   "bun" "run" "$NODE_IMG"
fi

if [[ ",$ONLY," == *,1,* ]]; then
  echo "=== workload 1 (small): json pipeline @ 100 records ==="
  SMALL_IN="$ROOT/assets/input_small.json"
  run_one json_pipeline_small rust  "$ROOT/workloads/1_json_pipeline/rust/target/release/json_pipeline"  "$SMALL_IN" "/tmp/out_rust.json"
  run_one json_pipeline_small zig   "$ROOT/workloads/1_json_pipeline/zig/zig-out/bin/json_pipeline"       "$SMALL_IN" "/tmp/out_zig.json"
  run_one json_pipeline_small perry "$ROOT/workloads/1_json_pipeline/perry/json_pipeline"                 "$SMALL_IN" "/tmp/out_perry.json"
  run_one json_pipeline_small node  "node" $NODE_FLAGS "$NODE_JSON" "$SMALL_IN" "/tmp/out_node.json"
  run_one json_pipeline_small bun   "bun" "run" "$NODE_JSON" "$SMALL_IN" "/tmp/out_bun.json"

  echo "=== workload 1 (full): json pipeline @ 500k records ==="
  FULL_IN="$ROOT/assets/input.json"
  run_one json_pipeline_full rust  "$ROOT/workloads/1_json_pipeline/rust/target/release/json_pipeline"  "$FULL_IN" "/tmp/out_rust.json"
  run_one json_pipeline_full zig   "$ROOT/workloads/1_json_pipeline/zig/zig-out/bin/json_pipeline"       "$FULL_IN" "/tmp/out_zig.json"
  run_one json_pipeline_full perry "$ROOT/workloads/1_json_pipeline/perry/json_pipeline"                 "$FULL_IN" "/tmp/out_perry.json"
  run_one json_pipeline_full node  "node" $NODE_FLAGS "$NODE_JSON" "$FULL_IN" "/tmp/out_node.json"
  run_one json_pipeline_full bun   "bun" "run" "$NODE_JSON" "$FULL_IN" "/tmp/out_bun.json"
fi

if [[ ",$ONLY," == *,4,* ]]; then
  HTTP_KERNEL_DIR="$ROOT/workloads/4_http_fastify"
  NODE_KERNEL_DIR="$HTTP_KERNEL_DIR/node"
  # <workload> <language> <route> then the run-cmd — the harness splits the
  # run-cmd from the route on the trailing `--`.
  run_http() {
    local workload="$1" lang="$2" route="$3"; shift 3
    echo "--- running $workload / $lang"
    # Tolerate a harness exit (e.g. a kernel that SIGSEGVs under load, or never
    # starts listening) so one bad kernel doesn't abort the whole suite under
    # this script's `set -e` — matching the "record the failure, don't abort"
    # contract in the header. Already-collected rows survive and finalize runs.
    bash "$ROOT/harness/run_http_bench.sh" "$workload" "$lang" "$@" -- "$route" >> "$RESULTS.rows" \
      || echo "--- WARNING: $workload / $lang harness exited nonzero; continuing" >&2
  }
  for kernel in minimal text parametric; do
    echo "=== workload 4: http_fastify / $kernel ==="
    case "$kernel" in
      parametric) route="/users/u1" ;;
      *)          route="/" ;;
    esac
    run_http "http_fastify_$kernel" perry "$route" "$HTTP_KERNEL_DIR/perry/$kernel"
    # node resolves `--import tsx` + fastify relative to cwd, so cd into the
    # kernel dir before exec.
    run_http "http_fastify_$kernel" node  "$route" \
      bash -c "cd '$NODE_KERNEL_DIR' && exec node --import tsx ./$kernel.ts"
    run_http "http_fastify_$kernel" bun "$route" \
      bash -c "cd '$NODE_KERNEL_DIR' && exec bun run ./$kernel.ts"
  done
fi

# ------------------------------ 5. finalize -----------------------------------
python3 - <<PY > "$RESULTS"
import json
rows = []
with open("$RESULTS.rows") as f:
    for line in f:
        line = line.strip()
        if not line: continue
        try: rows.append(json.loads(line))
        except Exception: pass
print(json.dumps({"rows": rows}, indent=2))
PY
rm -f "$RESULTS.rows"

N=$(python3 -c "import json; print(len(json.load(open('$RESULTS'))['rows']))")
echo "--- wrote $RESULTS with $N rows"
echo "--- metadata: $RESULTS_DIR/metadata.json"

# ------------------------------ 6. summary (#441) -----------------------------
python3 "$ROOT/scripts/summary.py"

if [[ "$STRICT_OUTPUT" == "1" ]]; then
  N_BAD=$(python3 -c "
import json
rows = json.load(open('$RESULTS'))['rows']
print(sum(1 for r in rows if r.get('output_match') is False))")
  if [[ "$N_BAD" -gt 0 ]]; then
    echo "ERROR: --strict-output: $N_BAD measured rows had output mismatches" >&2
    echo "       see $RESULTS_DIR/summary.txt" >&2
    exit 1
  fi
  echo "--- --strict-output: all rows matched the Bun reference"
fi
