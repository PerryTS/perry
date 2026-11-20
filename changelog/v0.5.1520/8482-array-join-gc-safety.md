### Fixed

- Keep `Array.prototype.join` separators, receivers, and coerced elements valid
  when user `toString` code triggers a moving garbage collection.
