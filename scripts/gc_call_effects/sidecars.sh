#!/usr/bin/env bash
# Write `<archive>.gc-effects` beside every prebuilt runtime archive in a
# directory (design: GC_EFFECTS_AT_BUILD_DESIGN, 4.3). Run it after the last
# cargo call that writes the archives, over the exact bytes that get staged:
#
#   scripts/gc_call_effects/sidecars.sh target/release
#   scripts/gc_call_effects/sidecars.sh target/x86_64-pc-windows-msvc/release
#
# Every recipe that builds libperry_runtime*.a / perry_runtime*.lib calls this
# one script; it is the only way a sidecar is made. The analyzer runs on the
# host, whatever target the archives are for. GC_EFFECTS_BIN=<path> uses a
# prebuilt perry-gc-effects instead of building it.
#
#   scripts/gc_call_effects/sidecars.sh --cargo-profile debug
#
# names the profile directory of the cargo target dir (`$CARGO_TARGET_DIR`,
# default `<workspace>/target`) instead; the test harnesses that build their
# own archives use it.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ "${1:-}" == "--cargo-profile" ]]; then
  profile="${2:?usage: sidecars.sh --cargo-profile <profile>}"
  target_dir="${CARGO_TARGET_DIR:-$ROOT/target}"
  [[ "$target_dir" = /* ]] || target_dir="$ROOT/$target_dir"
  dir="$target_dir/$profile"
else
  dir="${1:?usage: sidecars.sh <lib-dir> | --cargo-profile <profile>}"
fi
dir="$(cd "$dir" && pwd)"
cd "$ROOT"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    # The analyzer imports LLVM-C.dll (llvm-sys without static linking).
    if [[ -n "${LLVM_SYS_221_PREFIX:-}" ]]; then
      export PATH="$(cygpath -u "$LLVM_SYS_221_PREFIX")/bin:$PATH"
    fi ;;
esac
effects() {
  if [[ -n "${GC_EFFECTS_BIN:-}" ]]; then
    "$GC_EFFECTS_BIN" "$@"
  else
    cargo run --quiet --release -p perry-gc-effects -- "$@"
  fi
}
effects sidecars "$dir"
# Lazy realm publication allocates nothing on the GC heap and never collects,
# so the first-touch publisher every receiver read can reach is a GC leaf
# (receivers keep nounwind / "gc-leaf-function"). A change that reconnects an
# allocator to it fails here.
lib=""
for name in libperry_runtime.a perry_runtime.lib; do
  [[ -f "$dir/$name" ]] && lib="$dir/$name" && break
done
[[ -n "$lib" ]] || { echo "sidecars.sh: no perry-runtime archive in $dir" >&2; exit 1; }
why="$(effects why --archive "$lib" --rules scripts/gc_call_effects/seeds.txt --root . js_realm_touch_pending)"
if ! grep -qx "== js_realm_touch_pending: Leaf" <<<"$why"; then
  printf '%s\n' "$why" >&2
  echo "sidecars.sh: js_realm_touch_pending is not a GC leaf (lazy publication reaches an allocator)" >&2
  exit 1
fi
