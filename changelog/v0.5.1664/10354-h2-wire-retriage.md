Re-triage the five `http2_wire` gap entries from `crash` to `parity_fail`.

`test_gap_http2_wire_{client_ping,goaway_recv,goaway_send,session_lifecycle,settings_ack}`
were skip-listed on 2026-09-16 as TIMEOUTs — the fixture printed its full
output and then failed to exit. `run_parity_tests.sh` buckets that as a crash:
`PERRY_RUN_TIMEOUT` (default 10s) kills the run, and `perry_abnormal_exit` maps
the resulting exit 124 to a crash classification. That left `gap_snapshot.json`
accepting a `crash`, which #7530 says it must never do.

Re-measured on 8360231ec2, ubuntu-latest, `PERRY_SKIP_BUILD=1` against prebuilt
release archives, `run_gap_tests.sh --filter http2_wire`: 11 parity_fail,
**0 crashed**. The five complete in roughly 7-8s each, well inside the run
budget. The hang no longer reproduces, so the recorded status was stale rather
than wrong when written.

The behavioural gaps these entries document are unchanged and still tracked by
#10327 — `session.ping()` puts nothing on the wire, a peer GOAWAY fires no
`'goaway'` event, `settings()` applies with no peer ACK, and so on. Only the
failure CLASS changes: these are ordinary output mismatches, not hangs.

**Cause not bisected.** Something on this branch fixed the run-phase hang,
plausibly later turnloop work, but that was not measured and this entry should
not be cited as evidence for any particular change. Note this is NOT #10757's
compile-budget family (`http2_settings`, `3527_http_ctor_prototype`), which is
a separate mechanism: a compile-phase timeout produces `compile_fail`, not
`crash`.

Two files move together and the split is intentional: `known_failures.json`
carries no `status` field, so it takes the reason text, while the status lives
only in the generated `gap_snapshot.json` and is rewritten by
`gap_snapshot.py update` from the Linux report. A reason-only change in one
file and a status-only change in the other is the expected shape, not drift.
