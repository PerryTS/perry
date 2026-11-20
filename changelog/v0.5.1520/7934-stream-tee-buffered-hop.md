**A buffered tee source no longer pays the cold-pull two-hop cadence.**
`ReadableStream.tee` on an already-buffered source delivered its first chunk
one observable microtask late (`t2` where node lands `t1`), because the
buffered case rode the cold/pull-driven pipeline calibrated in #6657. The two
cases are now distinguished: pre-buffered sources deliver through the normal
queued fanout reaction (one job), empty cold sources keep their two-hop
demand cadence. `test_gap_stream_tee_tick_parity` leaves the known-failure
ratchet with it. (Fragment added at merge; full analysis in the PR body.)
