Fixed a constructor-less subclass of a native base (`AsyncResource`,
`AsyncLocalStorage`, `EventEmitter`, `EventEmitterAsyncResource`, `LRUCache`,
`WebSocketServer`, the genuine `node:stream` classes) losing its `super()`
argument forwarding and native-surface install inside a CommonJS-wrapped
module — the shape real npm packages use. `const { AsyncResource } =
require("node:async_hooks")` is a genuine local there (the whole module body
runs inside the CJS wrap's IIFE), which class-heritage resolution could not
tell apart from a real user shadow of the same name, so it fell back to a
generic dynamic-value dispatch. For a base whose runtime value is a real ES
`class` (`AsyncResource`, `AsyncLocalStorage`), that dispatch called the value
without `new` and threw; for an old-style-function base (`EventEmitter`, the
stream classes) it happened to work, through a much slower indirect path
(measured ~5.5x more instructions per construction than the direct native
path). Class-heritage resolution now tracks a `require()`-destructured
binding's provenance and only treats it as shadowing when it did NOT come
from the real native module.
