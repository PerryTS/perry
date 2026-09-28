Loops now guard a receiver's shape ONCE instead of at every property access
(#10884, charter step 4b). When a `for`/`while`/`do` loop reads or overwrites
own data properties of a receiver that the loop does not reassign — a
parameter, a local, a module `const`, a captured binding or `this` — the
preheader checks the receiver's shape once and the loop runs a split body: the
accesses are bare slot loads and stores while the shape is proven, and the
ordinary code (the property ICs) runs otherwise. A loop whose receiver is born
in the body (`const o = objs[k & 7]`) guards once per iteration and covers the
rest of the body the same way.

The proof follows the shape: anything in the body that can run JavaScript
ends the fresh facts (a later access re-checks the shape, or the iteration
runs the ordinary code), and an IR check discards the fast body if a
JS-capable call precedes a bare access. A guard that fails selects an
unchanged copy of the loop, so a receiver the region cannot serve pays one
compare per loop entry; a region whose shape could never be learned retires
in its word after a bounded number of attempts. Keys a receiver keeps in its
spill buffer are read through it (the S5 facts: the word carries the ShapeId
with `PACKED_SPILL_FLIP` flipped, and the loop runs its spill-reading copy);
a stored key must be inline. A sum of property reads is verified by its
result (one ordered compare) instead of one tag test per leaf. Stores keep
the store IC's GC obligations and the two per-object store facts are tested
at the guard. `PERRY_REGIONS=0` disables regions at compile time;
`PERRY_RECV_ROUTE_COUNT=1` builds count region entries, F/G iterations, bare
accesses, prime verdicts and why a prime was refused.
