#!/usr/bin/env bash
# Build the instrumented prebuilt variant of the runtime, stdlib and wrapper
# archives, beside the default ones.
#
# The default archives carry no GC/diagnostic instruments (census, from-space
# quarantine and scan, seeded schedule, verifiers, telemetry and typed-feedback
# trace JSON, hot-path probes). A compile that sets an instrument knob
# (PERRY_GC_SCHEDULE_SEED, PERRY_GC_VERIFY_MARK, PERRY_IC_DIAG, ...),
# PERRY_GC_INSTRUMENTS=1, PERRY_GC_TRACE=1 or PERRY_CHECK_CONTRACTS=1 links
# these `_instr` archives instead, chosen before codegen (perry compile:
# optimized_libs::instrumented_variant_requested). The variant also carries the
# contract checks.
#
#   scripts/build_instr_runtime.sh                       # host, release
#   scripts/build_instr_runtime.sh x86_64-unknown-linux-gnu prod
#
# PERRY_INSTR_PACKAGES=runtime builds only the runtime and stdlib archives.
#
# The variant is a second Cargo graph (PERRY_RUNTIME_INSTRUMENTS=1 changes the
# runtime's build-script cfgs, so every crate that bundles the runtime is built
# again) in its own target dir, `${CARGO_TARGET_DIR:-target}-instr` unless
# PERRY_INSTR_TARGET_DIR is set. Its archives are copied next to the default
# ones as lib<name>_instr.a (<name>_instr.lib on Windows), and their gc-effects
# sidecars are written over the copies (scripts/gc_call_effects/sidecars.sh).
set -euo pipefail
target="${1:-}"
profile="${2:-release}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
ordinary="${CARGO_TARGET_DIR:-target}"
instr="${PERRY_INSTR_TARGET_DIR:-${ordinary}-instr}"
profile_dir="$profile"
[[ "$profile" == "dev" ]] && profile_dir=debug
args=(--profile "$profile")
sub="$profile_dir"
if [[ -n "$target" ]]; then
  args+=(--target "$target")
  sub="$target/$profile_dir"
fi
# Inherit the ordinary build's compiler flags. The instrumented variant must
# keep the same unwind and target settings while changing its build-script cfgs.
if [[ "${PERRY_INSTR_PACKAGES:-}" == "runtime" ]]; then
  # Runtime and stdlib only (no wrapper archives), for jobs that never route a
  # module to a wrapper.
  env CARGO_TARGET_DIR="$instr" PERRY_RUNTIME_INSTRUMENTS=1 \
    cargo build -p perry-runtime-static -p perry-stdlib-static "${args[@]}"
else
  env CARGO_TARGET_DIR="$instr" PERRY_RUNTIME_INSTRUMENTS=1 cargo build-perry-instr "${args[@]}"
fi
mkdir -p "$ordinary/$sub"
copied=0
for f in "$instr/$sub"/libperry_runtime.a "$instr/$sub"/libperry_stdlib.a "$instr/$sub"/libperry_ext_*.a \
         "$instr/$sub"/perry_runtime.lib "$instr/$sub"/perry_stdlib.lib "$instr/$sub"/perry_ext_*.lib; do
  [[ -f "$f" ]] || continue
  name="$(basename "$f")"
  case "$name" in
    *.a) out="${name%.a}_instr.a" ;;
    *.lib) out="${name%.lib}_instr.lib" ;;
  esac
  cp "$f" "$ordinary/$sub/$out"
  copied=$((copied + 1))
done
[[ "$copied" -gt 0 ]] || { echo "build_instr_runtime.sh: no archives found in $instr/$sub" >&2; exit 1; }
"$root/scripts/gc_call_effects/sidecars.sh" "$ordinary/$sub"
echo "build_instr_runtime.sh: $copied instrumented archives in $ordinary/$sub"
