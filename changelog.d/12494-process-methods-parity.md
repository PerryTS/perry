Point `process.ref` / `process.unref` at the timer registry through the single
brand-checked decoder from a JS timer handle to its registry id
(`timer::timer_handle_parts`), instead of a private copy that still decoded the
pre-#340/#341 model — a `POINTER_TAG`-boxed small integer whose low 48 bits are
the id. Since #340/#341 the handle is an ordinary object carrying its id in
`ObjectMeta.native_state`, so the old mask produced the handle's heap address,
the liveness gate always refused, and both entry points were silent no-ops:
`process.unref(timeout)` left `timeout.hasRef()` true. That helper was the last
consumer of the stale decode; its accessor's re-export moves out of the
test-only cfg, which the new lib-build consumer now justifies.

Stop a native-module namespace from being answered by the recorded-shape
prototype walk in `shape_inherited::get`. The gate there admitted a namespace
because it shares the ordinary object LAYOUT, but not the ordinary read
SEMANTICS: a namespace resolves against the module table keyed by its module
name, not the shape's key list (which holds only `__module__`). Only
`cluster.default` carries a recorded `[[Prototype]]` — `EventEmitter.prototype`,
wired for the #3687 default-import surface — so only
`process.getBuiltinModule('cluster')` was affected: the chain walk answered
`isPrimary`, `setupPrimary`, `Worker` and `fork` with `undefined` before the
namespace vtable's own-field resolver ran, while `Object.keys` matched on its
separate own-keys path. Every prototype-less namespace already declined the same
gate further down, so those reads simply return to the tail that owns the export
table, and a name that misses both still continues at the namespace's
`[[Prototype]]` (#11542).

Observable in the node-suite faithful lane (auto-optimize on, Node 26.5.1
oracle): `process/methods/ref-unref` and
`process/methods/get-builtin-module-transport-surfaces` now match node, taking
the `process` module sweep from 103/105 to 105/105 with `globals` (117/7) and
`events` (65/5) unchanged, failure sets identical.
