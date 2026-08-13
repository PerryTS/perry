**Fixed: remaining computed reads and writes could reuse GC-stale receivers, keys, and backing pointers (#7640).**

Several typed-array, numeric-array, and generic computed-access paths evaluated
a receiver, then evaluated a key or value that could run user code and trigger
a moving collection, and finally consumed the receiver from its original LLVM
register. The same audit found typed-array stores deriving raw backing pointers
before an allocating RHS, and the array-growth path issuing its write barrier
against the pre-growth handle even when the helper returned a replacement.

The remaining read/write operands now use selective rooting and are re-read
after collecting expressions. Masked-window and `Ptr<NumArray>` native paths
use the custom-lowering form of the same scope; raw backing pointers are loaded
only after the RHS; and reallocating array stores shade through the returned
live head. Literal keys, loop counters, and other proven non-collecting windows
still emit no temporary-root traffic.

Class-field stores now distinguish their two receiver shapes explicitly. Bare
locals and `this` retain the existing zero-cost `root_reload` repair, while a
compound receiver such as `this.target.x` is conditionally rooted across an
allocating RHS because its phi result cannot be re-derived from a local root.

IR regressions assert the typed-array read/write groups, the erased-receiver
store, and the realloc-path barrier operand. Both end-to-end fixtures remain
byte-identical with Node 26.5.1. The 143-source shadow and native/statepoint GC
corpora report zero dominance, unrooted-allocation, or statepoint hazards and
catch all 40 seeded violations in each lowering.
