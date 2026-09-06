### Fixed

- Preserve side-table young-root logs when the first copying collection
  abandons speculative in-place promotion, so the evacuation retry does not
  lose live objects or corrupt full applications during startup.
