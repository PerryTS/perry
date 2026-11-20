Link-time static ShapeIds are placed by content hash in a band sized to the
program (the next power of two at or above four slots per distinct shape,
at least 1024 ids) instead of across the whole reserved range. The runtime
allocates shape records in 32-id chunks, so the scattered ids gave almost
every static shape a chunk of its own; the sized band restores tsc startup
RSS to the pre-static level. A shape keeps its id when unrelated shapes are
added, so the object cache still reuses modules whose ids did not move.
