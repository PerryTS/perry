#!/bin/bash -l
# Instruction-count A/B on perrymaster: perf stat -e instructions:u over the
# stripped auto-optimized probe binaries of both arms, two-N differential
# (per-op = (I(N2)-I(N1))/(N2-N1)) with the bare-loop control, arms interleaved.
set -u
W=/root/claude-tokio-measure-work; OUT=$W/results-instr.jsonl; : > $OUT
REPS=${REPS:-7}; N1=${N1:-100}; N2=${N2:-500}
for rep in $(seq 1 $REPS); do
  for p in hello ops_loop ops_timers ops_http ops_fetch ops_net ops_crypto ops_zlib; do
    arms="before after"; [ $((rep % 2)) = 0 ] && arms="after before"
    for arm in $arms; do
      b=$W/$arm/probes/$p; [ -x $b ] || continue
      for n in $N1 $N2; do
        [ $p = hello ] && [ $n = $N2 ] && continue
        la=$(cut -d' ' -f1 /proc/loadavg)
        v=$(cd $W/probes && timeout 120 perf stat -x, -e instructions:u -e task-clock $b $n 2>&1 >/dev/null | awk -F, '/instructions/{i=$1} /task-clock/{t=$1} END{print i","t}')
        echo "{\"rep\":$rep,\"arm\":\"$arm\",\"probe\":\"$p\",\"n\":$n,\"instructions\":${v%%,*},\"task_clock_ms\":${v##*,},\"load\":$la}" >> $OUT
      done
    done
  done
done
echo INSTR_DONE
