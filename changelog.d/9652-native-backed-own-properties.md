### Fixed

- **Native-backed `Uint8Array` and `Date` instances now honor ordinary own
  property overrides (#9529).** `Object.defineProperty` data and accessor
  descriptors on a `Uint8Array` remain visible to reads, reflection, and key
  enumeration, including an own `length`. `Date.prototype.toJSON` and
  `JSON.stringify` now invoke an instance override of `toISOString` instead of
  bypassing it with the builtin formatter.
