The previous record-output paired run recovers small lazy-parse RSS but raises
8 MB record-array parse peak RSS from 88.17 to 102.48 MiB. Building the native
tape before versus after collection is not a complete memory solution.

The source exposes a concrete avoidable overlap: TapeScratch.entries grows a
Vec<TapeEntry>; alloc_lazy_array then calls json_tape_store::allocate to allocate
and copy another exact-size tape while the Vec is still live. Scratch capacities
over 1 MiB are discarded afterward, so large inputs cannot amortize that Vec.
Each TapeEntry has two u32 values and a u8 kind (12 bytes with its padding).

An ownership-transfer experiment can eliminate the detached copy for large
lazy-array tapes. Take the completed Vec from scratch, convert it into an exact
boxed slice, and transfer that allocation into TapeSideAllocation. Preserve
external-side byte accounting, finalizer ownership and old-generation header
placement. Preserve the input root across any pressure hook and all existing
allocation-point rooting. Small cached scratch and eager/deep fallback routes
need their existing behavior. Into-boxed-slice can shrink/reallocate, so the
benefit must be measured; do not assume the conversion itself is free.

Tests must cover exact-once ownership/release, malformed input after capacity
growth, retained independent lazy results, materialization releasing the tape,
thread teardown, and forced collection during header/cache construction.
Compare all CPU/RSS rows, especially 1/8/20 MB churn and retained outputs. This
is proposed work only; no adoption API or tape-store change is implemented yet.

For stringify, the new small-record profile points to string-piece planning,
plan copies and per-call prototype/key checks. Optimize validated input access
without caching user output or allowing stale prototype/descriptor decisions.
The empty-object parse leaf described in the earlier review is still pending.
