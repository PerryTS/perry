### Fixed

- **`arr[i] === void 0` (and other undefined-valued comparisons) against an
  out-of-bounds or hole read of a `var`-declared number array always
  compiled `false`, and `!==` always `true`.** A hoisted `var` lowers to two
  HIR declarations sharing one local id (a body-entry predefine, then the
  real declaration); the codegen redeclaration path refreshed the numeric
  type PROOF used by `is_numeric_expr` but left the declared-type map used
  by the boxed-fallback hazard guard stale at `Any`. The two disagreed about
  the same local, so the hazard guard never caught the case and a strict
  equality compare against the array element compiled to a bare `fcmp` —
  which cannot represent the NaN-boxed `undefined` tag a hole/out-of-bounds
  read actually produces. Both are now kept in sync on every `var`
  redeclaration. This was blocking `decimal.js`'s `toHexadecimal`/
  `toBinary`/`toOctal` (`convertBase`'s carry-slot initialization check).
