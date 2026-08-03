#!/bin/bash
# Phase 0 corpus A/B: every 6th gap test, text backend vs in-process backend.
# For each test both arms compile with the SAME perry binary; only
# PERRY_LLVM_CLANG differs (default /usr/bin/clang vs the in-process shim).
# Divergence classes reported: SAME, DIFF (output/exit differs), CFAIL_T/CFAIL_P
# (one arm's compile failed), CFAIL_BOTH (pre-existing, not backend-related).
set -u
# Worktree root: derived from this script's own location so a fresh clone on
# any host works unmodified; override with WT=... for an out-of-tree layout.
WT=${WT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}
SPIKE=$WT/experiments/llvm-inprocess-spike/target/release/perry-llvmc-spike
PERRY=$WT/target/perry-dev/perry
WORK=${1:?usage: batch_ab.sh <workdir> [shim|flag] [stride] [offset]}
# Arm P mechanism: "shim" = PERRY_LLVM_CLANG=<spike binary> through an
# unmodified perry; "flag" = PERRY_LLVM_INPROCESS=1 through a perry built
# with the llvm-inprocess feature.
MODE=${2:-shim}
STRIDE=${3:-6}
OFFSET=${4:-0}
# Portability: coreutils installs `gtimeout` on macOS (brew), `timeout` on Linux.
TIMEOUT=$(command -v gtimeout || command -v timeout)
# Probe the filesystem the work dir actually lives on, not "/" — on a typical
# Linux box /home is a separate (and much larger) filesystem than /.
free_gb_now() {
  if [ "$(uname)" = "Darwin" ]; then
    df -g "$WORK" | awk 'NR==2{print $4}'
  else
    df -BG --output=avail "$WORK" | awk 'NR==2{gsub("G","",$1); print $1}'
  fi
}
mkdir -p "$WORK"
export PERRY_RUNTIME_DIR=$WT/target/perry-dev
export PERRY_NO_AUTO_OPTIMIZE=1
export PERRY_LLVMC_SPIKE_LOG=$WORK/liveness.log
: > "$WORK/liveness.log"
touch "$WORK/results.txt"

# A full-suite run is ~100 minutes of a developer workstation's life, so it has
# to survive that workstation being used. Two guards, learned the hard way when
# a concurrent cargo build pushed the box into the OOM killer and six compiles
# died mid-link — recorded as CFAIL_BOTH, which reads exactly like a real
# pre-existing failure:
#
#   1. Re-running skips tests already recorded, so an interrupted run resumes
#      instead of starting over (delete results.txt for a clean run).
#   2. A compile killed by a signal (128+n; the OOM killer's SIGKILL is 137) is
#      never scored. It waits for memory, retries once, and if it dies again it
#      is recorded as OOM/INFRA — a category no backend conclusion may be drawn
#      from, and one the summary reports separately.
free_mb_now() {
  if [ "$(uname)" = "Darwin" ]; then
    vm_stat | awk '/page size of/{p=$8} /Pages free/{f=$3} END{print int(f*p/1048576)}'
  else
    awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo
  fi
}
MIN_FREE_MB=${MIN_FREE_MB:-2048}
wait_for_memory() {
  local waited=0
  while [ "$(free_mb_now)" -lt "$MIN_FREE_MB" ] && [ $waited -lt 600 ]; do
    sleep 10; waited=$((waited+10))
  done
}
# A compile that exits 128+n died on signal n rather than failing to compile.
# (`timeout`'s own 124 is deliberately NOT in this class: a compile that hangs
# for 180s is a result worth seeing, not infrastructure noise.)
killed_by_signal() { [ "$1" -gt 128 ]; }

