### Fixed

- **GC: a heap value in a scalar-replaced object field or array element is now a precise root (#6968).**

  Scalar replacement deletes the object and keeps one entry-block alloca per field/element. Those allocas belong to no HIR local, so `collect_pointer_typed_locals` — which assigns shadow slots by walking `Stmt::Let` — never saw them and nothing bound them. With precise roots only (`PERRY_CONSERVATIVE_STACK_SCAN=off`) a collection landing between the store and the read swept the value out from under the alloca: `{ const o = { a: fresh(0), b: churn(N) }; console.log(o.a, o.b) }` printed an empty `o.a`, or a recycled one, with no crash and no diagnostic. The array form (`const a = [fresh(0), churn(N)]`) was identical.

  #6951/#6972's object-literal rooting could not reach this shape: that path roots the object *handle*, and scalar replacement leaves no handle. The object local *does* get a shadow slot reserved — it is pointer-typed — but lowering only ever **cleared** it.

  Each replaced slot is now shadow-bound at the store, the same treatment `emit_shadow_slot_update_for_expr` gives an ordinary pointer-typed local, at object-literal fields, array-literal elements, scalar-replaced `split()` parts, anonymous-shape constructor arguments, and both `expr::property_set` arms. Two properties keep it cheap:

  - **The frame grows on demand.** `LlFunction::reserve_shadow_slot` rewrites the slot-count operand of the already-emitted `js_shadow_frame_push` (creating the frame if the pre-lowering count was zero), because the escape facts that decide scalar replacement are not computed until after the frame is sized.
  - **Reservation is lazy and gated on the lowering, never a declared type** (#6997): the predicate is `expr_is_known_non_pointer_shadow_value`. A literal whose fields are all numbers takes no slot, emits no call, and does not grow the frame. Measured: `{ x: i & 1023, y: (i>>3) & 1023 }` over 40 M iterations is unchanged (313–355 ms → 319–353 ms), as is the array twin. A pointer-capable field store costs **~2.6 ns** (118–121 ms → 144–148 ms over 10 M iterations) — against 4682–4997 ms for the heap object scalar replacement removes, so the optimization still wins by ~32× after paying for the root.

  Corpus effect, `scripts/gc_repsel_matrix.sh --pressure 8` on the evacuating precise-roots arm: `test_gap_repsel_gc_stress` goes FAIL → PASS (deterministic over 3 repeats, `moved=1 230 900` in both arms), and no cell regresses. Ten of the thirteen files #6981 lists compile to **byte-identical LLVM IR** with and without this change, so #6968 is provably not their cause; they belong to the argument-passing families (#6969/#6970/#6971) and their neighbours.

  New coverage: `test-files/test_gap_repsel_scalar_replaced_locals.ts` (registered in `test-parity/gc_repsel_corpus.txt`; red on `cons_scan_off` — a PR arm — before this change, green after) and `crates/perry-codegen/tests/scalar_replaced_slot_roots.rs` (5 codegen-contract tests, teeth verified in both directions, including the gate tests against a deliberately coarsened gate).
