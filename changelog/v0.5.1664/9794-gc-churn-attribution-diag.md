### Runtime

- `PERRY_GC_DIAG=1` now says WHY the collector ran, not only what it did:
  `[gc-trigger]` prints every predicate input at each collection decision
  (armed arena trigger vs `arena_total`, from-space vs the nursery cap,
  old-gen reclaimable pressure vs baseline/band, the malloc pair, the
  pending/retaining flags); `[gc-full]` names the arm behind every full
  mark-sweep with a per-site count; `[gc-budgeted] start/done` reports each
  incremental cycle's steps, per-phase step time and root-scan share;
  `[gc-charge]` attributes mutator-assist and synchronous-full time to the
  calling site (return-address chain resolved to the JS display name);
  `[gc-survival]` gives, per copying minor, which root first reached each
  surviving byte — shadow stack, native stack map, a named side-table
  scanner, or the remembered set split by the old parent's type — with
  transitive reach charged to the originating root.
- `PERRY_ALLOC_SITE_SAMPLE=<bytes>` (arena/alloc_sample.rs): byte-proportional
  allocation-site sampling for the GC arena, covering the runtime allocators
  and the codegen inline bump path (the mirrored inline block limit is capped
  at one interval while sampling). `[alloc-site]` reports bytes by object type
  and the top sites after each copying minor and at exit. Off by default; one
  relaxed atomic load per allocation when off; the OFF state and the magnitude
  parse are pinned in `gc/tests/env_knob_parse.rs`.
