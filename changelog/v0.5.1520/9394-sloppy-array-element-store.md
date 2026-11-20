### Fixed

- **A rejected array element write no longer throws in sloppy code.**

  ```js
  const a = [1]; Object.freeze(a); a[0] = 9;               // node: silent   Perry: TypeError
  const a2 = [1]; Object.freeze(a2); a2[5] = 9;            // node: silent   Perry: TypeError
  Object.defineProperty(a3, 0, {writable:false}); a3[0]=9; // node: silent   Perry: TypeError
  Object.preventExtensions(a4); a4[5] = 9;                 // node: silent   Perry: TypeError
  const o = {x:1}; Object.freeze(o); o.x = 9;              // node: silent   Perry: silent (correct)
  ```

  ES2024 §6.2.5.7 (`PutValue`) calls `Set(O, P, V, Throw)` with
  `Throw = IsStrictReference`, so a failed `[[Set]]` throws **only in strict
  mode** — for an Array exactly as for the ordinary object that was already
  right. A CommonJS bundle is sloppy code from top to bottom, which is where
  this surfaced.

  Introduced by #9326 (the merge of #9297, live again on `main` via #9370).
  That change is right about what it set out to fix — an inherited accessor
  must run, an inherited non-writable index must reject — but it reached the
  rejection by routing the cold element-store continuation through the STRICT
  runtime entry unconditionally. The inline store guard declines exactly the
  receivers whose write can be rejected (frozen, sealed, non-extensible,
  descriptor-bearing, prototype-sensitive), so every one of those shapes
  arrived at that continuation and threw.

  The fix carries the assignment's own `Throw` flag, which codegen already had
  and already passes to the ordinary-object `[[Set]]` and to
  `js_dyn_index_set_strict`. Finding the target is unchanged in both modes —
  the #9220 inherited-descriptor walk still runs, so a prototype setter still
  fires on a sloppy assignment; only the rejection differs.

  - `crates/perry-codegen/src/expr/index.rs`,
    `crates/perry-codegen/src/expr/index_set.rs`,
    `crates/perry-codegen/src/runtime_decls/objects.rs` — pass the site's
    `assignment_strict` to `js_typed_feedback_array_index_set_fallback_boxed`
    and `js_typed_feedback_array_set_index_or_string` (one new trailing `i32`
    each).
  - `crates/perry-runtime/src/typed_feedback.rs` — both helpers take that flag
    and dispatch on it.
  - `crates/perry-runtime/src/array/indexing.rs` — the strict entry's body
    becomes strictness-parameterised (`js_array_set_f64_extend_sloppy` is the
    sloppy twin); `array_spec_set` takes `Throw` and returns the receiver
    unchanged instead of throwing when it is false. Array mutators keep
    `Throw = true`: their own algorithms specify it regardless of the calling
    code.
  - `crates/perry-runtime/src/array/indexing_keyed.rs` — the same for the
    numeric/string-key dispatcher.
  - `crates/perry-runtime/src/value/dyn_index.rs` — `js_dyn_index_set_strict`
    already carried the flag and its array arm forced `true`; it now uses it.

  The realloc arm in `expr/index.rs` deliberately keeps the strict entry: it
  runs only for a receiver the guard already accepted, which cannot reject.

  Validation: `test-files/test_gap_9394_array_element_store_strictness.cts`
  — a `.cts` file, so it is a CommonJS script in **both** runtimes, with a
  sloppy arm and a `"use strict"` arm. **Both arms are asserted.** Asserting
  only the throw is precisely what let this through: #9326 shipped with a
  64-check differential and a 205-line gap fixture, all green, none of it
  sloppy code. Byte-compared against node 26.5.1; Perry built from unfixed
  `origin/main` reports `TypeError` for six sloppy cases where node is silent,
  and with this change is identical to node. The #9326 fixture
  (`test_gap_9220_9221_array_proto_paths.ts`, an ES module and therefore
  strict) is unchanged and still byte-identical to node.

  Unit tests, both arms: `array/strict_store_tests.rs`
  `element_store_rejection_throws_only_in_strict_mode`, and #9326's own
  `typed_feedback_array_set_guards_reject_frozen_arrays`, which now asserts the
  silent sloppy call alongside the strict throw.

  Three pieces of test infrastructure had to admit a `.cts` fixture at all —
  each of which would have made it a **dark test**, green because it never ran:

  - `run_parity_tests.sh` discovered the suite with `find … -name '*.ts'`,
    which does **not** match `foo.cts` (the suffix is `.cts`). The fixture was
    invisible to the harness — confirmed empirically: `--filter test_gap_9394`
    selected 0 tests before the change and reports
    `PASS test_gap_9394_array_element_store_strictness` after it.
  - the same script derived a test's name with `basename … .ts`, which left
    such a file called `…strictness.c`.
  - `.gitignore` ignores `test-files/test_*` (compiled test binaries) and
    re-included only `.ts` / `.tsx`, so the fixture could not be committed.

  Not addressed here, found while writing the fixture: Perry emits
  `js_put_value_set(..., strict = 0)` at **every** property-set site, so a
  rejected *strict* ordinary-object write (`"use strict"; Object.freeze(o);
  o.x = 9`) is silent where node throws. That is the mirror-image gap on the
  object path and is out of scope for #9394.
