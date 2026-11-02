### Fixed

- `Intl.Segmenter` segment records now share one shape. They were built
  property-by-property with `set_field`, which allocates a fresh key string
  per call and clones the object's key list before each write, so every record
  got its own keys array — and, because the shape table is keyed on that
  array's address, its own ShapeId. That made every read of `.segment` /
  `.index` / `.input` a guaranteed inline-cache miss and added one descriptor
  to the shape table per record. Grapheme-aware text measurement segments
  every string a terminal UI renders: one 400-character reply in the compiled
  claude-code TUI produces 175,797 segment records, and `PERRY_IC_DIAG`
  attributes 175,797 of that turn's 2,589,696 IC misses to the `.segment` read
  site alone. The two record shapes (with and without `isWordLike`) now share
  one `GC_FLAG_SHAPE_SHARED` keys array each, built at most twice per thread —
  the same construction #7564 used for `{ value, done }` iterator results.
