### lint: unbreak the raw-handle debt ratchet on `main` (#8269 follow-up)

#8269's `native_module_enum` was correctly rooted but read its handles through
bare `get_raw_*_ptr`, pushing `object/field_get_set/enumeration.rs` to 14
bare reads against its per-module ceiling of 5 — which turned the required
`lint` context red on `main` and with it `pr-gate` on every rebased PR. All
nine new reads are converted to the #7341 blessed `with_{mut,const}_ptr`
forms (behaviorally identical: `with_mut_ptr(f)` is `f(get_raw_mut_ptr())`,
a scoped re-read from the handle at each call site). The global baseline is
ratcheted DOWN 990 → 983, locking in unrelated debt drops in
`builtins/globals.rs` (15→12) and `global_this_webassembly.rs` (9→5) that
had landed without an update.
