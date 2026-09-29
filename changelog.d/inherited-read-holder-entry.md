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
