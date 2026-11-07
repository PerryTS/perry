### Fixed

- Forward MessagePort `close(callback)` through the existing once-close-listener sequence, preserving callback ordering, deduplication and the actual receiver. Report the Node-compatible method length of one.
