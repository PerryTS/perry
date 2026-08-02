#!/bin/bash
# Phase 0 corpus A/B: every 6th gap test, text backend vs in-process backend.
# For each test both arms compile with the SAME perry binary; only
# PERRY_LLVM_CLANG differs (default /usr/bin/clang vs the in-process shim).
# Divergence classes reported: SAME, DIFF (output/exit differs), CFAIL_T/CFAIL_P
# (one arm's compile failed), CFAIL_BOTH (pre-existing, not backend-related).
set -u
WT=/Users/amlug/projects/perry/wt-llvmbackend
SPIKE=$WT/experiments/llvm-inprocess-spike/target/release/perry-llvmc-spike
PERRY=$WT/target/perry-dev/perry
WORK=${1:?usage: batch_ab.sh <workdir>}
mkdir -p "$WORK"
export PERRY_RUNTIME_DIR=$WT/target/perry-dev
export PERRY_NO_AUTO_OPTIMIZE=1
export PERRY_LLVMC_SPIKE_LOG=$WORK/liveness.log
: > "$WORK/liveness.log"

i=0 same=0 diff=0 cfail_t=0 cfail_p=0 cfail_both=0
for t in $(ls "$WT"/test-files/test_gap_*.ts | sort); do
  i=$((i+1))
  [ $((i % 6)) -ne 0 ] && continue
  name=$(basename "$t" .ts)
  free_gb=$(df -g /System/Volumes/Data | awk 'NR==2{print $4}')
  if [ "$free_gb" -lt 6 ]; then echo "ABORT: only ${free_gb}GB free" | tee -a "$WORK/summary.txt"; exit 2; fi

  gtimeout 180 "$PERRY" "$t" -o "$WORK/$name.t" >"$WORK/$name.t.compile" 2>&1
  ct=$?
  PERRY_LLVM_CLANG=$SPIKE gtimeout 180 "$PERRY" "$t" -o "$WORK/$name.p" >"$WORK/$name.p.compile" 2>&1
  cp=$?

  if [ $ct -ne 0 ] && [ $cp -ne 0 ]; then cfail_both=$((cfail_both+1)); echo "CFAIL_BOTH $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi
  if [ $ct -ne 0 ]; then cfail_t=$((cfail_t+1)); echo "CFAIL_T $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi
  if [ $cp -ne 0 ]; then cfail_p=$((cfail_p+1)); echo "CFAIL_P $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi

  gtimeout 20 "$WORK/$name.t" >"$WORK/$name.t.out" 2>"$WORK/$name.t.err"; rt=$?
  gtimeout 20 "$WORK/$name.p" >"$WORK/$name.p.out" 2>"$WORK/$name.p.err"; rp=$?
  if [ $rt -eq $rp ] && cmp -s "$WORK/$name.t.out" "$WORK/$name.p.out" && cmp -s "$WORK/$name.t.err" "$WORK/$name.p.err"; then
    same=$((same+1)); echo "SAME $name (exit=$rt)" >> "$WORK/results.txt"
  else
    diff=$((diff+1)); echo "DIFF $name (exit t=$rt p=$rp)" >> "$WORK/results.txt"
  fi
  rm -f "$WORK/$name.t" "$WORK/$name.p"
done
compiles=$(grep -c . "$WORK/liveness.log" || true)
{
  echo "batch A/B complete: same=$same diff=$diff cfail_t=$cfail_t cfail_p=$cfail_p cfail_both=$cfail_both"
  echo "in-process compiles proven live: $compiles"
} | tee "$WORK/summary.txt"
