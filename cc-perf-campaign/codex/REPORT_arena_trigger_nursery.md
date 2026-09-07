# ArenaBytes excludes the copying nursery

Implementation SHA: `008fa075d542d9c09646a974593db03c6023ebc0`

Branch: `perf/arena-trigger-excludes-nursery` (push target: `fork` only)

## Mechanism map

- `crates/perry-runtime/src/arena/block.rs:1008-1064`: `ARENA_TOTAL_BYTES`
  is the cached reserved capacity of every arena region. The new adjacent
  `NURSERY_RESERVED_BYTES` is the reserved-capacity share owned by Eden and
  both survivor semispaces. Initial/fresh blocks update both counters according
  to `HeapGeneration`; reset/release and quarantine detach subtract in the same
  place as their cost.
- `crates/perry-runtime/src/arena/walk.rs:321-338`: `arena_total_bytes()` is
  still the whole-arena diagnostic/RSS quantity. `old_space_total_bytes()` is
  `arena_total - copying_nursery_reserved`, i.e. long-lived plus old reserved
  capacity. The `[gc] blocks` `general` count is Eden; `non_general` combines
  both survivor semispaces with long-lived and old, so neither printed bucket
  alone was an old-space quantity.
- `crates/perry-runtime/src/arena/promote.rs:334-357`: in-place promotion moves
  blocks from Eden/survivor ownership into `OLD_ARENA`. It now subtracts the
  transferred capacity from the nursery counter without changing
  `ARENA_TOTAL_BYTES`, making promotion visible as old-space growth.
- `crates/perry-runtime/src/gc/policy.rs:83-124,3144-3165`:
  `next_arena_trigger_base()` is the armed `GC_NEXT_TRIGGER_BYTES` value (or
  the device-derived absolute ceiling before the first arm). The ArenaBytes
  predicate now compares `old_space_total_bytes()` with that base. Nursery
  occupancy remains owned by the independent `young_scavenge_cap_due()` arm
  and is handed to a copying minor.
- `crates/perry-runtime/src/gc/heap_budget.rs:119-132` and
  `crates/perry-runtime/src/gc/policy.rs:2316-2508`: rebaselining uses the same
  old-space capacity. Its default headroom floor remains exactly 16 MiB (scaled
  only by the pre-existing constrained-device policy); adaptive step, ceiling,
  and promotion-runway credit are unchanged. `next_base` is therefore based on
  `old_total`, never nursery capacity. This is a unit change, not threshold
  tuning.
- `crates/perry-runtime/src/gc/policy.rs:1570-1637,1831-1839`:
  `old_reclaimable = old_gen_in_use - old_free`. Its baseline is the last
  reclaim/growth decision's old occupancy; proven-live promotion is credited
  to it so the signal remains reclaimable growth rather than absolute old
  occupancy. ArenaBytes no longer needs to duplicate that evidence gate.
- `crates/perry-runtime/src/gc/arena_right_size.rs:142-176` and
  `crates/perry-runtime/src/gc/idle_reclaim.rs:406-450`: idle right-sizing now
  evaluates old-space live bytes against old-space capacity. The existing
  sustained <=50% utilization rule, two-full bound, hysteresis, and idle-window
  `arena_right_size` start remain unchanged.
- `crates/perry-runtime/src/gc/oldgen.rs:1405-1427,1487-1650` and
  `crates/perry-runtime/src/arena/walk.rs:118-195`: a budgeted full's arena
  cursor includes Eden, both survivor regions, long-lived, and old blocks. The
  sweep therefore visits nursery objects and accounts dead Eden bytes in
  place; block cleanup calls `arena_reset_empty_blocks` and survivor reclaim.
  `crates/perry-runtime/src/arena/reset.rs:173-251` shows what the subsequent
  copying minor normally does instead: after rewriting survivors it resets
  Eden plus the active survivor from-space and flips semispaces. Thus the
  measured major was doing nursery-death work before the next minor could.

## Change

The selected structural fix is to exclude all reserved copying-nursery
capacity from ArenaBytes arming and from every consumer of that trigger's
units: rebaseline, debt scaling, tiny-parse pressure/bump, and the explicit
memory-pressure clamp. The old-space ownership boundary is exact and already
has a separate nursery-cap consumer; adding a second reclaim-evidence policy
would overlap `old_reclaimable` and make arming depend on two baselines.

