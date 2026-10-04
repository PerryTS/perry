The per-object prototype-divergence flag is gone (Refs #10507). Since an
object's prototype is a fact of its shape, the caches that consulted the flag
(store plans, array-subclass dense layouts, the defineProperty key-add path,
`instanceof` against a class object) already key on the shape or the
prototype, so a re-parented object simply meets a different cache entry. A
runtime-wired function-constructor instance no longer allocates a metadata
record.

The other prototype-link flags and the metadata allocation for them are gone
too (Refs #10507): `instanceof`, JSON.stringify's plain-record check, symbol
reads and the static shapes read an object's recorded prototype from its
shape, `instanceof` walks the live chain for a receiver on a foreign
prototype (so the `util.inherits` escape hatch and its latch are deleted),
and linking a prototype no longer allocates a metadata record.
