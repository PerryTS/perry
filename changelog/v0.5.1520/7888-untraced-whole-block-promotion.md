### Performance

**A copying minor that promotes the whole young generation in place no longer traces it.**

#7742's whole-block in-place promotion stopped the *copy*. It did not stop the *walk*: the
remembered-set dirty scan still marked every survivor, the drain re-touched every marked
header, and `clear_marks` touched them a third time — three passes over a cohort far larger
than any cache, on a cycle where nothing moves and nothing can be freed. Measured per cycle
on `gc-handoff/bench/retain.ts` (`PERRY_GC_TRACE=1`, per-phase): the trace is ~55–67 ns per
promoted object and the finish walk that does the actual promotion is ~9 ns.

What the trace still produced on such a cycle turns out to be enumerable, because
`retag_young_for_in_place_promotion` takes **every** in-use Eden and survivor block:

* nothing in the heap classifies as `Nursery` afterwards, so `move_young` is unreachable and
  every root walk, slot rewrite and forwarding repair is a provable no-op rather than an
  approximation;
* no remembered-set entry can be created — that is #7744's `skip_remembering` proof, reused
  verbatim as a precondition — so clearing the whole remembered set is exact;
* the address-keyed death-pruning passes (`dead_owner::owner_is_dead`, the map/set/error
  finalizers) all require the owner to classify as `Nursery` on a minor, so they already
  prune nothing;
* which leaves the old-gen page index's live filter, and the survival ratio itself.

So a promoting cycle now skips the trace when the last *measured* ratio is in the fully-live
regime (≥ 999‰), registers **every** object on the promoted block into the page index rather
than only the marked ones, and charges the promoted bytes against a budget —
`max(128 MB, old-gen at the last measurement)` — that forces a measuring cycle before the
assumption can run away. A full collection resets the budget: its trace is a stronger answer
to the same question, and it reclaims whatever the untraced run retained.

The untraced path refuses to run when any of its assumptions is not free: a registered
weak-target holder (weak tombstoning reads marks), a non-empty malloc registry (the malloc
sweep reads marks), an incremental mark in progress (an allocate-black birth puts
`GC_FLAG_MARKED` on a nursery object, and a cycle that neither reads nor clears marks would
carry that bit into old-gen where a stale mark reads as live), or any of
`PERRY_GC_VERIFY_EVACUATION` / `PERRY_GC_FROMSPACE_SCAN` / `PERRY_GC_VERIFY_MARK` — each of
those instruments takes the trace as its subject, and a cycle that produced no marks would
let all three report success having examined nothing. `PERRY_GC_PROMOTE_IN_PLACE=0` turns
the whole family off, as before.

`PERRY_GC_DIAG=1`'s `[gc-copy-minor]` line gained `untraced=`, `untraced_cycles=` and
`untraced_objects=`, so a run that never entered the path reads as one that never entered it.
