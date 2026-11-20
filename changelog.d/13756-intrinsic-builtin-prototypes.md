Every builtin family answers its realm intrinsic prototype (ECMA-262 Realm
[[Intrinsics]]) rather than the `prototype` of whatever `globalThis.<Name>`
currently holds: arrays, typed arrays, every Error kind, Date, Promise,
ArrayBuffer, SharedArrayBuffer, DataView, Map, Set, WeakMap, WeakSet, WeakRef,
FinalizationRegistry, RegExp, the boxed primitives and Iterator. After
`globalThis.Date = function F() {}`, an existing date is no longer
`instanceof Date` and `Object.getPrototypeOf(date)` is still the original
`Date.prototype`, as in node.
The realm entry also holds each family's original constructor, so inherited
`constructor` reads (`promise.constructor`, `ta.buffer.constructor`, ...),
species defaults and primitive method lookups answer `%Promise%`, `%Array%`,
`%String.prototype%`, ... after the global binding is replaced.
