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
release each store once. A move hands ownership to the copy: the move hook
nulls the original header's `store` and `entries`, because old-generation
evacuation (tenured nursery objects and old-page defrag) releases FORWARDED on
the original before the sweep, which would otherwise finalize the live copy's
store as a dead Map, and the exit walks would free it a second time.

Regression coverage includes collision deletion, foreign/interior brand rejection,
value-only versus key rewrites, actual evacuation with iterator compaction history,
bounded/unbounded full sweeps of live and dead Maps in an active block, tenured
evacuation and old-page defrag followed by sweeps and the exit walk (exactly one
free), non-copying monolithic and budgeted minors finalizing a dead active-block
Map, and the copying minor's from-space walk finalizing a dead Map.
