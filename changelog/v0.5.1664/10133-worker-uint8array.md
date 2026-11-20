### Fixed

- Preserve `Uint8Array` bytes and constructor brand across native Worker structured-clone boundaries, including a Web-style parent Worker talking to a `node:worker_threads` `parentPort` child (#10103).
