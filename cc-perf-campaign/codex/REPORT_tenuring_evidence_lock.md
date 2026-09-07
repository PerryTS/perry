# Tenuring steady-evidence lock report

Code SHA: `d229d7d715cee4725776a83fa210ac5727ce1862`

Branch: `perf/tenuring-evidence-lock`

## Lock map before and after this change

The copying collector builds the cohort-scoped signal in
`crates/perry-runtime/src/gc/copying.rs:621-640`: `eden_copied_bytes` counts
fresh Eden objects copied into the survivor space, while
`survivor_first_round_live_bytes` counts age-1 objects from that same cohort
which return alive on the next copying minor. The completed minor passes those
values to `retune_after_scavenge` at `copying.rs:1895-1909`.

`crates/perry-runtime/src/gc/tenuring.rs:600-639` swaps the current
`eden_copied_bytes` into `PREV_COPIED_BYTES`. A rated round is, as before, a
cycle with `prev_cohort_copied > 0`: a previous copying minor admitted a fresh
cohort, so this minor can divide its `first_round_live_bytes` by that cohort's
intake. Before this change that single condition was also sufficient to make
the first startup cohort eligible for the lock. That is why TN4's startup
cohort qualified: the first minor copied it, the second minor returned it
nearly intact, and the code had no process-phase evidence.

The entry decision is now at `tenuring.rs:676-694`. A rated round contributes
only when it is post-startup, its previous fresh intake is at least the existing
`desired / 4` substantial-volume bar, and its survival is at least the existing
90% bar. Any rated round that fails those conditions resets
`PROMOTE_LOCK_STREAK`. `PROMOTE_LOCK` latches only when that streak reaches
three.

Startup is the first two **rated survivor cohorts**
(`tenuring.rs:195-205, 613-627`), not a wall-clock duration and not the
allocation-census flag. This uses the lock's own threshold-invariant evidence
and cannot re-pace collection. The allocation census is deliberately not the
marker: it seeds halfway to the first nursery cap, before the first copying
minor, so it was already true for precisely the TN4 startup cohort that must be
excluded. Two excluded ratings cover the observed two-minor startup phase; K=3
is the smallest steady window that rejects a one-off or two-cycle phase
boundary while still reaching S=1 within five rated cohorts on a truly
non-dying workload (two startup plus three deciding rounds).

The unlock path remains semantically unchanged at `tenuring.rs:642-664`:
while locked, substantial Eden influx holds S=1; two consecutive cycles below
`desired / 4` clear the lock and resume at S=2. The only added bookkeeping is
clearing the entry streak when unlocking, so stale entry evidence cannot be
reused after a later phase change.

`seed_promote_lock_from_sweep` remains the same two-condition census rule
(occupancy computes S=1 and Eden survival is at least 90%), but
`tenuring.rs:838-890` now refuses even a qualifying census while fewer than two
survivor cohorts have been rated. After startup it still latches immediately
from those two conditions and hands off to the unchanged unlock path.

## Prices recorded, not yet used to decide

The 90% constant was not replaced. TN4 does not contain the two aligned unit
prices needed to validate the structural inequality, so changing the decision
rule now would merely substitute another assumption.

Under `PERRY_GC_DIAG` only, each copying minor accumulates total copied and
promoted bytes (`copying.rs:1817-1824`, `instruments.rs:350-371`). The existing
cumulative minor pause is the copy-side numerator; cumulative
`step_us + remark_us` is the promote-side numerator
(`instruments.rs:391-402`). TN5 can therefore compute:

```text
copy_cost    = copy_pause_us / tenuring_copied_bytes
promote_cost = promote_us / tenuring_promoted_bytes
keep aging while mortality > copy_cost / promote_cost
```

The new byte atomics are not touched when diagnostics are off.

## Diagnostic format

Every adaptive S transition now has this format (`tenuring.rs:910-939`):

```text
[gc-tenuring] survivals FROM -> TO (REASON, eden_live_bytes=N desired=N rounds_rated=N streak=N survival_permille=N copied_bytes=N startup=true|false copy_pause_us=N tenuring_copied_bytes=N promote_us=N tenuring_promoted_bytes=N)
```

For a sweep-seed transition, `copied_bytes` is the live Eden cohort the next
minor would otherwise copy; the adjacent `sweep-seed` line prints live/dead,
the two-condition verdict, and `startup=`. The process-exit block is
(`gc/mod.rs:1410-1419`):

```text
[gc-time] wall_us=N step_us=N remark_us=N minor_us=N full_sync_us=N share_permille=N copy_pause_us=N tenuring_copied_bytes=N promote_us=N tenuring_promoted_bytes=N
```

## Tests and explicit sabotages

- `startup_shaped_survivors_do_not_contribute_to_the_lock_streak`: remove the
  `!startup` entry conjunct; the two startup ratings plus the first steady
  rating reach K and latch S=1.
- `k_steady_fully_surviving_rounds_latch_promote_on_first_copy`: raise K or
  stop advancing the streak; the exact-K final round fails to latch.
- `mortality_inside_the_steady_window_resets_the_lock_streak`: retain the
  streak on a below-bar rated round; the final surviving cohort becomes the
  cumulative Kth and latches S=1.
- `sweep_seed_cannot_latch_from_a_startup_census`: remove the sweep startup
  conjunct; a census satisfying both original conditions immediately latches.
- Existing pinned-S coverage and
  `occupancy_may_not_claim_the_ceiling_before_any_round_is_measured` were left
  in place. They were not executable locally because of the disk gate below.

## Gates

Not run (zero tests/builds executed):

- `cargo test -p perry-runtime --release --lib -- --test-threads=1`
- `cargo build --release -p perry-runtime --features wasm-host`
- `cargo build --release -p perry`

Reason: `df -g /` was below the binding 12 GB floor before every possible
Cargo invocation. It was polled every 60 seconds for 30 minutes, declining
from 2 GB to 0 GB free. No Cargo command was invoked.

Non-Cargo checks run and green:

- `rustfmt --check` on all four changed Rust files
- `git diff --check`
- `python3 scripts/gc_runtime_root_holders.py`
- `python3 scripts/gc_runtime_root_holders.py --self-test`
- `scripts/check_file_size.sh`

The new pointer-free TLS counters have explicit custody verdicts. The
`PASS1_MARKED` non-moving-snapshot pin was re-audited and refreshed because
`gc/mod.rs` changed only in the process-exit diagnostic path, outside its
mark-complete to sweep-entry window.

## Exact perrymaster request and falsifiable predictions

Request TN on **main6 + this branch**: arms `unset`,
`PERRY_GC_TENURING_SURVIVALS=1`, and
`PERRY_GC_TENURING_SURVIVALS=2`; workloads 3300 and 400 characters; two rounds
per arm/workload; four turns per process with graceful exit. Capture complete
`PERRY_GC_DIAG=1` output and run both `tn_summary` and `tenure_an`, including S
history, transition evidence, promoted bytes by turn, peak/settled RSS, and the
four cumulative price counters.

Predictions to falsify:

- On cc, `unset` shows no startup latch: S-history is S2 only, or any latch is
  after a post-startup three-round steady window.
- Turn 1 equals pinned S2 within noise; turns 2-4 equal pinned S2.
- Promoted bytes in turns 2-4 are approximately 12 MB.
- Peak and settled RSS match the pinned-S2 arm (within the allowed 1-10% RSS
  range).
- `k_steady_fully_surviving_rounds_latch_promote_on_first_copy` is the kill
  condition proving the adaptive S1 lock still exists for non-dying workloads.
