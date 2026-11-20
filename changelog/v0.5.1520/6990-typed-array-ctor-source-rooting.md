### Fixed

- **GC: a typed-array constructor source is now a precise root (#6981).**
  `new Int32Array([7, 8])` silently produced a **length-0** array under a
  relocating minor with precise roots — `a[0]` read `undefined`, no crash.
  That is the failure in #6981's minimal reproducer,
  `test_gap_specabi_reassign.ts`, and it is not what the issue's analysis
  proposed.

  **It is not the spec-ABI `TaPtr` shortcut.** The reproducer emits no
  specialized entry at all (`nm` finds no spec symbol) and fails identically
  with `PERRY_SPECIALIZED_ABI=0`. The never-reassigned proof in
  `collectors/spec_abi_sites.rs` is correct *and* is consulted on this route:
  `P = new Int32Array([7, 8])` is a `GlobalSet`, so `P` lands in
  `ModuleScan::writes`, never enters `ta_bindings`, and `judge_arg` returns
  `Boxed`. The test's own header comment says as much — it is a *negative*
  test for the spec-ABI that happened to be failing for an unrelated reason.

  **Root cause, one layer out and on the runtime side.** A constructor source
  reaches `js_typed_array_new` only as a bare NaN-boxed C-ABI argument, which
  is not a precise root, and the helper allocates before it ever dereferences
  the source. Instrumented ordering puts the collection inside the
  source-classification chain (`is_registered_map || is_registered_set ||
  is_builtin_iterator_class_id || js_util_types_is_generator_object`);
  `clean_arr_ptr` then nulls the swept source and the constructor falls
  through to `typed_array_alloc(kind, 0)`.

  The fix roots the observed value in a `RuntimeHandleScope` and re-reads it
  after every allocating step, across `js_typed_array_new`'s heap-source arm,
  `js_typed_array_new_from_array`, `typed_array_from_source_raw_values` and
  `typed_array_plain_object_values`. The handle is a **snapshot of the
  argument**, never the caller's binding — re-deriving from `P`'s slot after a
  safepoint would hand the constructor the *new* array and convert a
  stale-pointer bug into a silent wrong-answer one.

  Measured on the evacuating precise-roots arm (`PERRY_GC_HEAP_LIMIT=8
  PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off`, oracle = pinned
  Node 26.5.0): the representation corpus goes from **14 red to 12**, with
  `test_gap_specabi_reassign` (9 670 objects copied) and
  `test_gap_specabi_polymorphic_coexist` (9 648 copied) repaired. Both cells
  relocated thousands of objects, so the arm was live rather than inert. The
  PR-gated arms are 21/21 green.

  New gate: `test-files/test_gap_gc_ta_ctor_source_rooting.ts`, registered in
  `test-parity/gc_repsel_corpus.txt`, verified to **fail** on the unfixed
  build (`literal: 0 undefined undefined`, 9 675 objects copied) and pass
  after.

### Changed

- **Docs: the `TaPtr` spec-ABI no-shadow-bind comments now state the real
  invariant** (`codegen/function.rs`, `codegen/spec_abi.rs`). Their conclusion
  holds, but the stated reason — "typed-array storage is non-movable" — named
  the wrong object: what is passed and hoisted through is the typed-array
  *header*, which is an object. The address is stable because
  `typed_array_alloc` places header + inline payload in the OLD arena with
  `GC_FLAG_TENURED`, which the nursery copying minor never relocates and
  old-page defrag skips (`gc_type_is_movable(GC_TYPE_TYPED_ARRAY)` is
  `false`). Both comments now also say explicitly that this does not
  generalize to any other raw-pointer representation.
