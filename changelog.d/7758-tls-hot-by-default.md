### `perry_thread_local!`: thread-locals are on the fast path by default, and a gate now says so (#7469)

On Darwin every `thread_local!` access is an out-of-line call to `_tlv_get_addr`
in libdyld — a real call, not inlined, clobbering caller-saved registers.
`crates/perry-runtime/src/tls_hot.rs` has removed that cost three times, and it
has come back three times:

| build | workload | `_tlv_get_addr` share |
|---|---|--:|
| after #7565 | `churn_alloc` | 0% |
| later | `churn_alloc` | 8-9% |
| later | `interp` / `retain` | 11% |
| v0.5.1434 | **`asyncpipe`** | **20.5%** |

**The mechanism never decayed. The coverage policy was the bug.** `HotTls`
carried sixteen hand-wired slots against ~520 `thread_local!` declarations, and
the sixteen were curated against whichever workload was profiled last — the
allocation path. `churn` is covered by construction and reads 0% forever;
`asyncpipe` pays 20.5% through Map/Set registries, buffer brands, descriptor
state and field-lookup tails that were on nobody's list. Adding a slot took
four manual steps including a hand-written test, and *forgetting* them produced
a working slow path rather than a build error.

#### What changed

`crate::perry_thread_local!` — same syntax as `thread_local!`, same `with` /
`try_with` at every call site, so converting a declaration converts all of its
uses. The address of the value lands in a generic slot of the same per-thread
cache `hot()` already reaches with an `mrs` plus two loads (#7565), so a read
costs loads instead of a call. There is nothing to wire: no slot to add, no
provider function, no line in `fill`, no line in a test.

That also removes the hazard the old contract needed a test to catch. The
untyped named slots could hand out a correctly-typed reference to the *wrong*
object if `fill` was mis-wired; here the storage, the resolver and the key's `T`
all come from one declaration, so the mis-pairing cannot be expressed.

It is also safer on thread teardown than the mechanism it extends. `HotCell<T,
GUARD>` takes `GUARD = needs_drop::<T>() as usize` from the macro: a
`RefCell<HashMap<…>>` gets a one-element guard array whose `Drop` runs before
the value's and un-publishes this thread's cached address, so a later access
falls back and gets std's "accessed during or after destruction" panic instead
of reading a dropped map. A `Cell<u64>` gets a zero-length array, which has no
drop glue at all — no destructor is registered and std's `const`-init fast path
is preserved. The sixteen named fields have no such hook in either direction.

155 declarations across 51 files — every subsystem the two profiled programs
resolve — now use it. The sixteen named fields are unchanged and stay a closed
set: a fixed offset is one load cheaper than a claimed slot, and the allocation
path is where that matters.

#### The gates

Two, because they fail on different things.

`scripts/check_thread_locals.py` is the structural half: a new raw
`thread_local!` in `perry-runtime` is a build error unless it is recorded in
`scripts/thread_local_cold_allowlist.json` as deliberately cold. The counts are
a ratchet in both directions — a file that *loses* a declaration fails too,
because a stale entry is one nobody has to justify any more. It also fails when
declarations approach `HOT_SLOT_CAPACITY`, since slot exhaustion is correct but
silent. `--self-test` drives all four rejections.

`scripts/tls_budget_gate.sh` is the outcome half, and its design is about
vacuity. Profiling `churn_alloc` — the benchmark every previous fix was tuned
against — would pass forever while the real cost grew, because churn's
thread-locals are exactly the covered ones: a gate green because its subject
never ran (CLAUDE.md's fourth kind). So the subjects are
`benchmarks/tls-budget/asyncpipe.ts` and `interp.ts`, and
`scripts/tls_budget_check.py` refuses a pass unless the run proves it was live:
`PERRY_TLS_HOT_STATS=1` must report `direct_tsd=1` (otherwise `hot()` is itself
calling `_tlv_get_addr`, the mechanism is inert, and a low share would mean the
program resolved nothing) and `claimed` above a floor no allocation
microbenchmark clears. Its `--self-test` drives seven rejections and runs on
every PR, compiler-free.

Neither gate is wired into branch protection by this change: a new gate has
never been green, so promoting it immediately would block every open PR. That
is a maintainer action after the first observed green run on `main` — and per
CLAUDE.md's corollary, not optional follow-through.
