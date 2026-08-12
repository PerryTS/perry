### Fixed

- **Guarded `.length` reads now preserve ordinary property semantics when a
  declared Array/String/Named receiver holds a different runtime value**
  (#7853). The inline layout check was already safe, but its numeric fallback
  collapsed a missing `length` to `0` and let `null`/`undefined` continue.
  Source-level reads now use a property-semantic sibling that returns
  `undefined` for a missing property, preserves non-numeric values, delegates
  normal object/function/native/proxy lookup, and throws a catchable TypeError
  for nullish receivers. Array-internal length coercion remains unchanged.
