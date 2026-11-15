#!/bin/bash
# inv_fixtures.sh: compile + run + check the inventory fixtures with the instrumented cohort.
# Env: W (worktree), A (instrumented release dir), K (work dir), FXSUM, TAG, JOBS (default "i1 i2 i3").
export PATH=$HOME/.cargo/bin:/usr/lib/llvm-22/bin:$PATH PERRY_TRAP_CLANG=/usr/bin/clang-22 LLVM_SYS_221_PREFIX=/usr/lib/llvm-22 PERRY_ALLOW_PERRY_FEATURES=1
K=${K:-/tmp/inv-work}; W=${W:-$(git rev-parse --show-toplevel)}; A=${A:-$W/target/release}; S=${FXSUM:-$K/fxi.sum}; TAG=${TAG:-good}; mkdir -p $K/T; : > $S
for d in ${JOBS:-i1 i2 i3}; do
  F=$K/T/$TAG-$d; rm -rf $F; mkdir -p $F; dir=$W/scripts/snap_inputs/fixtures/inv/$d
  ( cd $dir
    env -u PERRY_RUNTIME_INSTRUMENTS PERRY_RUNTIME_DIR=$A PERRY_WORKSPACE_ROOT=$W PERRY_GC_INSTRUMENTS=1 PERRY_SNAP_CENSUS=1 PERRY_KEEP_SYMBOLS=1 timeout 900 setarch -R "$A/perry" compile --cache-dir "$F/cache" main.ts -o "$F/bin" > "$F/compile.log" 2>&1
    echo "inv-$TAG-$d compile rc=$?" >> $S
    # The last init the program enters: the report is written before it.
    PERRY_SNAP_CENSUS="$F/probe" timeout 300 setarch -R "$F/bin" > /dev/null 2>&1 < /dev/null
    n=$(python3 $W/scripts/snap_inputs/check_inv.py last-init "$F/probe")
    echo "$n" > $F/before
    PERRY_SNAP_INVENTORY_BEFORE=$n PERRY_SNAP_CENSUS="$F/report" timeout 300 setarch -R "$F/bin" > "$F/stdout" 2> "$F/stderr" < /dev/null
    echo "inv-$TAG-$d run rc=$? before=$n out=$(head -c 120 $F/stdout | tr '\n' '|')" >> $S )
  python3 $W/scripts/snap_inputs/check_inv.py check $d "$F/report" "$F/bin" > $F/check.txt 2>&1
  echo "inv-$TAG-$d check rc=$?: $(tail -1 $F/check.txt | cut -c1-500)" >> $S
done
echo FXDONE >> $S
