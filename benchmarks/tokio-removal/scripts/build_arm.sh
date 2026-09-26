#!/bin/bash
# usage: build_arm.sh <arm-name> <tree>
set -u
ARM=$1; TREE=$2
OUT=/root/claude-tokio-measure-work/$ARM; mkdir -p $OUT
cd $TREE
unset RUSTFLAGS
export CARGO_TARGET_DIR=$TREE/target
export PERRY_RUNTIME_DIR=$TREE/target/release
log() { echo "[$(date -Is)] $*" | tee -a $OUT/build.events; }
log "arm=$ARM head=$(git rev-parse HEAD) load=$(cut -d' ' -f1-3 /proc/loadavg)"
s=$(date +%s.%N)
/usr/bin/time -v -o $OUT/time_core.txt nice -n 10 cargo build --release -j4 -p perry -p perry-runtime-static -p perry-stdlib-static > $OUT/build_core.log 2>&1
rc=$?; e=$(date +%s.%N)
log "core rc=$rc wall=$(echo "$e - $s" | bc) load=$(cut -d' ' -f1-3 /proc/loadavg)"
mkdir -p $OUT/core; cp -p target/release/libperry_runtime.a target/release/libperry_stdlib.a target/release/perry $OUT/core/ 
EXT=$(./scripts/release_ext_packages.sh | sed 's/^/-p /' | tr '\n' ' ')
s=$(date +%s.%N)
/usr/bin/time -v -o $OUT/time_ext.txt nice -n 10 cargo build --release -j4 -p perry -p perry-runtime-static -p perry-stdlib-static $EXT > $OUT/build_ext.log 2>&1
rc=$?; e=$(date +%s.%N)
log "ext rc=$rc wall=$(echo "$e - $s" | bc) load=$(cut -d' ' -f1-3 /proc/loadavg) pkgs=$EXT"
ls -l target/release/*.a > $OUT/archives_after_ext.txt
log DONE
