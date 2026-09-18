### Performance

### Performance

- **A moving collection no longer re-derives an object's layout header (#10362).** `layout_transfer`
  runs for every evacuated object on every copying minor, both old-generation evacuations and
  `js_array_grow`. All four callers copy the source header's `_reserved` into the destination
  first, so the layout state, `GC_LAYOUT_ALL_POINTERS`, the raw-f64 / holes flags, the
  element-shape bit and `GC_OBJ_TYPED_LAYOUT_INTACT` have already arrived — yet the funnel rewrote
  those bits anyway, classified both headers, evaluated #7510's flag-and-filter gate twice, and
  for every intact object re-resolved the intact bit through a ShapeId-keyed `SHAPE_LAYOUTS`
  probe whose answer a relocation cannot change. Measured by single-stepping the #10362
  retained-graph fixture: 160 instructions per moved array and 245 per moved object, 518M in all
  (4.2% of the run), none of which reached a side-table record.
  That contract is now stated and asserted, and the funnel moves only what a header cannot carry:
  the element-shape record (#7480), the residual static-prototype registry (#9304) and the
  per-object `TYPED_LAYOUTS` / `LAYOUT_SLOT_MASKS` entries (#7510) — each behind the bit or latch
  that governs it, with the record moves in a `#[cold]` path entered on 0.05% of relocations.
  The one behavioural change is that the lazy intact downgrade is gone: the bit is a fact of the
  object and of tables a move does not touch, the state it cleared is legal and handled
  (`shape_install_shared` leaves still-INTACT siblings to fall back, #8115 clears at the first
  contradicting store, the trace falls back to scanning every slot), and an unmoved sibling keeps
  its bit today. A new test builds a poisoned-shape intact receiver, moves it through a real
  copying minor, and checks the bit, every query answer and the survival and rewrite of its child;
  it fails on the parent commit and under a funnel that skips the per-object record move.
  instructions:u, min of 5: gc3 12.276G -> 11.881G (-3.22%), retained-set variants -2.50% to
  -4.88%, old->young churn -2.13%, allocation-only unchanged. 2,560,042 relocations on both arms.

