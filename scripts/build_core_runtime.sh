#!/usr/bin/env bash
# Build the feature-trimmed unwind runtime without changing the ordinary archive.
# Run in the same toolchain/sysroot as the full runtime. Optional second argument
# lets the verification campaign use release instead of the packaging profile.
set -euo pipefail
target="${1:?usage: build_core_runtime.sh TARGET [PROFILE]}"
profile="${2:-dist}"
case "$target" in
  *-apple-darwin|*-unknown-linux-gnu|*-unknown-linux-musl) ;;
  *) echo "unsupported core runtime target: $target" >&2; exit 2 ;;
esac
ordinary_target_dir="${CARGO_TARGET_DIR:-target}"
core_target_dir="${PERRY_CORE_TARGET_DIR:-${ordinary_target_dir}-core}"
CARGO_TARGET_DIR="$core_target_dir" cargo build --locked --profile "$profile" --target "$target" \
  -p perry-runtime-static --no-default-features --features perry-runtime/prebuilt-core
mkdir -p "$ordinary_target_dir/$target/$profile"
cp "$core_target_dir/$target/$profile/libperry_runtime.a" \
  "$ordinary_target_dir/$target/$profile/libperry_runtime_core.a"
