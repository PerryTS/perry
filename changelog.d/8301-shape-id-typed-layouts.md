Improved allocation-heavy typed classes with pointer fields by keying shared
typed-layout metadata and its construction memo on the object's immutable
runtime `ShapeId`. Repeated instances now reuse the first instance's validated
slot-count proof instead of hashing the shape-descriptor table on every
allocation. On the `cycles` reproducer from #8289, retired instructions drop
from 2.177B to 1.804B (17.1%) while preserving per-instance value validation,
ambiguity fallback, and moving-GC tracing.
