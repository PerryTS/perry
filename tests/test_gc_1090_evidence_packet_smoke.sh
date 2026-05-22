#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tmp/gc-1090-evidence-smoke-$(date -u +%Y%m%dT%H%M%SZ)}"

"$ROOT/scripts/gc_1090_evidence_packet.sh" \
  --base-ref HEAD \
  --head-ref HEAD \
  --runs 1 \
  --out "$OUT" \
  --skip-perf-comprehensive

python3 - "$OUT" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8"))
packet = json.loads((root / "gc-1090-packet.json").read_text(encoding="utf-8"))

assert metadata["base_sha"] == metadata["head_sha"], metadata
assert (root / "gc-1090-packet.md").exists()
assert (root / "gc-1090-packet.json").exists()

for name in ("bench_json_roundtrip", "bench_gc_pressure", "07_object_create", "12_binary_trees"):
    assert name in packet["benchmarks"], packet["benchmarks"].keys()

head = packet["copied_minor"]["head"]["summary"]
assert "fallback_reason_counts" in head
assert "conservative_pinned_bytes" in head
PY
