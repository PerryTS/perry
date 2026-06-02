#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tmp/perf-frontier-smoke-$(date -u +%Y%m%dT%H%M%SZ)}"

if [[ ! -d "$ROOT/tmp/benchmark-math" ]]; then
  echo "SKIP: tmp/benchmark-math fixture is not present"
  exit 0
fi

set +e
"$ROOT/scripts/perf_frontier_gate.sh" \
  --base-ref HEAD \
  --head-ref HEAD \
  --runs 1 \
  --trace-row bench_json_roundtrip \
  --math-slice-row 01_free_function_numeric \
  --math-slice-row 02_class_method_no_field_access \
  --gate \
  --out "$OUT"
STATUS=$?
set -e

python3 - "$OUT" "$STATUS" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
status = int(sys.argv[2])
packet = json.loads((root / "perf-frontier-packet.json").read_text(encoding="utf-8"))
profile = json.loads((root / "profile_summary.json").read_text(encoding="utf-8"))

if status == 0:
    assert packet["status"] == "pass", packet["errors"]
else:
    assert packet["status"] == "fail", packet
    assert packet["errors"], packet
assert packet["refs"]["base"]["sha"] == packet["refs"]["head"]["sha"], packet["refs"]
assert "bench_json_roundtrip" in packet["direct_trace_summaries"]["head"]
assert profile["top_non_gc_costs"], profile
PY
