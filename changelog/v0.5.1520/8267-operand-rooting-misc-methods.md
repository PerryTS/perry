**GC: root the operands of three native-method lowerings.**

`expr/misc_methods.rs` lowered the operands of `ProcessOn`, `ProcessOnce` and
`ObjectDefineProperty` with sequential `lower_expr` calls and no rooting — the
first operand was live across the lowering of the second and third, either of
which can collect. All three now go through `with_operands_rooted`.

This is not observable as a wrong answer unless a collection lands in the
window, which is why it survived: an unrooted register only goes bad when a GC
lands inside its live range.

Extracted from #8260 by @jdalton, whose two other hunks are deliberately not
taken — see that PR for the reasoning (box pointers are outside the GC heap, so
binding a shadow slot for them roots nothing and would restore the live-set
population #8143 removed to fix #8132; and rooting closure captures across
`js_closure_alloc` regresses
`shadow_slot_hygiene::closure_body_write_to_captured_outer_local_is_visible_to_shadow_analysis`).
