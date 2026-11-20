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

**Why an erased annotation cannot reach this guard (#7831/#7837 collision).**
A `number[]` really can hold heap strings at runtime, so it matters exactly
which values arrive at the live-bits test. Two independent predicates decide,
and only the second is load-bearing here. `is_numeric_expr` DOES admit a read
off a `number[]` (#7810), so an annotation alone would put a heap string on a
numeric push path — but `expr_produces_canonical_raw_f64` excludes every READ
("cold fallbacks return boxed bits"), which keeps `keep_guarded_numeric_push`
true and routes those pushes to the pre-existing RUNTIME numeric tier
(`js_array_numeric_push_f64_unboxed` behind its feedback guard), never to this
inline guard. The inline guard is reached only for values that are canonical
raw f64 BY CONSTRUCTION — a machine FP op, which cannot produce a pointer
except by ARM NaN-payload propagation from a NaN-boxed operand, and that is
precisely the case the live-bits test catches.

Verified rather than asserted: `gc-handoff/m0810/numarr_lie.ts` and four more
declared-type-lie shapes (a `number` parameter, a `number` object field, a
module-level `number` global, an element read off a `number[]`) emit **zero**
guard blocks. `a_declared_type_lie_is_routed_to_the_runtime_tier_not_the_guard`
pins that routing, and `the_guard_branches_on_the_live_bits_not_on_a_constant`
pins the guard's condition to a computed register. Both are sabotage-verified:
hard-wiring the branch to `false` fails the second, and widening
`expr_produces_canonical_raw_f64` to admit a read fails the first.
