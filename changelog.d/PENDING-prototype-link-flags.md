The per-object prototype-divergence flag is gone (Refs #10507). Since an
object's prototype is a fact of its shape, the caches that consulted the flag
(store plans, array-subclass dense layouts, the defineProperty key-add path,
`instanceof` against a class object) already key on the shape or the
prototype, so a re-parented object simply meets a different cache entry. A
runtime-wired function-constructor instance no longer allocates a metadata
record.
