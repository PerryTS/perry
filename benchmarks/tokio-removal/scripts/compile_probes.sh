#!/bin/bash -l
# usage: compile_probes.sh <arm> <tree>
# Phase B (out-of-tree install, fast): a copy of the arm's perry + every prebuilt
#   archive in <arm>/dist, NO workspace reachable -> links the prebuilt full
#   stdlib (+ ext archives), with and without PERRY_NO_AUTO_OPTIMIZE=1.
# Phase A (the user path in a source checkout): auto-optimize ON, per-program
#   feature-stripped runtime/stdlib rebuilt by cargo into target/perry-auto-<hash>.
#   perry-auto dirs are only cleared at the start, so a probe whose feature set was
#   already built is a cache hit — each row records whether it rebuilt (cold) or not.
#   Then: warm recompile, PERRY_KEEP_SYMBOLS=1 compile for the symbol census.
set -u
ARM=$1; TREE=$2
W=/root/claude-tokio-measure-work; P=$W/probes; OUT=$W/$ARM/probes; D=$W/$ARM/dist; mkdir -p $OUT
unset RUSTFLAGS PERRY_WORKSPACE_ROOT
export PATH=/opt/node-v26.5.1-linux-x64/bin:$PATH CARGO_BUILD_JOBS=4
PROBES=${PROBES:-hello ops_loop http ops_http bench_http_server https fetch_local ops_fetch bench_fetch_client net ops_net tls_net ws crypto ops_crypto zlib ops_zlib child timers ops_timers worker backend container}
sz() { [ -f "$1" ] && stat -c %s "$1" || echo null; }
cd $P
# ---------------- Phase B ----------------
RB=$OUT/prebuilt.jsonl; [ "${SKIP_B:-0}" = 1 ] || : > $RB
for p in $( [ "${SKIP_B:-0}" = 1 ] || echo $PROBES ); do
  for mode in default noauto; do
    export PERRY_CACHE_DIR=$OUT/cache-pb-$p; rm -rf $PERRY_CACHE_DIR
    if [ $mode = noauto ]; then export PERRY_NO_AUTO_OPTIMIZE=1; else unset PERRY_NO_AUTO_OPTIMIZE; fi
    /usr/bin/time -f '%e %U %S' -o $OUT/$p.pb-$mode.time env PERRY_RUNTIME_DIR=$D PERRY_LIB_DIR=$D nice -n 10 $D/perry compile $p.ts -o $OUT/$p.pb-$mode > $OUT/$p.pb-$mode.log 2>&1; rc=$?
    read w u s < $OUT/$p.pb-$mode.time
    nt=null; [ -f $OUT/$p.pb-$mode ] && nt=$(strings $OUT/$p.pb-$mode | grep -c 'tokio-1\.')
    rebuilt=$(grep -c 'rebuilding' $OUT/$p.pb-$mode.log)
    echo "{\"arm\":\"$ARM\",\"probe\":\"$p\",\"mode\":\"prebuilt-$mode\",\"rc\":$rc,\"size\":$(sz $OUT/$p.pb-$mode),\"wall\":$w,\"user\":$u,\"sys\":$s,\"strings_tokio_1x\":$nt,\"rebuilt\":$rebuilt}" | tee -a $RB
    rm -rf $PERRY_CACHE_DIR
    # keep only the ops_* prebuilt binaries (fallback subjects for instruction counts)
    [[ $p == ops_* ]] || rm -f $OUT/$p.pb-$mode
  done
