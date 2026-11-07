### Fixed

- Compile literal child-worker filenames referenced by immutable CommonJS `Worker` bindings and namespaces, fixing nested workers that reported `ERR_WORKER_NOT_COMPILED` without changing runtime constructor evaluation.
