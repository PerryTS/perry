Speed up generic `Array.prototype.sort` and `toSorted` comparator sorts by
sorting an index permutation over rooted values. Natural run detection, stable
binary insertion, balanced merges, and block searches reduce comparisons, while
applying the final permutation in one callback-free phase removes repeated GC
layout and write-barrier work. The same engine handles numbers, strings,
objects, and arbitrary comparators; collections remain enabled during callbacks.
GC-owned workspaces also remain reclaimable when a comparator throws.

The M1 Max comparison against the same main build improves all 24 tested
distribution/value-type combinations by 1.89–36.07×. Node remains faster
in these measurements. The change includes stability and inconsistent-comparator
tests, forced moving-GC coverage, a compiled semantic regression, and a
reproducible benchmark with full results in `benchmarks/array-sort/`.
