A synchronous full mark-sweep costs less per live object, in three exact
changes:

- **Census membership from per-block start bitmaps.** `ValidPointerSet` used
  to answer "is this an arena object start?" with two binary searches (over
  every 1024-start run's first key, then inside the run), about 46 % of a full
  mark on a live JSON tree. The census now records one object-start bitmap per
  block it walks (one bit per 8-byte alignment unit, allocated in 8 KiB chunks;
  oversized blocks keep a sorted start list), so a traced pointer field costs a
  search over the block fences and one bit test. Interior-pointer lookups scan
  the bitmap backwards. The bitmaps are 1/64 of the walked bytes instead of
  8 bytes per censused object. A single contiguous bitmap vector grew into the
  allocator's large-page class and cost `records_array_1m:sparse` +4.8 MiB
  peak RSS for a ~100 KB index; the chunked storage reads 68 MiB, as before.
- **No remembered-set rebuild when nothing young is live.** When the census
  shows no marked or pinned young object after the mark and the malloc
  registry is empty, the old-to-young rebuild can only produce an empty set,
  so the full installs one without walking the old generation.
  `PERRY_GC_DIAG=1` prints `[gc-remembered-rebuild] full
  skipped=young_generation_unmarked`.
- **Sweep page accounting once per page.** Consecutive single-page old objects
  are summed and applied to their page metadata once, always before a
  page-index flush that could zero that page's accounting, instead of one
  overlap vector allocation and one hash lookup per object.

On a loop that keeps one parsed `records_array_20m.json` tree in the old
generation and calls `gc()`, a full drops from 82–83 ms to 37–39 ms: mark
43 → 25 ms, rebuild 21 → 0 ms, sweep 13 → 7 ms, census 5 ms unchanged; peak
footprint 129 → 119 MiB. The 22-row JSON matrix stays within ±2 % CPU with no
row worse on RSS, and the gc-ratchet gated counters are identical.

Retrying the promoted-cohort pacing bound with these costs still does not
meet the #10182 bar: at `k=1, floor 16 MB` the 20 MB parse/scan/sparse rows
reach 179 MiB (below Node's 220) but cost 266–275 ms of CPU against a 208 ms
best, and `records_array_8m:scan` has 13.7 ms of CPU headroom for 8
iterations, less than one ~30 ms full over its tree. No pacing change is included.
