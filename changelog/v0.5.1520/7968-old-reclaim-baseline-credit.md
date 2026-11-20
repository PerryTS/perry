### Fixed

- **GC: credit every promotion to the old-reclaim baseline, untraced or not (#7965).**
  #7902 made a copying minor skip `credit_promoted_bytes_to_old_baseline` for an
  untraced whole-block promotion, on the grounds that "live by construction" is a
  marked-liveness claim `PromotionLiveness::AssumeAllLive` does not make. The
  premise is right and the conclusion does not follow, because
  `GC_LAST_OLD_RECLAIM_IN_USE_BYTES` is **not a liveness claim** — it is the base
  of a growth measurement. `old_in_use - baseline` is meant to read "how much has
  old-gen grown since the last reclaim decision", and bytes a minor has just
  relocated there are growth that decision has already seen.

  A fully-live young generation promotes untraced on *every* cycle, so on exactly
  the workloads that reach this path nothing else credits the baseline and it
  stays pinned at 0. `old_in_use - baseline` then collapses into absolute
  occupancy, and `gc_old_reclaim_growth_band_bytes`'s proportional half
  (`baseline / OLD_RECLAIM_GROWTH_DIVISOR`) collapses with it, leaving the
  constant floor — **a constant band pacing a collector whose per-cycle cost is
  O(live)**, which is the quadratic shape #7592 removed at this trigger and #7594
  removed one generation down. The pin re-created it.

  Bisected on the **full count** over `8260a9e50..d78efca41`: `0809ada92 →
  e886b56dd` (#7900 + #7901) is flat at ≤0.6% and both are exonerated;
  `e886b56dd → 1bd5eeb6b` (#7902) carries the whole regression. The
  granularity hypothesis originally attached to #7965 — that the absolute arm is
  decided by promotion *step size* — is refuted here by `deeplist`'s
  untraced-promotion trace being byte-identical across the boundary.

  Measured on the `gc-handoff` corpus, 19/19 byte-exact against the expected
  output and exit 0 in both arms: `retain` **1 full → 0** and 8 237 M → 2 176 M
  instructions retired (−73.6%) with peak RSS 311 → 249 MB; `retain_wide`
  **1 full → 0** and 5 741 M → 2 859 M (−50.2%). No program gains a cycle or a
  full, and the other 14 move by at most 0.15%. Against the pre-regression base
  the arm lands *below* it (`retain` −23.4%, `retain1` −31.5%, `deeplist`
  −17.5%), which is #7960's independently-reported figure and the arithmetic
  check that the regression is gone rather than masked.

  #7902's three other changes are kept — the bounded, heap-scaled untraced
  budget, the clamped `implied_dead_bytes` charge, and
  `request_old_reclaim_for_untraced_promotions`. They close a real defect and
  none of them paces on this quantity: each acts on *evidence about the
  promoted cohort*, whereas a pinned pacing base acts on every program that
  retains.

  Gated by `an_untraced_promotion_credits_the_old_reclaim_baseline`, which
  drives a real untraced-promoting copying minor through the production entry
  point and asserts the baseline advanced by exactly the bytes it relocated
  (with the untraced counters asserted non-zero, so an evacuating or traced
  cycle cannot satisfy it), then replays `retain`'s measured promotion series
  and asserts both that the credited baseline keeps `old_reclaim_pressure_due`
  false **and** that the same series against an uncredited baseline fires. That
  second assertion caught its own first draft, which did not cross the band
  while the `RETAINING` latch was armed.
