// Issue #2210 — `http.createServer(handler, options)` accepts a 2nd
// options arg (Node 18.4+) and exposes the six numeric timeout knobs
// as readable + writable instance properties. Pre-#2210 every
// property read returned NaN and every property write threw "value is
// not a function" because the typed-feedback fallback didn't model
// them.
//
// The parity check here only pins the property-setter path: Node's
// per-prop *initial* read after the options-form constructor is
// inconsistent (some defaults reflect the option, some return null /
// the prototype default), so a strict "options round-trip" check
// would diverge through no fault of the fix. The Perry-side
// options-round-trip is covered by a Rust unit test in
// perry-ext-http-server::server.
//
// Phase 1 (this PR) stores + reads back the values; Phase 2 wires
// them to hyper's connection lifecycle, tracked under the same issue.
import { createServer } from "node:http";

const server = createServer((_req: any, res: any) => res.end("ok"));

server.headersTimeout = 0;
server.keepAliveTimeout = 0;
server.requestTimeout = 60_000;
server.timeout = 120_000;
server.maxHeadersCount = 2000;
server.maxRequestsPerSocket = 0;

console.log("headersTimeout:", server.headersTimeout);
console.log("keepAliveTimeout:", server.keepAliveTimeout);
console.log("requestTimeout:", server.requestTimeout);
console.log("timeout:", server.timeout);
console.log("maxHeadersCount:", server.maxHeadersCount);
console.log("maxRequestsPerSocket:", server.maxRequestsPerSocket);

// `server.setTimeout(ms, cb)` — canonical EventEmitter-style setter.
// Returns the server for chaining; updates the `timeout` accessor.
const chained = server.setTimeout(45_000, () => {});
console.log("chained === server:", chained === server);
console.log("post-setTimeout timeout:", server.timeout);
