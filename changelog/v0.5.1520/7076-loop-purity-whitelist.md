### Fixed / Performance

**Pure numeric loops no longer emit a GC back-edge poll they cannot need.**
`loop_may_allocate` (`crates/perry-codegen/src/loop_purity.rs`) decides, per loop
back edge, whether a `js_gc_loop_safepoint()` call has to be emitted to drain a
deferred minor collection. Its whitelist omitted relational comparisons,
arithmetic `Binary` and `Update`, so `for (let i = 0; i < n; i++) { sum = sum + 1; }`
failed the purity test on its *condition*, its *body* and its *update* — three
runtime calls per iteration in a loop that allocates nothing.

Measured on a Raspberry Pi 5 (Cortex-A76, 2.400 GHz verified before and after via
`vcgencmd measure_clock arm`, idle), 12 interleaved reps under `perf stat`:
**12,604,634,901 → 252,268,002 instructions retired (49.97x, cv 0.00%/0.01%)**,
8.87x cycles, 9.93x wall.

The widening reuses `expr_is_inert_primitive` (#6975) rather than growing a second
predicate answering the same question. Those operators run ToPrimitive / ToNumeric,
and a user-defined `valueOf` is arbitrary JS that allocates and collects — recursing
into the operands never sees that, since two plain `LocalGet`s recurse clean while
the *operator* calls into user code. They are alloc-free only when every operand is
a proven non-pointer primitive. `Add` additionally requires
`expr_is_known_non_pointer_shadow_value` on both operands, because it is the only
operator whose result can be a fresh heap value: a string *literal* is inert, and
`"a" + "b"` still allocates.

`expr_is_inert_primitive` also gained one restriction, in the safe direction for
`expr_may_trigger_gc` as well: `local_is_inert_primitive` refuses module-level
globals. `local_types` and `shadow_slot_map` are computed per function from that
function's body alone, and a module global can be assigned an object by a
different function the scan never sees.

Regression coverage is 13 unit tests in `loop_purity.rs` plus 7 IR tests in
`crates/perry-codegen/tests/loop_safepoint_purity.rs`, and was sabotage-verified
in both directions: breaking the implementation five ways turns exactly the
intended tests red, and the one guard no fixture can isolate is documented as
such in the test header rather than claimed as covered.
