`import * as traceEvents from "node:trace_events"` now exposes `createTracing`
and `getEnabledCategories` on the namespace object. The ESM namespace module
forwards the API manifest's public names, and `node:trace_events` had no
manifest surface, so the namespace held only `default` and
`traceEvents.createTracing(...)` threw "createTracing is not a function" (native
and WASI; it failed the WASI `process_id` acceptance test). The manifest now
declares the module and its two exports, and a unit test checks that every
export the runtime implements for a Node builtin submodule is forwarded by its
ESM namespace.
