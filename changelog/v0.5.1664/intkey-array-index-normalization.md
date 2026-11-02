Array index GET/SET now retain native integer provenance instead of normalizing a freshly widened index as an arbitrary NaN-boxed value. Signed negatives and unsigned values above the signed element tier retain exact property-key fallbacks. String `charCodeAt` keys and safe local aliases use a compact admission check that preserves the out-of-bounds NaN key.

Unproven keys share one linker-coalesced, non-inlined, non-allocating numeric normalizer across both operations. It replaces the duplicated per-site tag/range/integrality towers without adding runtime tables or weakening receiver/shape checks. Fractional, negative, non-finite, large, and nonnumeric keys still reach the existing exact helpers.

Char-code alias admission checks every explicit binding writer, excluding parameters, catch bindings, captures, implicit updates, and unbounded or fractional writes. Bounded surrogate construction remains eligible. A hoisted uninitialized alias still declines to its original undefined-key fallback.

Adds a Node-parity gap covering integer provenance, rejected keys, evaluation order, and revoked aliases, plus a reproducible char/surrogate/mask/dynamic-key benchmark. Native split-unit construction retains the numeric helper's purity and GC-leaf call attributes.
