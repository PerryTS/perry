Report how pointer-shape proofs pass into the one-shape region path. Static
region suppliers explicitly use available receiver-class provenance, while the
live ShapeId guard remains the authority for offsets and field representation.
Consumption is recorded only when that supplier serves an emitted read or
write. Learned suppliers and type hints do not count as proof consumption;
refused unguarded routes name the region handoff.

Preserve every existing promotion floor and fixture. Add a straight-line
fixture for the surviving guard-free load/update sites and five compiler tests
covering actual region accesses, removal of the static supplier, absence of a
selected proof, reporting OFF, and the surviving straight-line sites.
The Python census checks and formatting pass. Compiler execution, the rebuilt
census and knob isolation remain required before acceptance.
