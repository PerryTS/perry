Guard canonical-key trie pruning at minor collections with regression tests
for copying and fallback minors, same-cycle promotion, live children of dead
parent backings, and repeated re-interning of 2,048 short-lived shapes under
forced evacuation. A test-only sabotage skips the minor trie prune and must
turn the bounded-entry witness red.

The base revision already prunes this weak trie at minors: `young_prune: None`
selects the ordinary prune on every collection, with the current collector's
dead-owner predicate. Clarify that contract rather than introducing a second
pruning mechanism, young-entry log, latch, or registry. Production collection
behavior is unchanged.

The existing `PERRY_GC_DIAG=1` exit summary also reports agent-local trie edge/address
counts and reserved node/index payload bytes (excluding hash-table control
bytes and allocator overhead); backing element bytes remain a separate metric.

Validation: all five focused tests pass with `PERRY_GC_FORCE_EVACUATE=1`;
the full runtime unit suite passes (5,294 passed, five ignored). The sabotage
retains 4,098 edges and 2,049 published lists where an ordinary minor retains
only the sentinel's two edges and one published list. This is regression
coverage, not an RSS optimization: setter/transition-cache carrier retention
requires a separate ownership-policy change.
