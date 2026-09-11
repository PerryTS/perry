# Next indexed-read candidate — not applied

The prior R23 scalar probe for canonical_u32_index remains byte-for-byte applicable: jsvalue.rs, array/subclass.rs and value/tags.rs match every recorded source hash. Its 15,525,263 checked bit patterns cover boxed INT32, IEEE numbers/NaNs, all tag bands, integer boundaries and random payloads. Preserve that original result as reused model evidence, not a new runtime test.

The candidate retains boxed INT32 handling, uses Rust's saturating f64-to-u32 conversion, excludes u32::MAX (the maximum property key is not an array index), then requires the integer-to-f64 roundtrip to match. NaNs and every nonnumeric NaN-boxed value fail equality, negatives and fractions fail roundtrip, -0 remains index zero. This can remove fmod from the packed numeric-index predicate without GC changes. Main benefit is a hypothesis until a normal all-three runtime build and read controls run.

Production coverage should include a table for every tag/number category and boundary, plus computed array reads showing that fractional, negative, NaN and uint32-max named properties still use their semantic fallback. Avoid huge dense allocations for maximum indices. Keep exact input coercion, descriptors and moving/protected GC checks. The R26 20 MB sequential control has a persistent ~0.5 ns median cost; use it as a control for this next general-index optimization.
