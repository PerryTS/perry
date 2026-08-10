### Array element reads stop asking whether an array is a Map (#7765)

`gc-handoff/apps/asyncpipe.ts` — an async service pipeline, and the worst gap in
the corpus at 13x node — spent **13.5% of its run in `set::is_registered_set` +
`map::is_registered_map`**. Not on Set or Map work: on `js_array_get_f64` and
`js_array_length` asking both collection registries whether an ordinary array
was secretly a collection, on every element read.

#7755 made *unused*-feature registry probes free with monotone latches and named
Map/Set as the deliberate residual: asyncpipe uses both, so the #7474 latch is
correctly armed and each probe is real work — a Darwin `_tlv_get_addr`, a
`RefCell` borrow and a hash, per read. It also named why the trick that works
for typed arrays does not transfer: an address-keyed negative memo is an ABA
hazard for Map/Set, whose headers are recyclable arena objects.

**The object already knows.** `js_map_alloc` and `js_set_alloc` allocate their
headers through `arena_alloc_gc(_, _, GC_TYPE_MAP | GC_TYPE_SET)`, and each is
the single registration site for its registry — so a registered collection's
address *is* its GC header, and `obj_type` answers "is this a Map?" from one
byte. Both hot call sites now gate their probes on it.

This is ABA-proof by construction rather than by bookkeeping: the tag lives
INSIDE the candidate bytes, so whatever allocation owns those bytes next stamps
its own `obj_type` before the pointer is handed out. A recycled address answers
for its new owner with no invalidation step to get wrong — which is exactly what
an address-keyed memo could not offer. The registry remains authoritative for
the positive answer, so nothing about *when* an entry is added or swept moves.

Also correct for a header-*less* receiver. Buffers and typed arrays are
`std::alloc`-backed, so the eight bytes below them are allocator bookkeeping
that can read as any value — but both are already routed by the (latched,
free) probes above, and either way the bookkeeping byte reads the outcome is
unchanged: a byte that happens to read as `GC_TYPE_SET`/`GC_TYPE_MAP` still
falls through to the authoritative registry, and any other value skips a probe
that would have answered `false` anyway. Neither call site gains a dereference:
`js_array_get_f64` reads this header through `clean_arr_ptr`, and
`js_array_length` reads it eight lines further down for its
`GC_TYPE_LAZY_ARRAY` / `GC_TYPE_OBJECT` arms, under the same magnitude guard.

The same one header read also feeds the descriptor-flag check further down
`js_array_get_f64`, which `array_object_flags` used to re-derive through a
second `clean_arr_ptr` and a second header read (3.1% of the profile on its
own).

`crates/perry-runtime/src/array/collection_tag_tests.rs` asserts THE SUBJECT,
not just the answer — the registry is a correct fallback, so a test that only
compared values would still pass with the gates deleted (CLAUDE.md, "four ways a
gate can be unable to fail", case 4). `is_registered_map` / `is_registered_set`
carry a test-only entry counter, and `plain_array_element_reads_never_probe_the_collection_registries`
asserts 64 passes over a 4-element array move it by zero while both registries
are non-empty. Delete either gate and that is what fails.
`a_stale_registry_entry_over_recycled_bytes_does_not_read_as_a_map` plants the
ABA state directly — a live registry entry over bytes re-stamped `GC_TYPE_ARRAY`
— and pins that the answer comes from the bytes; it fails if the header
confirmation at the end of `is_registered_map` is removed.
`every_registered_collection_address_carries_its_own_type_tag` pins the
invariant the gates rest on, across capacity growth, so a future registration
path that forgot the tag goes red here rather than silently.

Two comments claiming Map/Set headers are `alloc()`-backed with no `GcHeader`
— the stated reason the registries are consulted before any header read — are
corrected in place; they predate the move into the managed arena and are how the
answer stayed hidden.
