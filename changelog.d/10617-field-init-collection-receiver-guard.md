### Fixed

A class whose direct parent is a built-in (`Error`/`TypeError`/other non-user base) and that
declares its own `super()`-calling constructor now runs its own field initializers. Every other
non-user-parent `super()` arm already did this after the base constructor returns; the
`Error`-family arm was the one that skipped it, so `class E extends Error { labels =
new Set(); constructor(m) { super(m); } }` left `labels` `undefined`.

Calling a method (`.add`/`.set`/`.get`/`.has`/`.delete`/`.clear`/`.forEach`/...) on a
statically-typed `Set<T>`/`Map<K, V>` value that holds `undefined`, `null`, or another
primitive at runtime now throws a catchable `TypeError` instead of segfaulting. The static-type
fast path unboxed the receiver's declared-type payload with no tag check; a receiver guard now
runs first, on the fast/common path costing one compare of the receiver's tag bits.

Together these fixed a crash in the mongodb 7.5.0 driver: `MongoError`'s
`errorLabelSet: Set<string>` field was left `undefined` by the first bug, and
`addErrorLabel()` calling `.add()` on it segfaulted via the second, about 100ms after
`client.connect()`.
