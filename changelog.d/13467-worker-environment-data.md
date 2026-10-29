### Fixed

- Connect captured and CommonJS `worker_threads.getEnvironmentData` and `setEnvironmentData` calls to the existing implementation.
- Inherit a creation-time environment-data snapshot in workers using one structured-clone graph with `workerData`, preserving cycles, shared references and transfer rollback on failure. Clone failures stop traversal before later getters run.
