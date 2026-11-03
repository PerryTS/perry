### Fixed

- **`Object.freeze`/`Object.seal` and `Object.defineProperty(arr, "length", …)` hung on any array that had outgrown its dense capacity (#7548).** `js_array_grow` reallocates an array's header+elements as one allocation and leaves a #233 forwarding stub at the old address — and the stub's first 8 bytes are exactly where `length` and `capacity` live, so they read back as the two halves of the forwarding *pointer*. The array branches of `Object.*` reinterpreted the caller's pointer with a bare `obj as *ArrayHeader` cast, so a JS binding still holding the pre-grow address made `(*arr).length` return a heap address: `615098568` instead of `6` in the observed case. `is_array_object` cannot tell a stub apart (it keeps `obj_type == GC_TYPE_ARRAY`; only `GC_FLAG_FORWARDED` plus the clobbered payload distinguish it), so the bad pointer passed every guard.

  Two loops are driven by that length and became bounded-but-unreachable walks — one `to_string()` plus an attrs side-table probe per index, ~6·10^8 iterations:

  - `mark_all_array_props`, i.e. `Object.freeze` / `Object.seal`. `const t = [1, 2]; t.push(3); Object.freeze(t)` never returned.
  - `array_set_length_from_descriptor`, i.e. ArraySetLength's shrink walk — reached by the `Set(receiver, "length", n)` tail of an `Array.prototype.splice` that grows a **Proxy** receiver. That is the reported symptom: `test-files/test_gap_6908_proxy_array_mutators.ts` stopped after `sort-cmp` and was killed at the harness's 10 s budget. The mutator's element writes all completed; only the final length write walked.

  **The hang was bounded, not infinite** — a 10 s budget simply cannot tell the two apart, and the distinction matters because the fixes differ.

  Fix: one `array_header` / `array_header_mut` helper in `crates/perry-runtime/src/object/array_object_ops.rs` that follows the forwarding chain (`clean_arr_ptr`) before the cast, applied at all four header casts there. It falls back to the raw cast when the chain does not resolve, so no caller loses a pointer it previously accepted.

  Deliberately unchanged: the `obj as usize` side-table keys. The array attrs table is keyed inconsistently across the runtime (`getOwnPropertyDescriptor` reads at the caller's address; the element-write rejection path resolves through `clean_arr_ptr` first), and re-keying only these writers was measured to regress `getOwnPropertyDescriptor` on a grown frozen array without gaining the write rejection. Unifying the readers is a separate change.

  Affected mutators on a Proxy receiver were exactly those that write an index at/beyond the dense capacity and then write `length`: `push`, `unshift`, `splice` with inserts > deletes, and `splice(len, 0, x)`. `pop`, `shift`, `reverse`, `sort`, `fill` and `copyWithin` never grow and were never affected, in direct or `.call` form.

  There is no bisectable regression commit — the test never passed. Building `perry` at `d255ae604` (#7424, the PR that *added* `test_gap_6908_proxy_array_mutators.ts`) and running the test that commit ships reproduces the timeout exactly, same five lines, same last line `sort-cmp: 2,4,10,33`. It was broken on arrival and invisible because `parity` is gated to tag pushes. The bare casts themselves date to #4709 (ArraySetLength) and #5025 (freeze/seal on arrays), so the `Object.freeze`-on-a-grown-array hang had been shipping for two months independent of any Proxy work.

  Coverage: new gap test `test-files/test_gap_7548_grown_array_object_ops.ts` (byte-identical to node 26.5.1; the pre-fix build hangs on it with zero output) and a sabotage-tested unit test `stale_pre_grow_array_pointer_reads_the_real_length_in_object_ops`, which asserts the header read *before* the walks so a regression fails in 0.00 s rather than hanging the suite, and asserts non-vacuity (the stub's length word must actually be clobbered).
