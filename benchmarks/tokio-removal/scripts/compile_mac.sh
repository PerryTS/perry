#!/bin/bash
# usage: compile_mac.sh <arm>  — compile the mini's runtime probes with auto-optimize ON (user path)
set -u
ARM=$1; TREE=/Users/amlug/projects/perry/claude-tokio-measure-mac; W=/Users/amlug/projects/perry/claude-tokio-measure-mac-work
OUT=$W/$ARM/bin; mkdir -p $OUT
unset RUSTFLAGS
export LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm@22 PATH=/opt/homebrew/opt/llvm@22/bin:/opt/homebrew/bin:$HOME/.cargo/bin:$PATH
export PERRY_WORKSPACE_ROOT=$TREE PERRY_RUNTIME_DIR=$W/$ARM/core PERRY_LIB_DIR=$W/$ARM/core CARGO_BUILD_JOBS=6
cd $W/probes
for p in hello backend bench_http_server bench_fetch_client http ws worker timers crypto zlib net fetch_local; do
  rm -rf $TREE/target/perry-auto-*; export PERRY_CACHE_DIR=$OUT/cache-$p; rm -rf $PERRY_CACHE_DIR
  s=$(python3 -c 'import time;print(time.time())')
  nice -n 10 $W/$ARM/core/perry compile $p.ts -o $OUT/$p -v > $OUT/$p.cold.log 2>&1; rc=$?
  m=$(python3 -c 'import time;print(time.time())')
  nice -n 10 $W/$ARM/core/perry compile $p.ts -o $OUT/$p.w > $OUT/$p.warm.log 2>&1
  e=$(python3 -c 'import time;print(time.time())')
  echo "{\"arm\":\"$ARM\",\"probe\":\"$p\",\"rc\":$rc,\"cold_wall\":$(python3 -c "print(round($m-$s,2))"),\"warm_wall\":$(python3 -c "print(round($e-$m,2))"),\"size\":$(stat -f %z $OUT/$p 2>/dev/null || echo null),\"load\":\"$(sysctl -n vm.loadavg)\"}" | tee -a $OUT/compile.jsonl
  rm -f $OUT/$p.w; rm -rf $PERRY_CACHE_DIR
done
rm -rf $TREE/target/perry-auto-*
echo DONE >> $OUT/compile.jsonl
