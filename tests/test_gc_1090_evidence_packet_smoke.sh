#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tmp/gc-1090-evidence-smoke-$(date -u +%Y%m%dT%H%M%SZ)}"

set +e
"$ROOT/scripts/gc_1090_evidence_packet.sh" \
  --base-ref HEAD \
  --head-ref HEAD \
  --runs 1 \
  --out "$OUT" \
  --gate \
  --perf-math-slice-row 01_free_function_numeric \
  --perf-math-slice-row 02_class_method_no_field_access \
  --skip-perf-comprehensive
STATUS=$?
set -e

python3 - "$OUT" "$STATUS" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
status = int(sys.argv[2])
metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
packet = json.loads((root / "gc-1090-packet.json").read_text(encoding="utf-8"))

assert metadata["base_sha"] == metadata["head_sha"], metadata
assert (root / "gc-1090-packet.md").exists()
assert (root / "gc-1090-packet.json").exists()
assert (root / "old-page-policy.json").exists()
assert (root / "perf-frontier" / "perf-frontier-packet.json").exists()
if status == 0:
    assert packet["status"] == "pass", packet["errors"]
else:
    assert packet["status"] == "fail", packet

for name in ("bench_json_roundtrip", "bench_gc_pressure", "07_object_create", "12_binary_trees"):
    assert name in packet["benchmarks"], packet["benchmarks"].keys()

head = packet["copied_minor"]["head"]["summary"]
assert "fallback_reason_counts" in head
assert "conservative_pinned_bytes" in head
assert "compiled_frame_conservative_pinned_bytes" in head
assert "conservative_stack_truncated_cycles" in head
assert "old_page_policy" in packet
assert "bench_json_roundtrip" in packet["old_page_policy"]
PY