`old_space_live_allocated_bytes()` subtracts the running from-space live census
from the running whole-arena live census, so #9838's tiny-parse guard no longer
treats a large nursery as pressure. The explicit safepoint deferral slack valve
remains whole-arena by design because it bounds RSS growth while a collection
waits; it does not arm ArenaBytes.

With `PERRY_GC_DIAG=1`, `[gc-trigger]` now prints `arena_total`, the actual
`old_total` compared, and `nursery_excluded`. `[gc-budgeted] start` prints an
`arming_reason` plus the same totals. `[gc-arena-rebaseline]` prints the
old-space total and excluded nursery capacity on one complete line.

## Sabotage-able tests

- `crates/perry-runtime/src/gc/tests/arena_trigger_old_space.rs:57-115` fills
  dead nursery objects to the real cap, proves whole-arena bytes crossed the
  base while old bytes did not, asserts the incremental-start counter does not
  move, then proves the precise copying-minor counter advances and from-space
  shrinks. Restoring `arena_total_bytes()` in the ArenaBytes comparison starts
  budgeted work and fails the status/counter assertion.
- `crates/perry-runtime/src/gc/tests/arena_trigger_old_space.rs:119-181`
  transfers more than the unchanged headroom from nursery to old with the
  nursery empty, then asserts exactly one ArenaBytes budgeted start. Removing
  the nursery-counter transfer from `finish_in_place_promotion` keeps the old
  quantity below the base and fails the start counter.
- `crates/perry-runtime/src/gc/tests/idle_reclaim.rs:153-226` retains the
  bounded idle-window right-size test and names its sabotage: deleting the
  `arena_right_size::owed()` start arm prevents the dedicated
  `arena_right_size_starts` counter from advancing.
- Existing trigger, rebaseline, debt-pacer, copying-survival, host-safepoint,
  arena-right-size, and idle-reclaim coverage was updated only where its unit
  is now old-space capacity.

## Validation

No Cargo command was invoked. The required pre-Cargo `df -g /` check reported
0 GB available (97% capacity), below the binding 12 GB floor, so the build and
test gates were not run and no build lock was acquired or waited on.

Non-Cargo checks run:

- `rustfmt --edition 2021` on every modified Rust file: passed.
- `git diff --check` (working tree and alternate-index commit): passed.
- `bash scripts/check_file_size.sh`: passed (`OK: no Rust source files exceed
  2000 lines`).

Not run locally; request these perrymaster gates from the pushed SHA:

1. `cargo test -p perry-runtime --release --lib -- --test-threads=1`
2. `cargo build --release -p perry-runtime --features wasm-host`
3. `cargo build --release -p perry`
4. Coordinator label: `run-extended-tests` (GC-adjacent change).

## Predictions and exact perrymaster request

Prediction: budgeted cycles armed by ArenaBytes while `old_reclaimable` is at
baseline fall to 0 per run; in-turn `[gc-charge] budgeted-done total_us` falls
from 169-213 ms to approximately 0; minors per turn and `old_in_use` are
unchanged; idle-window reclaim is unchanged. The expected paired bound is
approximately -2% at 3300 characters from removing about 50 ms/turn, larger at
400 characters, with peak and settled RSS unchanged. RSS +1-10% remains
acceptable. Because the change removes majors that a larger nursery was
arming, the NS2 n64 arm needs a runtime-only rerun to determine whether its
previous +13% peak moves.

Exact request:

> From the pushed SHA, run the three release gates listed above and apply the
> `run-extended-tests` label. Relink that runtime on the I7-view tree
> (runtime-only). Run one 4-turn graceful 3300-character reply and one
> 400-character reply with `PERRY_GC_DIAG=1`; use `ns2_majors_an.py` and
> `ns2_alloc_an.py`. Verify ArenaBytes budgeted cycles with
> `old_reclaimable` at baseline are 0/run, in-turn `[gc-charge]
> budgeted-done total_us` moves from 169-213 ms to approximately 0, minors per
> turn are unchanged, `old_in_use` is unchanged, and idle-window reclaim is
> unchanged. Then run paired 5x3300 + 3x400 versus I7-view; expect about -2%
> at 3300 from the roughly 50 ms/turn removal, a larger win at 400, and
> unchanged peak/settled RSS. Finally rerun the NS2 n64 arm on this runtime to
> see whether its +13% peak moves now that the larger nursery no longer arms
> those majors.
