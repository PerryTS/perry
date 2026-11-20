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
# Each run qualifies fresh outputs: the acceptance scripts refuse an existing
# output dir, and CI restores target/ (with a previous run's outputs) from cache.
rm -rf "$out"
mkdir -p "$out"
git rev-parse HEAD > "$out/source-revision.txt"
rustup target add wasm32-unknown-unknown wasm32-wasip2
# COMPONENT_CHECK_UNIT_TESTS=0: the caller runs component_unit_tests.sh itself.
if [[ "${COMPONENT_CHECK_UNIT_TESTS:-1}" != 0 ]]; then
  scripts/component_unit_tests.sh
fi
logs="$out/build-logs"
mkdir -p "$logs"
# Independent builds run beside the main one. finish prints a build's log and
# fails the script with its status; on any other exit the EXIT trap stops the
# builds still running and prints what they logged.
bg_names=()
background() {
  local name="$1"
  shift
  "$@" >"$logs/$name.log" 2>&1 &
  printf -v "pid_$name" '%s' "$!"
  bg_names+=("$name")
}
finish() {
  local name="$1" pid_var="pid_$1" status=0
  wait "${!pid_var}" || status=$?
  printf -v "done_$name" 1
  cat "$logs/$name.log"
  if [[ "$status" -ne 0 ]]; then
    echo "component_check.sh: $name failed ($status)" >&2
    exit "$status"
  fi
}
kill_tree() {
  local child
  for child in $(pgrep -P "$1" 2>/dev/null); do kill_tree "$child"; done
  kill "$1" 2>/dev/null || true
}
stop_background() {
  local name pid_var done_var
  for name in ${bg_names[@]+"${bg_names[@]}"}; do
    done_var="done_$name"
    [[ -n "${!done_var:-}" ]] && continue
    pid_var="pid_$name"
    kill_tree "${!pid_var}"
    echo "component_check.sh: stopped the unfinished $name build; its log so far:" >&2
    cat "$logs/$name.log" >&2 || true
  done
}
trap stop_background EXIT
# The instrumented graph builds in its own target dir, so it never waits for
# target/'s lock.
background instr env PERRY_INSTR_BUILD_ONLY=1 PERRY_INSTR_PACKAGES=runtime \
  scripts/build_instr_runtime.sh '' perry-dev
cargo build --locked --profile perry-dev -p perry -p perry-runtime-static \
  -p perry-stdlib-static -p perry-component-host -p perry-wit -p perry-gc-effects \
  --features perry/target-wasi
cargo build --locked --profile perry-dev -p perry-wit --example componentize
cargo build --locked --profile perry-dev -p perry-component-host --example component_wat
export GC_EFFECTS_BIN="$CARGO_TARGET_DIR/perry-dev/perry-gc-effects"
scripts/gc_call_effects/sidecars.sh "$CARGO_TARGET_DIR/perry-dev"
finish instr
PERRY_INSTR_PACKAGES=runtime scripts/build_instr_runtime.sh '' perry-dev
# The guest suite's wasm32 archive builds beside the native suites. Their
# acceptance scripts set PERRY_NO_AUTO_OPTIMIZE=1, so perry links the prebuilt
# archives without a Cargo build; only their Rust guests (four in
# component_host_acceptance.py, one in component_command_acceptance.py) build
# with Cargo, and those wait for target/'s lock while the wasm32 build holds it.
background wasm python3 scripts/wasi_build_runtime.py --gc-instruments
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
finish wasm
python3 scripts/component_guest_acceptance.py \
  --perry "$CARGO_TARGET_DIR/perry-dev/perry" \
  --wit "$CARGO_TARGET_DIR/perry-dev/perry-wit" \
  --componentize "$CARGO_TARGET_DIR/perry-dev/examples/componentize" \
  --runtime-dir "$CARGO_TARGET_DIR/wasm32-wasip2/release" \
  --wasi-sdk "$WASI_SDK_PATH" --output "$out/guest" \
  --native-runtime-dir "$CARGO_TARGET_DIR/perry-dev" --moving-gc --guest-moving-gc
