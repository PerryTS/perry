Fixed two turnloop-era regressions that had dropped `test-parity/node-suite/timers`
from 90/90 to 81/90.

**Slot-reuse (ABA) crash.** `timer/store.rs`'s `remove_by_id` frees an
Immediate/Pending entry's slab slot for O(1) cancellation, but deliberately
leaves its index queued as a placeholder in the check/poll queues. A later
`setTimeout` could reuse that freed slab index before the placeholder was
popped; `pop_check`/`pop_poll` then saw a live entry there and took it, even
though the timer heap still referenced it — a later heap operation then
panicked `key()`'s "heap index is live" assert. Queue placeholders now carry
`(slab index, seq)` instead of a bare index, and a `seq` mismatch is treated
the same as an empty slot: a stale, already-consumed placeholder to drop, not
an entry to steal. Repro: `const im = setImmediate(f); clearImmediate(im);
setTimeout(g, 20)` used to print `done` then panic; it now prints only `done`,
matching Node.

**Callback `this` was not the handle.** Since #10821 the JS-visible
`Timeout`/`Immediate` handle is an ordinary GC object, but the scheduler
never retained it: `call_timer_callback`/`call_timer_callback_entry` still
installed the pre-#10821 pointer-tagged numeric id as `this`, so
`this === theReturnedTimeout` was always `false`. `Entry` now carries the
entry's NaN-boxed JS handle as a GC root, scanned mutably alongside
`callback`/`args`/`context` so evacuation rewrites it in place; an interval's
re-armed copy carries the same handle forward, so `this` stays the same
object across every tick. Entries with no JS handle (native completion
callbacks) keep the id fallback.

Validation: `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime timer`
(58 tests, including two new ABA regression tests in `store_tests.rs`);
`scripts/node_suite_run.py … timers` 90/90 in both fast (`PERRY_NO_AUTO_OPTIMIZE=1`)
and CI mode; the ABA repro above; `PERRY_GC_SCHEDULE_SEED=1
PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1` on
`timers/namespace/args-and-this.ts` and `timers/interval/args-and-this.ts`
(output unchanged under forced evacuation); `scripts/check_file_size.sh`,
`scripts/gc_runtime_root_holders.py` and `scripts/addr_class_inventory.py`
all pass with no new holder or address-class site.
