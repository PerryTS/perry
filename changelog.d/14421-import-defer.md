### Fixed

- Preserve the `defer` phase of static and dynamic imports, delay synchronous module evaluation until a triggering namespace operation, and keep deferred namespace identity through reexports. Reuse module initialization guards and cached evaluation failures, and distinguish evaluating modules from completed modules during synchronous readiness checks. Full asynchronous module scheduling remains incomplete.
