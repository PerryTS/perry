**A class's typed-shape layout is now declared at the allocation site instead
of validated after the constructor, so the constructor's own field stores can
see it (#7510 item 1, closing the residual half of #7512).**

`js_gc_init_typed_shape_layout` was emitted *after* the constructor call. The
consequence was that no raw-f64 class-field store **inside** a constructor could
pass its `GC_OBJ_TYPED_LAYOUT_INTACT` guard — neither the inline
`and i16 %r, 4096` test nor `class_field_fast_contract`'s
`layout_typed_raw_f64_slot_for_user` — so every one fell back to
`js_put_value_set`. Stated plainly, because it inverts the usual assumption:

> **Declaring the fields `number` was what made the class slower.** More type
> information selected a representation whose guard the construction path had
> made unsatisfiable.

The one-line reorder does not work, and that is the whole design problem.
`init_typed_shape_layout` validates that each raw-f64 slot already holds a plain
double; a fresh slot holds `TAG_UNDEFINED`, whose `0x7FFC` tag is inside
`layout_raw_f64_bits`' reject range, so an early call downgrades every instance
it touches.

A second entry point, `js_gc_declare_typed_shape_layout`, **skips that
validation** — which moves the burden of proof to codegen, where two conditions
now discharge it (`typed_shape::class_layout_declarable_at_allocation`):

1. **Every** raw-f64 field is assigned by the constructor prologue from a plain
   parameter — #7486's `ctor_prologue_param_assigned_fields`, which is non-empty
   only for a class with no heritage, no field initializers or computed keys, no
   decorators, plain parameters, and no setter shadowing an assigned field. A
   `LocalGet` of a plain parameter cannot throw, allocate, or observe `this`, so
   nothing can read a raw-f64 slot between the declaration and its first write.
   *Every*, not *some*: one field assigned later would still be exposed.
2. **The pointer mask is empty**, so the declared state is `POINTER_FREE` —
   byte-identical to what `layout_init_pointer_free` already sets on every fresh
   instance. The only delta emitted is the intact bit and the shape-shared
   descriptor install; the collector's view at birth is unchanged. Classes with
   pointer fields would install `SIDE_MASK` over the allocator's fill, which is
   sound on the pre-filling allocation path but would rest on that pre-fill, so
   they are out of scope.

**Nothing rests on the values actually being numbers.** A constructor that
stores a string into a `number`-declared field is rejected by the store guard
(`is_plain_number_bits`, and the inline path's finite-exponent test), falls back
to the boxed setter, and downgrades the descriptor through `layout_note_slot` —
the same path any post-install contradiction has always taken. There is a
witness for exactly this: 20,000 instances constructed with heap strings in a
`number` field, collected hard, all 20,000 still readable and `typeof` `string`,
under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`.

Interleaved A/B (arms alternating per round, best-of-9 user CPU):

| bench | speedup |
|---|--:|
| `push_cls` (`new Node(v, w)` ×20M) | **1.54×** |
| `push_cls_read` (same, plus reading both fields back) | **1.53×** |

The collector result is strictly better, not merely equal: `push_cls` promotion
falls **210,488 → 64 bytes**, bytes copied 0.0042 → 0.0036 GB, peak RSS 29.6 →
24.1 MB, cycle count unchanged at 105 — i.e. the class instance now behaves
exactly like the equivalent object literal (`churn`: 0.0036 GB, 64 B, 24.2 MB),
which is #7512's anomaly closed on the memory axis too.

Every other benchmark compiles to **byte-identical generated objects** across
the two arms (`churn`, `churn_alloc`, `push_num`, `tree`, `deeplist`,
`churn_read` — compared as object bytes, since the object *cache key* hashes
codegen env and compiler identity and so always differs). Their ±3% A/B spread
is host noise, not a regression.

**Object literals do not qualify, and the reason is worth recording.** HIR
rewrites a closed-shape literal to `new __AnonShape_<hash>(…)`, whose
synthesized constructor *is* a qualifying prologue — but the minted class's
field types come out as `Any`, not `Number`. `Any` is pointer-bearing, so the
gate refuses, and there is no raw-f64 store path to unlock in the first place.
That also means `{v: number, w: number}` is currently declared to the collector
as **two pointer slots**. The literal path's remaining cost is therefore a type
propagation gap, not an ordering one — a separate lever from this ticket's.

New tests: `perry-codegen/tests/typed_shape_declared_at_allocation.rs` (the
declaration dominates the constructor call, replaces the post-constructor
install, carries a raw-f64 mask with a null pointer mask, and is refused for a
non-prologue number field, a pointer field, an untyped field, an all-boolean
class, and a class with no constructor) and
`perry-runtime/src/gc/tests/layout_trace/declared_at_allocation.rs` (the
validating install still refuses a fresh instance — executable documentation for
why the split exists — the declaring one accepts it, both still reject a
slot-count mismatch and overlapping masks, and a contradicting store both evicts
the descriptor and leaves the object conservatively scanned so its string child
is still traced).
