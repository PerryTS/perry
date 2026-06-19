#!/usr/bin/env bash
# #1114 bisect helper. Run from the workspace root against the user's
# actual reproducer (the synthetic in /tmp/repro1114_real *did not*
# trigger the wedge even with a live MySQL — shop-admin's
# ~68-server-file shape is required).
#
# Usage:
#   PERRY_REPRO_CMD="/path/to/shop-admin/dist/server-binary"
#   PERRY_REPRO_CPU_LIMIT=80     # consider wedge when ./repro CPU >80% for 5s
#   PERRY_REPRO_TIMEOUT=15       # kill after 15s
#   bash scripts/bisect_1114.sh
#
# The script:
#   1. Walks first-parent commits between v0.5.1008 (0a908394) and
#      v0.5.1009 (c71c780b) on `main`.
#   2. For each commit: cargo build --release -p perry-runtime
#      -p perry-stdlib -p perry, then rebuild the user's binary
#      ($PERRY_REPRO_CMD presumed to be a script that rebuilds it),
#      run it, sample CPU.
#   3. Reports the first commit at which CPU exceeded the limit.
#
# IMPORTANT: this rebuilds perry-runtime + perry-stdlib + perry per
# step (~2 min each). With 8 commits in the candidate range, expect
# ~15 minutes. Pin `PERRY_NO_AUTO_OPTIMIZE=1` if your repro doesn't
# rely on the auto-optimize flip — saves a per-step rebuild of the
# auto-opt cache.
set -euo pipefail

CMD="${PERRY_REPRO_CMD:-}"
if [ -z "$CMD" ]; then
  echo "PERRY_REPRO_CMD must be set to the binary (or wrapper script) you want to run."
  exit 2
fi
CPU_LIMIT="${PERRY_REPRO_CPU_LIMIT:-80}"
TIMEOUT="${PERRY_REPRO_TIMEOUT:-15}"

# Candidate commit range (oldest first), from `git log --first-parent
# 0a908394..c71c780b`. Update when the upstream lineage changes.
COMMITS=(
  aa6a2cd2  # fix(security): #999 — validate explicit bundle IDs at read time
  3856caad  # fix(transform): #1047 — async early return followed by an unreached await
  91bb8b5a  # fix(wasm): #1049 instances 2+3 — wrapForI64 BigInt-coerce non-i64 returns
  1a51a2f0  # test(async): #1013 pin Promise.all + array destructure
  d16d832a  # fix(perry-jsruntime): #1022 — v8 proxies for sqlite Database/Statement
  52847008  # fix(jsruntime): #1021 — break V8-fallback CJS require cycles + process.exit
  fcb097c9  # test(async): extend #1013 coverage to cross-module property-return shape
  7e3bd5a4  # fix(codegen): #321 — Effect.succeed via named-import-of-namespace-reexport
  634e1f58  # fix(fastify): non-blocking listen() + main-thread pump
  c71c780b  # fix(object): skip misaligned non-object pointers (v0.5.1009 release commit)
)

probe_cpu() {
  local pid="$1"
  ps -p "$pid" -o %cpu= 2>/dev/null | tr -d ' '
}

verdict_at_head() {
  local pid
  "$CMD" >/dev/null 2>&1 &
  pid=$!
  sleep 5
  local cpu1; cpu1=$(probe_cpu "$pid"); cpu1=${cpu1:-0}
  sleep 5
  local cpu2; cpu2=$(probe_cpu "$pid"); cpu2=${cpu2:-0}
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "cpu_after_5s=$cpu1 cpu_after_10s=$cpu2"
  # awk-style comparison for floats
  awk -v c="$cpu2" -v l="$CPU_LIMIT" 'BEGIN { exit !(c > l) }'
}

FIRST_BAD=""
for sha in "${COMMITS[@]}"; do
  echo "==> checkout $sha"
  git checkout -q "$sha"
  cargo build --release -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static -p perry --quiet 2>&1 \
    | tail -3 || { echo "build failed at $sha — skipping"; continue; }
  echo "==> probing CPU at $sha"
  if timeout "$TIMEOUT" bash -c "$(declare -f probe_cpu verdict_at_head); verdict_at_head"; then
    echo "==> $sha : WEDGED (CPU > $CPU_LIMIT)"
    FIRST_BAD="$sha"
    break
  else
    echo "==> $sha : clean"
  fi
done

git checkout -q main
if [ -n "$FIRST_BAD" ]; then
  echo ""
  echo "First wedge observed at: $FIRST_BAD"
else
  echo ""
  echo "No commit in the candidate range exceeded the CPU limit. The"
  echo "regression may be earlier (pre-v0.5.1008), in an interaction"
  echo "with auto-optimize feature flags, or in a build artifact not"
  echo "covered by this script."
fi
