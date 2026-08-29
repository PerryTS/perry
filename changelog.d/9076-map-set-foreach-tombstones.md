### Fixed

- `Map.prototype.forEach` and `Set.prototype.forEach` now skip tombstones left
  by callback-side deletes and continue through the raw insertion-order extent,
  so deleting the current or an earlier entry no longer exposes hole values or
  skips later live entries.
