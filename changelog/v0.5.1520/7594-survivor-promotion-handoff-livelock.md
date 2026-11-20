**perf(gc): break the survivor-promotion handoff livelock (#7592)**

`copied_minor_promotion_handoff_due` replaces a minor collection with a full
mark-sweep to make room in old-gen for survivors about to be promoted. But a
full mark-sweep is non-moving — it promotes nothing — so it cannot relieve the
pressure that scheduled it: the survivor space still holds the same bytes, the
reclaim baseline it resets does not count them, and the predicate is true again
at the very next minor. The copying minor that would have performed the
promotion never ran.

Measured on `json_pipeline` at 200k records: 19 of 22 collections were
`survivor_promotion_bytes` fulls, each freeing 0.0 MB at ~400 ms — 7.6 s of an
8.6 s phase, with peak RSS the same whether they ran or not.

Latched to one handoff per copying minor: the handoff makes room, the copying
minor performs the promotion that consumes it. The latch clears only on a
*copying* minor, since a non-moving minor fallback promotes nothing and would
reinstate the livelock at half rate. The guard precedes the
`copied_minor_promotable_active_survivor_bytes()` walk, so a suppressed handoff
also skips that O(n) survivor pass.

The same workload now runs 6 collections instead of 22, with the copying minor
promoting 110 MB. `build_out` at 500k records goes 57,242 ms → 10,551 ms
(5.4×), output hash identical; peak RSS rises 17–24 % at the large sizes,
because the run performs fewer collections.

Confined to workloads that hit the livelock: measuring both arms back to back on
one host (the pinned ratchet baseline is from 0.5.1315 on another machine and
cannot separate this change from drift), all 12 probes agree on every semantic
counter across 144 metric medians, with only ungated RSS/wall cells moving, all
≤ 0.12 %.
