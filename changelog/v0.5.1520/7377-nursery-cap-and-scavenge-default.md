### Changed

- **Peak RSS drops ~69%.** The 16 MB nursery cap and the evacuating scavenge are
  now on by default; `PERRY_GC_SCAVENGE=0` reverts both for bisection.

  Neither is worth shipping alone, which is why they go together. Measured as a
  2×2 over the 8 `gc_ratchet` probes:

  | | no scavenge | scavenge |
  |---|---:|---:|
  | **no cap** | baseline | +0% RSS, +2% wall |
  | **cap 16 MB** | −33% RSS, **+23% wall** | **−69% RSS, +3% wall** |

  Scavenge alone moves nothing. The cap alone trades a third of the footprint
  for a quarter of the wall time. Together the cap makes collections frequent
  and scavenge makes them *evacuating* (O(live) copying) rather than O(heap)
  sweeps, so the frequency is cheap: **799,604,736 → 245,055,488 bytes at +2%
  wall**, all 8 probes byte-identical to Node.

  #7056 measured this and recommended "decouple the cap and keep it" — but the
  cap was gated behind two knobs that both defaulted **off**, so it had never
  been active in a shipped build, and acting on that recommendation literally
  ships the −33%/+23% arm.

  Enabling scavenge also defers alloc-point collections to a precise safepoint
  rather than collecting behind a forced conservative scan. That became
  reasonable only when #7370 made native roots the default.

- **`force_legacy_gc_pacing()` now pins all three pacing knobs.** It set only the
  moving-loop-polls flag, which used to be sufficient because the cap and the
  deferral branch both hung off it. With the cap unconditional and scavenge
  default-on, the guard silently stopped pinning anything — that alone accounted
  for 10 of the 23 `gc::` test failures this change first produced. The
  remaining 13 were tests that drive the budgeted/incremental stepper without
  the guard at all; they now pin it explicitly, since the shipped default
  bypasses that path by design.
