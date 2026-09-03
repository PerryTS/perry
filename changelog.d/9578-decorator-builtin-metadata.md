### Fixed

- **Legacy decorator metadata now stores the real global constructors for
  built-in types (#9501).** With `emitDecoratorMetadata`, reflected `number`,
  `string`, `boolean`, Array/tuple, Function, Symbol, BigInt, Promise, and
  Object-like types previously became numeric `0` or `undefined` instead of
  `Number`, `String`, `Boolean`, and their corresponding constructors. This
  restores the identities expected by NestJS injection and by
  class-transformer/class-validator coercion while preserving user-class
  metadata.
