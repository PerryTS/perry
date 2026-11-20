### Fixed

- Keep partially filled `slice` and `splice` result arrays' reference-slot
  descriptions current when an indexed source getter interrupts the copy.
- Assert the exact partial result's GC slot enumeration in runtime regressions,
  independent of whether a collector's optional diagnostic hook executes.
