- **`compiler-output-regression`: a quoted LLVM label now starts a new basic block.**
  `native-region-proof` failed `packed_f64_loop_versioning` with
  `hot_loops_no_runtime_calls: {"for.packed_f64_fast.body.54.i.epil": ["js_array_alloc"]}`
  on correct codegen. The named block holds no calls at all — it is a clean
  scalar epilogue; the `js_array_alloc` belongs to the following block, which
  builds `console.log`'s argument array.

  The block splitter matched labels with `^([A-Za-z0-9_.$-]+):(?:\s|$)`. LLVM
  quotes any identifier outside its bare-name set, and #9337's specialized
  functions carry a `$`, so the next label is emitted as
  `"perry_fn_…$spec_i32.exit":`. That line starts with `"`, so it never matched:
  no new block began and the quoted block's body was appended to the preceding
  label, moving a call into an unrolled hot-loop epilogue. The mis-attribution
  can only ever move calls *into* the preceding block, which is exactly the
  false-positive shape observed.

  `extract_blocks` / `extract_blocks_with_functions` now accept optionally-quoted
  labels and quoted `define` names. Verified against the exact IR CI analyzed
  (run 33598905771): 510 → 512 blocks, hot-loop count unchanged at 29, subject's
  hot-loop runtime calls `{"…epil": ["js_array_alloc"]}` → `{}`. Sweeping every
  workload in that artifact, `packed_f64_loop_versioning` is the only verdict
  that moves, so no masked failure is exposed. The regression test is
  sabotage-checked: reverting the pattern fails 2 of its 3 cases.
