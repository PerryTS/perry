**A declined idle compaction is no longer a terminal state: the memory reducer
re-arms on elapsed idle as well as on mutator collections, so a heap that parks
1.3 points under the compactor's residue gate gets revisited instead of holding
221 MB until the next turn.**

Measured on the compiled claude-code TUI, one 400-char turn then a 120 s idle
window, quiet host (load < 0.1), both rounds of each arm:

| arm | after turn | after 120 s idle |
|---|---|---|
| A | 757 / 759 MB | **512 / 527 MB** |
| R | 738 / 742 MB | **748 / 748 MB** |

R *ends the turn 19 MB better than A* and finishes 221 MB worse. The reclaimer's
own diagnostic says why, and it is a closed loop:

1. **The compactor's residue gate declines**, reproducibly and narrowly.
   `compaction_owed` gate 1 wants residue ≥ 25 % of old-gen occupancy; A is at
   **25.94 / 25.95 %** and starts two compactions, R is at **23.68 / 23.67 %**
   and starts none. Within-arm spread across rounds is 0.01–0.02 points: a
   stable operating point just under a threshold, not a coin-flip.
2. **The decline removes the only event that could revisit it.** The reducer's
   activity gate needs `2^backoff` collections *it did not start*, and
   `external_collections()` subtracts only the reducer's own — so a **compaction
   is what registers as external**. A's trace shows each one contributing
   exactly +1 (`external_collections` 13 → 14 → 15 across three attempts, one
   compaction between each). R stays at 9, `since_attempt` never reaches 1, and
   there is no second attempt in the whole window.
3. So the heap parks, and the largest piece of the loss is downstream of that:
   A right-sizes the arena from **182.45 MB of capacity to 81.79 MB** across its
   three observations, while R holds **168.82 MB** on one. Roughly 87 MB of
   capacity + 57 MB of young blocks + 38 MB of old-gen ≈ 182 of the 221 MB.

**The fix extends an exemption that already exists twelve lines above it**, for
the identical deadlock: `StartReason::ArenaRightSize` bypasses the same gate
because arena blocks need a second full observation that an idle mutator will
never produce (#9709). This adds `StartReason::IdleElapsed` on the same
reasoning — a requirement denominated in *mutator collections* cannot be met by
a heap whose mutator is idle, which is precisely when the reducer is wanted.

**Why the gate constant was not the fix, on measurement rather than principle.**
Lowering `IDLE_COMPACT_MIN_RESIDUE_PCT` from 25 to 23 would have let R start a
compaction — and the same R binary in a 5 s window *did* clear the gate, at
25.81 %, ran the compaction, and **released 0** (`kept_promise=false`,
`backoff_shift 0→1`). Nor is that peculiar to R: A's own second compaction
releases 0 at **54.6 %** residue. Half of A's compactions in this capture
released nothing, aborting ~4x earlier (`pause_us` 107k/161k against 442k) on
what looks like a budget. The knob is not merely forbidden; it does not work.

**Anti-spin needs no new rule.** The elapsed wait is
`IDLE_RECLAIM_REARM_MS << backoff_shift` — the *same* shift that prices the
activity arm — so an unproductive full doubles it: 15 s, 30 s, 60 s, 120 s,
240 s. And the arm is **disarmed entirely at `IDLE_RECLAIM_MAX_BACKOFF_SHIFT`**
rather than merely slowed, because five unproductive attempts establish there is
nothing to give and an idle process must not pay a whole-heap mark forever.
A productive full resets the shift, so a heap still returning memory keeps being
asked every 15 s — which is the case this exists for. `IDLE_RECLAIM_REARM_MS` is
deliberately larger than `IDLE_RECLAIM_MIN_INTERVAL_MS` so the rate floor is
never the binding constraint and the two gates cannot be confused in a diag.

Two tests, each sabotage-proved: a parked heap with **no** external collection
anywhere gets a second attempt at the wait and not before, identified by reason
rather than by attempt count; and an unproductive streak doubles the wait each
time and then stops. Removing the arm fails the first, removing the backoff
scaling fails the second's "must not re-arm before the doubled wait", and
removing the disarm fails its "at the maximum shift the elapsed arm is
disarmed".

The young half of the loss is **not** addressed here and is measured, not
assumed: after R's single reclaim, `[gc-general-reclaim] examined=66 released=0
has_live=39 aging=22` — 39 of 66 arena blocks hold a live object, against 3 of
65 in A, and only an evacuation can consolidate those. Whether an idle young
evacuation is also needed is a separate question and a separate change.
