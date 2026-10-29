#!/bin/bash
# scripts/a1/build-arm.sh <srcdir> <target-dir> <outdir> [cargo args...]
# Build one irdiff arm: `cargo build-perry --release` with the measurement feature
# `perry-runtime/fixed-hash-seeds`, the gc-effects sidecars, then snapshot the release
# directory's top-level files (perry, runtime/stdlib archives, sidecars) to <outdir>.
# Run it for BOTH arms with the same cargo args; the snapshot is what irdiff.sh runs.
set -e
src=$1; td=$2; out=$3; shift 3
cd "$src"
export CARGO_TARGET_DIR=$td PATH=/usr/lib/llvm-22/bin:$PATH PERRY_LLVM_OPT=/usr/lib/llvm-22/bin/opt
cargo build-perry --release --features perry-runtime/fixed-hash-seeds "$@"
scripts/gc_call_effects/sidecars.sh "$td/release"
mkdir -p "$out"
find "$td/release" -maxdepth 1 -type f ! -name '*.rlib' ! -name '*.d' ! -name '.*' -exec cp {} "$out/" \;
echo "built $src -> $out"
