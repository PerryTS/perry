#!/usr/bin/env bash
# Tier 13 - native_abi_evidence
#
# Runs the native ABI evidence packet smoke in gate mode. In a full release
# sweep, tier 00 has normally built target/release/perry already, so prefer
# that binary to avoid rebuilding the compiler. Standalone tier runs still
# work: the packet script falls back to building Perry if no binary is provided.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 13)"
LOG="$TIER_DIR/native_abi_evidence.log"
SUMMARY="$TIER_DIR/summary.json"
PACKET_OUT="$TIER_DIR/packet"

start="$(date +%s)"

perry_env=()
if [[ -n "${PERRY_BIN:-}" ]]; then
    perry_env=(PERRY_BIN="$PERRY_BIN")
elif [[ -x "$REPO_ROOT/target/release/perry" ]]; then
    perry_env=(PERRY_BIN="$REPO_ROOT/target/release/perry")
elif [[ -x "$REPO_ROOT/target/debug/perry" ]]; then
    perry_env=(PERRY_BIN="$REPO_ROOT/target/debug/perry")
fi

{
    echo "tier 13 native_abi_evidence"
    echo "packet out: $PACKET_OUT"
    if [[ "${#perry_env[@]}" -gt 0 ]]; then
        echo "perry: ${perry_env[0]#PERRY_BIN=}"
    else
        echo "perry: (packet script will resolve/build)"
    fi
    echo
} > "$LOG"

set +e
(
    cd "$REPO_ROOT"
    env "${perry_env[@]}" bash tests/test_native_abi_evidence_packet_smoke.sh "$PACKET_OUT"
) >> "$LOG" 2>&1
rc=$?
set -e

end="$(date +%s)"
dur="$((end - start))"

if [[ "$rc" -eq 0 ]] && grep -q '^SKIP:' "$LOG"; then
    reason="$(grep '^SKIP:' "$LOG" | tail -1 | sed 's/^SKIP:[[:space:]]*//')"
    cat > "$SUMMARY" <<EOF
{"script": "tier13_native_abi_evidence.sh", "passed": 0, "failed": 0, "skipped": 1, "reason": "$(sweep_json_escape "$reason")"}
EOF
    sweep_tier_emit "$OUT" 13 "native_abi_evidence" "SKIP" "$dur" "$reason"
elif [[ "$rc" -eq 0 ]]; then
    cat > "$SUMMARY" <<EOF
{"script": "tier13_native_abi_evidence.sh", "passed": 1, "failed": 0, "skipped": 0}
EOF
    sweep_tier_emit "$OUT" 13 "native_abi_evidence" "PASS" "$dur" "native ABI evidence packet smoke passed"
else
    packet_status="missing"
    if [[ -f "$PACKET_OUT/native-abi-evidence.json" ]]; then
        packet_status="$(
            python3 - "$PACKET_OUT/native-abi-evidence.json" <<'PY' 2>/dev/null || echo unknown
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")).get("status", "unknown"))
PY
        )"
    fi
    cat > "$SUMMARY" <<EOF
{"script": "tier13_native_abi_evidence.sh", "passed": 0, "failed": 1, "skipped": 0, "exit_code": $rc, "packet_status": "$(sweep_json_escape "$packet_status")"}
EOF
    sweep_tier_emit "$OUT" 13 "native_abi_evidence" "FAIL" "$dur" \
        "evidence packet smoke exited $rc (packet status: $packet_status)"
fi
