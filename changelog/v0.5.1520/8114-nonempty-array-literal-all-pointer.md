### Fixed

- **A non-empty array literal silently voided its `#7469` all-pointer element
  declaration, costing 26% on a push loop (#8102).**
  `js_array_declare_all_pointer_elements` refused every array with
  `length != 0`. But `emit_all_pointer_array_declaration` is emitted from the
  `Stmt::Let` tail — *after* an array literal's element stores have run and
  installed a per-slot side mask — so for `const a: C[] = [x, y]` the
  declaration was a no-op. The side mask survived, every later `a.push(…)`
  failed the elided-push header test in `expr/array_push.rs`, and each push paid
  the per-store `js_gc_note_slot_layout` that #7469 exists to delete.
  `collectors/all_pointer_arrays.rs` already admits such a literal (its
  `literal_of_object_elements_is_admitted` test), so the proof was issued at
  compile time and discarded at run time — CLAUDE.md failure mode 4, inside the
  optimization rather than inside a gate.

  The declaration now *discharges* the claim rather than assuming it:
  `layout_all_pointer_slots_would_hold` walks the initialized prefix and
  requires every slot to be pointer-bearing by `layout_pointer_bearing_bits`,
  the same predicate the layout-mask builder and `GC_LAYOUT_UNKNOWN`'s per-slot
  re-validation use, so it never has to trust the caller's static proof. A
  refusal leaves the header byte-unchanged (the raw-f64 bits are cleared only
  once the declaration is known to stick) and `length == 0` holds vacuously, so
  both pre-existing paths are bit-identical. Runtime-only: no ABI change, no
  codegen change.

  Measured with one compiler binary and only the `libperry_{runtime,stdlib}.a`
  pair swapped, `--release` both sides, instructions retired, medians of 3
  interleaved reps: 4,000,000 pushes into `const a: C[] = [x, y]` go
  20,755,859,948 → **15,309,077,609 (−26.2%)**, while the two controls — the
  same pushes into `const a: C[] = []`, and `[]` plus two pushes then the same
  loop — move −0.1% and +0.1%. Validated by `cargo test -p perry-runtime --lib`
  (2334 passed) and an output/exit-code A/B over 61 programs
  (`benchmarks/suite`, `benchmarks/app-patterns/kernels`, the beat-scriptc sweep
  corpus): 61/61 structurally identical, the only differences being printed
  elapsed-millisecond lines.

  The new coverage carries its own sabotage arm:
  `a_non_pointer_element_in_the_literal_still_refuses_the_declaration` runs the
  identical construction sequence with one numeric element and asserts both the
  refusal and that `_reserved` is unchanged, so a green positive test means the
  predicate discriminates rather than that nothing was tried.
