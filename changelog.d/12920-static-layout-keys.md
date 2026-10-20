Use precomputed image-key hashes when registering static object layouts.
Eligible all-string layouts now initialize their normal GC-owned key and
attribute arrays and canonicalize them in one trie walk. The existing
canonical table remains the authority for key identity and lifetime.

Preserve allocation order, exact key/attribute/slot comparisons, and GC
suppression during registration. Function-constructor, symbol-containing,
and non-image layouts keep their existing initialization path.

Add differential tests for static/dynamic canonical identity, attributes
and slot mappings, GC witness repair, and fallback eligibility.
