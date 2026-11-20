**The GC trigger path and the dirty-page barrier stop paying `_tlv_get_addr`
per read, and the policy gate that let them stop paying it now counts the
thing it is bounding.**

`gc_check_trigger` runs on every `gc_malloc`, and its predicate
(`gc_budgeted_due_trigger`) resolved eleven raw `thread_local!` declarations
one out-of-line call at a time. Measured with `sample` on the compiled
claude-code TUI streaming a 3300-char reply (14,578 active main-thread
samples, callers resolved by an explicit ancestor walk rather than
nearest-symbol labels): `_tlv_get_addr` was 380 main-thread leaf samples,
**71 of them with `gc_budgeted_due_trigger` as the immediate caller**, 36 in
`old_page_account_dirty_slots`, 31 in `scan_dirty_object_slots`, 27 in
`gc_malloc_header_is_tracked`. `crates/perry-runtime/src/tls_hot.rs` has
existed to abolish exactly this since #7469; the allocation path's *fields*
were covered and the trigger path never was.

Sixty-seven declarations across `gc/policy.rs`, `gc/malloc.rs`, `gc/old_free.rs`,
`gc/tenuring.rs`, `gc/trace.rs`, `gc/barrier/mod.rs`, `arena/block.rs` and
`arena/page_meta.rs` move to `crate::perry_thread_local!` — same syntax, same
`.with()` at every call site, the address served from this thread's hot cache
instead of a libdyld call.

**Why they were still cold is a measurement bug in the gate, not an oversight
anyone could have noticed.** `scripts/check_thread_locals.py` ratchets on the
number of raw `thread_local!` **blocks** per file, while `thread_local! { … }`
holds any number of declarations — so `gc/policy.rs` counted as **6** while
declaring **28**, and adding a `static` to an already-recorded block passed
the gate silently. Counted in the same unit as the hot side, `main` was **318
hot declarations against 339 cold ones** — cold was the majority, reported as
a 2.6:1 minority. The gate now ratchets on declarations (`385 hot / 272
cold`), and `--self-test` grew a seventh direction that fails when a `static`
is added to a recorded block; restoring the block count makes that case, and
only that case, fail.

Three declarations stay deliberately raw and say so at their declaration:
`ARENA_TOTAL_BYTES`, `BLOCK_POOL` and `BLOCK_POOL_BYTES` are read from
`Arena::new`, which runs as `tls_hot::fill`'s **first** provider, so a
`HotKey` there re-enters `fill` — which by design has not yet written the
`temp_roots` field it gates on — and re-runs `ARENA`'s initializer without
bound. It is a stack overflow at thread start, not a slow path, and it is the
first documented instance of the rule that a declaration read from inside a
`fill` provider cannot use the macro. `gc::tests::tls_fill_reentrancy` is the
standing guard, and it is sabotage-proved: moving `ARENA_TOTAL_BYTES` alone
into the neighbouring hot block aborts that test with `fatal runtime error:
stack overflow`.

`gc::tests::trigger_path_tls` is the runtime half of the gate: it drives
`gc_check_trigger` on a fresh thread and asserts every trigger-path
declaration owns a hot slot and that the path publishes slots at all.
Reverting any one of them to a raw `thread_local!` removes `slot_index` and
breaks the build at that declaration's own name. It is a test that can fail
and did: the first run rejected `GC_DEFERRED_REQUEST` with `index 4294967295`,
correctly — `defer_gc_request` reads it only while a root lock is held, so it
is not a fast-path read and never claims a slot. The list is what the fast
path reads, not what the module declares.
