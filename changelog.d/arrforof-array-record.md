Array for-of and array patterns lower through one IteratorRecord whose
ordinary-array representation uses a source and index, guarded once from
receiver and intrinsic shapes. Length changes and inherited hole getters remain
observable; captured next and IteratorClose preserve protocol ordering.
Array symbol properties now share object-owned shape storage through the
array's existing traced named-property reserve.
