### Fixed

- **`JSON.stringify(v, null, 2)` returned nondeterministic garbage — sometimes a crash — for an array grown past its initial inline capacity (#7269).** `js_array_grow` (#233) reallocates a grown array and leaves a `GC_FLAG_FORWARDED` stub at the OLD address, whose first 8 bytes are exactly `ArrayHeader.length` + `.capacity`, now holding the raw forwarding pointer to the new array. The plain (non-pretty) path always resolves this via `clean_arr_ptr` before every header read (`json/stringify.rs::stringify_array_depth`); the pretty-print path in `json/replacer.rs` did not, so a caller still holding the pre-grow address — the stub is retained *specifically* so such callers keep working — fed the raw forwarding-pointer bytes straight into `(*arr).length`/`.capacity`. That is why the symptom was "garbage" rather than a clean failure: the bytes are a real, live heap pointer, just not the field values they are pretending to be, so they vary run to run purely from ASLR/allocator placement. In the worst case the misread `length` (up to ~4 billion) drove the pretty-printer into a deep, self-feeding recursion over adjacent heap bytes reinterpreted as more NaN-boxed values, which overflowed the stack.

  Five call sites in `crates/perry-runtime/src/json/replacer.rs` cast an array pointer without resolving forwarding first:

  - `stringify_value_pretty`'s `TYPE_UNKNOWN` structural-fallback probe — the site actually hit by the issue's repro (`JSON.stringify(v, null, 2)`, no replacer).
  - `stringify_array_pretty` — hardened as the shared choke point for both its callers (the explicit `type_hint == TYPE_ARRAY` dispatch and the fallback above).
  - `stringify_array_with_replacer_pretty` — safe today only because its one current caller (`dispatch_pointer_with_replacer`) happens to resolve first; fixed to not depend on that.
  - `extract_string_array` and `is_array_value` — the PropertyList *replacer* array (`JSON.stringify(v, ['a','b'])`) has the identical hazard on its own pointer.

  A sixth site the issue also cited, `stringify_array_with_array_replacer`, already resolved via `clean_arr_ptr` (predates this issue's filing) and needed no change.

  Fix: every site now resolves through `crate::array::clean_arr_ptr` before the first header read, matching the pattern already used by the plain-stringify path and by the two sites above that were already correct.

  Coverage: `pretty_stringify_resolves_array_grown_past_inline_capacity` in `crates/perry-runtime/src/json/replacer.rs` grows a real array past its allocated capacity (no GC cycle needed — `js_array_grow` installs the forwarding stub unconditionally on every reallocating grow), asserts the stale header's raw `(length, capacity)` bytes really do reconstruct the grown array's exact address (sabotage precondition), then pretty-stringifies the stale pre-grow pointer and asserts the output matches the array's real, current contents. Reverting the fix makes the test abort (stack overflow via runaway recursion) rather than merely fail an assertion — confirmed by hand before submitting.
