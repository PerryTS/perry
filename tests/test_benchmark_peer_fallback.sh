#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NODE_REAL="$(command -v node)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Keep Node on PATH but deliberately omit the user's Bun installation. The
# fake compiler emits Node-backed executables so this test exercises the real
# compare.sh collection/correctness path without requiring a Perry build.
mkdir -p "$TMP/bin"
ln -s "$NODE_REAL" "$TMP/bin/node"

FAKE_PERRY="$TMP/perry"
cat >"$FAKE_PERRY" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  echo "perry-fallback-fixture 1.0.0"
  exit 0
fi
source_file="$1"
shift
output_file=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-o" ]]; then
    output_file="$2"
    shift 2
  else
    shift
  fi
done
source_file="$(cd "$(dirname "$source_file")" && pwd)/$(basename "$source_file")"
source_name="$(basename "$source_file")"
printf '#!/usr/bin/env bash\n%q %q\nstatus=$?\nif [[ "${PERRY_FAKE_FAIL_SOURCE:-}" == %q ]]; then exit 7; fi\nexit "$status"\n' \
  "$PERRY_FAKE_NODE" "$source_file" "$source_name" >"$output_file"
chmod +x "$output_file"
SH
chmod +x "$FAKE_PERRY"

PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
PERRY_BIN="$FAKE_PERRY" \
PERRY_FAKE_NODE="$NODE_REAL" \
  "$ROOT/benchmarks/compare.sh" \
    --quick \
    --runs 2 \
    --warn-only \
    --json-out "$TMP/current.json" \
    >"$TMP/output.txt"

python3 - "$TMP/current.json" <<'PY'
import json
import sys

artifact = json.load(open(sys.argv[1], encoding="utf-8"))
assert artifact["schema_version"] == 2
assert artifact["run_config"]["requested_samples"] == 2
assert artifact["runtimes"]["node"]["available"] is True
assert artifact["runtimes"]["bun"]["available"] is False
assert artifact["runtimes"]["bun"]["command"] == ["bun", "run", "<source.ts>"]
for entry in artifact["benchmarks"].values():
    assert "bun" not in entry["runtimes"]
    assert entry["ratios"]["perry_to_bun"] is None
    assert entry["correctness"]["status"] == "pass"
    assert entry["correctness"]["reference"] == "node"
    assert entry["runtimes"]["perry"]["wall_ms"]["sample_count"] == 2
    assert entry["runtimes"]["node"]["wall_ms"]["sample_count"] == 2
PY

# A process that emits valid timing/correctness output and then exits nonzero
# must still make the artifact incomplete and fail with the invalid-data code.
echo 'stale artifact' >"$TMP/failed.json"
set +e
PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
PERRY_BIN="$FAKE_PERRY" \
PERRY_FAKE_NODE="$NODE_REAL" \
PERRY_FAKE_FAIL_SOURCE="02_loop_overhead.ts" \
  "$ROOT/benchmarks/compare.sh" \
    --quick \
    --runs 2 \
    --warn-only \
    --json-out "$TMP/failed.json" \
    >"$TMP/failed-output.txt" 2>"$TMP/failed-error.txt"
failed_status=$?
set -e
if [[ "$failed_status" -ne 2 ]]; then
  cat "$TMP/failed-error.txt" >&2
  echo "expected failed runtime samples to exit 2, got $failed_status" >&2
  exit 1
fi
if [[ -e "$TMP/failed.json" ]]; then
  echo "compare.sh left a stale JSON artifact after failed collection" >&2
  exit 1
fi
if [[ -e "$ROOT/benchmarks/suite/02_loop_overhead" ]]; then
  echo "compare.sh did not clean compiled benchmark artifacts after failure" >&2
  exit 1
fi

echo "benchmark Bun-absent fallback: ok"
