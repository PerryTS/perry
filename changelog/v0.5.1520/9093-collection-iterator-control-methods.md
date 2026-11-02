### Fixed

- Map and Set iterator objects no longer expose synthetic `return` and `throw`
  methods. Those generator-only properties now resolve through ordinary
  prototype lookup, while `[Symbol.iterator]()` continues to return the
  iterator itself and user-defined overrides still work (#9086).
