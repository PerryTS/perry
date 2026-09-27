### Fixed

- **A built-in module namespace now inherits from its prototype.** Every read on
  a namespace object (`require("process")`, `require("path")`, ...) that missed
  the module's exports answered `undefined` instead of continuing at the
  namespace's `[[Prototype]]`. So `ns.constructor`, `ns.hasOwnProperty` and
  `ns.toString` were all `undefined`, and so was every read that inherits
  *through* a namespace. `Object.create(require("process")).constructor` was
  `undefined`, and rolldown's `__toESM` prelude threw `TypeError: Cannot convert
  undefined or null to object`. `"constructor" in ns` answered `false` for the
  same reason. Both `[[Get]]` and `[[HasProperty]]` now fall back to the
  prototype that `Object.getPrototypeOf(ns)` reports, which is the recorded one
  or `Object.prototype`. Own exports still shadow it, and `hasOwnProperty` stays
  own-only. Fixes #11542.
