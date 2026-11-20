### Fixed

- `String.prototype.toWellFormed()` and `structuredClone()` now keep source strings rooted while allocating their copies, preventing moving garbage collection from reading stale string payload pointers.
