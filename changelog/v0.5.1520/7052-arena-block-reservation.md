Closed the second instance of the #7022 aliasing violation. #7050 moved the
allocation-point GC trigger out of `Arena::alloc`'s `&mut self` borrow, but the
emergency-reclaim path kept the same shape: `alloc_after_gc` →
`alloc_fresh_block` → `install_fresh_block` → `alloc_block`, all under `&mut self`,
with `alloc_block` calling `gc_try_emergency_reclaim()` when the OS refuses
memory. That collection allocates into the arenas, so `self.blocks.push(..)` could
grow the `Vec` underneath the live borrow — the same silent memory corruption,
reachable only under heap exhaustion, which is when it is least survivable.

Block acquisition is now split by whether collecting is permitted:
`reserve_arena_block` may collect and requires that no arena borrow is live;
`alloc_block_no_gc` never collects and serves the callers that hold one (two of
which are already executing inside a collection). `Arena::install_reserved_block`
installs a block obtained from the former.

The `cfg(test)` borrow-depth probe added in #7050 now covers this path as well,
and `try_alloc_block` gained an injectable failure hook so the null-allocation
branch can be exercised deliberately instead of waiting for real heap exhaustion —
the invariant is asserted rather than assumed.
