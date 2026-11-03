### Fixed

- Exported arrays of object literals now remain materialized across module
  boundaries, preserving their identity, length, iteration behavior, and
  element values instead of producing an `undefined` export getter.
