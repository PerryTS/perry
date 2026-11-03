### Fixed

- Perry now compiles ioredis `setex`, `ping`, `hget`, `hset`, `hdel`, `hlen`, and `hgetall` calls to their native runtime implementations instead of falling through to undefined dynamic dispatch.