done
unset PERRY_NO_AUTO_OPTIMIZE
echo PHASE_B_DONE | tee -a $OUT/events
# ---------------- Phase A ----------------
export PERRY_WORKSPACE_ROOT=$TREE PERRY_RUNTIME_DIR=$TREE/target/release PERRY_LIB_DIR=$TREE/target/release
PERRY=$TREE/target/release/perry
rm -rf $TREE/target/perry-auto-* $TREE/target/perry-no-auto-*
RES=$OUT/results.jsonl; : > $RES
# Order siblings adjacently: after each probe only the auto dir it used is kept,
# so a sibling with the same feature set is a cache hit and disk stays bounded.
for p in $PROBES; do
  av=$(df --output=avail -BG /root | tail -1 | tr -dc 0-9); [ "$av" -lt 12 ] && { echo "LOWDISK $av" | tee -a $OUT/events; break; }
  export PERRY_CACHE_DIR=$OUT/cache-$p; rm -rf $PERRY_CACHE_DIR
  load0=$(cut -d' ' -f1 /proc/loadavg)
  /usr/bin/time -f '%e %U %S %M' -o $OUT/$p.cold.time nice -n 10 $PERRY compile $p.ts -o $OUT/$p -v > $OUT/$p.cold.log 2>&1; crc=$?
  /usr/bin/time -f '%e %U %S %M' -o $OUT/$p.warm.time nice -n 10 $PERRY compile $p.ts -o $OUT/$p.warm > $OUT/$p.warm.log 2>&1
  PERRY_KEEP_SYMBOLS=1 nice -n 10 $PERRY compile $p.ts -o $OUT/$p.sym > $OUT/$p.sym.log 2>&1
  rebuilt=$(grep -c 'auto-optimize: rebuilding' $OUT/$p.cold.log)
  autodir=$(grep -o "/[^ ]*perry-auto-[0-9a-f]*" $OUT/$p.cold.log | head -1)
  autoarch=""; [ -n "$autodir" ] && autoarch=$(find $autodir/release -maxdepth 1 -name 'lib*.a' -printf '%f=%s,' 2>/dev/null)
  if [ -f $OUT/$p.sym ]; then
    nm -C --size-sort -S $OUT/$p.sym 2>/dev/null | gzip > $OUT/$p.sym.nm.gz
    cnt() { zcat $OUT/$p.sym.nm.gz | grep -cE "$1" ; }
    ntok=$(cnt '(^| |<)tokio(_[a-z]+)?::'); nhyp=$(cnt '(^| |<)hyper(_util)?::'); nreq=$(cnt '(^| |<)reqwest::'); nh2=$(cnt '(^| |<)h2::')
    ntow=$(cnt '(^| |<)tower(_[a-z]+)?::'); nmio=$(cnt '(^| |<)mio::'); ntr=$(cnt 'tokio_rustls::'); ntl=$(cnt 'turnloop')
  else ntok=null; nhyp=null; nreq=null; nh2=null; ntow=null; nmio=null; ntr=null; ntl=null; fi
  stok=null; [ -f $OUT/$p ] && stok=$(strings $OUT/$p | grep -c 'tokio-1\.')
  if [ -x $OUT/$p ] && [[ $p != bench_* ]]; then
    arg=""; [[ $p == ops_* ]] && arg=20
    if [ $p = container ]; then res=$(PATH=$P/stubbin:$PATH timeout 30 $OUT/$p 2>&1); rc=$?; exp=$(printf 'backend is string: true\nlist settled: resolved object')
    else res=$(timeout 30 $OUT/$p $arg 2>&1); rc=$?; exp=$(timeout 30 node --no-warnings --experimental-strip-types $p.ts $arg 2>&1); fi
    printf '%s\n' "$res" > $OUT/$p.perry.out; printf '%s\n' "$exp" > $OUT/$p.node.out
    [ "$res" == "$exp" ] && same=true || same=false
  else rc=null; same=null; fi
  read cw cu cs cm < $OUT/$p.cold.time; read ww wu ws wm < $OUT/$p.warm.time
  printf '{"arm":"%s","probe":"%s","compile_rc":%s,"rebuilt_auto_libs":%s,"size":%s,"size_sym":%s,"cold_wall":%s,"cold_user":%s,"cold_sys":%s,"cold_maxrss_kb":%s,"warm_wall":%s,"warm_user":%s,"warm_sys":%s,"load_at_start":%s,"auto_dir":"%s","auto_archives":"%s","nm_tokio":%s,"nm_hyper":%s,"nm_reqwest":%s,"nm_h2":%s,"nm_tower":%s,"nm_mio":%s,"nm_tokio_rustls":%s,"nm_turnloop":%s,"strings_tokio_1x":%s,"run_rc":%s,"matches_node":%s}\n' \
    $ARM $p $crc $rebuilt $(sz $OUT/$p) $(sz $OUT/$p.sym) $cw $cu $cs $cm $ww $wu $ws $load0 "$(basename "$autodir")" "$autoarch" $ntok $nhyp $nreq $nh2 $ntow $nmio $ntr $ntl $stok $rc $same | tee -a $RES
  rm -f $OUT/$p.warm $OUT/$p.noauto; rm -rf $PERRY_CACHE_DIR
  for d in $TREE/target/perry-auto-*; do [ "$(basename $d)" = "$(basename "$autodir")" ] || rm -rf "$d"; done
done
echo PROBES_DONE | tee -a $OUT/events
