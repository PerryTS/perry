### Repsel: per-array homogeneous element-shape invariant (#7480)

`keep[j].v` measures **6.2× vs node** on the pure shape because a shape proof
does not survive an array element read: the element-read tier and the
field-read precheck stack two inline guard diamonds per access. #7480's
scoping found that its two candidate routes — extending the #5093
versioned-loop clone, and full element `Ptr<Shape>` representation — are not
alternatives: both need the same missing fact first. This lands that fact,
and only that fact. **No consumer, no emitted-code change** (the diff touches
`crates/perry-runtime` only), because #6377's lesson is that every added
proof un-gates latent fast paths its own microbench never exercises.

The invariant is "every element of this array is an object of class `C`".
Its storage deliberately mirrors Phase 4a's dense bit, which works because
the collector copies the whole `_reserved` word on a move: a
`GcHeader._reserved` bit (11, disjoint-by-`obj_type` with the object-only
`OBJ_FLAG_HAS_DESCRIPTORS`, the same reuse `GC_ARRAY_RAW_F64_HOLES` already
makes) rides relocation for free, and the shape id — which does not fit in a
bit — lives in an address-keyed record moved by `transfer_element_shape` from
inside `layout_transfer`, the same call site and the same split as
`TYPED_LAYOUTS`. The bit stays the authority: a fresh allocation's
`_reserved` is zero, so a stale record at a recycled address is unreachable,
and a bit with no record fails closed. The record is four integers and holds
no heap pointer, so it is deliberately not a `gc_register_mutable_root_scanner`
entry; dead keys are dropped on the hook that already prunes
`ARRAY_NAMED_PROPS`.

Maintenance hangs off one funnel, `gc::layout_note_slot` — the only place
both the runtime's element-store helpers and codegen's *inline* element
stores already meet, since `array_store_needs_layout_note` elides the note
only for arrays statically proven numeric and pointer-free, which an
element-shape array can never be. It sits ahead of that function's
`GC_LAYOUT_UNKNOWN` early return and costs a `GC_TYPE_ARRAY` compare plus a
bit test on a header word the next line reads anyway; an array without the
bit leaves on one predictable-not-taken branch.

The record also pins the `length` it was verified against, which is what
keeps the invalidation matrix small enough to enumerate: `pop`, `shift`,
`splice`, `length = n`, sparse extend and codegen's inline append all fail
the proof automatically, with no call site of their own. Explicitly wired:
mismatched or non-pointer stores and `delete arr[i]` clear through the store
hook; `shift`/`unshift`/`splice`/`fill`/`copyWithin`/`reverse`/`sort` clear
through `rebuild_array_layout`, the post-hoc funnel they all already use;
`set_array_numeric_layout` clears (the numeric and element-shape invariants
are mutually exclusive); and prototype surgery bumps a global generation
counter from `invalidate_class_prototype_fast_guards` and from
`object_set_static_prototype_impl`'s `instance_override` arm, retiring every
outstanding record at O(1) without enumerating arrays. Arrays built outside
the funnels (inline literals, `JSON.parse`, `map`) re-earn the proof through
`ensure_element_shape`, a rescan that mirrors `ensure_array_numeric_raw_f64`.

Two counters, both `AtomicU64` starting at 1 like `PROP_PLAN_EPOCH`:
`js_array_element_shape_epoch()` advances on every clear anywhere and is the
one-load word a hoisted guard re-reads without rescanning, while the
per-array `epoch` in the record distinguishes "still the same proof" from
"re-established after a break". `js_array_element_shape_check` pins both.

26 new tests. 24 cover the matrix row by row in
`array/element_shape_tests.rs`; 2 in `gc/tests/layout_trace/element_shape.rs`
put a proven array through a real copying minor and assert the subject was
live first — the collector actually copied, and the array actually moved —
so a run with zero copying minors cannot pass. One of them pushes *after* the
move and asserts the proof both extends on a match and retires on a
mismatch, which is what proves the record is reachable at the new key rather
than merely readable once.
