Several `gc::tests::runtime_roots` helpers and tests in `prototype_addr_cache.rs`
and `side_table_scanners.rs` held a raw GC pointer across `arena_alloc_gc` /
`arena_alloc_gc_old` without suppressing automatic GC triggers. Those
allocations can land on the block-full slow path, which calls
`gc_check_trigger()` — an uncontrolled collection there could relocate or free
the held pointer's object before the test finished with it, since the
pointer was live only as a native local with no root registered.

Both files now use the same `GcTriggerThresholdTestGuard::suppress_automatic_triggers()`
pattern already used by `callback_scanners.rs`, `hook_dispatch_handles.rs`, and
`transient_handles.rs`, applied at every call site where a raw pointer/address
is live across a further allocation. Sites that are not exposed carry a brief
comment explaining why.
