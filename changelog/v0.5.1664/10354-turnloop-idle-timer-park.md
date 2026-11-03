### perf(turnloop): halve the per-request completions of a keep-alive HTTP request

A reused keep-alive connection serving a 6-byte body cost **4 completions per
request** on the turnloop P1 net path. Two of them were the request's read and
the response's write — the floor. The other two were the connection's idle
keep-alive deadline being destroyed and rebuilt on **every single request**, and
neither was routed anywhere.

`turnloop_serve`'s `on_data` disarmed the idle close with `tl::timer_cancel`,
which is `driver.close(timer_handle)`. turnloop answers a close with two
completions — `cancel_inner` finishes the pending timer operation with
`OpResult::Cancelled` (turnloop `driver.rs:956`), then `maybe_closed` enqueues
the handle's own `OpResult::Closed` (`driver.rs:335`). Both arrive with an
`OP_TIMER` token, and `turnloop_net::dispatch` drops both, because an unfired
deadline has nothing to deliver. `complete_response` then had to build a fresh
handle, since `timer_cancel` had removed the record that `timer_arm` looks for.

That is the exact case `timer_arm` was written to avoid — its doc says "arming
an id that already has a deadline moves it, so a per-connection timeout can be
refreshed on every read without churning handles" — but the cancel in between
destroyed the handle the move needed.

`turnloop_net::timer_park` disarms a deadline by moving it out of reach instead
of destroying it, so the disarm and the later re-arm are both a `timer_reset`:
no completion at all, and one timer handle for the life of a connection rather
than one per request. `timer_cancel` keeps its destroying meaning and is still
what every teardown path uses. A timer that has already fired or is closing
cannot be moved, so that case falls back to the destroying path rather than
leaving a live deadline armed.

Measured with a single keep-alive connection and `PERRY_LOOP_STATS=1`, taking
the **slope** between 1,000 and 3,000 requests so process startup, listen,
accept and teardown cancel out:

| per request | before | after |
|---|---|---|
| `completions` | 4.000 | **2.000** |
| net read completions | 1.000 | 1.000 |
| net write completions | 1.000 | 1.000 |
| net timer completions | 2.000 | **0.000** |
| timer handles created | 1.000 | **0.000** |

The response itself was already one write submission — `send_response` encodes
head and body into a single buffer and calls `write_raw` once — so there was no
header/body coalescing left to do, and the read is multishot and never re-armed
per request.

**CPU is unchanged within measurement error.** Interleaved A/B of two binaries
differing only in this call site, slope method, 15 rounds at 64 connections
(12,800 → 64,000 requests): median 22.64 µs/req before, 22.40 µs/req after
(−1.1 %), against a per-round spread of 12–28 µs on a contended host. At one
connection, 9 rounds: 37.74 → 37.40 µs (−0.9 %). The two completions are real
work — a handle allocation and release, two enqueues, two dispatch lookups and
two deadline reprogrammings — but they are well under this host's resolution.
The win claimed here is the completion count and the handle churn, not CPU.

Behaviour is unchanged and matches the oracle. With `keepAliveTimeout = 500`,
an idle connection is closed 1501 ms after the last response before the change,
1501 ms after it, and 1501 ms under Node 26.5.1; a connection reused after a
partial idle wait still works in all three.

`turnloop_net::census` is a new `PERRY_LOOP_STATS=1` diagnostic behind the same
gate as the rest of the loop stats: two `[perry-loop] p1` lines at the
process-exit funnel giving submissions and completions per op class, with the
timer class split into expiry / cancel / close / park / reset. The aggregate
`completions=` field on the `driver=turnloop` line cannot answer "what did one
request cost" — it is summed in `agent_loop` before routing, so it mixes P1 net
with P2 process, P3 JS timers and P4 pool. Unset, every hook is one relaxed load
of the cached `loop_stats` state; the `driver=turnloop` line is untouched, since
two instruments parse it positionally.
