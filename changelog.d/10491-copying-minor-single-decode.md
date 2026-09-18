Decode each word the copying minor visits once. A raw (untagged) word was
classified twice: `CopyingPointerSet::decode_bits` classified it only to
validate it, and `mark_addr` classified it again. Every traced shaped object
visits its shape record's `keys` word, a raw address, so that was a second
page-table probe and header read per traced object. The slot visit's
remembering arm then re-decoded the slot it had just decoded. The validating
classification is now the one the mark uses (the memo is still consulted after
it, as before), and the remembering arm reuses the child the visit decoded;
only a raw word that moved is validated again, which is all the re-decode could
still reject.

Two codegen facts are load-bearing and pinned by comment. The decode is
`#[inline(always)]`: out of line, its call frame and the by-memory return of
its result cost as much as the classification it saves (the first cut measured
flat to +1.05%). And `barrier_parent_needs_remembering` is asked before the
visit rather than after. It reads only the parent and the slot's address, so
the order does not change the answer, but asked after, the optimizer
duplicated the call into both decode arms and stopped inlining it, which gave
back a third of the win on gc3 (-1.20% instead of -1.80%) and more than a third
on w20000 (-0.84% instead of -1.44%).

Measured on six GC fixtures, instructions:u min-of-5: gc3 -1.83%, w5000 -1.77%,
w20000 -1.52%, oldyoung -1.46%, w1000 -0.84%, alloc flat (-951 instructions).
Exact instruction counts under callgrind agree: gc3 -1.79%, with
`classify_arena` calls down from 6.09M to 4.20M. On the pointer-slot control
(60k records whose K fields all point at one shared object, against the same
records holding doubles) the per-slot term falls from 379.2 to 349.6
instructions at K=16, counted exactly under callgrind: a memo hit no longer pays a call to
`mark_addr`, and the re-decode's classification is gone.

The page-generation cache was read before any of this was attempted. It runs
the direct-mapped table arm with a 93.4-97.3% hit rate, and at most 0.02% of
lookups are capacity misses. Nearly every miss is an address in no registered
block: the shape record's `keys` slot, which lives outside the heap. So the
cache's size was not the problem, and nothing here changes it.
