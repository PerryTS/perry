Stop re-deriving the receiver on every `Array.prototype.push`. A hot
`for (…) { a.push(v); a.pop(); }` loop cost 1,350 instructions per push+pop
pair, and a symbol-resolved profile showed more than half of that was not the
append but the append re-asking questions about a receiver it had already
resolved, once per helper in the chain. 1,350.6 -> 432.0, **-68.0%**, with
nothing inlined and nothing cached.

Four removals. `typed_feedback::numeric_array_push_guard` probed the property
descriptor of `"length"` on every push — the heaviest frame in the loop at
16.5% of samples — although the flag test three lines above had already
rejected every receiver that could make it answer true (a non-writable
`length` always marks `OBJ_FLAG_ARRAY_DESCRIPTORS`).
`js_array_numeric_push_f64_unboxed` re-ran `clean_arr_ptr` three more times by
asking `array_is_sealed_or_no_extend`, `array_is_frozen` and
`guard_writable_length` in sequence, each of which goes through the
non-resolved `array_object_flags`; that function alone was 24.0% of the loop.
The append chain resolved twice more, and the exotic and numeric-layout checks
once each. `js_array_pop_f64` already carried this exact fix — `push` never got
it.

Also fixes a parity bug the new fixture found:
`Object.preventExtensions(a); a.push(1)` silently kept the old length where
node throws. The dense append answers `SEALED | NO_EXTEND` with a bare
`return arr`, which is correct for the internal CreateDataProperty-style append
that builds fresh result arrays and wrong for user `push`; the observable entry
now throws, with node's wording. `Object.seal` had masked it by routing down
the exotic path for an unrelated reason.

Known divergence left: frozen `pop` still reports "Cannot mutate a frozen
array" rather than node's "Cannot delete property 'N' of [object Array]".
