#!/usr/bin/env bash
# The host-side unit tests of the component model: perry-component-host,
# perry-wit, and the perry-runtime modules the component harnesses exercise.
# component_check.sh runs them first; CI runs them in wasi-component-units, in
# parallel with the component builds and harnesses.
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
rustup target add wasm32-unknown-unknown wasm32-wasip2
cargo test --locked --profile perry-dev -p perry-component-host -p perry-wit --lib
for filter in component:: promise:: async_hooks:: weakref:: finreg; do
  cargo test --locked --profile perry-dev -p perry-runtime "$filter" --lib -- --test-threads=1
done
