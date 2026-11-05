#!/usr/bin/env bash
#
# `perry-runtime` must keep building for the standalone WASI target
# (wasm32-wasip2; tracking issue #11375, phase 2a #11377). Runs locally and in
# `.github/workflows/compiler-runtime.yml`:
#
#   rustup target add wasm32-wasip2
#   toolchain_env="$(./scripts/wasi_toolchain.sh)" && eval "$toolchain_env"
#   ./scripts/wasi_check.sh
#
# Two builds:
#   1. `--no-default-features` — the core runtime.
#   2. Every DEFAULT feature except the ones WASI cannot build yet, listed in
#      WASI_EXCLUDED with the reason. The list is derived from `cargo metadata`
#      rather than restated here, so a new default feature is checked the day
#      it lands instead of silently drifting out of coverage.
#
# Liveness: a green run must mean perry-runtime was actually checked FOR
# WASI, so each build asserts cargo reported a wasm32-wasip2 perry-runtime
# artifact.
#
# An optional leading `wasm32-wasip1` argument checks that target instead.
#
# `--print-features` prints the feature list of build 2 and exits;
# `scripts/wasi_build_runtime.sh` builds the linkable archive with it.

set -euo pipefail

TARGET=wasm32-wasip2
if [[ "${1:-}" == wasm32-wasip1 || "${1:-}" == wasm32-wasip2 ]]; then
  TARGET="$1"
  shift
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Default features WASI cannot build yet — each one must name its blocker.
WASI_EXCLUDED=(
  # mimalloc's dependency is 64-bit-only (`cfg(target_pointer_width = "64")`),
  # so on wasm32 this feature enables nothing — excluded to say so explicitly.
  alloc-mimalloc
)
# Even the core runtime compiles C (hot_text.c); the Rust target alone is
# insufficient. Require the same C toolchain as the linkable runtime build
# instead of silently dropping default features when it is absent.
cc_var="CC_${TARGET//-/_}"
if [[ "${1:-}" != "--print-features" && -z "${!cc_var:-}" ]]; then
  echo '::error::configure wasi-sdk first: toolchain_env="$(./scripts/wasi_toolchain.sh)" && eval "$toolchain_env"' >&2
  exit 1
fi

features="$(
  cargo metadata --format-version 1 --no-deps --locked |
    python3 -c '
import json, sys
excluded = set(sys.argv[1:])
meta = json.load(sys.stdin)
pkg = next(p for p in meta["packages"] if p["name"] == "perry-runtime")
defaults = pkg["features"]["default"]
stale = excluded - set(defaults)
if stale:
    sys.exit(f"WASI_EXCLUDED names features that are no longer default: {sorted(stale)}")
print(",".join(f for f in defaults if f not in excluded))
' "${WASI_EXCLUDED[@]}"
)"

if [[ "${1:-}" == "--print-features" ]]; then
  echo "$features"
  exit 0
fi

# `cargo check` with JSON artifact messages; fails unless cargo reports the
# perry-runtime lib artifact built (or fresh) under the WASI target directory.
# Reads cargo's own report rather than guessing its on-disk layout, which
# moves between cargo versions.
check_for_wasi() {
  cargo check --locked -p perry-runtime --target "$TARGET" \
    --message-format=json-render-diagnostics "$@" |
    python3 -c '
import json, sys
target = sys.argv[1]
live = False
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    if (msg.get("reason") == "compiler-artifact"
            and msg["target"]["name"] == "perry_runtime"
            and any(f"/{target}/" in f for f in msg.get("filenames", []))):
        live = True
if not live:
    sys.exit(f"::error::cargo reported no {target} perry-runtime artifact; the check did not run for WASI")
' "$TARGET"
}

echo "== perry-runtime, $TARGET, --no-default-features"
check_for_wasi --no-default-features

echo "== perry-runtime, $TARGET, default features minus: ${WASI_EXCLUDED[*]}"
echo "   features: $features"
check_for_wasi --no-default-features --features "$features"

echo "OK: perry-runtime builds for $TARGET"
