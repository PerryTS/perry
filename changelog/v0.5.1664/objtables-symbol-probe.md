Reject an owned cell of another GC kind before the generic header validation and immortal-symbol registry fallback. Allocator ownership still precedes every header read; positive symbol admission retains the kind, storage flags, size, forwarding and magic checks. No cache, latch or address registry is introduced.

Validation: the full runtime suite retains only its eight baseline failures. The moving-symbol owner test passes, and removing the symbol's brand makes its negative control fail at the intended assertion.
