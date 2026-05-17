// Regression for the pino-downstream `diagnostics_channel.tracingChannel`
// surface (follow-up to #906). Pre-fix `require('node:diagnostics_channel')`
// from a CJS-wrapped compiled package returned a default-import that the
// codegen catch-all NaN-boxed as `TAG_TRUE`, so the very first property
// access (`.tracingChannel`) threw
//   TypeError: (boolean).tracingChannel is not a function
// at top-level module init of `pino/lib/tools.js`, halting the whole
// program before `pino()` was ever called.
//
// The fix wires `node:diagnostics_channel` through the same node-submodule
// runtime helpers Perry already ships for `node:timers/promises` &
// friends. Default imports of this specific submodule route to the
// NAMESPACE stub (not the function-singleton form) so `diagChan` is a
// real object whose `tracingChannel` slot is a callable thunk. That
// thunk returns a stub object whose `hasSubscribers` is `false`, which
// is exactly what pino tests before deciding to enter its tracing branch.

import * as diagnostics_channel from "node:diagnostics_channel";

// 1. The module surface is an object, not a boolean / function.
console.log("typeof diagnostics_channel:", typeof diagnostics_channel);

// 2. `tracingChannel` is callable.
console.log("typeof tracingChannel:", typeof diagnostics_channel.tracingChannel);

// 3. Calling it returns an object (not undefined / boolean).
const tc = diagnostics_channel.tracingChannel("pino_asJson");
console.log("typeof tracingChannel(...):", typeof tc);

// 4. The pino fast-path predicate must read `false`. If this is anything
//    else, pino's `asJson` falls into `traceSync` and downstream crashes.
console.log("tc.hasSubscribers:", tc.hasSubscribers);
console.log("tc.hasSubscribers === false:", tc.hasSubscribers === false);

// 5. `traceSync` / `tracePromise` / `traceCallback` slots are functions
//    so `typeof` gates pass even if a consumer doesn't gate on
//    `hasSubscribers` first.
console.log("typeof traceSync:", typeof tc.traceSync);
console.log("typeof tracePromise:", typeof tc.tracePromise);
console.log("typeof traceCallback:", typeof tc.traceCallback);

// 6. `channel(name)` mirrors the same shape.
const ch = diagnostics_channel.channel("pino_test");
console.log("typeof channel(...):", typeof ch);
console.log("ch.hasSubscribers:", ch.hasSubscribers);
console.log("typeof ch.publish:", typeof ch.publish);
