An inherited or absent property read is now answered by the read site itself,
from facts of two shapes, instead of the inherited-read side table. When `o.k`
misses because `k` is not own, the miss handler records in the site cache the
receiver ShapeId, the object that holds `k` (a strong GC root), that
object ShapeId and the slot; an absent key records the terminal object
instead. The emitted read compares the receiver ShapeId and the holder
ShapeId and loads the slot inline (depth 1), or calls a GC-leaf stub that also
compares the intermediate hops (depth 2 to 4). A key added, deleted or
redefined on any object on the chain, or a `setPrototypeOf`, moves a ShapeId
the entry compares; a value store is seen because the slot is loaded. No global
validity word is involved. `process.env` and `arguments` now carry a per-object
prototype identity in their shape, so no shape-keyed memo admits them.

The entry is asked where the site own-slot word and its polymorphic ways
miss (a megamorphic site goes straight to the call, as before). A receiver it
does not describe (a compiled-class instance) keeps the inherited-read hook
it had before on the never-primed edge, and a site that refused once, or was
re-primed for four different receiver shapes, is latched: it stops priming,
so its misses do not walk the chain and run the getter again, and it asks the
inherited-read hook as a never-primed site does. A miss the live entry
answers primes nothing (the class-field read miss arm asks the entry
directly).
