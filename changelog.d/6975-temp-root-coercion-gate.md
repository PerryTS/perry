**fix(gc): coercion-capable operators can collect — four rooting gaps from the #6972 review**

Follow-up to #6972 (#6951). One soundness hole in the gate predicate plus three
additional sites of the same bug class, all found by review after #6972 merged.

**The gate was unsound.** `expr/temp_root.rs` documents that
`expr_may_trigger_gc` is deliberately one-sided — `false` must mean "provably
allocates nothing" — and then answered `false` for `Compare` / `Unary` /
non-`Add` `Binary` whenever recursing into the operands found no allocation.
But `o < x`, `-o` and `o * 2` run ToPrimitive / ToNumber on their operands, and
a user-defined `Symbol.toPrimitive` / `valueOf` / `toString` is arbitrary JS: it
allocates and it collects. `a < b` over two plain `LocalGet`s recursed straight
to `false`, so `f(freshString(), a < b)` skipped rooting its first argument —
the #6951 use-after-free in a narrower case. These operators are now GC-capable
unless **every** operand is a proven inert primitive (`expr_is_inert_primitive`:
literals, plus locals the type analysis proved Number / Int32 / Boolean / Null /
Void / Never with no reserved shadow slot — a reserved slot means
pointer-possible whatever the refined type says). `Add` is never inert, since
concatenation allocates even over two literals. The predicate now takes
`&FnCtx`; `any_later_arg_may_trigger_gc` had no callers and is deleted rather
than shipped dead. Cost is unchanged: `i < n` and `x * 2` on proven-numeric
locals stay inert, and the hot-loop benchmark from #6972 still emits 12 rooting
calls.

**Three more sites.** (1) `expr/binary.rs`'s BigInt dynamic helper had a second
copy of the two-`lower_expr` shape in its `!inline_bitwise` branch that #6972's
pass missed. (2) `lower_canonical_str_self_append`: `s += rhs` must load `s`
*before* evaluating `rhs` (a `rhs` that reassigns `s` must not be observed), so
the pre-rhs value crosses both `rhs` and `js_jsvalue_to_string` — re-reading the
slot would take the wrong value, so it goes into a temp root; the coerced rhs
handle is rooted too, because the cold arm's `unbox_str_handle` materializes an
SSO destination onto the heap with that bare handle live. (3)
`lower_object_literal`'s `this_patches` queue holds method-closure values across
every remaining property's initializer and then passes them to
`js_closure_set_capture_bits` as raw pointers; they are now rooted and refreshed
before the patch loop.

Re-verified against pinned Node 26.5.0: the #6951 repro stays fixed under
`PERRY_CONSERVATIVE_STACK_SCAN=off`; the 431-file gap corpus is byte-identical
to the `origin/main` baseline; `scripts/gc_repsel_matrix.sh --arms all` is
361/361 byte-exact with FAIL=0 and XFAIL=0 and both `cons_scan_off` cells still
PASS; and a throw through a protected argument list is byte-exact under both
arms.
