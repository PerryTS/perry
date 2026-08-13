### Representation selection: preserve fresh shapes returned by methods (#7170 R2)

Native compilation now propagates proven fresh return shapes through instance
method calls when the receiver has an exact contained shape and its prototype
dispatch is stable. Results can use guard-free fixed-offset field access just
like values returned by direct function calls.

The proof stays fail-closed for decorated classes, mutable prototype dispatch,
unproven or escaping receivers, and modules containing shape barriers. The
optimization report also classifies allocations served by method return-shape
facts separately from the remaining rule-1 provenance wall.
