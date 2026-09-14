A synchronous full over a heap of promoted JSON trees costs about half as much,
and a full can now be paced by the bytes promoted since the previous full
(#10182). Stacked on #10220.

**Cheaper full** (each change has a planted test and a sabotaged twin):

- **Runs expanded only where the sweep reshapes.** `GcCycleState::new_full`
  no longer expands every described promoted page run.
  `PendingOldUnregister::defer` now expands the runs on a dead object's pages
  before it invalidates that object.
- **One-pass census.** An unbounded census hands each arena block to
  `census_whole_block`, which parses headers and start bits in one loop.
  - A block the preceding minor stamped while promoting it in place is adopted
    from a record built during that stamping (`gc/trace/adopt_census.rs`),
    without being read again.
  - Adoption happens only in a promoted-cohort full that follows that minor
    at the same safepoint.
  - A census block is found through a direct-mapped 1 MiB window index.
- **One-pass sweep and fewer hole scans.** An unbudgeted sweep walks each
  block in one pass. The hole-list rebuild skips live blocks the census proved
  hole-free, filtering the block list in place.
- **Cheaper mark.** The mark reads its per-object facts (proxy tracing, weak
  holder) once per object. It sets a pointer-free object's mark bit without
  queueing it. It prefetches ahead in the worklist and before a range
  descriptor's children. `GcMutableSlot` classifies `external` lazily.
- **Stack scrub.** Between the census and the root scan, a full zeroes 16 KiB
  of dead stack below its own frame. Census residue there had become one extra
  conservative root. That root kept a dead 7 MB string alive and cost
  `records_array_8m:roundtrip` +30 MiB peak RSS.

On #10220's probe (one 20 MB tree in the old generation, `gc()` in a loop), a
steady full drops from 38 ms to 25 ms: census 4.9 → 2.4 ms, mark 25 → 18 ms,
sweep 7.2 → 3.3 ms.

The probe's pacing full drops from 80 ms to 27 ms: rebuild 23 → 0 ms, sweep
21 → 4.4 ms. On base that full fires at an allocation point, with a forced
conservative scan. Here it is a promoted-cohort full at the safepoint, and it
fires earlier. The `gc()` that follows therefore sweeps more, but still costs
less than base's: 42 ms against 47 ms.

**Promoted-cohort fulls** (`gc/promoted_cohort.rs`):

- **Trigger.** A copying minor at a precise safepoint runs a full right after
  it when the bytes promoted since the last full reach
  `max(nursery cap, old live at the last full << shift)`. That full uses the
  safepoint's roots, with no forced conservative scan.
- **Backoff.** A full that reclaims less than half its cohort raises `shift`
  (capped at 3). A productive full resets it to 0.
- **Old-reclaim baseline untouched.** The cohort does not change that
  baseline, so both baseline tests #10204 broke still pass.
- **Diagnostics.** `PERRY_GC_DIAG=1` prints `[gc-promoted-cohort]` and adds
  `promoted_since_full=` / `cohort_bound=` to `[gc-trigger]`.

On a real cohort full in `records_array_20m:parse` the pause is 30–32 ms, down
from the 62–68 ms #10220 measured.

**Cohort survival feed** (`gc/promoted_cohort/survival.rs`):

- **Defect.** Every full resets the untraced-promotion budget, so a cohort
  full every ~live bytes kept `14_grow_then_churn` promoting its churn
  untraced for good. No minor measured again: 13 cohort fulls, 0 copied
  objects, wall 429 → 594 ms against #10220.
- **Measurement.** A cohort full's sweep measures how much of what the minor
  at its own safepoint promoted is still live. Blocks walked whole report their
  live bytes; dead blocks reclaimed unwalked count as 0.
- **When it is fed.** The figure replaces the young-survival predictor (below
  950‰) only when it equals what a minor would measure. After the mark, the
  unmarked objects on the minor's dirty old pages have their dirty slots read.
  A slot pointing into the promoted blocks means a dead remembered parent
  held them, and then nothing is fed.
- **Why the gate.** The JSON rows measure 500‰ (8m scan: 333‰), because each
  minor promotes the dead previous tree along with the live one. Their tree
  arrays are exactly such dead parents. Fed unconditionally, the next minor
  evacuates both trees: `records_array_20m:parse` +20.0 % CPU / +54.9 MiB,
  `records_array_8m:scan` +14.1 % / +31.2 MiB (mini, best of 3).
- **Diagnostics.** `[gc-promoted-cohort]` adds `promoted_by_minor=`,
  `live_of_promoted=`, `survival_permille=`, `minor_view=` and `predictor=`.

**Only in-place promotions fill the cohort.** A copying minor tenures an object
only after it survived a minor, so a full scheduled for tenured bytes is
futile. `12_large_live_set`'s one cohort full was reached by 21.2 MB of tenured
bytes over a 16 MB bound and reclaimed 8.3 MB; every cohort full on the JSON
rows was reached by in-place promotions alone. The old-reclaim baseline credit
still takes every promoted byte.

Laptop, interleaved, 7 rounds (median wall / peak RSS):

| probe | #10220 | before these two changes | now |
|---|---|---|---|
| `14_grow_then_churn` | 0.44 s / 284.6 MiB | 0.63 s / 131.1 MiB | 0.29 s / 65.1 MiB |
| `12_large_live_set` | 0.26 s / 109.1 MiB | 0.28 s / 115.6 MiB | 0.24 s / 109.0 MiB |

On the mini, every JSON row's collection schedule (minors, fulls, cohort
sizes, bounds and reclaimed bytes) is identical before and after both changes,
and CPU stays within ±1.0 %, so the table below stands.

**This does not meet #10182's acceptance bar.** Interleaved best-of-3 on the
same tree, base `9a05821b9e`:

| row | base | this branch | best node/bun |
|---|---|---|---|
| `records_array_20m:parse` | 143.8 ms / 240 MiB | 209.6 / 167 | 208.0 / 220 |
| `records_array_20m:scan` | 153.7 / 240 | 218.4 / 167 | 212.0 / 225 |
| `records_array_20m:sparse` | 145.5 / 240 | 214.7 / 167 | 208.4 / 220 |
| `records_object_20m:parse` | 146.3 / 240 | 210.9 / 167 | 207.4 / 220 |
| `records_array_20m:roundtrip` | 105.0 / 260 | 117.2 / 231 | 162.1 / 261 |
| `records_array_8m:scan` | 182.0 / 189 | 227.1 / 127 | 188.7 / 110 |

- **Target rows.** All four now beat node/bun on RSS but are +0.8 to +3.0 %
  over the best CPU. Each iteration promotes its dead predecessor's tree, so
  each row runs three fulls of about 22 ms net against a lead of 58–64 ms.
- **Other rows.** `records_array_8m:scan` loses its CPU lead (4 cohort fulls),
  and `records_array_20m:roundtrip` is +12 % CPU.
- **Mechanism commits alone** (cohort trigger disabled locally): no row is
  outside ±2 % CPU in both of two interleaved matrices. The 20 MB rows read
  +0.0 to +1.8 %. `records_array_8m:parse`/`sparse` peak RSS falls from
  109 MiB to 97/98 MiB.
