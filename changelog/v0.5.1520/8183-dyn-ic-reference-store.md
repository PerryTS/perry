`perf(codegen)`: the inline dynamic-key write IC now stores **reference** values
(object / string / bigint) through a barriered inline arm instead of diverting
them to the outlined helper. `o.k = { … }` in a loop drops **32.6% of its
instructions, 28.9% of its cycles and 29.2% of its wall time** — 7.85x → 5.56x
vs Node on the object-write matrix's `rhs_allocating` shape — with peak RSS and
binary size unchanged (33,248 → 33,200 KB; 13,544,480 bytes both arms).

Before this, `lower_put_value_dyn_ic_inline`'s entry predicate ANDed in "the
value tag is not pointer/string/bigint", so every reference-valued write left
the inline path before the receiver guards and took
`js_put_value_set_dyn_ic` — one cross-crate call per write that re-validated,
in Rust, exactly the guards the inline block had already proved. The tag now
SELECTS a store arm: `put.dynic.store.scalar` keeps the pre-existing bare store
(its `GC_STORE_AUDIT(POINTER_FREE)` claim is unchanged), and
`put.dynic.store.ref` runs `emit_jsvalue_slot_store_scalar_aware_on_block` —
byte-for-byte the static write PIC's pointer-capable store, reached under
strictly stronger conditions, since the guards above it are that PIC's guards
and this block additionally knows the value carries a reference tag.

No new rooting obligation: the target is materialised below every operand that
can collect (the call site's existing evaluation-order argument), and all three
bookkeeping helpers are `gc-leaf-function`, so nothing between the re-read and
the store is a collection point. `scripts/gc_root_dominance_check.py` over
`scripts/gc_root_dominance_corpus.sh` stays at 0 violations / 0 unrooted
allocas with an empty allowlist and 40/40 seeded violations caught.

### This is #8108's measured prize, reached by a different route

#8108 ("the static write PIC still rejects any safepointing RHS") names three
cells — `rhs_call` 18.28x, `rhs_pointer` 9.21x, `rhs_allocating` 4.64x — and
proposes admitting a safepointing RHS into the static write PIC. Measured on
`a3118cfea` before writing any code, two thirds of that framing is wrong and
the named lever would have made the largest cell slower:

* **`rhs_pointer` is already on the static PIC.** Its RHS is `Expr::LocalGet`,
  which `put_value_rhs_is_safepoint_free` has always admitted. The gate never
  rejected it; its 9.21x is the pointer store path, not the safepoint rule.
* **`rhs_call` would REGRESS.** Splitting the call into a local
  (`const v = f(); o.x = v`) is exactly the IR slice A would produce, and it
  costs **+21.4% instructions** (5.362G → 6.512G, +18% wall): the static PIC's
  hit block emits three unconditional `gc-leaf` bookkeeping calls whenever the
  value is not statically provable non-pointer, where the dyn IC proves it at
  runtime and stores bare. 95% of that cell's cost is the closure call
  (425 of 447 instructions per iteration), not the write.
* **`rhs_allocating` is the real prize**, and it does not need the gate touched
  at all: the dyn IC already roots correctly, so widening its inline store to
  reference values captures the same win with no change to any safepoint rule.
  Arm B lands within 2.8% of the static-PIC ceiling (2.860G vs 2.782G).

The safepoint gate at `expr/proxy_reflect.rs` is therefore left in place and
#8108's premise is corrected on the issue rather than implemented.

### Tests

`dyn_ic_inline_store_barriers_a_reference_value` pins all three bookkeeping
calls in the reference arm, their ABSENCE from the scalar arm, and — because an
emitted block is not a reached block — the `br i1` INTO the reference arm.
`dyn_ic_inline_store_keeps_its_semantic_fallback_for_reference_values` pins the
tag as an arm SELECTOR rather than an entry gate, and the retained
`js_put_value_set_dyn_ic` fallback. Four sabotages were run and all four are
caught: dropping the write barrier, dropping the layout note + string addref,
routing reference values back to `put.dynic.slow` (dead-IR arm), and leaking a
barrier into the scalar arm.

`test-files/test_gap_8108_dyn_ic_reference_store.ts` is the behavioural half:
every value tag through one site, frozen / sealed / non-extensible / accessor /
read-only receivers, an inherited setter, a Proxy trap, array and typed-array
receivers, a mid-loop shape transition, a throwing RHS leaving no store,
target→key→RHS evaluation order, and a volume section whose producer is reached
through an `any[]` so it cannot be inlined into a rooted temp. Byte-identical
to Node 26.5.1 under the default GC and under
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`, `PERRY_GEN_GC=0`,
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`, and `PERRY_WRITE_BARRIERS=0`.

**Recorded because it is the reason the IR assertions exist**: a release build
with the write barrier removed from the reference arm passes that entire
behavioural matrix — all four GC modes, byte-identical output, exit 0. A
dropped barrier is invisible to every runtime probe here, so the static IR test
is the only thing that can say no.
