### Fixed

- Validate truthy non-string `worker_threads.Worker` names before reading them as strings, preventing constructor crashes and preserving transferable data on rejection. Falsy names retain their default behavior.
- Validate and consume Worker transfer lists even when `workerData` is undefined, including unreferenced ArrayBuffers.
