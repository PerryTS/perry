### Fixed

- Preserve `net.Socket.allowHalfOpen` as a mutable own data property across reconnects, with independent TLS wrapper snapshots.
- Check for strict `false` after `end` listeners and defer writable shutdown without acting on retired connections.
- Add focused coverage for property descriptors, TCP policy mutations, accepted-child defaults, reconnects, and the TLS policy journey.
