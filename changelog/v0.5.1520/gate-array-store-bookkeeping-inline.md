An in-bounds array element store no longer calls into the runtime twice per
element to be told there is nothing to do.

`gc::layout::layout_note_slot_aware` opens with
`if !value_is_pointer && !old_is_pointer { return; }`, and
`js_array_note_numeric_write` returns as soon as the receiver's raw-f64 layout
bits are clear. Both are cheap tests on values the call site already holds — and
both were emitted unconditionally by `lower_index_set_fast`, so a `boolean[]`
store loop paid two calls per element for the privilege of being declined. A
profile of such a loop put ~82% of its time in per-store bookkeeping and 16% in
the loop itself (#9237).

Both early returns now happen inline, under two **separate** gates, because they
answer different questions:

* the layout note, the string addref and the write barrier are dead unless a
  pointer is involved, so they sit behind `may_carry_heap_pointer(new) ||
  may_carry_heap_pointer(old)`. The test is `new || old` and not `new` alone:
  overwriting a pointer with a boolean is a pointer→scalar transition the runtime
  must still see, which is why the existing new-value-only gate
  (`emit_jsvalue_slot_store_pointer_tested`, for class fields under a conforming
  layout) is not usable here;
* the numeric-write note is what *downgrades* a raw-f64 array on its first
  non-numeric store, so gating it on pointer-ness would skip it forever and the
  array would never downgrade. It keeps its own raw-f64-bits gate — the test
  `expr/index_set_guarded.rs` already applies on the sibling path.

Measured on an idle Mac mini, both binaries built in one run, interleaved, min of
five, self-timed: a boolean-store loop 206 → 137 ms (−33%), and
`benchmarks/suite/11_prime_sieve.ts` 26 → 20 ms (−23%, 4.3× → 3.3× Node). A
nested-loop read benchmark is unchanged, as expected. Node remains 12 ms and 6 ms
respectively — this closes part of the gap, not all of it; the per-store guard
**call** is the larger remaining piece and is still open in #9237.

`typed_shape_descriptors`'s `pointer_store_into_numeric_array_keeps_layout_note_and_barrier`
needed updating and is the reason to trust the change: it pins that a pointer
store into a statically numeric array still reaches both the layout note and the
barrier. Both assertions now follow the EDGE through the new gate blocks —
exactly the treatment #7715 gave the barrier assertion when it moved the barrier
behind a live value test. Verified independently that the chain
`idxset.inbounds → gc_bookkeeping → numnote → barrier.maybe → barrier` is intact
for a pointer store, and that such a store still answers identically to Node.

Also removes `emit_jsvalue_slot_store_scalar_aware_with_flags_on_block`, added in
#9195 and left with no callers by this change.

Five differentials stay byte-identical to Node: the string-aliasing hazard the
addref exists to prevent, the interval bounds proof, the byte-read battery, the
nested-loop exits, and the packed-loop carry battery.
