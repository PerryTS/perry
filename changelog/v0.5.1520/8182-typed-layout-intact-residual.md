**GC: `GC_OBJ_TYPED_LAYOUT_INTACT` no longer outlives the descriptor it claims (#8115).**
The bit is documented to mean "a canonical `TypedLayoutDescriptor` is reachable
for this object", and four readers rely on it. #7834's at-allocation bake sets it
with no descriptor behind it, and `set_layout_state` masks
`!(GC_LAYOUT_STATE_MASK | GC_LAYOUT_ALL_POINTERS)` = `!0xE000` — it cannot clear a
`0x1000` bit — so `layout_note_slot`'s generic pointer-mask branch could publish
`GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` **without** a descriptor. Three
codegen consumers read that state as a licence to skip a map:
`class_field_inline_guard` (which tests the bit and no layout state at all),
`element_shape_guard`'s packed `0x1800_80FF` header compare, and
`class_field_store_layout_note_is_conforming`, which elides the layout note
outright on `_reserved & 0xD000 == 0x9000`.

`layout_note_slot` now clears the bit at the fall-through past its descriptor
probe — the one point where the broken state is observable, reached only when
*both* maps answered `None`. A descriptor-backed object returns from the
`Some(verdict)` arm above it, so the legitimate `SIDE_MASK | INTACT` case is
untouched; pre-#7834 the descriptor existed and any contradicting store evicted
it, bit included, so this restores that outcome without paying for the descriptor.
Cost is one 16-bit store on a path a baked object only reaches for a store the
descriptor path would have downgraded on.

Acceptance: `perry-runtime/src/gc/tests/typed_layout_intact_residual.rs` (7
tests). It reaches `SIDE_MASK` without a descriptor through the real
`layout_note_slot`, from the exact header the inline `new` bakes, and then
evaluates the codegen predicate over the header the runtime produced — asserting
the state transition alone passes on an unfixed tree. Premises come from the maps
(a new `#[cfg(test)] layout_descriptor_reachable`), never from the bit under test.
Sabotage-verified: with the clear disabled, exactly the two residual tests fail at
`_reserved = 0x9000` and the four controls still pass.

Two findings recorded rather than fixed. `mark_object_dynamic_shape_unknown` is
**not** inert on baked instances as #8115 assumed — `layout_has_typed_descriptor`
answers from the same stale bit, so the guard does not early-return and
`layout_mark_unknown` heals the object; the stale claim was its own antidote, and
a test now pins that coupling. And the lost-child use-after-free the old
`docs/engine-plan.md` ⛔ posited needs the note elision, whose precondition (the
class declares a pointer slot) is mutually exclusive with the bake's (the pointer
mask is statically empty), so the reachable consequence was a wrong-value raw-f64
read, not a collector fault.

`docs/engine-plan.md` item 2 is rewritten: it forbade code that shipped four days
after it was written and is covered by an IR census with a negative control. It
now records that the bake shipped, what #8115 changed, and what the design still
rests on (`TypedShapeProof::FreshlyAllocated`).
