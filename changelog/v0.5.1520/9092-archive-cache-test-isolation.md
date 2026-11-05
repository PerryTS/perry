### Fixed

- Stabilized the archive-cache fallback/cache-hit test under the parallel `perry`
  binary suite by serializing its process-global tool-environment reads with
  sibling tests that temporarily replace `PATH`.
