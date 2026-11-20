Make `turnloop_pool::OUTSTANDING` a `per_test_global!`.

The per-test global-sink audit flags a bare process-global `static` that a test
reset helper writes, and `OUTSTANDING` is one: `reset_for_test` subtracts this
thread's leftover jobs from it. Tests also assert on it through
`has_pending_jobs()` — `tests.rs:180` ("an accepted job keeps the loop alive")
and `:533` ("the keep-alive gate is released") — so a bare static lets one
test's residue decide another's verdict, which is the hazard the gate exists
for. That rules out the allowlist route the gate's message offers.

**No `adopt` is needed, because every access is on the submitting thread.**
This mattered enough to verify rather than assume: `OUTSTANDING` is a keep-alive
counter, and splitting one per-thread would be a correctness bug, not a test
artifact — a process that will not exit. `deliver` resolves the job out of the
thread-local `POOL` table and returns early on a miss, so it can only retire a
job on the thread that owns it; `OutstandingGuard` is constructed inside that
function, so its `fetch_sub` is necessarily on the thread that did the
`fetch_add`. The suite asserts this directly (`tests.rs:172`: "the delivery must
run on the submitting thread, where JS lives"). Contrast `NOTIFY_AT_NS`, which a
different thread writes and so does need `shared_key`/`adopt`.

Outside a test build `per_test_global!` IS the plain `static`, byte for byte, so
the process-wide keep-alive gate is unchanged in every shipped configuration.
