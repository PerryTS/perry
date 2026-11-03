#!/usr/bin/env bash
# Build matching tools/archives and qualify native and guest components on Linux.
# Requires wasi-sdk configured as for wasi_check.sh. Run heavy local builds
# through the build host's concurrency gate; CI invokes this after WASI smoke.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
: "${WASI_SDK_PATH:?Configure wasi-sdk before running component_check.sh}"
: "${CC_wasm32_wasip2:?Configure the WASI C compiler before running component_check.sh}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$CARGO_TARGET_DIR"
CARGO_TARGET_DIR="$(cd "$CARGO_TARGET_DIR" && pwd)"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
out="$CARGO_TARGET_DIR/component-validation"
mkdir -p "$out"
git rev-parse HEAD > "$out/source-revision.txt"
rustup target add wasm32-unknown-unknown wasm32-wasip2
cargo test --locked --profile perry-dev -p perry-component-host -p perry-wit --lib
cargo test --locked --profile perry-dev -p perry-runtime component:: --lib -- --test-threads=1
cargo test --locked --profile perry-dev -p perry-runtime promise:: --lib -- --test-threads=1
cargo test --locked --profile perry-dev -p perry-runtime async_hooks:: --lib -- --test-threads=1
cargo test --locked --profile perry-dev -p perry-runtime weakref:: --lib -- --test-threads=1
cargo test --locked --profile perry-dev -p perry-runtime finreg --lib -- --test-threads=1
cargo build --locked --profile perry-dev -p perry -p perry-runtime-static \
  -p perry-stdlib-static -p perry-component-host -p perry-wit -p perry-gc-effects \
  --features perry/target-wasi
cargo build --locked --profile perry-dev -p perry-wit --example componentize
cargo build --locked --profile perry-dev -p perry-component-host --example component_wat
export GC_EFFECTS_BIN="$CARGO_TARGET_DIR/perry-dev/perry-gc-effects"
scripts/gc_call_effects/sidecars.sh "$CARGO_TARGET_DIR/perry-dev"
PERRY_INSTR_PACKAGES=runtime scripts/build_instr_runtime.sh '' perry-dev
for suite in persistent async reader producer; do
  python3 "scripts/component_${suite}_acceptance.py" \
    --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
    --wit "$CARGO_TARGET_DIR/perry-dev/perry-wit" \
    --wat-encoder "$CARGO_TARGET_DIR/perry-dev/examples/component_wat" \
    --runtime-dir "$CARGO_TARGET_DIR/perry-dev" --output "$out/$suite" --moving-gc
done
python3 scripts/component_http_p3_acceptance.py \
  --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
  --wit "$CARGO_TARGET_DIR/perry-dev/perry-wit" \
  --wat-encoder "$CARGO_TARGET_DIR/perry-dev/examples/component_wat" \
  --runtime-dir "$CARGO_TARGET_DIR/perry-dev" --output "$out/http-p3" --moving-gc
python3 scripts/component_host_acceptance.py \
  --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
  --wit "$CARGO_TARGET_DIR/perry-dev/perry-wit" \
  --componentize "$CARGO_TARGET_DIR/perry-dev/examples/componentize" \
  --runtime-dir "$CARGO_TARGET_DIR/perry-dev" --output "$out/native" --moving-gc
python3 scripts/component_command_acceptance.py \
  --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
  --runtime-dir "$CARGO_TARGET_DIR/perry-dev" --output "$out/command" --moving-gc \
  --library-component "$out/native/transform/transform.component.wasm" \
  --library-bindings "$out/native/transform/generated"
python3 scripts/wasi_build_runtime.py --gc-instruments
python3 scripts/component_guest_acceptance.py \
  --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
  --wit "$CARGO_TARGET_DIR/perry-dev/perry-wit" \
  --componentize "$CARGO_TARGET_DIR/perry-dev/examples/componentize" \
  --runtime-dir "$CARGO_TARGET_DIR/wasm32-wasip2/release" \
  --wasi-sdk "$WASI_SDK_PATH" --output "$out/guest" \
  --native-runtime-dir "$CARGO_TARGET_DIR/perry-dev" --moving-gc --guest-moving-gc
