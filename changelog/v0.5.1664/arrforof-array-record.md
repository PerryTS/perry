Array for-of and array patterns lower through one IteratorRecord whose
ordinary-array representation uses a source and index, guarded once from
receiver and intrinsic shapes. Length changes and inherited hole getters remain
observable; captured next and IteratorClose preserve protocol ordering.
Array symbol properties now share object-owned shape storage through the
array's existing traced named-property reserve.

Dense for-of literals now use the same scalar record as destructuring, including
transparent TypeScript wrappers. Elements are evaluated once before the entry
proof; holes and spreads retain their observable array semantics. Counted
consumers take the record's numeric length expression directly, avoiding a
private literal receiver until an override or observable close needs it.
