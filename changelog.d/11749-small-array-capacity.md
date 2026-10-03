Reduce the general array allocator's minimum initial capacity from 16 to four
element slots. Empty child arrays retain distinct mutable identities and four
children fit without reallocating; larger arrays continue to grow by doubling.
Exact-sized literal and key-list allocation paths keep their existing sizing.

Regression coverage checks hole initialization, independent empty arrays,
growth through stale aliases, named properties, Array subclasses, and young
string elements retained by old arrays through forced copying minors.
`benchmarks/array_capacity/` contains the unchanged cyclic workload from
#11743 and a rotating Node-checked CPU/RSS benchmark for short and large
arrays. See its results for the live-capacity saving and growth tradeoffs.
