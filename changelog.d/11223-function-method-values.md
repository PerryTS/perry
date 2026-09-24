Fix inherited `Function.call`, `Object.call`, and other function `call`/`apply`/`bind` value reads returning receiver-bound wrappers instead of the shared `Function.prototype` methods (#11175). Ordinary and reflective reads now preserve method identity and support the `Function.call.bind(fn)` uncurry idiom. Prototype replacements and getters use the actual function as their receiver. Add runtime identity and native parity regressions for borrowed calls, uncurrying, overrides, reflection, and callable proxies.

Honor explicit null/custom function prototype chains. Validate proxy callability before invoking traps, and convert `apply` argument lists into fresh arrays before proxy dispatch, preserving array-like getter order and preventing traps from mutating the caller's input array.

Read Proxy-wrapped `apply` argument arrays through their property traps instead of treating Proxy handles as raw arrays. Cover wrapped and nested arrays, trap ordering, and revoked argument proxies.
