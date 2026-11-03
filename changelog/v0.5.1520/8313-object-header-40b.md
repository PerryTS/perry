Shrink the common JavaScript object layout by removing the derivable per-object
keys-array mirror. Ordered keys now come exclusively from the authoritative,
moving-GC-rewritten ShapeId descriptor, reducing `ObjectHeader` from 24 to 16
bytes and a two-slot object from 48 to 40 bytes (eight-slot objects: 96 to 88).
