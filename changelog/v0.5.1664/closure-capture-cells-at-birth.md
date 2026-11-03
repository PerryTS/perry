A closure is now born with every cell capture slot filled, so its entry reads
captured cells with plain loads instead of asking the runtime whether each
slot holds one.

A binding that a closure captures and someone mutates lives in a GC cell (a box
or a scope object), and the closure's capture slot holds that cell. When the
binding's declaring statement had not run on the path where the closure was
created (a declaration the async and generator transforms emit on mutually
exclusive paths), the slot held the frame root's `undefined` sentinel, and a
closure born in an outlined module-entry chunk that has no root for the
binding got `0`. Every public closure body therefore re-validated its
cached capture cells on every call through `js_scope_capture_base` /
`js_box_capture_cell_ptr`: an object-start probe plus a typed header read.
Removing it saves about 174 instructions per closure entry. On the program set
(instructions:u, n=5 interleaved; same-binary A/A within 0.01% on these three): tsc -6.14%
(93.0 M entries), qs stringify -4.04% (12.1 M), qs parse -2.12% (2.7 M); the
other programs are within their noise floor. Outputs, full-collection counts
and RSS are unchanged. A closure calling itself through a captured `const`
costs 74 instructions per call instead of 249 (Node: 70).

Closure birth (`perry-codegen/src/stmt/binding_cell.rs`) now gives such a
binding its cell before the closure reads its capture words: the cell is
minted there, seeded exactly as its declaring statement seeds it (the TDZ
sentinel for a lexical binding, so a read before initialization still throws a
ReferenceError; `undefined` otherwise; the compiler-private async control
cells as i32/bool boxes), and published into the frame root so the frame and
every closure share it. A closure with no root for the binding in its context
gets a cell of its own, seeded `undefined` (the value such a capture always
read). The check is emitted only where it can be needed:
roots that a preallocation, a boxed `let` or a boxed parameter of an enclosing
statement list already initialized are tracked per statement list and skip
it. All of tsc keeps 229 checked captures.

A closure created where such a binding has no storage mints that cell while it
is still gathering its capture words, and a mint allocates. Those births now
mint their cells before reading any other capture word and keep each minted
cell rooted across the later mints, so no word sits in a register across an
allocation; births that mint nothing are unchanged.

What this removes or unifies:

- `js_scope_capture_base`, `js_box_capture_cell_ptr` and their two fallback
  statics are deleted. The public closure body's entry cache loads the slot.
- Preallocated boxes, scope objects, reused `let` cells and births mint cells
  through one helper.
- Closure prologues read the `this` / `new.target` captures and the typed
  trampolines and typed clones read their captures with inline loads instead
  of `js_closure_get_capture_bits` calls. Call-site typed guards keep the
  checked accessor, because the closure handle there is not proven.

`PERRY_ASSERT_CAPTURE_CELLS=1` makes every birth check each cell word it
installs (`js_capture_cell_assert` aborts on a word that is not a live box or
scope object). Compiling tsc that way runs and matches Node.

Tests: `closure_capture_cell_birth` (arrows, function expressions, hoisted
declarations, object-literal methods and accessors, closures in class methods,
generators, async, TDZ before and after initialization, scope groups,
per-iteration cells, transitive captures, births on paths that skipped the
declaration, births that mint two cells, under forced evacuation and scheduled collections) and the
runtime's `capture_cell_check_accepts_only_live_cells`.
