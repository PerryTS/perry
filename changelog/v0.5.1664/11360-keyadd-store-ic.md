Key-adding stores (`o.k = v` where `k` is not yet an own property, as in every
constructor that assigns `this.x = ...`) now have an inline path. A store site
memoizes the transition from the receiver's PRE-shape: one ShapeId compare, one
compare of the prototype-chain verdict's generation (`proto_validity +
VTABLE_GEN`), a few per-object header tests, then the successor ShapeId, the
value and the barrier. The memo is re-derived from the two immutable shapes and
the chain verdict, and the site owns both ShapeIds across collections.
Polymorphic sites (a base-class constructor sees one pre-shape per subclass)
keep further memos that the miss entry serves without the full `[[Set]]`.
Class-field stores (`this.x = v` on a declared field) drop the header checks a
matching ShapeId already proves, and a site whose subclasses the class guard
cannot name now takes the generic store path instead of missing on every store.
`PERRY_STORE_CENSUS=1` (at compile time and run time) prints per-route store
counts at exit.
