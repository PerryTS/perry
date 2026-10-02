Report how pointer-shape proofs pass into the one-shape region path. Static
region suppliers explicitly use available receiver-class provenance, while the
live ShapeId guard remains the authority for offsets and field representation.
Consumption is recorded only when that supplier serves an emitted read or
write. Learned suppliers and type hints do not count as proof consumption;
refused unguarded routes name the region handoff.

A single-receiver body region (one loop-local receiver) built its guard from
the learned word alone and never asked for the static supplier, so a receiver
whose class names a static ShapeId was still primed and guarded by a learned
word. That path now takes the static guard whenever one exists, as every other
region guard already did (DESIGN §4.1, static-exclusive).

Preserve every existing promotion floor and fixture. Add a straight-line
fixture for the surviving guard-free load/update sites and five compiler tests
covering actual region accesses, removal of the static supplier, absence of a
selected proof, reporting OFF, and the surviving straight-line sites. The
census baseline is re-measured with `census --update` on Linux x86_64; no
floor is lowered.
