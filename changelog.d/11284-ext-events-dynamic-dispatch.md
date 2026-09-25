An EventEmitter reached through a receiver the compiler cannot type now
dispatches to the right emitter under the default `node:events` routing
(perry-ext-events) when the prebuilt stdlib is linked (`PERRY_NO_AUTO_OPTIMIZE=1`,
or any install that cannot rebuild the runtime). Such receivers include an
element read out of an `EventEmitter[]`, a `const e = arr[i]` alias, an `any`,
and `new events.EventEmitter()` through a namespace import (#11270, #11273).
Before this fix, `.on`/`.emit` silently did nothing, method values such as
`typeof e.on` read as `undefined`, and `events.once` on such an emitter never
settled.

The cause: perry-stdlib's dynamic EventEmitter mapping calls `extern "C"`
declarations of the `js_event_emitter_*` symbols so that the linker picks the
linked implementation (#4995). But the prebuilt full stdlib is built with
`bundled-events`, which defines those same symbols, so rustc bound the calls to
perry-stdlib's own in-crate copies. Their registry never holds an ext-events
handle. perry-ext-events now registers its own method and method-value
dispatchers with the runtime (`js_register_event_emitter_{method,property}_dispatch`),
and perry-stdlib consults them before its in-crate mapping. The dispatchers claim
a call only for a handle that is live in the ext registry.
`test_gap_events_import_4995` now also passes against the ext-events archive.
Gap test: `test_gap_11270_emitter_from_array.ts`.
