**fix(codegen): typed-array element reads are not GC temporaries (#6996)**

`compiler-output-regression` — a required check — had been red on `main` since
`760db2fd8` (#6972, "argument temporaries are precise roots"): the
`native-abi-proof` workload `native_abi_packet_control` failed
`hot_loops_no_runtime_calls` on one call, `js_gc_temp_root_push`, inside its
hot loop.

**Bisected, not guessed.** Over the 28-commit window `95960e0df..760db2fd8`,
with the compiler rebuilt from source at each endpoint: `95960e0df` pass,
`1a533a3a8` (#6972's parent) pass, `760db2fd8` fail, current `main` fail. The
after-opt IR delta between the last two is exactly that one call.

**Root cause.** The fixture's kernel is `(buf[i] + packet.tag + i) & 255`.
`packet` is `any`, so the add lowers through `js_dynamic_string_or_number_add`
and #6972 roots the operand pair across the property get — correct in general,
because a heap operand held only in an SSA register while its sibling allocates
is precisely the #6951 use-after-free. But the left operand is a *byte*. #6972
anticipated this and gated emission on
`expr_is_known_non_pointer_shadow_value`; that predicate had no arm for
typed-array / Buffer element reads, so the workload's hottest loop
(4096 × 64 iterations) paid a push + re-read + truncate per iteration to root a
value that can never be collected.

**Fix.** The contract is unchanged — nothing was relaxed. The predicate learns
the element-read family, and the proof is about the LOWERING, not the declared
type (annotations are unenforced, so `buf: Buffer` holding something else must
not be load-bearing — and it isn't: `js_uint8array_index_get_value` and
`js_buffer_index_get_value` answer `undefined` for a receiver that is not a
Uint8Array/Buffer, and `lower_buffer_load`'s inline arm reads a raw byte). The
three lowerings of `Uint8ArrayGet` that CAN yield a heap value stay rooted and
have tests holding them from the other side: a symbol key
(`js_object_get_symbol_property` returns a prototype accessor), and — in
JS-value and in i32 context respectively — an unproven key
(`js_typed_array_index_get_dynamic`) or a key that is not numeric-proven
(`js_object_get_index_polymorphic`), both of which fall through to string-keyed
property lookup. Those last two are gated on different predicates in the
lowering, so the skip tests both. `BufferIndexGet` has none of these paths.

Since that argument depends on the gate testing the *same* index proof that
routes the read to a byte accessor, the verbatim copy of
`numeric_index_has_integer_array_index_proof` in `expr::arrays_finds` is folded
into the `expr::index_get` one — identical arms and thresholds, but two copies
that could drift would make the gate unsound the moment one was edited.

**Cost to #6972: none of its protection.** In this fixture the emitted push
count goes 5 → 4; the `console.log` accumulator pair and the two
`js_dynamic_string_or_number_add` results in the return expression (which
really can be strings) all stay rooted. The hot loop returns to its pre-#6972
IR.

Verified with the full harness (compile + link + run): `native-abi-proof` and
`native-region-proof` both `"status": "pass"`, both harness unittest modules
OK, four new cases in `crates/perry-codegen/tests/temp_root_argument_temporaries.rs`
(the two "not rooted" cases fail without the change).
