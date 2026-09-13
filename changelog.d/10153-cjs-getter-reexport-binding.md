### Fixed

- Recognize CommonJS exports installed with `Object.defineProperty`, including
  Babel's getter re-exports, as value exports. ESM named and namespace imports
  now read the current property through the existing getter-aware runtime
  lookup and call the returned value, instead of referencing a function symbol
  that the CommonJS barrel never defined.
- Avoid evaluating synthetic CommonJS property exports during initialization,
  preserving accessor side effects and exports assigned or replaced later.
  Regression coverage includes named imports, namespace calls, ESM barrels,
  materialized namespaces, writable descriptors, and late assignments (#10153).
