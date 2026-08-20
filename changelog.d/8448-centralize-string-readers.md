### Changed

- Centralized native stdlib and extension string-header reads behind the
  runtime and `perry-ffi` accessors, preserving strict UTF-8, lossy UTF-8, and
  raw-byte behavior while keeping results owned across allocations and async
  boundaries.
