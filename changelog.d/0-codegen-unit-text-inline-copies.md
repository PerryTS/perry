- **`--debug-symbols` on large modules.** A module big enough to split into
  codegen units failed to compile under `--debug-symbols` (or
  `PERRY_LLVM_INPROCESS=0`) with `use of undefined value
  '@perry_checked_number_remainder'` (three.js's `three.core.js` was one). The
  streamed text renderer those builds use dropped the `available_externally`
  copies of cross-unit callees, whose declarations the unit layout omits. Both
  text renderers now render a unit through one function.