i=0 same=0 diff=0 cfail_t=0 cfail_p=0 cfail_both=0 skipped=0 infra=0
for t in $(ls "$WT"/test-files/test_gap_*.ts | sort); do
  i=$((i+1))
  [ $((i % STRIDE)) -ne $OFFSET ] && continue
  name=$(basename "$t" .ts)
  if grep -q " $name\$\| $name " "$WORK/results.txt"; then skipped=$((skipped+1)); continue; fi
  free_gb=$(free_gb_now)
  if [ "$free_gb" -lt 6 ]; then echo "ABORT: only ${free_gb}GB free" | tee -a "$WORK/summary.txt"; exit 2; fi
  wait_for_memory

  compile_arms() {
    "$TIMEOUT" 180 "$PERRY" "$t" -o "$WORK/$name.t" >"$WORK/$name.t.compile" 2>&1
    ct=$?
    if [ "$MODE" = "flag" ]; then
      PERRY_LLVM_INPROCESS=${INPROCESS_VALUE:-1} "$TIMEOUT" 180 "$PERRY" "$t" -o "$WORK/$name.p" >"$WORK/$name.p.compile" 2>&1
      cp=$?
      # Liveness: the arm must have announced the in-process backend.
      if [ $cp -eq 0 ] && ! grep -q "in-process LLVM backend active" "$WORK/$name.p.compile"; then
        echo "NOT_LIVE $name" >> "$WORK/results.txt"; cp=97
      fi
    else
      PERRY_LLVM_CLANG=$SPIKE "$TIMEOUT" 180 "$PERRY" "$t" -o "$WORK/$name.p" >"$WORK/$name.p.compile" 2>&1
      cp=$?
    fi
  }
  compile_arms
  if killed_by_signal $ct || killed_by_signal $cp; then
    wait_for_memory
    compile_arms
    if killed_by_signal $ct || killed_by_signal $cp; then
      infra=$((infra+1)); echo "OOM_INFRA $name (signal t=$ct p=$cp — NOT a backend result)" >> "$WORK/results.txt"
      rm -f "$WORK/$name".[tp]; continue
    fi
  fi

  if [ $ct -ne 0 ] && [ $cp -ne 0 ]; then cfail_both=$((cfail_both+1)); echo "CFAIL_BOTH $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi
  if [ $ct -ne 0 ]; then cfail_t=$((cfail_t+1)); echo "CFAIL_T $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi
  if [ $cp -ne 0 ]; then cfail_p=$((cfail_p+1)); echo "CFAIL_P $name" >> "$WORK/results.txt"; rm -f "$WORK/$name".[tp]; continue; fi

  "$TIMEOUT" 20 "$WORK/$name.t" >"$WORK/$name.t.out" 2>"$WORK/$name.t.err"; rt=$?
  "$TIMEOUT" 20 "$WORK/$name.p" >"$WORK/$name.p.out" 2>"$WORK/$name.p.err"; rp=$?
  if [ $rt -eq $rp ] && cmp -s "$WORK/$name.t.out" "$WORK/$name.p.out" && cmp -s "$WORK/$name.t.err" "$WORK/$name.p.err"; then
    same=$((same+1)); echo "SAME $name (exit=$rt)" >> "$WORK/results.txt"
  else
    diff=$((diff+1)); echo "DIFF $name (exit t=$rt p=$rp)" >> "$WORK/results.txt"
  fi
  rm -f "$WORK/$name.t" "$WORK/$name.p"
done
if [ "$MODE" = "flag" ]; then
  compiles=$(grep -l "in-process LLVM backend active" "$WORK"/*.p.compile 2>/dev/null | wc -l | tr -d ' ')
else
  compiles=$(grep -c . "$WORK/liveness.log" || true)
fi
{
  echo "batch A/B complete: same=$same diff=$diff cfail_t=$cfail_t cfail_p=$cfail_p cfail_both=$cfail_both"
  echo "resumed (already recorded, not re-run): $skipped"
  echo "OOM/INFRA (killed by signal twice — excluded from every conclusion): $infra"
  echo "in-process compiles proven live: $compiles"
} | tee "$WORK/summary.txt"
