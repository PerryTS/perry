### Fixed

- Web Stream adapters now keep controllers, iterators, promises, and callbacks
  rooted across allocations, preventing stale references when the moving
  garbage collector runs during stream conversion.
