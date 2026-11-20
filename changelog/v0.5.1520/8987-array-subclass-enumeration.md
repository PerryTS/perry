### Fixed

- Array-subclass enumeration now matches Node: `Object.keys` and `for...in`
  report only enumerable indices, while `Object.getOwnPropertyNames` also
  reports `length`; inherited `Array.prototype.fill` no longer leaks as an own key.
