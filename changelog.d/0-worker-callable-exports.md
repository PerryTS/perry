### Fixed

- First-class `worker_threads` environment, marker, synchronous receive and direct-message functions now use their existing implementations when captured from named imports or CommonJS, including `call`, `apply` and spread invocation.
