## Suppress redundant promise-drain wakeups

Promise and `queueMicrotask` work queued by the main thread while it is already
draining microtasks no longer signals the event loop. Cross-thread producers,
timer callbacks, and rejection processing retain the normal wake path.

`Promise.all` over plain native promises also uses compact settlement records
instead of allocating a closure and `AlreadyCalled` array for every element;
observable constructor, resolver, `then`, and lifecycle-hook paths still use
the spec dispatch.
