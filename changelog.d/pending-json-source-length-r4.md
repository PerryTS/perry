Derive a dominant unescaped JSON string token's UTF-16 length from its exact
rooted source header when the surrounding bytes are bounded and ASCII. Preserve
the established malformed-tail counter behavior, existing allocation/copying,
output ownership and deferred reclamation. Keep the ordinary string constructor
out of line so the large-string proof does not enlarge small parse construction.

Add exact-owner, input-slice, ASCII-surrounding, astral/surrogate and exhaustive
two-byte counter-equivalence tests. Performance acceptance remains pending.
