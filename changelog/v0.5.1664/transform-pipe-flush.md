### Fixed

- **Piped `Transform` streams now emit their flush output before `end`.** Pipe
  completion runs `_flush` or the `flush` option before closing the readable
  side, including when the flush callback completes asynchronously. Fixes
  #10450 in #11037.
