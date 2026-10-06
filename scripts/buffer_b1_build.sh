#!/usr/bin/env bash
# qb6 build entry: separate arms, the same coherent archive set, max eight jobs.
set -euo pipefail
hostdir=$1
arm=$2
export PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH RUST_TEST_THREADS=1 CARGO_BUILD_JOBS=8
if [[ "$arm" == main ]]; then
    export CARGO_TARGET_DIR="$hostdir/base/target" PERRY_WORKSPACE_ROOT="$hostdir/base/src"
else
    export CARGO_TARGET_DIR="$hostdir/target" PERRY_WORKSPACE_ROOT="$hostdir/src"
fi
export PERRY_RUNTIME_DIR="$CARGO_TARGET_DIR/release" TMPDIR="$hostdir/tmp"
mkdir -p "$TMPDIR" "$hostdir/logs"
cd "$PERRY_WORKSPACE_ROOT"
until [[ $(df --output=avail -BG / | tail -1 | tr -dc 0-9) -ge 25 ]]; do sleep 300; done
df -h /
taskset -c 0-55 cargo build --release -j 8 \
    -p perry -p perry-runtime -p perry-runtime-static -p perry-stdlib-static \
    -p perry-ext-zlib -p perry-ext-http -p perry-ext-net -p perry-ext-ethers \
    -p perry-ext-sharp -p perry-ext-streams -p perry-ext-ws \
    --features perry-stdlib/external-zlib-pump,perry-stdlib/external-http-server-pump,perry-stdlib/external-http-client-pump \
    > "$hostdir/logs/$arm-coherent-build.log" 2>&1
echo "$arm coherent build: PASS"
