**The `10_store_receiver_across_alloc` GC-ratchet probe was running no
collection at all, and has been given margin** (#9833, fixes #9832).

The probe exists to catch a store receiver held in a register across an
evacuating minor — the stale-root class of #6970 / #9523 — and its own header
lists three conditions that must all hold for it to bite, the third being an
allocating right-hand side. On `main` it reported `minor_cycles = 0`: no minor
ran, so no evacuation happened, so there was no window and the probe measured
nothing. `freed_bytes = 0` alongside `copied_objects = 0` rules out "a minor ran
and found nothing live".

The cause was margin rather than a bug. At 200,000 iterations the probe crossed
the nursery threshold exactly once, and #8313 — shrinking a two-field object
from 56 to 40 bytes — put it under. A probe that fires exactly one collection is
one optimisation away from firing none. It is now 2,400,000 iterations, which
measured 9 minors and keeps several after a further eightfold reduction in bytes
per object, for about 120 ms on `wall_ms`, which the gate does not band.

Verified by sabotage rather than by the counter moving: removing the allocating
RHS returns `minor_cycles=0 copied_objects=0 freed_bytes=0`, the exact signature
the probe had while broken.

`heap_used_bytes` returns from 464,072 to 244,648 against a pinned baseline of
220,384 — the +110.57 % that cell showed on `main` was the post-`gc()` residue
of a run in which nothing was ever collected, not retention.

Five further probes (`01`, `02`, `03`, `09`, `11`) currently sit at
`minor_cycles == 1` and are one allocation win away from the same silent state;
that is recorded in #9832 and not addressed here.
