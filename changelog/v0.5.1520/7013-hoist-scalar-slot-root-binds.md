### Performance

- **GC: a scalar-replacement root slot is bound once per frame instead of once per store (#7013).**

  #7007 closed a real use-after-free (#6968) by emitting `js_shadow_slot_bind`
  at every store into a scalar-replaced object field or array element. In a loop
  that call is almost entirely redundant work. A bind does four things —
  `slot_ptrs[idx] = alloca`, `stack[idx] = *alloca`, `active[idx] = true`, and
  the incremental-mark root barrier — and for an entry-hoisted alloca the first
  three are loop-invariant: the address never changes, and every reader of a
  bound slot (`visit_shadow_stack_root_slots`, `js_shadow_slot_get`)
  dereferences `slot_ptrs[idx]` in preference to the `stack[idx]` mirror the
  bind refreshes, so the mirror is dead storage.

  The bind now runs once in the function-entry setup. Each store keeps only the
  part that is genuinely per-store — shading the newly written value so an
  in-flight incremental mark cannot miss a pointer written into an
  already-scanned root — emitted inline and guarded on
  `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`, so the common path is a load, a
  compare and a not-taken branch rather than a TLS-touching call. This is the
  same treatment `enable_persistent_shadow_slot_for_array_alias` already gives a
  `const item = arr[i]` alias.

  The hoist does **not** move when the rooted value is read. The collector reads
  the alloca at collection time, exactly as before; nothing is snapshotted into a
  register and re-read later, so the operand a safepoint observes is unchanged.
  What disappears is a redundant copy, not an observation point.

  Measured on a quiet Mac mini (load 1.9, pinned Node 26.5.0), 10 M iterations of
  `{ s: mkStr(i), y: (i>>3) & 1023 }` where `s` takes a call result and is
  therefore statically pointer-capable, best-of-5 per run, arms interleaved:

  | arm | best |
  |---|---|
  | pre-#7007 (no rooting) | 123 ms |
  | #7007 as merged (bind per store) | 164 ms |
  | this change (bind per frame) | 126 ms |

  The rooting cost drops from **41 ms to ~3 ms — 93 % recovered** — at 10 M
  pointer-capable stores, i.e. ~4.1 ns/store down to ~0.3 ns/store. The
  numeric-only twin is untouched: the #6997 gate still means a proven-numeric
  field takes no slot, no call and no frame growth, and it now also takes no
  entry bind. Emitted binary size is unchanged (byte-identical for the
  benchmark).

  **One soundness obligation the hoist creates.** Binding at entry makes the slot
  `active` from function entry, so the collector dereferences the alloca before
  any store reaches it and on paths where none ever does. An uninitialized alloca
  would hand the root-word decoder stack garbage that can pass
  `is_plausible_heap_addr`. The object-literal, anonymous-shape and `split()`
  paths already stored `undefined` into their slots in `entry_allocas`; the
  array-element and fused-uppercase-receiver paths did not, because pre-hoist
  nothing read those allocas before their store. Both now do.

  Correctness: `scripts/gc_repsel_matrix.sh --arms all --pressure 8` against
  pinned Node 26.5.0 measures **PASS=229 UNVER=190 XFAIL=1 FAIL=20** over 440
  cells (22 corpus rows, post-#7011). That is main's published 460-cell baseline
  (240/199/1/20) minus exactly the row #7011 de-duplicated, which contributed 11
  PASS and 9 UNVER — no cell changed state. Both FAIL files are the #6981 pair
  (`repsel_p4a3_numarray_barriers`, `repsel_p4a3_ptr_numarray`, 10 cells each)
  and were already red. `test_gap_repsel_scalar_replaced_locals` — the file that
  detects the #6968 hole — is PASS on all 20 arms, including the evacuating ones,
  where the arm was confirmed live (`copied_objects` 261 251–379 072, output
  byte-identical to the oracle): the hoisted binding is still rewritten through
  `slot_ptrs[idx]` when the collector relocates its referent.

  New coverage in `crates/perry-codegen/tests/scalar_replaced_slot_roots.rs`: four
  tests pinning the new contract — one bind per slot rather than per store, the
  bind sitting in the entry block ahead of the storing loop *and* after
  `js_shadow_frame_push` (binding before the push would write the caller's
  frame), a shading barrier at every store, and the entry initialization the
  hoist depends on. All four were verified to fail against the pre-hoist
  compiler.
