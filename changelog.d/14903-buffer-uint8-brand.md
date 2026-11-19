A Node `Buffer` is now treated as the `Uint8Array` it is, at the places that asked only for a plain `Uint8Array`:

- `Atomics.load` / `store` / `add` / … accept a `Buffer`.
- A non-empty `Buffer` follows the integer-indexed rules:
  - `Object.freeze` and `Object.seal` throw for it.
  - `Object.preventExtensions` makes it non-extensible.
  - With `Object.defineProperty`, `Reflect.set`, `Reflect.defineProperty` and `Reflect.deleteProperty`, an index write stores the byte, an out-of-range `Object.defineProperty` throws (`Reflect.defineProperty` returns `false`), and an index cannot be deleted.
- `Object.prototype.propertyIsEnumerable` sees a `Buffer`'s own properties.
- `console.table(buffer)` prints the index table.

The audit behind this is `docs/audits/2026-11-17-typed-array-kind-miss-audit.md`.
