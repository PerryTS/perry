The megamorphic write stub is 2-way set-associative, and the computed-key
**write loop is now faster than node**: 40 → 20 ms against node's 24 ms on the
same host (−50%), with the combined overwrite loop 70 → 48 ms (−31%).

A direct-mapped table cannot hold a colliding pair *at all*. The two keys evict
each other on every rotation through the key set, so both miss forever and
neither ever stabilises — the miss is not probabilistic, it is permanent. On
the 500-key write loop that left ~87k of 600k writes falling through every
cache into the full `[[Set]]` walk, which the call graph put at **11.9% of the
program**, essentially all of it (10.7%) inside `js_put_value_set`.

The cause was invisible in a profile and only showed up in counters: probes
were missing with the RIGHT shape token and the WRONG key in the way. Dumping
the colliding pairs named them outright — `"k10"`/`"k115"` and `"k11"`/`"k125"`,
landing on indices 3885 and 2220.

Two ways per bucket at the same total capacity (2048 × 2 rather than 4096 × 1),
so the table does not grow. A sweep of the working set puts the worst bucket at
exactly two, so two ways absorb the collisions that exist rather than merely
reducing them. Insert refreshes a resident key, then fills an empty way, and
only otherwise evicts, shifting way 0 down so the most recently primed key
survives.

Suite: 2779 passed, 0 failed. Computed-key differential output is byte-identical
to node.
