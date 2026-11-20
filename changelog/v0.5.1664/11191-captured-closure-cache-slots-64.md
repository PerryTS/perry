### Captured-closure cache: 64 slots again, and absent hints no longer count as misses

#11167 fixed the captured-closure cache retention by dropping a literal's cache
when the literal is disabled, and also cut `MAX_CAPTURED_CLOSURE_SLOTS` from 64
to 8. The cut was not needed for the retention fix, so this restores 64 and its
sizing comment. The comment now notes that `promise_all_chains` no longer
reaches this cache (0 hits, 0 misses at 8 and at 64 slots): its async steps go
through the step-chain reuse path.

A lookup whose hint array is absent now falls through to the exact linear scan.
It used to return a miss, and that spurious miss counted toward the adaptive
bypass and could disable the literal.

Tests in `closure/alloc.rs`: 50 interleaved capture tuples all hit on the second
round; a cache with no hint array still finds its entries. Each change turns its
test red when reverted.
