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
set -euo pipefail
dir="${1:?usage: sidecars.sh <lib-dir>}"
dir="$(cd "$dir" && pwd)"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if [[ -n "${GC_EFFECTS_BIN:-}" ]]; then
  exec "$GC_EFFECTS_BIN" sidecars "$dir"
fi
exec cargo run --quiet --release -p perry-gc-effects -- sidecars "$dir"
