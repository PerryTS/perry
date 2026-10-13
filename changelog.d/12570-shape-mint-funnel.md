Unify empty class key arrays with the canonical keys trie's root backing, removing the empty-layout exception in `shape_cache_insert` and per-class empty backing allocations. The backing follows the trie's existing weak full-trace lifetime.

Make the shape-mint census compare the canonical layout's key identities and attribute entries, and the memo's complete shape facts. Names with different attributes, or shapes with different prototypes, representations, brands, integrity, ConstFn bodies or object kinds, are no longer reported as duplicates or identical mints. Preserve descriptive name-only totals.

Add an ordinary runtime lib-test census gate covering empty classes, descriptor variants and repeated facts, with live-mint assertions and negative controls for a producer bypass and a missing memo entry. Add a Node parity fixture for empty-class growth, descriptors, prototypes and symbol keys.

Route JSON's parse-key construction through the same canonicalizer, including wide parses that bypass the existing parse cache. The census now checks overlapping indexed backings and preserves every overlap event: legitimate republication after weak retirement is distinguished from a producer bypass, and retiring a duplicate cannot erase its failure. Add JSON/class convergence and retirement coverage to the gate.
