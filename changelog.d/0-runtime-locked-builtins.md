Built-in prototypes are read-only in a compiled program (decision 250).
Replacing, deleting or redefining a member the runtime gives a built-in
prototype (`Object.prototype`, `Array.prototype`, `String.prototype`,
`Map.prototype`, `Date.prototype`, …) now throws a `TypeError` at the store
(`Array.prototype.push = …`, `delete Object.prototype.toString`,
`Object.defineProperty(Array.prototype, "push", …)`,
`Object.setPrototypeOf(Array.prototype, …)`), in compiled code and in code the
interpreter runs. Adding new members works (`Array.prototype.myHelper = …`), and
`constructor` stays patchable. Namespace and constructor members (`Math.random`,
`Date.now`, `JSON.stringify`), the global bindings, `console`, `process`, `fetch`
and `globalThis` stay patchable.

Compatibility: polyfills that only add keep working, including a second copy of
the same polyfill; a shim that overwrites an existing prototype method must be
removed or guarded. "No code rewrites a built-in prototype member" is true by
construction, so prototype-method calls stay direct.
