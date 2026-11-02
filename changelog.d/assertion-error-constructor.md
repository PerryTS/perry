`assert.AssertionError` is a real constructor export of `node:assert` and
`node:assert/strict` (one value for both, also on the CommonJS `assert`
function next to `Assert`), as in node: `class AssertionError extends Error`.
Its prototype inherits `Error.prototype` and has node's `toString`
(`AssertionError [ERR_ASSERTION]: message`), the constructor inherits `Error`,
and every AssertionError, thrown by an assert helper or constructed, is born
linked to that prototype, so `instanceof Error`, `instanceof AssertionError`
and `constructor` follow the prototype chain instead of a registered class id.
`new` reaches the constructor through any value of it (`new ns.AssertionError(o)`,
`new (require("assert").AssertionError)(o)`, `new Assert()`), and a class can
extend it. The codegen special case for `new <assert>.AssertionError(...)` is gone.
