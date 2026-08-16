### Fixed

- **Corrected the `array_receiver_gc_tag` doc comment that caused a recurring
  silent-drop bug family.** The comment stated flatly that `Buffer` and
  `TypedArray` payloads are `std::alloc`-backed with no `GcHeader`. That is
  true for only one of the two backings these receivers actually have, and
  reading it as universal is how the same defect kept being reintroduced —
  five separate PRs (#8090, #8109, #8119, #8120, #8130) each fixed one
  instance of a receiver being nulled or misread at an array funnel, and the
  sweep in `gc-handoff/ARRAY-SWEEP-NOTES.md` found more (#8137, #8138).

  Both backings exist and reach these funnels:

  - **arena-backed** — `buffer/header.rs`'s `arena_alloc_gc_old(…,
    GC_TYPE_BUFFER)` and `typedarray/mod.rs`'s `GC_TYPE_TYPED_ARRAY` site.
    These carry a genuine `GcHeader` with a correct `obj_type`. They are
    *pinned*, which is a different property from being *untracked* — the
    conflation is the root of the confusion. This is the population #8041
    began nulling.
  - **external** — `EXTERNAL_BUFFER_REGISTRY` / `EXTERNAL_UINT8ARRAY_REGISTRY`
    addresses and `shared_sab::alloc_shared_sab`'s `alloc_zeroed`. For these
    the eight bytes below the payload really are allocator bookkeeping.

  The tag is therefore authoritative only once the address is known to be
  arena-backed, which `typedarray::arena_payload_has_gc_type` already does
  properly (range check, `HeapSpace::Unknown` rejection against the *header*
  address, `gc_type_info` validation). The comment now says so and points at
  it, so the next reader does not open-code a floor instead.

  Comment-only; no behaviour change.
