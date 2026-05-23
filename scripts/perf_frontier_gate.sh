#!/usr/bin/env bash
# Collect exact-ref Perry performance frontier evidence and strict gate results.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE_REF="origin/main"
HEAD_REF="HEAD"
RUNS=5
OUT=""
GATE=0
KEEP_WORKTREES=0
UPDATE_BASELINE=""
BASELINE_IN=""
SKIP_PROFILE=0
TRACE_ROWS=()
MATH_SLICE_ROWS=()

usage() {
  cat <<'EOF'
Usage: scripts/perf_frontier_gate.sh [options]

Options:
  --base-ref REF              Comparison base (default: origin/main)
  --head-ref REF              Head/feature ref (default: HEAD)
  --runs N                    Benchmark samples per benchmark (default: 5)
  --out PATH                  Output root (default: tmp/perf-frontier-<utc>)
  --gate                      Fail on missing strict evidence
  --trace-row NAME            Benchmark suite row to rerun with PERRY_GC_TRACE=1
  --math-slice-row NAME       Limit benchmark-math slices to a named row/path
  --baseline-in PATH          Reference a locked baseline snapshot in this packet
  --update-baseline PATH      Write a reusable baseline snapshot
  --skip-profile              Skip typed-row profiler collection
  --keep-worktrees            Keep detached worktrees after the run
  -h, --help                  Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-ref) BASE_REF="$2"; shift 2 ;;
    --head-ref) HEAD_REF="$2"; shift 2 ;;
    --runs) RUNS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --gate) GATE=1; shift ;;
    --trace-row) TRACE_ROWS+=("$2"); shift 2 ;;
    --math-slice-row) MATH_SLICE_ROWS+=("$2"); shift 2 ;;
    --baseline-in) BASELINE_IN="$2"; shift 2 ;;
    --update-baseline) UPDATE_BASELINE="$2"; shift 2 ;;
    --skip-profile) SKIP_PROFILE=1; shift ;;
    --keep-worktrees) KEEP_WORKTREES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [[ "$RUNS" -lt 1 ]]; then
  echo "--runs must be a positive integer" >&2
  exit 2
fi

