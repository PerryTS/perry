**A repeat construction of an already-installed typed shape is now two header
bit-writes instead of a `SHAPE_LAYOUTS` round-trip (#7510 item 1, the
construction half of #5094).** On the pinned quiet host, interleaved best-of-9
user CPU over 20M constructions: **1.09× on a `{v, w}` object-literal churn,
1.10× on the `new Node(v, w)` class form, 1.06× on a 16-shape polymorphic
churn**. `shape_install_shared` is entered **once** in a 20,000,000-object run,
against 100,000 times in 100,000 on `main` (lldb breakpoint hit counts).

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
thread-local 32-entry direct-mapped table (1 KiB), and nothing else:

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

**32 entries, not 8, and that was measured rather than guessed.** A
direct-mapped table cycled round-robin by more shapes than it has slots hits
*zero* percent of the time — each entry is evicted by its partner before it is
read again — so it pays the probe and gets nothing. A 16-shape churn loop
against 8 slots was a reproducible 0.993×, the one regression this change had;
at 32 slots the same loop fits and turns into 1.059×, with the monomorphic
numbers unchanged. The slot index also mixes in the two mask-global addresses,
not just the shape, because two object-literal sites with the same key names
share one `keys_array` but get separate mask globals unless LLVM's constant
merger folds them.

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

**The constructor-ordering residual is not fixed here, and the reason is sharper
than the ticket had it.** It originated on #7512, whose other half merged as
#7515; #7510 then folded the remainder in as its construction-path item, which
is why it is tracked there rather than on #7512 (still open for the broader
class-vs-object-literal gap).

`js_gc_init_typed_shape_layout` is still emitted after the constructor call, so
raw-f64 class-field stores inside a constructor body still cannot pass their
guard. Moving it earlier fails validation because fresh slots hold
`undefined` — but *relaxing* validation to accept `undefined` in a raw-f64 slot
would be safe for the collector (`undefined` is not pointer-bearing, so a
skipped slot strands nothing) and unsafe for readers: `class_field_fast_contract`
documents that the codegen-inlined path concludes "slot K is raw-f64" from the
intact bit alone, and `class C { v: number; constructor() { console.log(this.v) } }`
must still print `undefined`. The fix needs a codegen-side definite-assignment
proof or a two-stage install, not a runtime relaxation; it stays on #7510.
