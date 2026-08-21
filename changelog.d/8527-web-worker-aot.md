### Added

- Compile statically discoverable Web Worker module URLs into native worker
  entry functions. Global `Worker` now shares Perry's in-process worker runtime,
  including worker-scope messaging globals, property and EventTarget handlers,
  environment options, reload, close, structured messages, startup errors, and
  termination, without evaluating worker source at runtime (#8509).
