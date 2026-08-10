#!/usr/bin/env bash
# The #7469 `_tlv_get_addr` budget gate: keeps Darwin's thread-local access
# cost from creeping back into the runtime a fourth time.
#
# WHY THIS EXISTS
#
# On Darwin a `thread_local!` access is an out-of-line call to `_tlv_get_addr`.
# `crates/perry-runtime/src/tls_hot.rs` has removed that cost three times —
# 0% of `churn_alloc` after #7565, then 8-9%, then 11% on `interp`/`retain`,
# then 20.5% of `asyncpipe` — and nothing ever noticed the creep, because
# nothing measured it. The mechanism was never the problem; the absence of a
# gate was.
#
# THIS SCRIPT IS DESIGNED TO BE ABLE TO FAIL, checked against CLAUDE.md's
# "four ways a gate can be unable to fail":
#
#   1. no `continue-on-error`, no `|| true`; `set -euo pipefail`, and the
#      final line is a bare `exit "$rc"`.
#   2. NOT wired into branch protection by the change that adds it: a new gate
#      has never been green, so promoting it immediately blocks every open PR
#      (CLAUDE.md's corollary). That is a maintainer action after the first
#      observed green run on `main` — and per the corollary, taking it is not
#      optional follow-through.
#   3. the workflow's `concurrency` block cancels `pull_request` runs only.
#   4. THE SUBJECT MUST BE THE UNCOVERED ONE. This is the specific vacuity
#      this gate exists to avoid. Measuring `churn_alloc` would pass forever
#      while the real cost grew, because churn's thread-locals are exactly the
#      sixteen the named-field cache was curated for — a gate green because
#      its subject never ran. So the subjects are `asyncpipe` (Map/Set
#      registries, buffer brands, descriptor state, template literals, async)
#      and `interp` (inline-cache misses, field lookup, arguments objects),
#      and `scripts/tls_budget_check.py` — the actual verdict logic — REFUSES
#      to return a pass unless the run proves it exercised paths outside those
#      sixteen: `PERRY_TLS_HOT_STATS=1` must report `direct_tsd=1` (else the
#      cache is inert and a low share means the program resolved nothing) and
#      `claimed` above a floor no allocation microbenchmark can clear.
#      `--self-test` drives all seven of its rejections and runs on every PR.
#
# SABOTAGE CHECK (the proof this can go red, re-runnable by hand)
#
#   Revert one hot declaration to a raw `thread_local!`:
#
#     sed -i '' 's/^crate::perry_thread_local! {/thread_local! {/' \
#         crates/perry-runtime/src/buffer/header.rs
#
#   Rebuild and re-run this gate. `asyncpipe`'s share goes from ~1% to ~9%,
#   `interp`'s from ~1% to ~6%, and both fail. Restoring the macro restores
#   the pass. Recorded in the PR that introduced this file.
#
# USAGE
#
#   scripts/tls_budget_gate.sh <perry-binary> [<out-dir>]

set -euo pipefail

PERRY_BIN="${1:?usage: tls_budget_gate.sh <perry-binary> [out-dir]}"
OUT_DIR="${2:-$(mktemp -d)}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="$REPO_ROOT/benchmarks/tls-budget"

mkdir -p "$OUT_DIR"
PERRY_BIN="$(cd "$(dirname "$PERRY_BIN")" && pwd)/$(basename "$PERRY_BIN")"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "tls-budget: _tlv_get_addr is a Darwin TLS artefact; nothing to measure on $(uname -s)." >&2
    exit 0
fi

# name | expected stdout | budget (% of root samples)
PROGRAMS=(
    "asyncpipe|39197275 4718978 6 39197275 0|5.0"
    "interp|17088400|3.0"
)

SAMPLE_SECONDS="${TLS_BUDGET_SAMPLE_SECONDS:-8}"
rc=0

for entry in "${PROGRAMS[@]}"; do
    IFS='|' read -r name expected budget <<<"$entry"
    src="$FIXTURES/$name.ts"
    exe="$OUT_DIR/$name"

    echo "--- compiling $name"
    # Debug symbols so `sample` can name frames: an unsymbolicated profile
    # reports 0% for everything, which would read as a pass.
    ( cd "$OUT_DIR" && PERRY_DEBUG_SYMBOLS=1 "$PERRY_BIN" "$src" -o "$exe" >/dev/null )

    # Correctness BEFORE timing: a program that prints the wrong answer is not
    # a faster program.
    actual="$("$exe")"
    if [[ "$actual" != "$expected" ]]; then
        echo "tls-budget: $name printed '$actual', expected '$expected'" >&2
        rc=1
        continue
    fi

    echo "--- profiling $name (${SAMPLE_SECONDS}s)"
    PERRY_TLS_HOT_STATS=1 "$exe" >/dev/null 2>"$OUT_DIR/$name.stats" &
    pid=$!
    sleep 1
    sample "$pid" "$SAMPLE_SECONDS" -f "$OUT_DIR/$name.sample" >/dev/null 2>&1 || true
    wait "$pid"

    if [[ ! -s "$OUT_DIR/$name.sample" ]]; then
        echo "tls-budget: sample produced no report for $name" >&2
        rc=1
        continue
    fi

    if ! python3 "$REPO_ROOT/scripts/tls_budget_check.py" \
        --report "$OUT_DIR/$name.sample" \
        --stats "$OUT_DIR/$name.stats" \
        --budget "$budget" \
        --label "$name"; then
        rc=1
    fi
done

exit "$rc"
