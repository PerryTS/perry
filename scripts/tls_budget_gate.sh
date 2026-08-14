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
#      `--self-test` drives every rejection and runs on every PR.
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

subject_is_running() {
    local state
    state="$(ps -p "$1" -o state= 2>/dev/null)" || return 1
    [[ -n "$state" && "$state" != *Z* ]]
}

sample_succeeded() {
    [[ "$1" -eq 0 ]]
}

if [[ "${1:-}" == "--self-test" ]]; then
    failures=0

    ( exit 0 ) &
    short_pid=$!
    wait "$short_pid" || true
    if subject_is_running "$short_pid"; then
        echo "self-test FAILED: accepted a subject that exited before attach" >&2
        failures=1
    fi

    sleep 0.2 &
    live_pid=$!
    sleep 0.05
    if ! subject_is_running "$live_pid"; then
        echo "self-test FAILED: rejected a live subject before attach" >&2
        failures=1
    fi
    wait "$live_pid" || true

    if ! sample_succeeded 0; then
        echo "self-test FAILED: rejected a successful sample command" >&2
        failures=1
    fi
    if sample_succeeded 1; then
        echo "self-test FAILED: accepted a failed sample command" >&2
        failures=1
    fi

    [[ "$failures" -eq 0 ]] || exit 1
    echo "self-test: pre-attach liveness and sample-status checks reject both failure modes"
    exit 0
fi

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
    stats="$OUT_DIR/$name.stats"
    report="$OUT_DIR/$name.sample"
    profile_stdout="$OUT_DIR/$name.profile.stdout"

    # OUT_DIR is caller-provided and may be reused. A report or stats file from
    # an earlier run must never let a failed compile/attach inspect stale proof.
    rm -f "$exe" "$stats" "$report" "$profile_stdout"

    echo "--- compiling $name"
    # Debug symbols so `sample` can name frames: an unsymbolicated profile
    # reports 0% for everything, which would read as a pass.
    #
    # Explicitly checked rather than left to `set -e`: a compiler that cannot
    # run is the one failure that must never be mistaken for "nothing to
    # measure", and a subshell's status is easy to lose behind a pipe.
    if ! ( cd "$OUT_DIR" && PERRY_DEBUG_SYMBOLS=1 "$PERRY_BIN" "$src" -o "$exe" >/dev/null ); then
        echo "tls-budget: compiling $name failed" >&2
        rc=1
        continue
    fi
    if [[ ! -x "$exe" ]]; then
        echo "tls-budget: compiling $name produced no executable" >&2
        rc=1
        continue
    fi

    # Correctness BEFORE timing: a program that prints the wrong answer is not
    # a faster program.
    # These are shipped-default measurements. Do not let a developer shell's
    # GC/debug/diagnostic variables silently select a different runtime mode.
    actual="$(env -i "$exe")"
    if [[ "$actual" != "$expected" ]]; then
        echo "tls-budget: $name printed '$actual', expected '$expected'" >&2
        rc=1
        continue
    fi

    echo "--- profiling $name (${SAMPLE_SECONDS}s)"
    if [[ "$name" == "asyncpipe" ]]; then
        # Repeat the same correctness-checked pipeline so the sampled process
        # remains live through Darwin's external attach delay. Its profile mode
        # asserts every repetition returns the same result before printing it.
        env -i PERRY_TLS_HOT_STATS=1 "$exe" --tls-budget-profile \
            >"$profile_stdout" 2>"$stats" &
    else
        env -i PERRY_TLS_HOT_STATS=1 "$exe" >"$profile_stdout" 2>"$stats" &
    fi
    pid=$!
    sleep 1
    if ! subject_is_running "$pid"; then
        wait "$pid" || true
        echo "tls-budget: $name exited before sample attached" >&2
        rc=1
        continue
    fi
    sample_rc=0
    sample "$pid" "$SAMPLE_SECONDS" -f "$report" >/dev/null 2>&1 || sample_rc=$?
    if ! wait "$pid"; then
        echo "tls-budget: profiled $name exited unsuccessfully" >&2
        rc=1
        continue
    fi
    if ! sample_succeeded "$sample_rc"; then
        echo "tls-budget: sample failed for $name (exit $sample_rc)" >&2
        rc=1
        continue
    fi
    if [[ "$name" == "asyncpipe" ]] \
        && ! grep -qE '^\[tls-budget\] completed=[1-9][0-9]*$' "$stats"; then
        echo "tls-budget: asyncpipe did not report a completed profiled pipeline" >&2
        rc=1
        continue
    fi

    profile_actual="$(<"$profile_stdout")"
    if [[ "$profile_actual" != "$expected" ]]; then
        echo "tls-budget: profiled $name printed '$profile_actual', expected '$expected'" >&2
        rc=1
        continue
    fi

    if [[ ! -s "$report" ]]; then
        echo "tls-budget: sample produced no report for $name" >&2
        rc=1
        continue
    fi

    if ! python3 "$REPO_ROOT/scripts/tls_budget_check.py" \
        --report "$report" \
        --stats "$stats" \
        --budget "$budget" \
        --label "$name"; then
        rc=1
    fi
done

exit "$rc"
