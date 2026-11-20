### Fixed: arena-growth pacing escalated on allocation volume, not on unreclaimed bytes

`gc-handoff/bench/tree.ts` and `tree_wide.ts` — the two slowest programs in the GC
benchmark corpus — spent **more than half their runtime in full mark-sweeps**, and the
copying minor that would have reclaimed the same bytes was never attempted. All 40 of
each program's collections were fulls (`PERRY_GC_TRACE=1`:
`{'full': 40}`, `copying_nursery.eligible: false`, `fallback_reason: "not_attempted"`).

**Root cause.** `arena_growth_full_escalation_due` escalates a minor to a full once the
arena reading passes `max(PERRY_GC_MAJOR_PACING_FLOOR_MB, K × baseline)`. The baseline
(`GC_LAST_FULL_ARENA_IN_USE_BYTES`) is measured *after* a full, so it is LIVE bytes; the
reading was `arena_in_use_bytes()` sampled when the trigger fires, which is ALLOCATED
bytes — the entire un-collected nursery, nearly all of it garbage a minor is about to
reclaim for free. The two sides of the comparison were different kinds of quantity.
`tree`'s nursery high-water is 37.7 MB against the 32 MB floor, so every cycle escalated,
and the escalation perpetuated itself: `note_copying_minor_young_survival` is the only
thing that can widen the band, and it runs only when a copying minor runs.

**Fix.** The escalation now tests the arena occupancy recorded at the **end of the last
collection** — the same kind of quantity as its baseline, and precisely what the
escalation exists to detect: bytes a collection could not reclaim. Array-growth
forwarding stubs, the hazard the escalation was written for, pin their blocks through a
non-moving minor and so remain in that reading and still escalate; nursery garbage does
not. Recorded once per cycle in `GcCycle::publish_reclaim_outcome`, the one site both
collection kinds pass through.

**Measured** (absolute seconds, quiet M1 mini, best-of-5, exit-checked, window verified
quiet at both ends; 22 programs byte-identical to `node --experimental-strip-types` and
exit 0 on both arms before timing):

| bench | main | after | delta | node |
|---|--:|--:|--:|--:|
| `tree` | 1.1671 | **0.5947** | **−49.0%** | 0.450 |
| `tree_wide` | 1.6468 | **1.0747** | **−34.7%** | 0.896 |
| every other of the 22 | | | within ±0.7% | |

`ns` per GC-managed allocation on `tree`: **55.65 → 28.36**. `churn_alloc` (12.03 → 12.02)
and `retain` (89.17 → 89.37) are unchanged *by construction* — identical cycle counts,
identical collection kinds, and identical `promoted_objects + copied_objects`
(47,707 and 2,358,760) on both arms.

**Peak RSS falls**: `tree` 57.0 → 45.0 MB (−21.0%), `tree_wide` 82.1 → 70.3 MB (−14.4%),
every other program within ±0.2%. A full mark-sweep's non-moving reclaim leaves the arena
fragmented; the copying minor returns whole blocks.

The GC trace now emits `major_pacing.escalation_reading_bytes` — the left-hand side of the
comparison, previously invisible, which is how a post-full live baseline could be tested
against a pre-collection allocated reading with nothing in the trace saying so. The new
unit test asserts **both** directions: an arena the last collection emptied must not
escalate, and an arena still at the floor after a collection must still escalate — only
the pair distinguishes this from switching the escalation off.
