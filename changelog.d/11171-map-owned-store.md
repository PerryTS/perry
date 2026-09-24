### Map storage belongs to the Map

Replace the address-keyed Map allocation registry, string index, pointer index,
and compaction log with one native `MapStore` owned by the header. Numeric index
storage is inline in that allocation. Preserve the 40-byte header ABI and existing
`ObjectMeta` edge by reusing the former numeric-index pointer slot for the store.

String content hashes store a single entry offset inline; collision vectors exist
only for actual hash collisions. GC skips pointer-index reconstruction when no
indexed key bits changed. Map branding validates the allocator's object-start
bitmap and `obj_type`, rejecting foreign and interior pointers before reading the
store.

Ordinary sweeps finalize owned stores through the Map type descriptor. Copying
collection uses the existing from-space Map-start bitmap, with no old-generation
registry walk or owner re-keying. Explicit shutdown and arena thread teardown
release each store once, while forwarded headers leave ownership to the copy.

Regression coverage includes collision deletion, foreign/interior brand rejection,
value-only versus key rewrites, actual evacuation with iterator compaction history,
and bounded/unbounded full sweeps of live and dead Maps in an active block.
