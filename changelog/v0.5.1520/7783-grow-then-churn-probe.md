**The grow-then-churn pacing transition is measured, and the answer is not the one the issue predicted** (#7737 item 3).

The major-pacing backoff (#7726) has two well-covered ends and an untested middle. `retain.ts`-shaped growth is pure — every escalated full reclaims almost nothing, so `update_major_pacing_backoff` walks the shift to its cap of 2 and holds it. `tree.ts`-shaped churn is always high-yield, so the shift stays at 0. Nothing exercised a workload that is the first and then becomes the second, which is the shape a warm-cache phase followed by steady churn actually has. #7737 reasoned that such a workload inherits 8× pacing for a phase whose garbage profile is completely different, and that because array-growth forwarding stubs are reclaimable only by a full mark-sweep, the delayed reclaim would cost "up to 4× more transient RSS".

`benchmarks/gc_ratchet/probes/14_grow_then_churn.ts` is that workload: an all-live 300k-row cache that walks the shift 0 → 1 → 2 on growth alone, then 40k push-grown arrays dropped immediately, with the cache still live so the pacing baseline stays up.

A same-binary A/B — this exact probe at its shipped constants, one runtime built with `MAJOR_PACING_BACKOFF_SHIFT_MAX = 2` (shipped) and one with `0`, archives rebuilt and mtime-verified between arms, stdout byte-identical across both:

| | cap 2 (shipped) | cap 0 (no backoff) |
|---|---|---|
| escalation boundary reached | 56.3 MB | 14.1 MB |
| full mark-sweeps | 3 | 5 |
| copying minors | 24 | 24 |
| objects moved | 306,715 | 306,715 |
| peak RSS, 3 runs | 70.5 / 70.2 / 70.5 MB | 72.6 / 72.6 / 72.6 MB |

The boundary does move 4×, exactly as the issue reasoned. **Peak RSS does not follow it, and does not even move in the predicted direction**: the shipped backoff is 2.2 MB (3%) *lower*, reproducibly, while running two fewer full mark-sweeps for identical collector work — same 24 copying minors, same 306,715 objects moved. Footprint on this shape is set by the steady state the minors hold rather than by cumulative stub debt, so the escalation boundary is never the binding constraint, and moving it 4× buys the two fulls back for free.

Provenance: M1 Max laptop, `perry-dev` profile, `PERRY_GC_SCAVENGE_NURSERY_MB=1 PERRY_GC_MAJOR_PACING_FLOOR_MB=1`, peak RSS from plain runs under `/usr/bin/time -l` with no GC tracing (tracing perturbs RSS, so counters and footprint are read from separate runs). Not the pinned M1 mini that owns the artifact — the comparison is what transfers, and the pinned cells are whatever the artifact records.

So the probe pins rather than asserting the predicted bound — the number worth defending is the measured one. A future change that makes the delayed reclaim start to bind moves `rss_bytes` off the pin and the ratchet reports it.

The probe declares `PERRY_GC_SCAVENGE_NURSERY_MB=1` and `PERRY_GC_MAJOR_PACING_FLOOR_MB=1`. The mechanism is a ratio, so it reproduces at any scale, but its absolute scale is set by the nursery cap (which decides how early the first collection lands, and therefore how small the first post-full baseline is) and by the pacing floor; at the shipped 16 MB / 32 MB the same three escalations need a live set in the hundreds of MB and a churn excursion approaching a gigabyte. Both directives are asserted by a new test in `tests/test_gc_ratchet.py`, the same discipline `13_large_eden_survivors` already gets: losing either line leaves a probe that still passes, still collects, and no longer reaches the cap it exists to hold.

Checked for the failure this suite has paid for most often — a knob that moves the workload off the path it was chosen to exercise. At these settings `PERRY_GC_TRACE=1` reports the shift reaching 2 with the boundary at 56.3 MB, and the harness records 24 minor cycles moving 306,715 objects, so the collector under test demonstrably ran. The probe is also the most stable in the suite: 0% spread on both retention metrics and 0.022% on RSS, against 4.7% and 5.4% for `12_large_live_set` and `13_large_eden_survivors`.

Adding a probe changes the baseline's probe set, so the pinned artifact is re-pinned deliberately, on the quiet pinned host, with the existing thirteen rows checked green against the old baseline first so the re-pin is provably additive rather than absorbing a drift.

Item 4 of #7737 — promoting `gc-ratchet` and `gc-stress` to required branch-protection contexts — is a maintainer action and is untouched here.
