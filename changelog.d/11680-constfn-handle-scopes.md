Use scoped runtime handles in ConstFn finalization and its moving-GC, key-add,
worker-seed and transfer tests. Calls that need the receiver after a collection
reload it through `across_mut`; leaf and self-rooting entries receive scoped
arguments. This removes 154 new raw-handle debt sites without raising ceilings.

Worker-launch tests now carry independently owned observations in their closure
captures instead of sharing asserted process globals. An escaped Arc protects
the launch even if a body runs too often or a test times out; workers retain an
owned observation for late preparation callbacks. The existing ON/OFF, ordering,
worker-identity, output and moving-GC assertions remain.

Raw-handle, global-isolation, address-class and root-holder source checks pass,
along with formatting. The 34 existing tests still require execution on the
updated build; source checks do not establish runtime correctness.
