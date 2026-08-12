Fixed the module-level `node:events` helpers returning `undefined` whenever they
were reached INDIRECTLY. `events.listenerCount(e, "x")` was correct, but
`const c = events.listenerCount; c(e, "x")`, `(events as any).listenerCount(e,
"x")` and `events.listenerCount(...args)` all silently answered `undefined`.

`nm_dispatch_events` had arms for exactly two names — `init` and
`EventEmitterAsyncResource` — and everything else fell to `_ => undefined`. The
seven module-level helpers (`listenerCount`, `once`, `on`, `getEventListeners`,
`getMaxListeners`, `setMaxListeners`, `addAbortListener`) are implemented in
perry-stdlib, which depends on perry-runtime, so the dispatch bucket cannot name
them; the static call reaches them directly through codegen's `NativeModSig`
rows, which is why only the indirect forms were dead. Added the
registered-pointer bridge every comparable module already has (zlib /
querystring / domain): `JS_NATIVE_EVENTS_DISPATCH` +
`js_set_native_events_dispatch` in perry-runtime and `js_events_native_dispatch`
in perry-stdlib, wired at `js_stdlib_init_dispatch`. Argument marshalling matches
the static path's rows — event names go through `ToString`, and
`setMaxListeners(n, ...targets)` rebuilds its trailing targets into the single
array the helper expects. Distinct from the existing `JS_NATIVE_EVENTS_CONSTRUCT`,
which serves only `new`.

Follow-up to #7734, which named this as the one remaining wrong-to-wrong case in
the #7720 spread-call matrix: `events.listenerCount(...args)` turned a bogus
`ERR_INVALID_ARG_TYPE` throw into `undefined`. It now returns the count.

`events_dispatch_parity_tests` walks the real `NET_EVENTS_ROWS` table and fails
on any `has_receiver: false` `events` row that is not classified as
routed-to-stdlib, answered-by-runtime, or deliberately-unrouted — the drift that
caused this bug — with a second test rejecting stale entries so the
classification cannot rot. `events.on`'s async ITERATION is deliberately not
asserted: it drops its first value in the static form too, on a tree without this
change (`events/on/async-iterator-abort` and `events/on/validation` are already
red for it); the helper is routed regardless, so it inherits the fix when that
gap closes.
