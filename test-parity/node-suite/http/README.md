# `node:http` granular parity suite

This directory holds small print-and-diff fixtures for HTTP/1 contracts that
`node:http` owns. The oracle is Node.js 26.5.0. Network fixtures bind only
`127.0.0.1` on port `0`, use bounded payloads, and close their servers, agents,
requests, and sockets through observable lifecycle barriers.

## Fixed upstream evidence

The audit used these primary sources:

- Node.js 26.5.0 (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`): all 414
  HTTP-named tests under `test/parallel` and `test/sequential`, plus
  `lib/http.js`, `lib/_http_agent.js`, `lib/_http_client.js`,
  `lib/_http_common.js`, `lib/_http_incoming.js`, `lib/_http_outgoing.js`, and
  `lib/_http_server.js`. The selected contracts trace to the upstream
  `test-http-methods`, `test-http-status-code`, `test-http-agent`,
  `test-http-agent-getname`, `test-http-agent-keepalive`,
  `test-http-request-url`, `test-http-request-invalid-method-error`,
  `test-http-invalidheaderfield`, `test-http-server-options`,
  `test-http-server-close`, `test-http-expect-continue`,
  `test-http-expectation-failed`, `test-http-join-duplicate-headers`,
  `test-http-trailers`, and `test-http-write-head` families.
- Deno (`34c46613cbe20450b74c0e8d4f0fd8f6f781d807`):
  `ext/node/polyfills/http_esm.ts`, `_http_agent.js`, `_http_client.js`,
  `_http_incoming.js`, `_http_outgoing.ts`, `_http_server.js`, and the current
  `tests/unit_node/http_test.ts` and `tests/specs/node/http_*` selections.
  Runtime comparison used Deno 2.9.3.
- Bun 1.2.18 (`0d4089ea7c48d339e87cc48f1871aeee745d8112`): the `bun-v1.2.18`
  `src/js/node/http.ts` and `_http_*` implementation, dedicated
  `test/js/node/http` cases, and selected `test-http-*` cases under
  `test/js/node/test/parallel`. Runtime comparison used the matching local
  1.2.18 build.

Node 26.5.0 also served as the exact output oracle. The audit compared the
implementation before choosing a fixture, then checked the same fixture under
Deno and Bun. We did not add a case just because upstream had many files.

## Coverage

The suite contains 54 fixtures, up from 19:

| Area                                 | Fixtures | Contracts                                                                                                                                           |
| ------------------------------------ | -------: | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| exports                              |        9 | methods, status codes, helper tail, classes, inheritance, descriptors, WebSocket event constructors, and receiver checks                            |
| request                              |        9 | URL and options overloads, `get()` auto-end, callback order, method/path/protocol/agent/signal validation, and coercion                             |
| `ClientRequest`                      |        5 | constructor state, header operations and validation, `flushHeaders()`, aliases, controls, and clean errors                                          |
| `IncomingMessage`                    |        4 | constructor defaults, request and response metadata, raw/distinct headers, encoding, body state, and trailers                                       |
| `ServerResponse` / `OutgoingMessage` |        5 | header mutation, `setHeaders()`, status and informational validation, and lifecycle state                                                           |
| `Server`                             |       13 | construction, options, parser knobs, listen/close, expectations, response surfaces, connection headers, trailers, timeout state, and async disposal |
| `Agent`                              |        9 | defaults, options, validation, names, idle maps, socket hooks, keep-alive reuse, pool state, and destroy                                            |

The adjacent suites keep ownership of generic behavior:

- `net`: socket/server construction, connect/listen validation, and raw TCP
  lifecycle;
- `stream`: readable/writable semantics, backpressure, encoding, and destroy
  rules not specific to HTTP messages;
- `events`: generic listener order, aliases, rejection capture, and listener
  mutation;
- `url`: URL parsing and formatting outside HTTP request overloads;
- `https` and `tls`: TLS agents, certificates, handshakes, ALPN, and secure
  transport;
- `http2`: sessions, streams, settings, and HTTP/2 compatibility messages.

## Repeated comparison

All final runtime comparisons ran three times. Each runtime produced the same
classification and output digest on every run:

| Runtime     | Pass | Diff | Error | Compile failure | Crash | Timeout |
| ----------- | ---: | ---: | ----: | --------------: | ----: | ------: |
| Node 26.5.0 |   54 |    0 |     0 |               0 |     0 |       0 |
| Perry       |   24 |   29 |     0 |               1 |     0 |       0 |
| Deno 2.9.3  |   43 |    7 |     4 |             n/a |     0 |       0 |
| Bun 1.2.18  |   23 |   27 |     4 |             n/a |     0 |       0 |

Deno's four clean errors come from missing Node 26.5.0 export-tail entries. Its
seven output differences are limited to server disposal, close errors, message
surface state, parser options, timeout defaults, and response status validation.
Bun's stable gaps span Agent pooling, request/message state, export descriptors,
parser options, response header mutation, and server lifecycle. Neither
comparison produced a crash or timeout.

Perry's one compile failure is the missing `CloseEvent` named export in
`exports/event-constructors.ts`. Its stable output differences group around
Agent pool fields, class and descriptor shape, request validation and state,
parser options, and response header/status validation.

## Exclusions and stopping rule

The audit rejected or removed cases that need external DNS, fixed ports,
arbitrary timers, scheduler races, large payloads, resource pressure, GC,
signals, inspector/tracing, external proxies, or hard-to-reproduce kernel
errors. It also excludes:

- parser fuzzing, request smuggling, ambiguous malformed bytes, and large
  header/body limits;
- generic WebSocket frames and protocol behavior; this suite keeps only the
  public `node:http` constructor/export contract;
- raw `clientError`, upgrade, and CONNECT probes when every tested runtime
  cannot close both endpoints through an event barrier;
- raw duplicate-header probes whose Bun request event is not stable across
  repeated runs; parser-option validation still covers this area without raw
  bytes;
- abort, request-trailer, and transport-failure probes that leave a live handle
  or timeout in any comparison runtime;
- keep-alive stress, scheduling races, stale-socket retries, proxy environment
  behavior, Unix sockets, and host-specific error text.

The suite stops here because each remaining deterministic HTTP behavior either
matches a contract the suite already represents, belongs to an adjacent suite,
or needs a transport/parser harness that can guarantee bounded cleanup across
Node, Perry, Deno, and Bun.
