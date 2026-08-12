### Fixed

**A bound Buffer-method closure captured a pointer to memory it did not own,
and dispatched on it later.** `typeof buf.readUInt8`, `const f = buf.readUInt8`
and `buf[k]` each produced a closure whose method-name pointer was already
dangling; the call then resolved the method from freed or relocated bytes.

`js_class_method_bind` stores the method-name POINTER in the closure and
`dispatch_bound_method` re-reads it at CALL time. Its own doc states the
contract — *"Method-name pointer is expected to be stable for the closure's
lifetime; codegen emits it from the per-module `.str.N.bytes` rodata global"* —
and codegen honours it. Two runtime callers on the Buffer path did not:

* `get_field_by_name_tail` derived `key_ptr` as
  `key + size_of::<StringHeader>()` — the **interior of a movable GC heap
  string** — and passed it through `buffer_own_prop_or_method`. The key string
  is unreachable the moment the read returns, so a copying minor could relocate
  or reclaim it out from under a closure that outlives it.
* `polymorphic_index`'s computed-key arm bound `name.as_bytes().as_ptr()` where
  `name` is a local `String`. That one dangles on return with no collector
  involvement at all.

Both now bind a `'static` literal. `buffer_dispatch`'s method-name list becomes
a single macro-generated source for both `is_buffer_method_name` and a new
`buffer_method_name_static`, which returns the literal out of that list rather
than a borrow of its argument — so there is one list to keep current, not two.

**Why it read as flaky.** Whether the stale bytes still spell the method name
is a property of the allocator, not of the bug, so the same code passed locally
and took a SIGSEGV on conformance-smoke: `test_gap_buffer_own_prop_shadow_intrinsic_6405`
on shard 7, joined by `test_gap_buffer_own_props` on shard 8 once collections
got denser. Neither test is in `gap_snapshot.json`; both are expected to pass.

**Tests** — `gc/tests/buffer_bound_method_name.rs`, in the required per-PR
`cargo-test` gate, asserting the contract structurally rather than asking
whether a given run happens to survive it:

* the closure's captured name must not alias the key string's interior;
* the computed-key arm's captured name must not come from a temporary;
* `buffer_method_name_static` must not return a borrow of its argument, and
  every caller must get the same literal whatever storage its own copy lives in.

Verified by sabotage: with the two call sites reverted and the tests unchanged,
the first two fail — the second on garbage bytes, i.e. the use-after-free
reproduced deterministically in-process on a host where the end-to-end tests
still passed.

`cargo test -p perry-runtime --lib`: 1979 passed, 0 failed. All 12
buffer/DataView gap tests byte-match Node 26.5.1, including both crashers.
