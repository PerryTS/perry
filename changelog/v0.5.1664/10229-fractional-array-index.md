Fix fractional numeric reads on arrays with erased receiver types, including
lazy JSON arrays: `rows[0.5]` now looks up the property `"0.5"` instead of
truncating to element zero. The runtime fallback validates the integer range
before narrowing, preserving negative, non-finite, and large property keys
and reads of named properties without changing the compiler's inline tiers.
