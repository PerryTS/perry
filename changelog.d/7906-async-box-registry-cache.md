**Async/await hot path: positive-cache the box-pointer registry probe, inline the runtime-handle accessors.**

The async-to-generator transform boxes every body local of an `async` function, and
`js_box_get`/`js_box_set` validate their operand against a thread-local hash set on every
access (perry#4898). Profiling `gc-handoff/apps/asyncpipe.ts` for the first time showed
that probe is the largest single item in Perry's async machinery: on a promise-only kernel
(24 000 activations, 48 000 awaits, no objects/strings/Map)
`is_registered_box_ptr` / `is_registered_i32_box_ptr` / `is_registered_bool_box_ptr` were
8.2 % + 5.9 % + 5.5 % of leaf samples. A state machine re-reads the same handful of boxes
(`__gen_state`, `__gen_done`, `__gen_executing`, plus the activation's locals) on every
step, so the probe almost always re-answers "yes" about an address it just answered "yes"
about.

Each registry now sits behind an 8-slot direct-mapped **positive** cache
(`crates/perry-runtime/src/box.rs`). This is sound because the registries are
**monotonic**: `js_box_alloc*` inserts and nothing ever removes — boxes are never freed —
so "this address is a registered box" can never become false, and an address can never be
recycled into a non-box allocation. A hit is exactly as authoritative as the hash probe it
replaces. A **negative** cache would not be sound (an address that is not a box today can
be minted as one tomorrow), so only confirmed positives are recorded and every miss still
falls through to the hash set; perry#4898's rejection of a read-only `__TEXT.__cstring`
address that passes every structural check is unchanged. The test-only
`test_clear_box_registry` — the single operation that breaks monotonicity — clears the
caches with the registries.

Separately, the `RuntimeHandle` accessors and `RuntimeHandleScope` root helpers
(`crates/perry-runtime/src/gc/roots/runtime_handles.rs`) are now inlineable: their
formatted `expect`/`panic!` arms were most of each function's estimated size and kept them
out of line in the release build, so a promise-heavy program paid a real call frame per
rooted-value read (`get_nanbox_u64` 6.4 %, `root_nanbox_f64` 3.0 %, `get_raw_mut_ptr`
2.1 % of leaf samples on that same kernel). The panics move to `#[cold] #[inline(never)]`
helpers. No semantic change.

Tests: `box_ptr_cache_rejects_a_colliding_unregistered_address` warms the cache with a real
box and then probes plausible-but-unregistered addresses that map to the *same* cache slot
— sabotage-verified, a slot-occupancy comparison instead of a full-address comparison makes
it fail. Plus `box_ptr_cache_eviction_does_not_lose_a_real_box` (the cache is an
accelerator, never the source of truth) and `box_ptr_caches_do_not_cross_kinds`.

The profile that motivated this, including two larger levers deliberately left open — ~14 %
of `asyncpipe` spent on incremental-GC work in a program that runs **zero** GC cycles, and
~9 % on the spec `Get(resolution, "then")` probe for every object an `async` function
resolves with — is written up in `gc-handoff/ASYNC2-NOTES.md`.
