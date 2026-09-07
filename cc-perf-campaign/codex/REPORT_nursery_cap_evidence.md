# Nursery-cap RSS evidence report

Code SHA: `c18648332347cd4314e8d7a3255aa1223ab5e44a`

Branch: `perf/nursery-cap-rss-evidence`, stacked directly on
`a771c33fce419fc04c1a811fd797f63e40cd8cbe` (#9949).

## Code facts

### Object denomination ends with the first completed copying minor

The exact regime signal is the new per-thread `COPYING_MINOR_COMPLETED` cell in
`gc/tenuring.rs`. `note_surviving_object_census` sets it before either
zero-census early return. That call is made only after a copying attempt has
finished evacuation and immediately before the completed minor is retuned and
reported. It therefore flips on the first completed copying minor even if that
minor moved zero objects.

This is deliberately not `OBJECT_CENSUS_SEEDED`: #8122's allocation walk sets
that before the first minor and must still denominate that first trace. It is
also deliberately not `SURVIVOR_ROUND_MEASURED`: that signal cannot become
true until a cohort admitted by one minor returns on a later minor, so it is
one minor too late.

Before `COPYING_MINOR_COMPLETED`, `influx_driven_nursery_cap_bytes` retains the
existing 72 B reference, integer per-mille arithmetic, 500 per-mille floor and
one-sided 1000 per-mille clamp. Afterwards it returns exactly
`base * NURSERY_CAP_SCALE`. `note_surviving_object_census` still updates the
mean. Its diagnostic says `applied=false`; the pre-minor allocation-census
diagnostic says `applied=true`.

There is no second production consumer of
`nursery_cap_object_scale_permille`: outside the cap's pre-copy branch, its
remaining calls are diagnostics and tests. There is likewise no production
reset of `NURSERY_CAP_SCALE`; its only reset is inside `reset_for_test` under
`cfg(test)`. The idle right-size does not reset it.

The #7929 doc comment measured the cost of tracing/moving many small objects on
`deeplist` and `retain1`. Rule 1 preserves that measurement for the first
tracing band, including #8122's allocation seed. It deliberately does not
claim that survivor-object size prices the fixed scanner/remembered-set cost of
later copying minors. `deeplist`, `retain1`, and `tree_wide` are consequently
the probes most likely to expose a regression if that distinction is wrong.

### Cap-due evidence is captured before evacuation

`copying.rs::run_copied_minor_attempt` captures a `NurseryCapRating` before
eligibility checks or heap mutation can rewrite from-space. The snapshot holds
the current `copying_from_space_in_use_bytes`, the effective cap, and the
verdict returned by `policy::young_scavenge_cap_due()`. This is the same exact
comparison used to arm a cap collection; it does not infer anything from the
diagnostic trigger label. The label can still be `ArenaBytes` because the
effective arena trigger folds in the young cap.

The completed attempt passes that snapshot as the fourth argument to
`retune_after_scavenge`, which passes it to `retune_nursery_cap_scale`.
Speculative attempts may take a snapshot, but only the attempt which completes
reaches the retune. A false verdict returns before touching either
`NURSERY_CAP_SCALE` or `CAP_GROW_STREAK` and emits one diagnostic:

```text
[gc-tenuring] nursery cap scale: not rated (from_space=N cap=N)
```

An influx transition now carries all attribution:

```text
[gc-tenuring] nursery cap scale 1x -> 2x (why=influx cap_due=true from_space=N cap=N eden_live_bytes=N)
```

The 4% threshold, two-rated-minor debounce, one-step `x2` growth, and `x4`
ceiling are unchanged. The survival-rate lock, occupancy rule, and
tenured-proportional cap term are unchanged.

### No readable RSS-pressure predicate exists on this base

Rule 3's shrink path is absent, as the design requires when no readable
predicate exists. `js_gc_memory_pressure` lowers `GC_NEXT_TRIGGER_BYTES` to
`arena_total + 1 MiB`, but neither that write nor the cell records its
provenance. `GC_TRIGGER_ARMED` is also set by ordinary re-arms and parse bumps.
Warning pressure may collect synchronously and have the normal finisher
overwrite the lowered trigger before any rated minor. Critical pressure sets
`GC_OLD_RECLAIM_PENDING`, but ordinary old-generation reclaim pressure sets
the same cell. The RSS evacuation threshold is an evacuation policy input, not
the arena-trigger clamp's active-state predicate.

Using any of those as “pressure active” would invent or conflate a signal.
Therefore this patch ships rules 1, 2 and 4, retains idle page release, and does
not add `nursery_cap_scale_shrinks_under_memory_pressure` or a test override
for a predicate that production cannot read.

## Fixture A replay: TN5 unset

`old_cap` is the captured trigger cap. `new_cap` is the cap computed at the
same row by this policy. The first row retains the allocation-census mean of
66 B (`916` per mille); rows 2-4 are the existing ramp. From row 5 onward the
band is 64 MiB. Every cap-due row after the ramp is therefore exactly
67,108,864 B; the old 32-42 MiB rows disappear.

| row | from-space B | old cap B | new cap B | mean after B | cap due |
|---:|---:|---:|---:|---:|:---:|
| 1 | 15,728,712 | 15,367,929 | 15,367,929 | 77 | yes |
| 2 | 17,407,296 | 16,777,216 | 16,777,216 | 84 | yes |
| 3 | 33,624,624 | 33,554,432 | 33,554,432 | 133 | yes |
| 4 | 34,829,760 | 33,554,432 | 33,554,432 | 84 | yes |
| 5 | 39,313,552 | 67,108,864 | 67,108,864 | 118 | no |
| 6 | 67,407,992 | 67,108,864 | 67,108,864 | 110 | yes |
| 7 | 67,230,456 | 67,108,864 | 67,108,864 | 61 | yes |
| 8 | 57,803,984 | 56,841,207 | 67,108,864 | 40 | yes |
| 9 | 37,882,104 | 37,245,419 | 67,108,864 | 47 | yes |
| 10 | 44,762,032 | 43,754,979 | 67,108,864 | 43 | yes |
| 11 | 40,614,704 | 40,063,991 | 67,108,864 | 36 | yes |
| 12 | 34,204,000 | 33,554,432 | 67,108,864 | 38 | yes |
| 13 | 35,524,488 | 35,366,371 | 67,108,864 | 80 | yes |
| 14 | 67,572,776 | 67,108,864 | 67,108,864 | 63 | yes |
| 15 | 59,188,240 | 58,720,256 | 67,108,864 | 45 | yes |
| 16 | 42,913,584 | 41,943,040 | 67,108,864 | 39 | yes |
| 17 | 36,981,552 | 36,305,895 | 67,108,864 | 84 | yes |
| 18 | 56,662,480 | 67,108,864 | 67,108,864 | 68 | no |
| 19 | 63,730,280 | 63,350,767 | 67,108,864 | 37 | yes |
| 20 | 34,583,744 | 34,426,847 | 67,108,864 | 39 | yes |
| 21 | 36,427,200 | 36,305,895 | 67,108,864 | 40 | yes |
| 22 | 37,347,336 | 37,245,419 | 67,108,864 | 73 | yes |
| 23 | 67,358,392 | 67,108,864 | 67,108,864 | 62 | yes |
| 24 | 58,724,472 | 57,780,731 | 67,108,864 | 38 | yes |
| 25 | 35,510,472 | 35,366,371 | 67,108,864 | 39 | yes |

## Fixture B replay: NS3 n16

The fixture code contains all 26 `(from_space, cap, eden_live)` rows. Per the
fixture contract, the four transition values are copied exactly and other
`eden_live` values are `survival_permille * from_space / 1000`. Row 9 is the
only non-cap-due row and is skipped without clearing the accumulated state.

| row | eden live B | cap due | recorded scale after row | new scale after row |
|---:|---:|:---:|---:|---:|
| 1 | 13,200,201 | yes | 1 | 1 |
| 2 | 3,208,856 | yes | 2 | 2 |
| 3 | 25,294,605 | yes | 2 | 2 |
| 4 | 9,684,456 | yes | 4 | 4 |
| 5 | 6,815,672 | yes | 4 | 4 |
| 6 | 542,697 | yes | 4 | 4 |
| 7 | 46,176 | yes | 2 | 4 |
| 8 | 34,602 | yes | 2 | 4 |
| 9 | 2,803,437 | no | 2 | 4 |
| 10 | 2,590,608 | yes | 2 | 4 |
| 11 | 32,928 | yes | 1 | 4 |
| 12 | 41,942 | yes | 1 | 4 |
| 13 | 125,733 | yes | 1 | 4 |
| 14 | 3,213,027 | yes | 1 | 4 |
| 15 | 3,102,415 | yes | 1 | 4 |
| 16 | 23,068 | yes | 1 | 4 |
| 17 | 23,068 | yes | 1 | 4 |
| 18 | 23,068 | yes | 1 | 4 |
| 19 | 459,049 | yes | 1 | 4 |
| 20 | 3,057,456 | yes | 1 | 4 |
| 21 | 3,155,564 | yes | 1 | 4 |
| 22 | 20,971 | yes | 1 | 4 |
| 23 | 20,971 | yes | 1 | 4 |
| 24 | 21,006 | yes | 1 | 4 |
| 25 | 44,094 | yes | 1 | 4 |
| 26 | 315,351 | yes | 1 | 4 |

Thus growth remains exactly at rows 2 and 4. The recorded mortality
transitions at rows 7 and 11 disappear.

## Tests and sabotage outcomes

All five shipped named tests ran and passed. The sabotage outcomes below are
the discriminating assertion each test produces:

- `steady_state_cap_is_not_object_denominated_after_first_copying_minor`:
  letting the survivor mean feed the steady cap makes row 8 return 56,841,207
  B instead of 67,108,864 B, followed by the captured 32-42 MiB dips; the
  suffix assertion fails.
- `first_minor_keeps_the_object_denomination`: deleting the pre-copy regime
  test returns the raw 16,777,216 B band instead of 9,311,354 B; the exact-cap
  assertion fails.
- `nursery_cap_scale_does_not_shrink_on_low_mortality`: restoring
  `< cap / 100` first changes row 7 from scale 4 to 2, so the all-4 suffix
  assertion fails. With the specified derived `eden_live` rows and cap-due
  qualification, later high-influx pairs can re-grow it before later low
  pairs shrink it again; the final row is 1, not 4.
- `nursery_cap_scale_still_grows_on_influx`: deleting or re-timing the grow
  branch makes the exact 26-row vector first differ at row 2 or row 4.
- `non_cap_due_minor_does_not_rate_the_scale`: deleting the early cap-due
  return makes its two forced high-influx minors grow the scale from 1 to 2;
  both the scale and zero-streak assertions discriminate.
- `nursery_cap_scale_shrinks_under_memory_pressure`: not shipped or run,
  because there is no production-readable active pressure-clamp predicate to
  override without inventing a new signal.

The #9949 startup-window, lock-entry, mortality-reset, and sweep-seed tests are
unchanged and green in both requested test gates. Existing object-denomination
integration coverage was updated only for the new completed-minor regime: the
census mean remains observable while the cap returns to its byte band.

## Design contradictions and audit notes

The requested “40 B means 500 per mille, bit-identical to today” assertion is
not compatible with the untouched arithmetic:

```text
40 * 1000 / 72 = 555 per mille
16,777,216 * 555 / 1000 = 9,311,354 B
```

The 500-per-mille floor begins at 36 B and below. The first focused run exposed
this mismatch (and the old integration assertion that a completed minor still
denominated the cap); both tests now assert the preserved rule. Changing 40 B
to 500 per mille would have re-tuned the first-minor regime forbidden by rule
1.

The design's shorthand that restoring mortality shrink makes “rows 7-8 shrink
it” describes the original diagnostic trace. Under the separately specified
test transcription (derive non-transition `eden_live` from total survival) and
the new cap-due filter, the first shrink is at row 7; row 8 starts the next
debounce, and subsequent derived high-influx rows can reset or re-grow it. The
test still fails at the first changed row and at the final scale.

No other denomination consumer or scale reset was found. No local cc run was
made. The three #7929-sensitive probes may regress if survivor denomination
was in fact pricing steady copying work there; that is precisely the
perrymaster falsifier, not a reason to hide the policy distinction in a unit
test.

## Gates

The root filesystem reported 15 GB available immediately before every Cargo
command, above the binding 12 GB floor. All Cargo commands ran through the
shared build lock with `-j4`; this sandbox rejected `nice -n 19` with
`setpriority: Operation not permitted`, so the commands continued at the
inherited priority.

- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 tenuring`:
  green, 35 passed, 0 failed, 3,241 filtered out.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1`:
  green, 3,272 passed, 0 failed, 4 ignored.
- `cargo build --release -p perry-runtime --features wasm-host -j4`: green.
- `rustfmt --edition 2021 --check` on all four touched Rust files: green.
- `git diff --check`: green.
- Every touched file is below 2,000 lines (largest: `copying.rs`, 1,998).

## Perrymaster falsifiers and predicted numbers

Stage NC should compare #9949 (`c18648332^`) with this code SHA, relinking the
runtime only on TN5's cache: alternating two runs per arm, four 3,300-character
turns with diagnostics; one 400-character turn; then the gc-ratchet ladder.

Predictions:

- By minor 6 the arm is at 67,108,864 B, and every later cap-due trigger stays
  there. Only the `1x -> 2x` and `2x -> 4x` ramp transitions appear. Survivor
  means keep changing, with `applied=false`, without moving the band.
- Four-turn copying-minor count falls from 25 to about 20-21. At the measured
  45-73 ms fixed pause per avoided minor, five avoided minors predict roughly
  0.25-0.35 s, or 2-4%, off the 8.87 s total. Minors falling without CPU
  falling falsifies the fixed-cost premise and must be reported, not averaged
  away.
- Peak RSS stays within 0-3% because the 64 MiB nursery was already reserved
  by minor 6; settled RSS after 120 seconds is equal; promoted bytes in turns
  2-4 stay within 2 MiB of control.
- The 400-character turn is flat or faster.
- Every gc-ratchet probe remains at or below baseline instructions and within
  its RSS tolerance, especially `deeplist`, `retain1`, and `tree_wide`. A
  regression in those three means steady denomination was still pricing the
  work #7929 measured; the evidence-driven fallback is to weight it by the
  preceding minor's copy-work share rather than restore it globally.

Kill the change for any probe or cc row above +10% max RSS, or for fewer minors
combined with higher CPU.
