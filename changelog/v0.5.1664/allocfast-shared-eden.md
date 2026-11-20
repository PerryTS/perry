Unify small runtime allocations with the existing generated Eden bump state
and write the complete young GC header with one aligned native word store.
Keep refills, collections, free-list reuse and large births out of line, and
flush collector bumps before leaving the existing allocation-accounting guard.
The Eden thread-exit walk also materializes its pending bump before finalizing
owned payloads. No pacing decisions, root rules or region descriptors change.

Object.create carries its resolved birth shape through a successful
no-collect allocation, avoiding redundant shape validation, a temporary zero
shape stamp and a no-op layout lookup. Zero-bound births skip hidden floor
initialization; tracking and wide births still initialize every traced slot.
Array growth retains geometric capacity growth and forwarding identity, but
roots its source only on the collecting branch and avoids nursery barrier replay
and repeated source classification. Header facts also skip metadata transfer
for arrays that cannot own a record. No cache, side table or latch is added.
