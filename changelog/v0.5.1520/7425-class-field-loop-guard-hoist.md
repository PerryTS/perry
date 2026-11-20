**Fixed** the #5093 class-field versioned loop matching **nothing**. It hoists a
monomorphic `this.field` shape check into a loop preheader and runs a guard-free,
call-free fast clone; it was written for `benchmarks/suite/09_method_calls.ts`,
and on that benchmark it is worth **9.0x** — 90 ms → 10 ms, min of 7 runs, which
puts Perry at Node 26.5.1's 11 ms instead of 8x behind it. `value:10000000` is
unchanged and still matches Node.

`09_method_calls` is filed as a *dispatch* benchmark. It is not one: `increment()`
is fully inlined, there is no call in the loop, and #7287 measured the entire gap
in the `this.value` read + write — ~6.8 ns/iteration, about 22 cycles for what
should be `ldr`/`fadd`/`str`. There were zero `js_*` calls on the fast path. The
cost was ~60 IR instructions of per-access guard protecting three of useful work.

**The guard hoist already existed and had been unreachable in both
configurations.** Representation-selection Phase 1 made a proven-integer local's
canonical i32 slot its *only* storage — such a local is registered in
`ctx.local_slot_reps` with its alloca in `ctx.i32_counter_slots`, and has no
`ctx.locals` entry at all. The matcher gated **both** its loop counter and its
loop bound on `ctx.locals.contains_key(..)`, so after Phase 1 it declined every
loop. Turning Phase 1 off does not recover it either: the counter regains its
`ctx.locals` entry but a bare `i++` never earns an i32 *shadow* under the
parallel-shadow model, which `lower_class_field_versioned_for` separately
requires. A sibling matcher had already been repaired for exactly this
(`local_bound_storage_accessible`); the class-field one was missed. Both sites now
share `local_has_readable_slot`.

Reviving the matcher alone was not enough. Module top-level in a plain directory
is **sloppy**, so the store lowers through `try_lower_sloppy_class_field_raw_store`
(#7423), which had no loop-fact branch — the fast clone would have contained a
`js_put_value_set` call, and `lower_class_field_versioned_for` refuses to enter a
fast clone whose call-freeness it cannot prove, so it would have branched
unconditionally to the slow clone and changed nothing. The sloppy store now takes
the same inline plain-finite check + bare slot store the strict arm has had since
#5093. That is sound in sloppy mode for #7423's reason: the preheader proved
not-frozen, no per-receiver descriptors, matching class id and keys token, and an
intact typed layout, and the clone is call-free, so a store reaching the raw slot
is one that could not have been *rejected* in either mode. Every other value
side-exits to the slow clone before storing.

Measured effect on the emitted IR for `09_method_calls`, post-`opt -O3`: the hot
loop goes from **71 instructions to 10**, and the 29-instruction guard chain moves
to the preheader where it runs once. LLVM then promotes the field load into a
loop-carried register.

**The volatile-gate hypothesis measured zero.** #7287 proposed that
`load volatile @PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` was pinning the guard
chain in the loop. De-volatilizing it in the emitted IR and re-running `opt -O3`
gives **71 hot-path instructions either way** — byte-identical block sizes bar one
instruction moving between two blocks. Marking every guard-arm call
`memory(none) nounwind willreturn` *as well* still leaves the two deref blocks at
22 and 25 instructions inside the loop: LLVM will not speculate loads out of a
conditionally-executed block whose receiver it cannot prove dereferenceable. The
hoist has to be Perry's own, which is what the #5093 preheader check is. The
"do not de-volatilize the gate" note in `expr/class_field_inline_guard.rs` stands.

Regression coverage is three new `perry-codegen` unit tests
(`stmt/class_field_loop_tests.rs`) that assert the versioned blocks appear in the
emitted IR — for a literal bound, for the benchmark's module-scope `const` bound,
and in strict mode — and that the fast clone contains neither the volatile gate
nor a class-field guard call. All three fail on the parent commit. The lowering
had no test at all, which is why it could stop firing without anything going red
(CLAUDE.md, "a gate must assert its subject was live").

Verified: 25-case differential against Node covering `Object.defineProperty` on
the field, `Object.freeze`, `Object.seal`, an own accessor, a prototype accessor,
`delete` (before and mid-run), a subclass with a different layout, a receiver
alternating shape between iterations, and the mid-loop store side exits to
`Infinity` and `NaN` — byte-identical to Node, before and after. 68 `test_gap_*`
class/field/prototype tests compile-and-diff identically in both arms (66 pass, 2
pre-existing failures unchanged). `cargo test --release -p perry-codegen --lib`
635 passed. The slow clone remains reachable and is demonstrably taken: a frozen
receiver runs 3186 ms → 3686 ms and a prototype accessor on a declared field runs
10.1 s → 11.3 s, both unchanged behaviour on the pre-existing cliff those cases
already fell off.
