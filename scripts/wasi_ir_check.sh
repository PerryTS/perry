#!/usr/bin/env bash
#
# Compile sample programs for the standalone WASI target and verify the
# emitted wasm32 IR against the runtime's real signatures (#11378):
#
#   cargo build --profile perry-dev -p perry --features target-wasi
#   ./scripts/wasi_ir_check.sh target/perry-dev/perry
#
# Linking a `.wasm` is phase 4 (#11379), so the compile is expected to fail
# at the link step for now; what this checks is the codegen output, which
# `PERRY_SAVE_LL` captures before that. A program that produces no IR fails,
# and so does one whose objects fail to compile ("Error compiling module"):
# only the link may fail.
#
# Liveness: every saved module must carry the wasm32 triple (a host compile
# would pass the ABI check vacuously against nothing) and the ABI check must
# have verified a non-zero number of runtime calls.

set -euo pipefail

PERRY="$(realpath "${1:?usage: $0 <perry built with --features target-wasi>}")"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# A fixed spread of the gap suite: strings, arrays, closures, classes, maps,
# JSON, regex, async, errors. Named, not globbed, so the sample is stable.
SAMPLES=(
  test_gap_array_methods
  test_gap_string_methods
  test_gap_closures
  test_gap_class_advanced
  test_gap_map_set_extended
  test_gap_json_advanced
  test_gap_regexp_advanced
  test_gap_async_advanced
  test_gap_error_extensions
  test_gap_object_methods
)

# The IR is saved before object emission, so saved IR alone does not show the
# objects compiled: reject a failure before the (expected) link failure.
require_objects() {
  if grep -q "Error compiling module" "$2"; then
    echo "::error::$1 failed to compile to wasm32 objects" >&2
    grep -A6 "Error compiling module" "$2" >&2
    exit 1
  fi
}

n=0
for name in "${SAMPLES[@]}"; do
  src="$ROOT/test-files/$name.ts"
  if [[ ! -f "$src" ]]; then
    echo "::error::sample $name.ts no longer exists; update SAMPLES in $0" >&2
    exit 1
  fi
  dir="$OUT/$name"
  mkdir -p "$dir"
  # The link step fails until #11379; the IR is saved before it runs.
  ( cd "$dir" && PERRY_SAVE_LL="$dir" PERRY_NO_AUTO_OPTIMIZE=1 PERRY_NO_CACHE=1 \
      timeout 300 "$PERRY" compile "$src" -o "$dir/out" --target wasi >"$dir/log.txt" 2>&1 ) || true
  if ! compgen -G "$dir/*.ll" >/dev/null; then
    echo "::error::$name produced no IR" >&2
    tail -20 "$dir/log.txt" >&2
    exit 1
  fi
  require_objects "$name" "$dir/log.txt"
  for ll in "$dir"/*.ll; do
    if ! grep -q '^target triple = "wasm32-' "$ll"; then
      echo "::error::$ll is not wasm32 IR; was perry built with --features target-wasi?" >&2
      exit 1
    fi
    n=$((n + 1))
  done
done

# A mismatch exits 1: capture it so the report still prints, then fail.
status=0
report="$(python3 "$ROOT/scripts/runtime_abi_check.py" --ir "$OUT"/*/*.ll)" || status=$?
echo "$report"
if ((status != 0)); then
  exit "$status"
fi
calls="$(sed -n 's/.*modules, \([0-9]*\) runtime calls checked.*/\1/p' <<<"$report" | head -1)"
if [[ -z "$calls" || "$calls" -eq 0 ]]; then
  echo "::error::the ABI check verified no runtime calls; nothing was checked" >&2
  exit 1
fi
if grep -q "MISMATCH" <<<"$report"; then
  exit 1
fi
echo "OK: $n wasm32 modules, $calls runtime calls match the runtime's signatures"
