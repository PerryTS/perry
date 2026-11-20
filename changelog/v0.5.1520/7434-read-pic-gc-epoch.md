Fixed the #6080(a) read-PIC ABA residual: a property-get site primed with a raw
keys-array POINTER token (class instances and other unstamped receivers) could
silently return the wrong slot after GC moved or freed the cached array and its
address was recycled by a different-shape keys array — the `@perry_ic_N` cache
globals are invisible to every GC scanner, so nothing invalidated them. The
runtime now exports `PERRY_IC_EPOCH` (bumped in `GcStats::record_collection`,
the single per-collection funnel, plus at budgeted-sweep entry where sweep
slices interleave with the mutator), the miss handler stamps it into `cache[2]`
at prime time, and the emitted hit predicate refuses a pointer-token hit whose
primed epoch is stale. Shape-ID tokens (#6804) skip the check — ids are never
reused — so stamped plain objects never re-prime after a collection; only
pointer-token sites pay one extra miss per GC cycle.
