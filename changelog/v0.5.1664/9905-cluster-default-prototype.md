### Fixed

- The `node:cluster` default export now inherits from the canonical
  `EventEmitter.prototype`, so reflective prototype checks agree with Node while
  preserving the cached singleton used by cluster event methods.
