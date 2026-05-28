// Issue #2210 — `http.createServer(handler, options)` accepts the
// Node 18.4+ options bag, and the resulting `Server` round-trips
// `headersTimeout` / `keepAliveTimeout` / `requestTimeout` / `timeout` /
// `maxHeadersCount` / `maxRequestsPerSocket` / `noDelay` / `keepAlive` /
// `keepAliveInitialDelay` through both reads and writes.
//
// Phase 1: round-trip via the runtime-side `HttpServerOptions` struct.
// Phase 2 (separate work) wires the values into hyper's connection
// lifecycle so the timeouts actually fire.

import { createServer } from "node:http";

const handler = (_req: any, _res: any) => {};

// Defaults — bare `createServer(handler)` should populate Node's
// documented defaults so user code reading the knobs before any
// `.listen()` already sees the right numbers.
const defaults = createServer(handler);
console.log("defaults.headersTimeout:", defaults.headersTimeout);
console.log("defaults.keepAliveTimeout:", defaults.keepAliveTimeout);
console.log("defaults.requestTimeout:", defaults.requestTimeout);
console.log("defaults.timeout:", defaults.timeout);
console.log("defaults.maxHeadersCount:", defaults.maxHeadersCount);
console.log("defaults.maxRequestsPerSocket:", defaults.maxRequestsPerSocket);
console.log("defaults.keepAliveInitialDelay:", defaults.keepAliveInitialDelay);
console.log("defaults.noDelay:", defaults.noDelay);
console.log("defaults.keepAlive:", defaults.keepAlive);

// createServer(handler, options) — the constructor-side overload that
// the issue (and Node's `test/parallel/test-http-server-keep-alive-*`
// fixtures) reach for. Every key here should override its default.
const configured = createServer(handler, {
  headersTimeout: 11_000,
  keepAliveTimeout: 22_000,
  requestTimeout: 33_000,
  timeout: 44_000,
  maxHeadersCount: 55,
  maxRequestsPerSocket: 66,
  keepAliveInitialDelay: 77,
  noDelay: false,
  keepAlive: true,
});
console.log("configured.headersTimeout:", configured.headersTimeout);
console.log("configured.keepAliveTimeout:", configured.keepAliveTimeout);
console.log("configured.requestTimeout:", configured.requestTimeout);
console.log("configured.timeout:", configured.timeout);
console.log("configured.maxHeadersCount:", configured.maxHeadersCount);
console.log("configured.maxRequestsPerSocket:", configured.maxRequestsPerSocket);
console.log("configured.keepAliveInitialDelay:", configured.keepAliveInitialDelay);
console.log("configured.noDelay:", configured.noDelay);
console.log("configured.keepAlive:", configured.keepAlive);

// Setter round-trip — the property-set form (`server.foo = N`) must
// flow through to the same storage the getter reads. This is what
// the cited `test-http-server-keep-alive-timeout*` neighbours do
// to set per-test timeouts before listen().
defaults.keepAliveTimeout = 0;
defaults.headersTimeout = 0;
defaults.requestTimeout = 60_000;
defaults.timeout = 120_000;
defaults.maxHeadersCount = 2000;
defaults.maxRequestsPerSocket = 99;
defaults.keepAliveInitialDelay = 1000;
defaults.noDelay = false;
defaults.keepAlive = true;
console.log("after-set.keepAliveTimeout:", defaults.keepAliveTimeout);
console.log("after-set.headersTimeout:", defaults.headersTimeout);
console.log("after-set.requestTimeout:", defaults.requestTimeout);
console.log("after-set.timeout:", defaults.timeout);
console.log("after-set.maxHeadersCount:", defaults.maxHeadersCount);
console.log("after-set.maxRequestsPerSocket:", defaults.maxRequestsPerSocket);
console.log("after-set.keepAliveInitialDelay:", defaults.keepAliveInitialDelay);
console.log("after-set.noDelay:", defaults.noDelay);
console.log("after-set.keepAlive:", defaults.keepAlive);