if [[ ${#TRACE_ROWS[@]} -eq 0 ]]; then
  TRACE_ROWS=(
    "bench_json_roundtrip"
    "bench_gc_pressure"
    "07_object_create"
    "12_binary_trees"
  )
fi
MATH_SLICE_ROWS_JOINED="$(printf '%s\n' "${MATH_SLICE_ROWS[@]}")"

if [[ -z "$OUT" ]]; then
  OUT="tmp/perf-frontier-$(date -u +%Y%m%dT%H%M%SZ)"
fi

cd "$ROOT"

BASE_SHA="$(git rev-parse --verify "$BASE_REF^{commit}")"
HEAD_SHA="$(git rev-parse --verify "$HEAD_REF^{commit}")"
if ! [[ "$BASE_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "base ref did not resolve to an exact 40-char SHA: $BASE_REF -> $BASE_SHA" >&2
  exit 2
fi
if ! [[ "$HEAD_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "head ref did not resolve to an exact 40-char SHA: $HEAD_REF -> $HEAD_SHA" >&2
  exit 2
fi

OUT_ABS="$(python3 - "$ROOT" "$OUT" <<'PY'
import os
import sys
root, out = sys.argv[1], sys.argv[2]
if not os.path.isabs(out):
    out = os.path.join(root, out)
print(os.path.abspath(out))
PY
)"
OUT_REL="$(python3 - "$ROOT" "$OUT_ABS" <<'PY'
import os
import sys
root, out = map(os.path.abspath, sys.argv[1:3])
rel = os.path.relpath(out, root)
if rel.startswith(".."):
    raise SystemExit(1)
print(rel)
PY
)" || {
  echo "output path must be inside the repository: $OUT_ABS" >&2
  exit 2
}

if ! git check-ignore -q -- "$OUT_REL"; then
  echo "output path must be ignored by git: $OUT_REL" >&2
  exit 2
fi

if [[ -n "$(git ls-files -- "$OUT_REL" "$OUT_REL/**")" ]]; then
  echo "output path contains tracked files; choose a fresh ignored path: $OUT_REL" >&2
  exit 2
fi

mkdir -p "$OUT_ABS"

BASE_WT="$OUT_ABS/worktrees/base"
HEAD_WT="$OUT_ABS/worktrees/head"
METADATA="$OUT_ABS/metadata.json"

cleanup() {
  if [[ "$KEEP_WORKTREES" -eq 0 ]]; then
    git worktree remove --force "$BASE_WT" >/dev/null 2>&1 || true
    git worktree remove --force "$HEAD_WT" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

write_metadata() {
  python3 - "$METADATA" "$BASE_REF" "$HEAD_REF" "$BASE_SHA" "$HEAD_SHA" "$RUNS" "$GATE" "$BASELINE_IN" "$MATH_SLICE_ROWS_JOINED" "${TRACE_ROWS[@]}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
base_ref, head_ref, base_sha, head_sha = sys.argv[2:6]
runs = int(sys.argv[6])
gate = sys.argv[7] == "1"
baseline_in = sys.argv[8]
math_slice_rows = [row for row in sys.argv[9].split("\n") if row]
trace_rows = sys.argv[10:]
existing = {}
if path.exists():
    existing = json.loads(path.read_text(encoding="utf-8"))
update = {
    "schema_version": 1,
    "generated_at": existing.get("generated_at") or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "base_ref": base_ref,
    "head_ref": head_ref,
    "base_sha": base_sha,
    "head_sha": head_sha,
    "runs": runs,
    "gate": gate,
    "trace_rows": trace_rows,
    "math_slice_rows": math_slice_rows,
    "commands": existing.get("commands", {}),
    "tool_versions": existing.get("tool_versions", {}),
}
if baseline_in:
    update["baseline_in"] = baseline_in
else:
    existing.pop("baseline_in", None)
existing.update(update)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(existing, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

record_command() {
  local label="$1"
  local name="$2"
  local status="$3"
  local exit_code="$4"
  local log_path="${5:-}"
  local reason="${6:-}"
  python3 - "$METADATA" "$label" "$name" "$status" "$exit_code" "$log_path" "$reason" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
label, name, status, exit_code, log_path, reason = sys.argv[2:8]
data = json.loads(path.read_text(encoding="utf-8"))
commands = data.setdefault("commands", {})
label_commands = commands.setdefault(label, {})
entry = {
    "status": status,
    "exit_code": int(exit_code),
    "finished_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
}
if log_path:
    entry["log"] = log_path
if reason:
    entry["reason"] = reason
label_commands[name] = entry
path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

capture_tool_versions() {
  python3 - "$METADATA" "$ROOT" <<'PY'
import json
import platform
import subprocess
import sys
from pathlib import Path

metadata = Path(sys.argv[1])
root = Path(sys.argv[2])

def run(cmd):
    try:
        completed = subprocess.run(cmd, cwd=root, text=True, capture_output=True, timeout=15)
    except Exception as exc:
        return {"available": False, "error": str(exc)}
    return {
        "available": completed.returncode == 0,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip().splitlines()[:3],
        "stderr": completed.stderr.strip().splitlines()[:3],
    }

data = json.loads(metadata.read_text(encoding="utf-8"))
data["tool_versions"] = {
    "platform": platform.platform(),
    "python": sys.version.split()[0],
    "git": run(["git", "--version"]),
    "cargo": run(["cargo", "--version"]),
    "rustc": run(["rustc", "--version"]),
    "node": run(["node", "--version"]),
    "time": run(["/usr/bin/time", "-p", "true"]),
    "sample": run(["/usr/bin/sample", "-h"]) if Path("/usr/bin/sample").exists() else {"available": False},
    "perf": run(["perf", "--version"]),
}
metadata.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

command_status() {
  local label="$1"
  local name="$2"
  python3 - "$METADATA" "$label" "$name" <<'PY'
import json
import sys
data = json.load(open(sys.argv[1]))
print(data.get("commands", {}).get(sys.argv[2], {}).get(sys.argv[3], {}).get("status", "missing"))
PY
}

run_logged() {
  local label="$1"
  local name="$2"
  local worktree="$3"
  local log="$4"
  shift 4
  mkdir -p "$(dirname "$log")"
  echo "=== $label: $name ==="
  set +e
  (
    cd "$worktree"
    "$@"
  ) >"$log" 2>&1
  local code=$?
  set -e
  local status="pass"
  if [[ "$code" -ne 0 ]]; then
    status="fail"
  fi
  record_command "$label" "$name" "$status" "$code" "$log" ""
  echo "  $status (exit=$code, log=$log)"
  return 0
}

run_direct_traces() {
  local label="$1"
  local worktree="$2"
  local label_out="$3"
  local trace_root="$label_out/direct-traces"
  local log="$label_out/logs/direct-traces.log"
  local code=0
  mkdir -p "$trace_root/bin" "$trace_root/stdout" "$trace_root/stderr" "$trace_root/compile" "$trace_root/summaries" "$label_out/logs"
  : >"$log"

  for row in "${TRACE_ROWS[@]}"; do
    local src="$worktree/benchmarks/suite/$row.ts"
    local bin="$trace_root/bin/$row"
    local compile_log="$trace_root/compile/$row.log"
    local stdout="$trace_root/stdout/$row.out"
    local stderr="$trace_root/stderr/$row.trace.log"
    local summary="$trace_root/summaries/$row.json"
    {
      echo "=== direct trace: $row ==="
      if [[ ! -f "$src" ]]; then
        echo "missing source: $src"
        code=1
        continue
      fi
      if ! "$worktree/target/release/perry" "$src" -o "$bin" >"$compile_log" 2>&1; then
        echo "compile failed: $compile_log"
        code=1
        continue
      fi
      if ! env PERRY_GC_TRACE=1 "$bin" >"$stdout" 2>"$stderr"; then
        echo "trace run failed: $stderr"
        code=1
        continue
      fi
      if ! python3 "$ROOT/scripts/perf_frontier_report.py" summarize-trace \
        --trace "$stderr" \
        --stdout "$stdout" \
        --workload "$row" \
        --json-out "$summary" \
        --copied-trace-path "$stderr"; then
        echo "trace summary failed: $summary"
        code=1
        continue
      fi
    } >>"$log" 2>&1
  done

  local status="pass"
  if [[ "$code" -ne 0 ]]; then
    status="fail"
  fi
  record_command "$label" "direct_traces" "$status" "$code" "$log" ""
  echo "=== $label: direct_traces ==="
  echo "  $status (exit=$code, log=$log)"
}

copy_math_outputs() {
  local fixture="$1"
  local out_dir="$2"
  mkdir -p "$out_dir"
  for file in node.out perry.out compile.out; do
    if [[ -f "$fixture/$file" ]]; then
      cp "$fixture/$file" "$out_dir/$file"
    fi
  done
  if [[ -d "$fixture/slice-out" ]]; then
    rm -rf "$out_dir/slice-out"
    cp -R "$fixture/slice-out" "$out_dir/slice-out"
  fi
}

resolve_math_slice_source() {
  local fixture="$1"
  local row="$2"
  if [[ "$row" == *.ts && -f "$fixture/$row" ]]; then
    printf '%s\n' "$row"
    return 0
  fi
  if [[ -f "$fixture/slices/$row.ts" ]]; then
    printf '%s\n' "slices/$row.ts"
    return 0
  fi
  local match
  match="$(find "$fixture/slices" -maxdepth 1 -name "*${row}.ts" -print 2>/dev/null | head -1)"
  if [[ -n "$match" ]]; then
    printf '%s\n' "${match#$fixture/}"
    return 0
  fi
  if [[ "$row" == "benchmark" || "$row" == "benchmark.ts" ]]; then
    printf '%s\n' "benchmark.ts"
    return 0
  fi
  return 1
}

run_selected_math_slices() {
  local fixture="$1"
  local perry_bin="$2"
  local out_dir="$fixture/slice-out"
  local runs_dir="$out_dir/runs"
  mkdir -p "$out_dir" "$runs_dir"
  rm -f "$runs_dir"/*.out "$out_dir"/slice-*

  printf "%-34s %12s %12s %12s %12s\n" "Benchmark" "Node ms" "Perry ms" "Perry/Node" "Checksum Δ"
  printf "%-34s %12s %12s %12s %12s\n" "---------" "-------" "--------" "----------" "----------"

  local row src base binary node_out perry_out compile_out
  for row in "${MATH_SLICE_ROWS[@]}"; do
    if ! src="$(resolve_math_slice_source "$fixture" "$row")"; then
      echo "missing benchmark-math slice row: $row" >&2
      return 1
    fi
    base="$(basename "$src" .ts)"
    binary="$out_dir/slice-$base"
    node_out="$runs_dir/$base.node.out"
    perry_out="$runs_dir/$base.perry.out"
    compile_out="$runs_dir/$base.compile.out"

    node --experimental-strip-types "$src" > "$node_out"
    "$perry_bin" "$src" -o "$binary" > "$compile_out" 2>&1
    "$binary" > "$perry_out"

    python3 - "$node_out" "$perry_out" <<'PY'
import math
import sys
from pathlib import Path

def fields(path):
    data = {}
    for line in Path(path).read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            data[key] = value
    return data

node = fields(sys.argv[1])
perry = fields(sys.argv[2])
bench = node.get("bench") or Path(sys.argv[1]).name.split(".")[0]
node_ms = float(node.get("medianMs", "nan"))
perry_ms = float(perry.get("medianMs", "nan"))
node_sum = float(node.get("checksum", "nan"))
perry_sum = float(perry.get("checksum", "nan"))
ratio = "-" if not math.isfinite(node_ms) or node_ms == 0 else f"{perry_ms / node_ms:.2f}x"
delta = "nonfinite"
if math.isfinite(node_sum) and math.isfinite(perry_sum):
    delta = f"{abs(node_sum - perry_sum) / max(abs(node_sum), abs(perry_sum), 1.0):.2e}"
print(f"{bench:<34s} {node_ms:12.3f} {perry_ms:12.3f} {ratio:>12s} {delta:>12s}")
PY
  done

  echo
  echo "Raw logs: $runs_dir"
}

run_math_for_label() {
  local label="$1"
  local worktree="$2"
  local label_out="$3"
  local fixture="$ROOT/tmp/benchmark-math"
  local math_out="$label_out/benchmark-math"
  local log="$label_out/logs/benchmark-math.log"
  local code=0
  mkdir -p "$math_out" "$label_out/logs"
  : >"$log"

  if [[ ! -d "$fixture" ]]; then
    record_command "$label" "benchmark_math" "fail" 1 "$log" "tmp/benchmark-math fixture missing"
    return
  fi

  {
    echo "=== benchmark-math full ==="
    (cd "$fixture" && env PERRY_BIN="$worktree/target/release/perry" ./run_benchmark.sh)
  } >>"$log" 2>&1 || code=1
  copy_math_outputs "$fixture" "$math_out"

  {
    echo "=== benchmark-math slices ==="
    if [[ ${#MATH_SLICE_ROWS[@]} -gt 0 ]]; then
      (cd "$fixture" && run_selected_math_slices "$fixture" "$worktree/target/release/perry")
    else
      (cd "$fixture" && env PERRY_BIN="$worktree/target/release/perry" ./run_slices.sh)
    fi
  } >>"$log" 2>&1 || code=1
  copy_math_outputs "$fixture" "$math_out"

  local results_arg=()
  if [[ "$label" == "head" ]]; then
    results_arg=(--results-md-out "$fixture/RESULTS.md")
  fi
  if ! python3 "$ROOT/scripts/perf_frontier_report.py" math-json \
    --repo-root "$ROOT" \
    --label "$label" \
    --out-dir "$math_out" \
    --math-json-out "$math_out/math-benchmark.json" \
    --slice-json-out "$math_out/slice-results.json" \
    "${results_arg[@]}" >>"$log" 2>&1; then
    code=1
  fi

  local status="pass"
  if [[ "$code" -ne 0 ]]; then
    status="fail"
  fi
  record_command "$label" "benchmark_math" "$status" "$code" "$log" ""
  echo "=== $label: benchmark_math ==="
  echo "  $status (exit=$code, log=$log)"
}

run_for_label() {
  local label="$1"
  local worktree="$2"
  local label_out="$OUT_ABS/$label"
  mkdir -p "$label_out/logs" "$label_out/benchmarks"

  run_logged "$label" "build" "$worktree" "$label_out/logs/build.log" \
    cargo build --release -p perry

  if [[ "$(command_status "$label" build)" == "fail" ]]; then
    record_command "$label" "memory_stability" "skipped" 0 "" "build failed"
    record_command "$label" "benchmarks" "skipped" 0 "" "build failed"
    record_command "$label" "direct_traces" "skipped" 0 "" "build failed"
    record_command "$label" "benchmark_math" "skipped" 0 "" "build failed"
    return
  fi

  run_logged "$label" "memory_stability" "$worktree" "$label_out/logs/memory-stability.command.log" \
    env "PERRY_GC_EVIDENCE_DIR=$label_out/memory" scripts/run_memory_stability_tests.sh

  run_logged "$label" "benchmarks" "$worktree" "$label_out/logs/benchmarks-full-runs${RUNS}.log" \
    benchmarks/compare.sh --full --runs "$RUNS" --warn-only --json-out "$label_out/benchmarks/full.json"

  run_direct_traces "$label" "$worktree" "$label_out"
  run_math_for_label "$label" "$worktree" "$label_out"
}

choose_profile_row() {
  python3 - "$OUT_ABS/head/benchmark-math/slice-results.json" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
if not path.exists():
    raise SystemExit(1)
rows = json.loads(path.read_text(encoding="utf-8")).get("rows", [])
rows = [row for row in rows if isinstance(row.get("perry_to_node_ratio"), (int, float)) and row.get("source")]
if not rows:
    raise SystemExit(1)
row = max(rows, key=lambda item: item["perry_to_node_ratio"])
print(row["name"])
print(row["source"])
PY
}

make_profile_source() {
  local source="$1"
  local dest="$2"
  python3 - "$source" "$dest" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
text = src.read_text(encoding="utf-8")

def bump_iterations(match):
    value = int(match.group(1))
    return f"const ITERATIONS: number = {max(value * 8, value + 1)};"

text = re.sub(r"const ITERATIONS: number = ([0-9]+);", bump_iterations, text, count=1)
text = re.sub(r"const WARMUP_RUNS: number = [0-9]+;", "const WARMUP_RUNS: number = 0;", text, count=1)
text = re.sub(r"const MEASURE_RUNS: number = [0-9]+;", "const MEASURE_RUNS: number = 1;", text, count=1)
dst.parent.mkdir(parents=True, exist_ok=True)
dst.write_text(text, encoding="utf-8")
PY
}

run_profile() {
  if [[ "$SKIP_PROFILE" -eq 1 ]]; then
    record_command "head" "profile" "skipped" 0 "" "skipped by --skip-profile"
    python3 - "$OUT_ABS/profile_summary.json" <<'PY'
import json
import sys
from pathlib import Path
Path(sys.argv[1]).write_text(json.dumps({
  "schema_version": 1,
  "status": "skipped",
  "requested": False,
  "reason": "skipped by --skip-profile",
  "top_non_gc_costs": []
}, indent=2) + "\n", encoding="utf-8")
PY
    return
  fi

  local profile_out="$OUT_ABS/profile"
  local log="$profile_out/profile.log"
  mkdir -p "$profile_out"
  : >"$log"

  local selected
  if ! selected="$(choose_profile_row 2>>"$log")"; then
    record_command "head" "profile" "fail" 1 "$log" "could not choose typed row"
    return
  fi
  local row_name source
  row_name="$(printf '%s\n' "$selected" | sed -n '1p')"
  source="$(printf '%s\n' "$selected" | sed -n '2p')"
  local profile_source="$profile_out/profile-${row_name}.ts"
  local bin="$profile_out/profile-${row_name}"
  local compile_log="$profile_out/compile.log"
  local raw="$profile_out/profile.raw.txt"
  local stdout="$profile_out/profile.stdout"
  local stderr="$profile_out/profile.stderr"
  local tool=""
  local code=0

  make_profile_source "$source" "$profile_source"
  if ! env PERRY_DEBUG_SYMBOLS=1 "$HEAD_WT/target/release/perry" "$profile_source" -o "$bin" >"$compile_log" 2>&1; then
    record_command "head" "profile" "fail" 1 "$log" "profile compile failed"
    return
  fi

  if [[ -x /usr/bin/sample ]]; then
    tool="sample"
    "$bin" >"$stdout" 2>"$stderr" &
    local pid=$!
    sleep 0.25
    if kill -0 "$pid" >/dev/null 2>&1; then
      /usr/bin/sample "$pid" 2 -file "$raw" >>"$log" 2>&1 || code=1
    else
      echo "profile target exited before sample attached" >>"$log"
      code=1
    fi
    wait "$pid" >>"$log" 2>&1 || true
  elif command -v perf >/dev/null 2>&1; then
    tool="perf"
    if ! perf record -F 99 -g -o "$profile_out/perf.data" -- "$bin" >"$stdout" 2>"$stderr"; then
      code=1
    fi
    if ! perf report --stdio -i "$profile_out/perf.data" >"$raw" 2>>"$log"; then
      code=1
    fi
  else
    record_command "head" "profile" "fail" 1 "$log" "no supported profiler found"
    return
  fi

  if ! python3 "$ROOT/scripts/perf_frontier_report.py" profile-summary \
    --raw "$raw" \
    --row "$row_name" \
    --tool "$tool" \
    --source "$source" \
    --json-out "$OUT_ABS/profile_summary.json" >>"$log" 2>&1; then
    code=1
  fi

  local status="pass"
  if [[ "$code" -ne 0 ]]; then
    status="fail"
  fi
  record_command "head" "profile" "$status" "$code" "$log" ""
  echo "=== head: profile ==="
  echo "  $status (exit=$code, log=$log)"
}

write_metadata
capture_tool_versions

echo "=== Perry perf frontier evidence ==="
echo "base: $BASE_REF -> $BASE_SHA"
echo "head: $HEAD_REF -> $HEAD_SHA"
echo "out:  $OUT_ABS"

mkdir -p "$OUT_ABS/worktrees"
git worktree add --detach "$BASE_WT" "$BASE_SHA"
git worktree add --detach "$HEAD_WT" "$HEAD_SHA"

run_for_label "base" "$BASE_WT"
run_for_label "head" "$HEAD_WT"
run_profile

PACKET_ARGS=(
  packet
  --root "$OUT_ABS"
  --json-out "$OUT_ABS/perf-frontier-packet.json"
  --md-out "$OUT_ABS/perf-frontier-packet.md"
  --classification-out "$OUT_ABS/classification.json"
)
if [[ "$GATE" -eq 1 ]]; then
  PACKET_ARGS+=(--gate)
fi
if [[ -n "$BASELINE_IN" ]]; then
  PACKET_ARGS+=(--baseline-in "$BASELINE_IN")
fi
if [[ -n "$UPDATE_BASELINE" ]]; then
  PACKET_ARGS+=(--baseline-out "$UPDATE_BASELINE")
fi

set +e
python3 "$ROOT/scripts/perf_frontier_report.py" "${PACKET_ARGS[@]}"
REPORT_EXIT=$?
set -e

echo ""
echo "packet markdown: $OUT_ABS/perf-frontier-packet.md"
echo "packet json:     $OUT_ABS/perf-frontier-packet.json"
echo "classification:  $OUT_ABS/classification.json"
echo "profile summary: $OUT_ABS/profile_summary.json"
exit "$REPORT_EXIT"
