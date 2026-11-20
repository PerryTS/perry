### Fixed

- **`s.has(makeKey())` / `s.delete(makeKey())` on a module-level `Set` no
  longer read a moved receiver.** `Expr::SetHas` and `Expr::SetDelete`
  (`expr/bigint_set.rs`) lowered the receiver, masked it to a raw `i64`
  handle, *then* lowered the value expression, and consumed the handle after
  — the #6970 shape their Map twins (`MapGet` / `MapHas` / `MapDelete`) were
  fixed for, found live by #9522's audit of every Map/Set/WeakMap lowering
  (#9523). A function-local Set was already covered — `root_reload` re-derives
  a shadow-slot load and its unmask below every collection point — but a
  module-level Set is a `@perry_global_*` load, which that pass deliberately
  does not reload, so an evacuating minor inside the value's evaluation left
  `js_set_has` a from-space header. Measured: the new gap fixture SIGSEGVs on
  unfixed main where node prints `bad=0`; the from-space quarantine reports the
  fault address as retired by the first minor with a last-known object of
  `GC_TYPE_SET`.

  Both arms now root the receiver in a `RootedGroup` before the value is
  lowered and re-read it from the slot afterwards, exactly as the Map twins
  do; when the value cannot collect nothing is pushed and the IR is unchanged.
  `bigint_set.rs` joins the rooting migration ledger.

- **`this.field.set(a, 1).set(b, 2)` consumes the receiver `js_map_set`
  returns.** `lower_call/property_get/map_set.rs`'s `"set"` arm called the
  helper as `void` and returned the receiver box it had read *before* the
  call. `js_map_set` returns the receiver as it stands after the insert — for
  a `class X extends Map` instance the runtime roots the movable
  `ObjectHeader` across the grow and hands back the relocated address — so the
  chained call could dispatch on a from-space pointer. The arm now re-boxes
  the returned pointer, as `Expr::MapSet` already did. Latent (the minor must
  fire inside the first insert's grow); pinned by a chained-set fixture.

  Fixtures: `test-files/test_gap_9523_set_receiver_roots_across_value.ts`
  (fails on unfixed main, byte-identical to node fixed) and
  `test_gap_9523_map_set_chain_returns_receiver.ts`; codegen contract in
  `temp_root_coverage/set_receiver.rs`, sabotage-verified under both root
  lowerings.
