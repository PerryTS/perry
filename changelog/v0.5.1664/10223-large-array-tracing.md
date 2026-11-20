Fix garbage collection of large arrays whose backing capacity exceeds their
live length. The collector now bounds the element range by the allocation size
instead of an unrelated capacity cutoff, preserving heap references after array
growth. Regression coverage checks the live range, shared slot descriptors and
child marking at 9, 10 and 16 million elements, plus invalid-capacity and sparse
array bounds.
