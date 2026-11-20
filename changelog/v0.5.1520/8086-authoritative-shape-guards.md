### Fixed

- Make `ShapeId` descriptors authoritative for runtime and generated object
  guards, including moving keys, exact logical/live slot facts, semantic
  transitions, agent-local installation, and fail-stop exhaustion. Class
  objects now carry their kind in the authoritative descriptor, and RegExp
  values use a dedicated GC kind with relocation-safe side tables, while the
  object header size remains unchanged.
