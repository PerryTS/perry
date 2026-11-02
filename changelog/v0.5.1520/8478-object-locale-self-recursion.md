### Fixed

- **`Object.prototype.toLocaleString()` no longer overflows the native stack.**
  The built-in now invokes the receiver's `toString` directly instead of
  redispatching itself when called on `Object.prototype` or aliased onto
  another object.
