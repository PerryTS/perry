### Fixed

- **An inherited Symbol-keyed accessor now runs with the original receiver, not `undefined`.**
  `obj[sym]` where the getter/setter lives on a prototype (`Object.defineProperty(Fn.prototype, sym,
  ...)`, an object-literal `get [sym]()` reached through `Object.create`, or a declared class
  prototype) used to invoke the accessor with no receiver at all, so it observed whatever `this`
  happened to be ambient — `undefined` at module top level. fastify 5.10.0's
  `Reply.prototype[kRouteContext]` getter crashed every HTTP request with `TypeError: Cannot read
  properties of undefined (reading 'request')`. `[[Get]]`/`[[Set]]` now thread the read/write's
  actual receiver through every prototype-chain walk (`crates/perry-runtime/src/symbol/get.rs`,
  `object/class_registry/prototype_objects.rs`); `Reflect.get`/`Reflect.set` for a Symbol key reach
  the receiver-aware entry points directly. An inherited *setter* is now consulted too —
  `obj[sym] = v` used to silently shadow it with a new own data property instead of running it —
  gated by a symbol-id-keyed accessor filter (`symbol_may_have_accessor`) so the common no-accessor
  write path stays cheap.
