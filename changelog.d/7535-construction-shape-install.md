**A repeat construction of an already-installed typed shape is now two header
bit-writes instead of a `SHAPE_LAYOUTS` round-trip (#7510 item 1, the
construction half of #5094).**

Since #6893 the descriptor `js_gc_init_typed_shape_layout` installs is
per-*shape*, not per-object: every same-shape object shares one canonical
`TypedLayoutDescriptor` in `SHAPE_LAYOUTS`, keyed by the shared `keys_array`.
The only per-object work left is two header bits —
`GC_OBJ_TYPED_LAYOUT_INTACT`, and `GC_LAYOUT_POINTER_FREE`/`GC_LAYOUT_SIDE_MASK`.

The call did not know that. For the 20-millionth `{v, w}` literal it still built
a `TypedLayoutDescriptor` (72 bytes, two 32-byte `Vec`-carrying enums, one
cloned, all of it dropped on the way out), took a `RefCell` borrow on the
thread-local `SHAPE_LAYOUTS`, hashed the `keys_array` pointer, and compared the
fresh descriptor field-by-field against the one already stored — to reach the
conclusion the *first* construction had already reached. `shape_install_shared`'s
hit arm writes nothing at all. On a symbolicated `churn_alloc` profile the two
functions were 8.9% + 2.2% of self time.

The new `gc/shape_install` module memoises exactly that map answer, in a
thread-local 8-entry direct-mapped table, and nothing else:

> `SHAPE_LAYOUTS[keys]` holds `Some(D)`, where `D` is the descriptor that
> `(slot_count, raw_words, pointer_words)` describes.

**The memo decides nothing about the object.** Everything the header declaration
rests on is still re-derived per construction, ahead of the memo: the
`field_count == slot_count` check, raw-f64/pointer mask disjointness, and the
per-slot validation that each raw-f64 slot holds a plain double and no
pointer-bearing slot sits outside the pointer mask. `POINTER_FREE` vs
`SIDE_MASK` is recomputed from the pointer mask, never read back from the entry.
That split is the soundness bar: a wrong `POINTER_FREE` is a use-after-free
factory — `heap_payload_slot_selection` short-circuits on it and skips the whole
payload without consulting any mask — so that decision must not depend on a
cache. A stale entry can only cost work.

Those two checks also stopped building a `LayoutSlotMask` per construction and
now read the caller's mask words directly, which takes the drop glue off six
early-return paths and the `Vec` allocation off every construction of a shape
wider than 64 slots. `LayoutSlotMask::intersects` survives as the test-only
reference implementation the three word helpers are pinned against.

**Self-healing.** An entry is falsified by exactly one transition:
`SHAPE_LAYOUTS[keys]` ceasing to be `Some(D)`. `shape_install_shared` is that
map's only writer and its only such transition is the ambiguity poison (two live
layouts sharing one key set), which now drops the table. Entries are never
removed from `SHAPE_LAYOUTS` and never overwritten with a different `Some`, so
there is no other way to go stale. A relocated or recycled `keys_array` degrades
to a miss, and `PERRY_SHAPE_LAYOUT_KEYED=0` leaves the table permanently empty
because the only writer is a successful shared install. The table is **not** a
GC root: `keys` is compared as an integer and never dereferenced.

**Testing.** Nine tests. `memo_fires_on_every_repeat_construction_of_one_shape`
counts hits — the assertion #7525 lacked, and whose absence let that PR's first
commit ship a fast path that fired once in 40 million calls.
`memo_installed_objects_survive_a_copying_minor_with_their_children` is the GC
witness: six instances of a `{ n: number; s: string }` shape, five of them
published by the memo (asserted, so a green run cannot mean the slow path ran),
through an evacuating minor that actually moved ≥ 12 objects, with every string
child relocated, re-pointed and byte-intact.
`a_pointer_free_declaration_on_this_shape_strands_the_child` is the permanent
sabotage arm: publishing this exact shape `POINTER_FREE` by hand makes the
collector enumerate zero payload children. Three hand-applied sabotages — the
fast path reading its state out of the memo, the memo short-circuiting
validation, and the poison branch not invalidating — each fail a different named
assertion.

**#7512's residual is not fixed here, and the reason is sharper than the ticket
had it.** `js_gc_init_typed_shape_layout` is still emitted after the constructor
call, so raw-f64 class-field stores inside a constructor body still cannot pass
their guard. Moving it earlier fails validation because fresh slots hold
`undefined` — but *relaxing* validation to accept `undefined` in a raw-f64 slot
would be safe for the collector (`undefined` is not pointer-bearing, so a
skipped slot strands nothing) and unsafe for readers: `class_field_fast_contract`
documents that the codegen-inlined path concludes "slot K is raw-f64" from the
intact bit alone, and `class C { v: number; constructor() { console.log(this.v) } }`
must still print `undefined`. The fix needs a codegen-side definite-assignment
proof or a two-stage install, not a runtime relaxation; it stays on #7510.
