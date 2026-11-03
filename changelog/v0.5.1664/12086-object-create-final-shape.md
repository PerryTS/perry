Allocate `Object.create(proto)` directly on the ordinary Shape naming its final
prototype and learned inline width. Remove the intermediate default-prototype
birth and the subsequent prototype transition, mark the prototype once, and
reuse its Shape-owned word without duplicating nursery log entries.

Keep the prototype rooted across allocation, re-resolve an uncarried birth
Shape if a full collection retired it, and preserve incremental shading and
old-generation carrier notes. Root and refresh a supplied descriptor bag across
the birth allocation. Null prototypes and descriptor definitions keep
their existing paths; non-serial prototype kinds retain their unique identity.

Add Node parity coverage for identity, descriptors, inherited setters, property
adds, later prototype changes and exotic prototype hops, plus runtime tests
that move the prototype inside the birth allocation, retain its edge through
the newborn's Shape, and move an unrooted caller's descriptor bag during birth.

Release runtime validation passes 5,115 tests (5,113 on the baseline), with no
new Node parity failures. The forced descriptor-bag test reproduces a missing
root on the baseline and passes with the adapter root. Workspace formatting
and Node-version consistency checks pass; existing size/address lint failures
remain unchanged.

The nine-program A/B matrix meets the owner's +0.5% gate for instructions:u,
cycles:u and peak RSS. Effect instructions improve 1.68% and peak RSS 7.52%;
the Object.create repro improves 35.10% and 20.32%, respectively. Measurements
use randomized interleaving, identical-main controls, CPU 7, disabled ASLR,
10–100 samples per arm, and mapped-file warming for RSS. TypeScript uses a
fixed extension to 30 samples retaining the initially noisy 10-sample result;
its final instruction/cycle deltas are +0.01%/+0.06% and RSS improves 1.93%.
