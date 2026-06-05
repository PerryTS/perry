// Issue #2210/#2545 — `http.createServer(options)` accepts Node's
// timeout options and exposes the numeric timeout knobs as readable +
// writable instance properties. Pre-#2210 every property read returned
// NaN and every property write threw "value is not a function" because
// the typed-feedback fallback didn't model them.
//
// Most constructor-option round-trips stay covered by Rust unit tests;
// `keepAliveTimeoutBuffer` is pinned here because Node reflects it
// consistently.
//
// Phase 1 (this PR) stores + reads back the values; Phase 2 wires
// them to hyper's connection lifecycle, tracked under the same issue.
import { createServer } from "node:http";

const server = createServer((_req: any, res: any) => res.end("ok"));

console.log(
  "default keepAliveTimeoutBuffer:",
  server.keepAliveTimeoutBuffer,
  typeof server.keepAliveTimeoutBuffer,
);
console.log("default listening:", server.listening, typeof server.listening);
console.log(
  "default maxHeadersCount:",
  String(server.maxHeadersCount),
  typeof server.maxHeadersCount,
  server.maxHeadersCount === null,
);

server.headersTimeout = 0;
server.keepAliveTimeout = 0;
server.keepAliveTimeoutBuffer = 250;
server.requestTimeout = 60_000;
server.timeout = 120_000;
server.maxHeadersCount = 2000;
server.maxRequestsPerSocket = 0;

console.log("headersTimeout:", server.headersTimeout);
console.log("keepAliveTimeout:", server.keepAliveTimeout);
console.log("keepAliveTimeoutBuffer:", server.keepAliveTimeoutBuffer);
console.log("requestTimeout:", server.requestTimeout);
console.log("timeout:", server.timeout);
console.log("maxHeadersCount:", server.maxHeadersCount);
console.log("maxRequestsPerSocket:", server.maxRequestsPerSocket);

// `server.setTimeout(ms, cb)` — canonical EventEmitter-style setter.
// Returns the server for chaining; updates the `timeout` accessor.
const chained = server.setTimeout(45_000, () => {});
console.log("chained === server:", chained === server);
console.log("post-setTimeout timeout:", server.timeout);

const optionsServer = createServer({ keepAliveTimeoutBuffer: 321 } as any);
console.log("options keepAliveTimeoutBuffer:", optionsServer.keepAliveTimeoutBuffer);
