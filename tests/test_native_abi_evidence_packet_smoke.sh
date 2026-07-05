#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tmp/native-abi-evidence-smoke-$(date -u +%Y%m%dT%H%M%SZ)}"

for tool in cargo cc ar clang; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "SKIP: $tool not available"
    exit 0
  fi
done

PYTHON_BIN="${PYTHON:-}"
if [[ -z "$PYTHON_BIN" ]]; then
  if command -v python3.11 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3.11)"
  elif command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3)"
  else
    echo "SKIP: python not available"
    exit 0
  fi
fi

if ! "$PYTHON_BIN" - <<'PY'
import sys
raise SystemExit(0 if sys.version_info >= (3, 11) else 1)
PY
then
  echo "SKIP: Python 3.11+ not available"
  exit 0
fi

set +e
PYTHON="$PYTHON_BIN" "$ROOT/scripts/native_abi_evidence_packet.sh" \
  --runs 5 \
  --out "$OUT" \
  --gate
STATUS=$?
set -e

"$PYTHON_BIN" - "$OUT" "$STATUS" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
status = int(sys.argv[2])
packet_path = root / "native-abi-evidence.json"
assert packet_path.exists(), packet_path
assert (root / "native-abi-evidence.md").exists()
packet = json.loads(packet_path.read_text(encoding="utf-8"))
if status == 0:
    assert packet["status"] == "pass", packet["errors"]
else:
    assert packet["status"] == "fail", packet
for section in ("gate_matrix", "correctness", "native_call_lowering", "gc_root_safety", "release_symbol_guard", "benchmark_deltas"):
    assert section in packet, packet.keys()
PY

exit "$STATUS"
