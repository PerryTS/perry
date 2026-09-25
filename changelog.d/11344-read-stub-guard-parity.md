Closed the two `read_stub` gaps from #10768 structurally rather than by
enumerating paths.

- **Probe and prime share one receiver guard.** The megamorphic read stub's
  prime path did not check `GC_FLAG_FORWARDED`, although its probe did. Both
  arms now go through `read_stub::stub_receiver_token` (heap-object type, not
  forwarded, no blocking flags, a real class id, a live shape). The table
  accessors are private, so every insert and lookup runs it: the by-name
  lane, typed feedback's content-bits read, and the stable-tombstone re-add
  in `js_put_value_set_dyn_ic_miss`, which used to insert raw. The gap has
  been harmless only because of layout: the forwarding word overwrites
  `class_id` and the ShapeId word, and on a 64-bit host an address's upper
  half never reaches `SHAPE_ID_BASE`, so the stamp read as absent.
- **A stable-tombstone ShapeId pins its inline/overflow boundary.** The read
  and write stubs record each slot's inline-or-overflow verdict at prime time
  and re-prove it only through the shape token.
  `try_update_stable_tombstone_shape{,_cached}` are the only writers that
  change `live_inline_slot_count` under an already-stamped id, and
  `publish_object_shape_from` routes every live-count change on such a
  receiver through them. They now refuse any update that moves
  `max(live, INLINE_SLOT_FLOOR)`, and the caller mints a successor. Before
  this, `publish_object_live_slot_count` lowering a stable receiver's bound
  kept the same id. #9064's re-add into an unused inline slot below the floor
  moves no slot and still keeps its id.

Pinned by `object/read_stub_tests.rs` (forwarded refusal plus a five-condition
probe/prime symmetry table, each with a positive control) and three
`tombstone_tests` cases. With either fix reverted, four of them fail.
