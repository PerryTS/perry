**perf/parity: EventEmitter methods live on one shared prototype (Refs #10508)**

`class X extends EventEmitter`, `util.inherits` + `EventEmitter.call(this)`,
`Object.create(EventEmitter.prototype)` and `setPrototypeOf` mixins no longer
get the 15 emitter methods stamped onto every instance as own closures. As in
node, `super()` / `EventEmitter.call(this)` run `EventEmitter.init`: the
instance owns `_events` (a null-prototype object), `_eventsCount` and
`_maxListeners`, and the methods are inherited from `EventEmitter.prototype`,
which carries node's defaults (`_events: undefined`, `_eventsCount: 0`,
`_maxListeners: undefined`) and its aliases (`addListener === on`,
`off === removeListener`).

- `Object.keys(new Sub())` is `["_events","_eventsCount","_maxListeners"]`,
  `hasOwnProperty('on')` is false and `sub.on === EventEmitter.prototype.on`,
  matching node.
- The listener store is node's: `_events[type]` holds one function or an
  array; a `once` listener is a wrapper with `.listener`; `rawListeners`,
  `listeners`, `eventNames`, `newListener` / `removeListener` and
  `removeAllListeners` follow `lib/events.js`, so code that reads or edits
  `_events` directly sees the same state.
- A subclass override still wins, and `super.emit()` / `super.on()` reach the
  base through the prototype chain (the #6316 behavior, without the
  per-instance stash).
- `super({ captureRejections: true })` now works for subclasses: an async
  listener's rejection reaches `[Symbol.for('nodejs.rejection')]` or
  `emit('error')`; a non-boolean value throws `ERR_INVALID_ARG_TYPE`.

Instructions per operation (t508, `class Q extends EventEmitter` with 12
fields): `new` 151.3k -> 77.1k (RSS 108 MB -> 47 MB), `emit` 51.1k -> 34.2k,
`this.m()` 190 -> 190. Commander `parse_argv` 13.39G -> 10.43G instructions
(-22.1%); tsc and Zod unchanged.

Plain `new EventEmitter()` is still a handle (its own keys are still `[]`).
That is the next step.
