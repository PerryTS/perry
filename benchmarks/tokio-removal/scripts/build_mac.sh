#!/bin/bash
# usage: build_mac.sh <arm>   (tree = claude-tokio-measure-mac at whatever HEAD is checked out)
set -u
ARM=$1; TREE=/Users/amlug/projects/perry/claude-tokio-measure-mac
OUT=/Users/amlug/projects/perry/claude-tokio-measure-mac-work/$ARM; mkdir -p $OUT
cd $TREE
unset RUSTFLAGS
export LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm@22
export PATH=/opt/homebrew/opt/llvm@22/bin:/opt/homebrew/bin:$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$TREE/target
mkdir -p target && printf 'OWNER: claude tokio-removal measurement agent (Ralph, tokio-removal impact report)\nPURPOSE: target for /Users/amlug/projects/perry/claude-tokio-measure-mac\nCREATED: 2026-09-26  SAFE-TO-DELETE: yes after 2026-09-28\n' > target/OWNER
echo "[$(date)] arm=$ARM head=$(git rev-parse HEAD) load=$(sysctl -n vm.loadavg)" | tee -a $OUT/build.events
s=$(python3 -c 'import time;print(time.time())')
/usr/bin/time -l nice -n 10 cargo build --release -j6 -p perry -p perry-runtime-static -p perry-stdlib-static > $OUT/build_core.log 2>&1
rc=$?; e=$(python3 -c 'import time;print(time.time())')
echo "[$(date)] core rc=$rc wall=$(python3 -c "print($e-$s)") load=$(sysctl -n vm.loadavg)" | tee -a $OUT/build.events
mkdir -p $OUT/core && cp -p target/release/perry target/release/libperry_runtime.a target/release/libperry_stdlib.a $OUT/core/
echo DONE >> $OUT/build.events
