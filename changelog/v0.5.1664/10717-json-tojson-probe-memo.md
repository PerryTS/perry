Memoise the two shape-keyed questions `JSON.stringify`'s `toJSON` probe was
re-deriving per object visited. Callgrind, N=20,000 minus N=2,000 over 18,000:
2,256.5 -> 1,472.3 instructions per object visited (-34.8 %), with the per-property
cost unchanged.

* `class_chain_may_have_to_json(class_id)` (448 Ir, five by-name/by-id lookups
  across five registries) is memoised per `class_id` in a 64-slot thread-local
  table keyed on three generations: `VTABLE_GEN`, the semantic property epoch,
  and a new `CLASS_LOOKUP_SURFACE_GEN`. The uncached walk is kept and is what
  fills the memo, so the two cannot disagree by construction.

* `CLASS_LOOKUP_SURFACE_GEN` covers six routes that flip the verdict
  false->true with no movement of either existing counter — all of them "a
  lookup surface appeared". It is bumped inside
  `class_prototype_object_root_store`, `class_decl_prototype_object_root_store`,
  `js_register_class_generic_origin` and the `CLASS_DELETED_KEYS` un-mark, after
  the store, so a new call site cannot forget it. `VTABLE_GEN` cannot be used
  for these: `class_decl_prototype_value` records (#7769) that bumping it there
  measured 384,000 of 384,000 shape-guard probes failing. GC is deliberately not
  an input — keying a per-object cache on a GC-bumped counter is a measured
  +35 % cliff (#7910).

* The `Object.prototype` verdict keeps its six-field signature and is still
  recorded only under the fully validated builder, but the per-call
  revalidation drops the two `try_read_tracked_gc_header` arena-range
  ownership proofs. The moving-GC defence is preserved as a comparison:
  `proto_addr` is re-read from `CACHED_OBJECT_PROTO_BITS`, a GC mutable root,
  and compared before anything is dereferenced, so a relocation is a miss
  rather than a stale hit. Nothing is dereferenced out of the cache.

* Both header reads in that revalidation go through
  `addr_class::try_read_gc_header` rather than a bare
  `(addr - GC_HEADER_SIZE) as *const GcHeader` cast. The ordering argument
  establishes that the addresses are the *validated* ones; it does not
  establish that they are addresses at all if the root is corrupted or zeroed,
  and the first thing the old code did with the header was test
  `GC_FLAG_FORWARDED` — a check that cannot protect the load feeding it. The
  allocator-ownership proof is still skipped, which is where the saving comes
  from; only the magnitude classification is restored, at a measured cost of
  10 instructions on the executed fast path (49 -> 59 at `-C opt-level=3` for
  `aarch64-apple-darwin`, 0.7 % of the memoised probe's 1,472 Ir/object).
  `try_read_gc_header_known_plausible` was rejected: `is_small_buf_slab_addr`
  has been a constant `false` since the 2026-07-09 slab audit, so that
  spelling compiles to code byte-identical to the bare cast — LLVM folds the
  two into one symbol — and would have cleared
  `scripts/addr_class_inventory.py` while checking nothing. A sabotage test
  (`object_proto_signature_fast_match_declines_a_handle_band_root`) pins the
  guard as live by arranging every other field of the signature to match.

* The three top-level stringify entries no longer force-invalidate the
  `Object.prototype` verdict. That made the first probe of every call a
  guaranteed recompute and was redundant: every route into
  `Object.prototype.toJSON` moves the semantic epoch or the keys array. The
  invalidations after user callbacks are unchanged.

Every invalidation point is sabotage-proved: removing the bump in
`class_prototype_object_root_store`, in `class_decl_prototype_object_root_store`,
in `js_register_class_generic_origin`, or reverting the memo's slot index each
fails exactly one named test.
