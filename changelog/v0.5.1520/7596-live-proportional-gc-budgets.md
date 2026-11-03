**perf(gc): live-proportional collection budgets at both generations (#7592)**

Three changes, one principle: no constant band may pace a collector whose
per-cycle cost is O(live) — total work goes quadratic in the live set, and a
bigger constant only moves the cliff.

1. The survivor-promotion handoff fires on **current** old-gen pressure only.
   It used to fire on `old + promotable` — but promotable bytes sit in the
   survivor space, where a full mark-sweep can neither reclaim them nor the
   old-gen space they have not yet occupied. A handoff over a near-empty
   old-gen is guaranteed futile (measured: 1,015 ms over 4.2 MB, 0 freed).
2. A copying minor's promoted bytes credit the old-reclaim baseline: they are
   live by construction, so a reclaim fired because promotion crossed a
   threshold finds them all live and frees nothing (measured: 2,100 ms over
   274 MB just-promoted, 0.0 MB freed). Standard GOGC trade: the one visible
   ratchet cost is `12_large_live_set.heap_total_bytes` +36 % (reserved
   high-water from collecting less often) while its heap_used is flat and its
   peak RSS and wall both improve.
3. Both pacing bands become live-proportional: the old-reclaim growth band is
   `max(32 MB, baseline/2)` (shared by dueness and debt so they cannot
   diverge), and the scavenge nursery cap is `max(influx_driven,
   old_gen_reclaimable/2)` — keyed on old-gen occupancy, not total arena
   in-use, which would be a fixed point from-space can never cross.

`json_pipeline` `build_out`, 500k records: 61.6 s (main) / 12.1 s (#7594
latch) → **5.8 s**, 10.6× vs main, ns/record flat within ~30 % across a 20×
size range (main grows 70×). Output hash identical on every row; RSS +1.7 %
over the latch arm.
