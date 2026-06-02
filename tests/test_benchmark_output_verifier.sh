#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERIFIER="$ROOT/benchmarks/verify_benchmark_output.py"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

assert_json() {
  local report="$1"
  local expression="$2"
  python3 - "$report" "$expression" <<'PY'
import json
import sys

report_path, expression = sys.argv[1], sys.argv[2]
data = json.load(open(report_path))
if not eval(expression, {"data": data}):
    raise SystemExit(f"assertion failed: {expression}\nreport={data!r}")
PY
}

cat >"$TMP_DIR/json-node.out" <<'EOF'
json_roundtrip:283
checksum:53735550
EOF

cat >"$TMP_DIR/json-perry.out" <<'EOF'
json_roundtrip:20
checksum:11842140
EOF

if python3 "$VERIFIER" \
  --expected "$TMP_DIR/json-node.out" \
  --actual "$TMP_DIR/json-perry.out" \
  --json-out "$TMP_DIR/json-report.json"; then
  echo "expected bench_json_roundtrip mismatch to fail" >&2
  exit 1
fi
assert_json "$TMP_DIR/json-report.json" "data['status'] == 'fail'"
assert_json "$TMP_DIR/json-report.json" "'checksum:53735550' in data['expected_lines']"
assert_json "$TMP_DIR/json-report.json" "'checksum:11842140' in data['actual_lines']"

cat >"$TMP_DIR/array-node.out" <<'EOF'
array_grow:9
length:2000000
checksum:2998500000
EOF

cat >"$TMP_DIR/array-perry.out" <<'EOF'
array_grow:263
length:2000000
checksum:2998500000
EOF

python3 "$VERIFIER" \
  --expected "$TMP_DIR/array-node.out" \
  --actual "$TMP_DIR/array-perry.out" \
  --json-out "$TMP_DIR/array-report.json"
assert_json "$TMP_DIR/array-report.json" "data['status'] == 'pass'"
assert_json "$TMP_DIR/array-report.json" "data['expected_lines'] == ['length:2000000', 'checksum:2998500000']"

cat >"$TMP_DIR/fibonacci-node.out" <<'EOF'
fibonacci:123
EOF

cat >"$TMP_DIR/fibonacci-perry.out" <<'EOF'
fibonacci:456
EOF

python3 "$VERIFIER" \
  --expected "$TMP_DIR/fibonacci-node.out" \
  --actual "$TMP_DIR/fibonacci-perry.out" \
  --json-out "$TMP_DIR/fibonacci-report.json"
assert_json "$TMP_DIR/fibonacci-report.json" "data['status'] == 'unchecked'"
assert_json "$TMP_DIR/fibonacci-report.json" "data['expected_lines'] == []"
