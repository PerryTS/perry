**CI: repair the raw-handle debt ratchet, which is failing `lint` on `main`.**

`lint` is red on `main` (`c721cb6a0`): the raw-handle debt ratchet reports 984 bare reads against a baseline of 978, with four per-module violations introduced by recently-merged work.

```
crates/perry-runtime/src/bun_ffi/callback.rs:        4 bare read(s) in a module with no ceiling
crates/perry-runtime/src/object/array_object_ops.rs: 2 bare read(s) in a module with no ceiling
crates/perry-runtime/src/object/typed_array_define.rs: 4 bare reads exceeds its ceiling of 2
crates/perry-runtime/src/promise/microtasks.rs:      11 bare reads exceeds its ceiling of 10
```

Every one of these is a *correct* rooting pattern that simply reads out of the handle directly instead of going through the combinators the ratchet exists to encourage, so the conversions are mechanical and behaviour-preserving:

- `typed_array_define.rs` — the two descriptor probes allocate a key string and then re-read the descriptor; that is exactly `across_mut`'s shape (run the call, rebind the pointer afterwards).
- `array_object_ops.rs` — both reads scope a pointer to a short non-allocating operation, so `with_mut_ptr`.
- `bun_ffi/callback.rs` — four argument-position reads feeding `set_field` / `JSValue::object_ptr`; `with_mut_ptr` per call, with the NaN-boxed operands hoisted so the closure borrows nothing it shouldn't.
- `promise/microtasks.rs` — one restore pair rewritten as `with_mut_ptr` around the `CURRENT_MICROTASK_*` cell writes.

Debt drops 984 → 974, below the previous baseline, and the baseline is re-locked at the lower number so the improvement cannot silently erode. `--self-test` passes, so the checker can still fail.

Verified: `gc_string_coerce_property_key_rooting_6943` (3/3) and `gc_property_key_operand_rooting_6935` (3/3) both green against a release build of this tree, and `cargo fmt --all -- --check` is clean.
