`new` on a cross-module, constructor-less subclass of a native base installs the
native surface.

A no-own-constructor class whose chain reaches one of the native bases perry
stamps onto the instance (`EventEmitter`, `Map`/`Set`, `Event`/`CustomEvent`,
`AsyncLocalStorage`, …) got that surface from the inline `new` lowering, but the
standalone `<class>_constructor` symbol never emitted it — and that symbol is the
body every cross-module `new` and every dynamic construct replay runs. So
`export class KeyHandler extends EventEmitter {}` constructed from another module
came back bare and `handler.on("keypress", …)` threw "on is not a function".

The base init is now emitted from the synthesized constructor too, at the same
spec position: after the implicit `super(...args)`, before the class's own field
initializers. `Array` is deliberately excluded — its base init reads the
forwarded value as a LENGTH, and this symbol's parameters are compiler-generated
forwarding slots, so feeding them to the array subclass init turned `new Sub()`
into a 9-element array. The inline `new` path keeps owning `Array`.
