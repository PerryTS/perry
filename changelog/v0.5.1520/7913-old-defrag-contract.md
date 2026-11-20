### Fixed

- Restored safe old-generation page defragmentation and enabled it by default.
  A minor trace does not prove unmarked old objects dead, so evacuation now
  snapshots every indexed occupant of a selected source block, rejects the
  block before forwarding if any occupant cannot move, and otherwise relocates
  the complete block before reclaiming it. `PERRY_GC_OLD_DEFRAG=0` remains an
  explicit diagnosis and rollback switch.

- Closed three runtime rewrite gaps: JSON's parse-key ring and the perf entry
  keys cache now follow their structurally rooted owners, while diagnostics
  symbol-keyed state is rekeyed after a move. Cached `@perry_class_keys_*`
  copies are precise function-lifetime mutable roots rather than relying on the
  old generation being immovable. The runtime-holder inventory now fails on
  known or unevaluated movable-address gaps.

The historical corruption workload fails on rebuilt main when old-page
relocation is enabled, but passed six consecutive verified runs after this
change with output identical to the clean control. A mixed-size regression
also demonstrates that a fragmented source block is actually released.

### Performance

Old-page metadata selection now runs only after the copying-minor fast path has
declined the collection. This removed an O(old pages) charge from ordinary
copying minors: the worst affected retention benchmark moved from +8.29% in the
initial implementation to +0.14% best / -0.09% median in the final quiet-M1
best-of-15 comparison.

Precise class-key roots cost 3.70-4.45% in four tight shape/class allocation
kernels. That is the remaining correctness tradeoff; `interp` improves 2.85%,
and the other 20 programs remain within +/-1.5%.
