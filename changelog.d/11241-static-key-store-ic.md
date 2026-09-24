**`o.k = v` takes ONE inline path for any right-hand side: one ShapeId
compare, the store, and the barrier the GC needs** (store microbenchmark,
parameter receiver: 65 → 38 instructions per store; node 13).

Every static-key store that no earlier specialisation takes (class field, POD
record, array index) now lowers to the same inline hit: the receiver tag and
handle-band test as one unsigned compare, one compare of the receiver's
ShapeId against the site's compact word (`@..._packed_set`, ShapeId in the low
half, slot in the high half — the store twin of the read path's
`@..._packed_get`), the per-object facts the shape does not carry (receiver
kind, the Array-subclass numeric proof), the store, and the write barrier
behind a live test of the stored bits. Every other case is one call,
`js_put_value_set_packed_miss`: the unchanged strict-aware `[[Set]]`, then the
site's publication, with up to four shapes served from the site's way cache
without the full walk.

The old static PIC admitted only a call-free RHS, because it held the
receiver in an unrooted register across the RHS. The receiver is now
evaluated first, rooted across the RHS, and its shape is read from the root
after the RHS returns, so an RHS that collects, moves the receiver, deletes or
adds a key, installs a setter or freezes it is handled by ORDER, not refused.
The hit path no longer reads the GC-kind byte, the forwarded flag or the
stable-tombstone marker: the ShapeId compare proves each (rule 3; a
forwarding stub's ShapeId word is an address's high half; #10826).
