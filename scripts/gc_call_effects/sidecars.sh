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
if [[ -n "${GC_EFFECTS_BIN:-}" ]]; then
  exec "$GC_EFFECTS_BIN" sidecars "$dir"
fi
exec cargo run --quiet --release -p perry-gc-effects -- sidecars "$dir"
