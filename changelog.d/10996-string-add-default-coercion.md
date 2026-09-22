### Fixed

- Folded string `+` chains now use default-hint primitive conversion in operator order, so object `valueOf` methods and `Symbol.toPrimitive` match pairwise addition ([#10996](https://github.com/PerryTS/perry/pull/10996)).
