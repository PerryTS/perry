### parity: baseline the six-week dark debt so the gate ratchets from today (#8271)

The first complete parity run since 2026-07-04 (the 8-shard job from #8244;
the suite was tag-gated + `continue-on-error` before #8187) surfaced 93
unlisted parity failures, 27 compile failures and 16 crashes against a
9-entry `known_failures.json`, at 90.7% aggregate. All 136 are triaged into
`test-parity/known_failures.json` with provenance — crashes cite #8272
(hard-defect work queue), the perry-tui/ink family cites #348, the rest cite
the #8271 audit — and the 3 stale entries the run exposed
(`test_gap_zlib_4917_level`, `test_ramda_sum`, `test_sock_write_map`) are
deleted, so the bidirectional ratchet is live again: any NEW failure and any
entry that starts passing both go red.

Deliberately NOT parked: the #8223 builtin-construct trio. The provenance
audit refuses those entries while `gap_snapshot.json` asserts the tests pass
(fast mode) — the mode divergence IS the #8223 regression, so parity stays
red on exactly those three until it is fixed, which is the ratchet working.
