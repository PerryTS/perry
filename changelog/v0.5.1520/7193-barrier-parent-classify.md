### perf(gc): stop the runtime write barrier re-deriving a parent address it was handed

`classify_heap_generation` is **19.03% of `benchmarks/app-patterns/kernels/batch.ts`**
(657M instructions, #7170's ranked profile) — the single largest symbol in that
program, called from the write barrier, **with zero collections running**. Four
page-map classifications happen per barriered element store on that path, and
one of them answers a question the caller had already answered.

**What was redundant.** `runtime_write_barrier_slot` receives `parent_addr: usize`
— a decoded GC user pointer — and handed it to `js_write_barrier_slot` as a bare
`u64`, so `decode_heap_addr` fell into its "possible raw pointer" arm and paid a
full `classify_heap_generation` to recover it, immediately before
`barrier_parent_needs_remembering` classified the same address again. The three
runtime entry points now share `write_barrier_slot_decoded`, which keeps the
parent as a `usize` throughout. Outcome-preserving: a malloc-GC parent used to
exit at `NonPointerParentSkips` (classification returned `Unknown`, so the decode
returned 0) and now exits at `ParentNotOldSkips` — different counter, same
remembered-set effect. `runtime_write_barrier_slot_matches_nanboxed_entry_point`
pins that across every parent generation x child kind.

**A latent deref the round-trip was hiding.** `malloc_gc_parent_addr`
dereferences `parent_addr - GC_HEADER_SIZE` behind a bare
`< GC_HEADER_SIZE + 0x1000` floor, which admits every handle-band id and every
out-of-range garbage word. It was safe only because its callers happened to
filter first — including *by accident*: `runtime_write_barrier_external_slot`
NaN-boxed its parent, and an address with high bits set ORs into something that
is no longer `POINTER_TAG`, so the decode rejected it.
`closure/dynamic_props.rs` parks props under exactly such non-address owner keys
and depended on that accident. Both filters are now one explicit predicate,
`barrier_parent_addr_is_dereferenceable` (the canonical
`addr_class::is_plausible_heap_addr`, plus the 8-alignment `addr_class` does not
cover), applied in `write_barrier_slot_decoded` *and* inside
`malloc_gc_parent_addr` itself — so the function that dereferences carries its
own guard rather than trusting callers.

**And the page-generation map's hasher.** `PAGE_GENERATIONS` carried a bespoke
identity hasher whose `write_usize` stored the key verbatim. `HashMap` is
hashbrown: the bucket index comes from the hash's low bits, but the SIMD control
byte is `hash >> 57`. Keys are `addr >> GENERATION_CLASS_SHIFT` — around 2^26 —
so **the control byte was zero for every entry in the table**, every group probe
matched every occupied slot, and each match cost a real key comparison before
the right one was found, on a lookup the write barrier performs several times
per heap store. Measured by the new regression test with the old hasher
reinstated: **1 distinct control byte across 64 consecutive buckets.** The map
now uses `fast_hash::PtrHasher`, the project's existing answer to this exact
failure (see its module doc and the 455 ms -> 830 ms regression its `mix` step
records). The map is only ever point-queried, so iteration order is not
observable.

Design note for the remaining three classifications — lazy barrier arming, and
why "nothing has collected yet" is *not* sufficient on its own (born-old
allocation creates old->young edges before any collector runs) — is on #7187.

Refs #7187, #7170, #5094.
