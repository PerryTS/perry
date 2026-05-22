#!/usr/bin/env bash
# Build an exact-head #1090 GC evidence packet from clean detached worktrees.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE_REF="origin/main"
HEAD_REF="HEAD"
RUNS=5
OUT=""
SKIP_PERF_COMPREHENSIVE=0
KEEP_WORKTREES=0

usage() {
  cat <<'EOF'
Usage: scripts/gc_1090_evidence_packet.sh [options]

Options:
  --base-ref REF                 Comparison base (default: origin/main)
  --head-ref REF                 Head/PR ref (default: HEAD)
  --runs N                       Benchmark samples per benchmark (default: 5)
  --out PATH                     Output root (default: tmp/gc-1090-evidence-<utc>)
  --skip-perf-comprehensive      Skip optional perf-comprehensive probe
  --keep-worktrees               Keep detached worktrees after the run
  -h, --help                     Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-ref) BASE_REF="$2"; shift 2 ;;
    --head-ref) HEAD_REF="$2"; shift 2 ;;
    --runs) RUNS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --skip-perf-comprehensive) SKIP_PERF_COMPREHENSIVE=1; shift ;;
    --keep-worktrees) KEEP_WORKTREES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [[ "$RUNS" -lt 1 ]]; then
  echo "--runs must be a positive integer" >&2
  exit 2
fi

if [[ -z "$OUT" ]]; then
  OUT="tmp/gc-1090-evidence-$(date -u +%Y%m%dT%H%M%SZ)"
fi

cd "$ROOT"

BASE_SHA="$(git rev-parse --verify "$BASE_REF^{commit}")"
HEAD_SHA="$(git rev-parse --verify "$HEAD_REF^{commit}")"
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
try:
    rel = os.path.relpath(out, root)
except ValueError:
    raise SystemExit(1)
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
  python3 - "$METADATA" "$BASE_REF" "$HEAD_REF" "$BASE_SHA" "$HEAD_SHA" "$RUNS" "$SKIP_PERF_COMPREHENSIVE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
existing = {}
if path.exists():
    existing = json.loads(path.read_text(encoding="utf-8"))
existing.update({
    "schema_version": 1,
    "generated_at": existing.get("generated_at") or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "base_ref": sys.argv[2],
    "head_ref": sys.argv[3],
    "base_sha": sys.argv[4],
    "head_sha": sys.argv[5],
    "runs": int(sys.argv[6]),
    "skip_perf_comprehensive": sys.argv[7] == "1",
    "commands": existing.get("commands", {}),
})
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

write_metadata

echo "=== #1090 exact-head GC evidence packet ==="
echo "base: $BASE_REF -> $BASE_SHA"
echo "head: $HEAD_REF -> $HEAD_SHA"
echo "out:  $OUT_ABS"

mkdir -p "$OUT_ABS/worktrees"
git worktree add --detach "$BASE_WT" "$BASE_SHA"
git worktree add --detach "$HEAD_WT" "$HEAD_SHA"

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
    return
  fi

  run_logged "$label" "memory_stability" "$worktree" "$label_out/logs/memory-stability.command.log" \
    env "PERRY_GC_EVIDENCE_DIR=$label_out/memory" scripts/run_memory_stability_tests.sh

  run_logged "$label" "benchmarks" "$worktree" "$label_out/logs/benchmarks-full-runs${RUNS}.log" \
    benchmarks/compare.sh --full --runs "$RUNS" --json-out "$label_out/benchmarks/full.json"

  run_perf_comprehensive "$label" "$worktree" "$label_out"
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

discover_perf_command() {
  local worktree="$1"
  if [[ -x "$worktree/scripts/run_perf_comprehensive.sh" ]]; then
    printf '%s\n' "scripts/run_perf_comprehensive.sh"
    return 0
  fi
  if [[ -x "$worktree/scripts/perf-comprehensive.sh" ]]; then
    printf '%s\n' "scripts/perf-comprehensive.sh"
    return 0
  fi
  return 1
}

run_perf_comprehensive() {
  local label="$1"
  local worktree="$2"
  local label_out="$3"
  local log="$label_out/logs/perf-comprehensive.log"

  if [[ "$SKIP_PERF_COMPREHENSIVE" -eq 1 ]]; then
    record_command "$label" "perf_comprehensive" "skipped" 0 "" "skipped by --skip-perf-comprehensive"
    return
  fi

  local cmd
  if ! cmd="$(discover_perf_command "$worktree")"; then
    record_command "$label" "perf_comprehensive" "skipped" 0 "" "command not found"
    return
  fi

  run_logged "$label" "perf_comprehensive" "$worktree" "$log" "$cmd"
}

run_for_label "base" "$BASE_WT"
run_for_label "head" "$HEAD_WT"

set +e
python3 "$ROOT/scripts/gc_1090_evidence_report.py" \
  --root "$OUT_ABS" \
  --json-out "$OUT_ABS/gc-1090-packet.json" \
  --md-out "$OUT_ABS/gc-1090-packet.md"
REPORT_EXIT=$?
set -e

echo ""
echo "packet markdown: $OUT_ABS/gc-1090-packet.md"
echo "packet json:     $OUT_ABS/gc-1090-packet.json"
exit "$REPORT_EXIT"
