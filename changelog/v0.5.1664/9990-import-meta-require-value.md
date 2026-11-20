### Fixed

- Lower bare, computed and destructured import.meta.require to one module-owned createRequire(import.meta.url) value; retain the existing direct-call static graph path.
- Build the standalone native regression's runtime and require-provider archives coherently, and include compiler diagnostics directly in failing CI output.
