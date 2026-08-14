### Fixed

- Make `ShapeId` descriptors authoritative for runtime and generated object
  guards, including moving keys, exact logical/live slot facts, semantic
  transitions, agent-local installation, and fail-stop exhaustion. Class
  objects and RegExp values now use GC metadata outside the compatibility
  `ObjectHeader` words, while the header size remains unchanged.
