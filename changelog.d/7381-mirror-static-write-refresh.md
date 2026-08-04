**Fixed** `js_object_set_field_by_name` passed a stale `obj` to
`mirror_class_object_static_write` on both own-data-write arms.

The two arms perform the property write (an inline slot store, or `overflow_set`
into the overflow map) and then call the class-static mirror hook. Both writes
can reach an allocator — `overflow_set` inserts into a Rust map, and a minor GC
triggers on the malloc-count threshold as well as on arena-block allocation — and
the mirror's first instruction dereferences `obj`. A collection in that write
leaves the hook reading from-space.

The function already carries a `refresh_roots_after_alloc!()` macro and calls it
after every other allocating step; these two sites were missed. Scoped to those
two arms on purpose: the macro also republishes `value` from its handle, so arms
that intentionally rebind `value` locally must not refresh.

Found by #7341's from-space quarantine via
`test_gap_gc_assign_string_source_rooting`. With the fix the fault moves out of
`mirror_class_object_static_write` entirely and into a separate, unrelated catch
in `js_string_index_get`, which remains open.

Baselines checked rather than assumed: `test_gap_2159_defineproperty_class_prototype`
and `test_gap_6301_event_target_subclass` fail identically on pristine `main`, and
`perry-runtime --lib` fails 3/5/3 tests across three runs of pristine `main`
(#7365), so neither is attributable to this change.
