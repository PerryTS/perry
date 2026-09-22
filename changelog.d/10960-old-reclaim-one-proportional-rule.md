Pace old-gen reclamation on one proportional rule, and price the full
collections it schedules.

The trigger had four pacing inputs and only one could fire. Measured per
trigger on a TypeScript transpile, the absolute first-crossing arm was true in
96% of observations, the proportional arm alone in ~1%, and both together in
none. The absolute arm is not a property of the heap: every reclaim resets the
baseline below the 48 MiB threshold, so the "first crossing" re-arms and fires
again on the next ~18 MiB of growth — 723 full collections in one transpile.
The other three inputs could not bind: the proportional band sat on its 32 MiB
floor in every observation because `baseline/2` needs a 64 MiB baseline; the
productivity backoff returned early on every old-reclaim full because the
predicate never recorded a pre-collection reading; and the retaining latch
needs 900‰ young survival against a measured 893‰ ceiling.

Delete the absolute arm, leaving one proportional rule. Record the pre-full
live reading inside the predicate so the productivity backoff finally sees
old-reclaim fulls, following the structural fix `arena_growth_full_escalation_due`
already carries. Give that backoff its own cell and a tighter cap than the
arena-growth one, because each doubling of this band is peak RSS.

On the transpile: instructions fall 57.0% (mean of five interleaved rounds,
non-overlapping ranges), full collections 723 → 236, and peak RSS falls 22.0%.
The memory result is not a pacing effect — 723 non-moving in-place sweeps
generate fragmentation faster than they reclaim it, so fewer collections also
consolidate the heap (old-gen holes 238 → 200 MB, heap-to-live 5.07× → 3.21×).

Validated on that workload plus a retaining-heap control, where the change is
bit-identical by construction because the deleted arm was already exempted
while retaining. The campaign's light fixtures run below the 48 MiB threshold
and never reach this trigger at all.
