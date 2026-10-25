Fixed `readableEnded` reporting `true` as soon as EOF was pushed or `end()` was called on a stream; it is now `true` only once the `end` event was emitted, as in Node.
