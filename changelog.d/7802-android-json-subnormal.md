**`perry-ui-android`'s JSON walk no longer dereferences positive subnormal numbers** (#7448).

`crates/perry-ui-android/src/json.rs` carried a verbatim copy of the predicate #7447 removed from the main runtime:

```rust
exponent == 0 && mantissa != 0 && sign == 0
```

That is bit-for-bit the IEEE-754 **positive-subnormal** test, so every positive denormal `Number` was classified as an untagged heap pointer and dereferenced. In the main runtime the identical code SIGSEGV'd on `JSON.stringify(1e-317)` and returned a silent `null` for `5e-324` — both reachable from untrusted input through `JSON.stringify(JSON.parse(text))`.

The issue asked for two decisions to be made alongside the fix, and both point the same way: **no bit test can decide this**. A raw untagged pointer and a positive subnormal occupy the same bit patterns by construction, which is why the runtime's own version burned through two failed narrowings (`top16 < 0x7FF8`, then `top16 == 0`) before landing on allocation membership. A third divergent copy is how that happened in the first place.

So the copy is deleted rather than ported: `perry_runtime::json::ptr_is_tracked_heap_object` is now exported (it was `pub(super)`, which is what blocked #7447 from fixing this) and the android path calls it. It answers from the page map and the malloc registry, both dereference-free, so a forged or unmapped address is rejected before any field is read.

#7447 left this unfixed because the crate could not be built or verified on the machine that made the fix. That is not true here — `cargo check -p perry-ui-android` builds on macOS arm64 in under a second, and both it and `perry-runtime` check clean with this change.

The second question the issue raises — whether the untagged-pointer branch is reachable on android at all, given it measured *dead* in the main runtime (155,540 rejections, zero admissions across the JSON corpus) — is deliberately left open. Deleting the branch outright would be the kill-policy answer, but that needs a measurement on a device, and this change makes the branch safe either way.

Verified: `JSON.stringify` of `1e-317`, `5e-324`, a nested object of subnormals, and a `JSON.stringify(JSON.parse(…))` round-trip all match Node v26.5.1 exactly through the runtime path; `perry-runtime`'s json unit tests are 78 passed / 0 failed and `test_gap_json` is 7/7. The only remaining matches for the old bit pattern in the tree are the two doc comments that quote it as history.
