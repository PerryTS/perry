### Added

- The ES2025 `Iterator` global: `Iterator.prototype` (the shared iterator prototype) with spec iterator helpers as real methods, `Iterator.from`, `Iterator.concat` and `Iterator.prototype[Symbol.dispose]`; generators inherit from `Iterator.prototype`. Test262 `built-ins/Iterator` goes from 79 to 434 of 513, and the full-corpus baseline to 90.15%.

### Fixed

- Iterator helpers follow the spec (counter argument, TypeError on non-callable callbacks, closing the underlying iterator, `take`/`drop` limit validation) and no longer answer helper names on objects that do not inherit from `Iterator.prototype`.
