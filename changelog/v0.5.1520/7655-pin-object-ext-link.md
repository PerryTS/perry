**`fix(gc)`: pin long-lived and malloc-resident objects without the space classifier (#7650 follow-up).**

#7650 routed every `GC_FLAG_PINNED` write through `gc::pin_object`, which reaches
`arena::classify_heap_space`. That new edge kept a reference chain alive that
`-Wl,-dead_strip` had been removing, and **five `perry-ext-*` crates stopped
linking**:

```
Undefined symbols for architecture arm64:
  "_js_blob_new",                     referenced from: … fetch_globals::global_this_blob_thunk
  "_js_fetch_with_options",           referenced from: … global_fetch::global_this_fetch_thunk
  "_js_fetch_notify_signal_aborted",  referenced from: … url::abort::fire_abort_listeners
```

`perry-ext-{pdf,lru-cache,node-forge,mongodb,http}`. They link a **feature-stripped**
runtime through `perry-ffi`'s `runtime-link`, so those thunks have no definition
and only survived because the stripper removed them.

Bisected rather than guessed: the commit before #7650 builds all five clean,
#7650 does not, and reverting **only** the two `perry-runtime` call sites
restores the link. `perry-stdlib`'s `async_bridge` keeps `pin_object` — its
`js_promise_new()` promises really are Eden-resident and must arm the young-pin
latch.

`pin_object_non_young` does the flag write directly for the sites #7650's own
comments already document as long-lived (`string/format.rs`, the interned format
buffer) and malloc-resident (`thread.rs`, the spawn promise and its handle) —
they never needed the classifier. Making `pin_object` conservative instead
(arming for any `GC_FLAG_ARENA` object) would also remove the edge, but it would
arm on exactly these long-lived pins and throw away the preflight skip #7645
bought.

**The claim is checked, not asserted.** `debug_assert` catches a young object in
test builds, and `pin_object_non_young_call_sites_are_never_young`
(`gc/tests/copying/latch.rs`) asserts non-youngness for each real call site plus
a control proving the predicate is not vacuously false for everything. Sabotage:
forcing the predicate false reddens the control (0 compile errors, test binary
reached).

**Why no gate caught it.** `cargo-test` scopes per-PR runs to the changed crates'
reverse-dependency closure (`scripts/ci_test_scope.py`); the full workspace runs
on tags and nightly only. `perry-ext-*` is outside the closure of a
`perry-runtime` GC change, so this could only have surfaced at the next tag.
Found by running `cargo test --release --workspace` by hand against `main`.
