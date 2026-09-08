### Fixed

- Lower bare, computed and destructured import.meta.require to one module-owned createRequire(import.meta.url) value; retain the existing direct-call static graph path.
