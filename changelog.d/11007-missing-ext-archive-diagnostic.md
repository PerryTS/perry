### Fixed

- Compiling with a missing native extension archive now reports the archive,
  its Cargo package, and where to make it available before invoking the linker.
  Network wrappers include a build command that keeps their Tokio copy paired
  with Perry's runtime and stdlib archives.
