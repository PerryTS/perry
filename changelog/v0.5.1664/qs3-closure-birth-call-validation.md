Remove redundant work from closure birth and dynamic calls. Non-collecting
boxed births retain the nursery's UNKNOWN scan layout and copy captures in
bulk, with the existing newborn barrier. Slow per-arity dispatch reuses the
public entry's closure validation and plain-call admission.

The captured-closure and native-call micros remove 51 and 10 instructions
per operation respectively. On c8b1f39b7c, interleaved n=5 qs measurements
reduce stringify instructions by 1.88% and parse by 0.51%, with unchanged
collection counts and about 8.5 KB less emitted text. These results fall
below the lane's 10% landing threshold; this is a review candidate, not a
qualified performance landing.

Validation: six bulk-capture GC tests, five call-entry/ABI tests, and 22
Node gap comparisons pass. A negative control removing newborn capture
shading fails the expected marking assertion.
