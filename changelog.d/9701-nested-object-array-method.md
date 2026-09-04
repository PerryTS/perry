### Fixed

- Nested object methods such as React's `Children.map` now preserve their own
  dispatch instead of being mistaken for `Array.prototype` calls (#9701).
