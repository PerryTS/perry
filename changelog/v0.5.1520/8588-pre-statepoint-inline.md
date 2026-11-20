Added a scoped pre-statepoint inline budget for helpers that must cross Perry's
RS4GC boundary. The proven nonnegative-index method clone is admitted before
calls become statepoints, while historical force-inline sites remain ordinary
LLVM hints under native roots; non-RS4GC behavior is unchanged.

On the `codehz/ecs` 10,000-entity query, an 11-pair contended-host A/B against
current `main` reduced paired medians by 2.22% for read-only iteration and 1.92%
for accumulation. The candidate won 10/11 and 11/11 pairs respectively, shrank
the executable by 148,664 bytes (1.264%), and all 22 processes passed the query
assertions and exact 50,005,000 accumulation oracle.
