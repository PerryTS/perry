### Added

- Atomically checkpoint completed native units under the driver's full object-cache identity and a fingerprint of each frozen LLVM input. Treat missing, truncated, corrupt or unwritable records as cache misses.
