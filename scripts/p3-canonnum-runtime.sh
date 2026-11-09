#!/bin/bash
# P3 B1 negative control (DESIGN-p3 rev 3.1 §5, REVIEW-p3 D4): build a runtime
# whose `js_math_round` returns an INT32-boxed word, for the S0 entry
# `canonical-number-negative` (scripts/test-verify-facts-s0.py
# --canonnum-runtime <target>/release).
#
#   scripts/p3-canonnum-runtime.sh <cargo-target-dir>
#
# The switch is build-time (`option_env!` in perry-runtime/src/math.rs), so the
# archive must really be rebuilt: the script fails (STALE-ARCHIVE) when
# libperry_runtime.a did not change.
set -euo pipefail
target=${1:?usage: p3-canonnum-runtime.sh <cargo-target-dir>}
archive="$target/release/libperry_runtime.a"
before=$(stat -c %Y "$archive" 2>/dev/null || stat -f %m "$archive" 2>/dev/null || echo 0)
PERRY_FACTS_SABOTAGE=canonnum CARGO_TARGET_DIR="$target" cargo build-perry --release
after=$(stat -c %Y "$archive" 2>/dev/null || stat -f %m "$archive")
if [ "$after" = "$before" ]; then
  echo "STALE-ARCHIVE: $archive was not rebuilt" >&2
  exit 1
fi
bash "$(dirname "$0")/gc_call_effects/sidecars.sh" "$target/release"
echo "canonnum runtime: $target/release"
