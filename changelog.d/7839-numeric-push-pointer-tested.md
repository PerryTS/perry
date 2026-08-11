### Fixed / Performance

**`arr.push(<number>)` no longer pays three GC-bookkeeping obligations per element.**

The inline array-append tier (`apush.inbounds`) emitted, on *every* element:
`js_string_addref_if_heap_string`, `js_gc_note_slot_layout`, and a seq_cst load
of `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` to gate `js_write_barrier_slot`.
On `gc-handoff/bench/push_num.ts` — 20,000,000 pushes of a double into a
`number[]` — all three are dead on all 20M of them.

The static proof that retires them (`array_store_needs_layout_note` →
`expr_produces_non_pointer_bits_by_construction`) cannot be made for the shape
that matters. `keep.push(base + j)` is an `Expr::Binary { Add }`, and that arm
answers `false` unconditionally, because `+` is string concatenation for
non-numeric operands. It fires only for a bare canonical-i32 local, which is why
`keep.push(j)` compiles to a materially different loop than `keep.push(base + j)`
does.

This is #7511's answer to the identical problem on class-field stores, applied to
the array append: ask the question ONCE inline, on the live bits, and branch over
all three calls. `emit_may_carry_heap_pointer_check` — already the codegen mirror
of `layout_pointer_bearing_bits` and `decode_heap_addr`, already contract-tested
over the whole 16-bit tag space — is the predicate. The store itself stays
unconditional and outside the branch; only the bookkeeping moves.

The array's own half of the proof rides the header test the `nofwd` block already
performs: the integrity mask widens from `0x0407` to `0x0407 | 0x3800` for a
numeric push, so reaching the inline store additionally proves
`GC_ARRAY_ELEMENT_SHAPE`, `GC_OBJ_TYPED_LAYOUT_INTACT` and
`GC_LAYOUT_ALL_POINTERS` all clear — the three states in which
`js_gc_note_slot_layout` does real work for a non-pointer value. An array in any
of them takes `js_array_push_f64`, which notes the slot exactly as before. That
costs those arrays the inline store and can never cost correctness.

**This is a guard, not an elision.** Perry does not validate declared types, so a
`number`-annotated value that is a heap string at runtime takes the guarded arm
and records the slot exactly as it always did. `the_guarded_arm_still_reaches_
every_call_it_moved` asserts the calls are still emitted, precisely so a future
"simplification" to an outright elision fails here rather than as heap corruption.

Gated on `is_numeric_expr`, so a pointer-pushing loop (`churn`, `tree`,
`push_cls`) emits byte-identical IR and pays nothing for a test it would always
fail.
