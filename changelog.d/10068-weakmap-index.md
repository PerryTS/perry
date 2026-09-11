Fix quadratic WeakMap and WeakSet lookup, insertion, and deletion (#10057).

- Index weak key identity to stable entry-array offsets and reuse tombstones, avoiding a scan of every live entry on each operation.
- Discard the untraced index before GC address reuse and during relocation; validate cached hits against the weak slot so a sliced collection cannot return a cleared entry.
- Preserve weak read and overwrite barriers, and reload rooted collections, keys, values, and new entries across allocating helpers.
- Cover identity, overwrite/receiver returns, deletion/reuse, moving minor and full GC, promoted keys, dead-key collection, holder cleanup, and bounded entry visits through 100,000 keys.
