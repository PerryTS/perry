`new EventEmitter()` and `new EventEmitterAsyncResource()` now return an
ordinary object on the shared `EventEmitter.prototype`, the same kind of value a
subclass instance is (#10508, #11919).

Listener state is node's own `_events` / `_eventsCount` / `_maxListeners`
properties, so the collector traces and moves it like any other object, and
`instanceof`, prototype identity and `Object.getPrototypeOf` behave as in node.
`EventEmitterAsyncResource.prototype` now chains to `EventEmitter.prototype`.

Node behaviours the handle path lacked:
- `ERR_UNHANDLED_ERROR` for a non-Error `'error'` event.
- `listenerCount(type, listener)`.
- `EventEmitter.defaultMaxListeners`.
- `MaxListenersExceededWarning`.
- Node's exact listener-type message.
- domain annotations on an unhandled `'error'`.

`events.once`, `events.on`, `getEventListeners`, `listenerCount` and
`get/setMaxListeners` work on any emitter through its own methods.

Removed: the `EventEmitterHandle` providers (perry-stdlib's events core and the
`perry-ext-events` crate), the runtime emitter hook registry, the
`external-events-construct` stdlib feature, and the events native-table rows.
`commander` runs 1.2% fewer instructions. A plain emitter's `new` / `on` / `off`
still costs more than the old handle did; that cost is in the object model's
per-object work.
