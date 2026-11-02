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

**The adjacent cluster falls to the same argument.** `js_array_get_f64` was
6.3%, and 78% of that came from one caller: the object field-get funnel walking
an object's `keys_array`. That funnel has *already* proved `keys` is a live
`GC_TYPE_ARRAY` — it reads the `GcHeader` and returns `undefined` otherwise —
and capped the index below the array's capacity, which is precisely the pair of
facts `js_array_get` re-established per key, per property read, through a
`clean_arr_ptr` forwarding walk, a lazy-header probe, the exotic-receiver
classifications and a descriptor-flag read. `keys_array_slot` serves the dense,
descriptor-free, non-forwarded case from the array's own two words and delegates
everything it cannot serve on those terms — a hole (which reads through the
prototype chain), an out-of-range index, a forwarded or descriptor-carrying
array, a null pointer — so no general semantics move.
`keys_array_len_capped_to_capacity` stops paying the same toll through
`js_array_length` once per property read.

Measured on the pinned mini, both arms rebuilt from the same merge-base and run
**interleaved** — 4 passes × best-of-5, alternating arms per benchmark per pass:
**`asyncpipe.ts` 0.9211 s → 0.7284 s, −20.9%** — it stops being the corpus's
worst gap. `is_registered_map` + `is_registered_set` fall from **13.51% to
1.2–1.4%** of the `asyncpipe_big.ts` profile (two agreeing runs), and
`array_object_flags`, `js_array_get_f64` and `js_array_length` all leave the top
of it. `shapes.ts` (−4.7%) improves as a side effect — same funnel. No protected
benchmark moves: the largest is `push_num` at +0.4%, and `churn_alloc`, `tree`,
`retain` and `fib40` are flat to four decimals.

Interleaving is not a nicety here. The mini oscillates ~3% between passes, so a
block-sequential A/B (all of one arm, then the other) manufactured a +7.3%
`interp` "regression" and a uniform ~3% shift on eight benchmarks purely from
which phase each block landed in — visible only because the per-pass series
showed passes 1 and 3 slow on *both* arms. An earlier +1.2% on `churn_alloc`,
measured that way against a different base, likewise disappears (+0.0%) once the
arms are interleaved.

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
`keys_array_slot` gets the same treatment from both sides: a per-thread
fallback counter asserted at zero for the dense arrays the fast path exists for
and at exactly one per refusal for every shape it must delegate, so "stopped
applying" and "started swallowing something it should have delegated" are
equally red. (Per-*thread*, because `cargo test` runs every case on its own
thread in one process — a process-global counter is moved by whatever else
happens to be running, which is how the first version of these assertions
passed for the wrong reason.)

Two comments claiming Map/Set headers are `alloc()`-backed with no `GcHeader`
— the stated reason the registries are consulted before any header read — are
corrected in place; they predate the move into the managed arena and are how the
answer stayed hidden.
