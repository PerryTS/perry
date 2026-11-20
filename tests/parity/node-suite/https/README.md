# `node:https` granular parity coverage

This directory contains 47 independent print-and-diff fixtures for contracts
owned by `node:https`. Node.js 26.5.0 is the oracle. Network fixtures bind only
`127.0.0.1` on port `0`, use fixed repository certificates, and close every
request, socket, agent, and server from an event barrier rather than a sleep.

## Pinned sources

- Node.js 26.5.0, commit
  [`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb):
  [`lib/https.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/https.js)
  and the HTTPS-related files under
  [`test/parallel`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel).
- Deno commit
  [`34c46613cbe20450b74c0e8d4f0fd8f6f781d807`](https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807):
  [`ext/node/polyfills/https.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/ext/node/polyfills/https.ts),
  [`tests/node_compat/config.jsonc`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/node_compat/config.jsonc),
  and
  [`tests/unit_node/https_test.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/unit_node/https_test.ts).
- Bun commit
  [`44f6469e0d4ae93467aa65c7e3bc9001000c7b31`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31):
  [`src/js/node/https.ts`](https://github.com/oven-sh/bun/blob/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/src/js/node/https.ts),
  its dedicated
  [`test/js/bun/test/parallel`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/test/js/bun/test/parallel)
  HTTPS cases, and its selected Node tests under
  [`test/js/node/test/parallel`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/test/js/node/test/parallel).

The exact Node tree has 71 `test/parallel` filenames containing `https`: 64
start with `test-https-`; seven belong to `http`, `http2`, `permission`, or
`tls`. The earlier 70-file estimate missed one cross-module filename. File
counts are only an audit input; the suite keeps semantic contracts, not copies.

## Coverage and traceability

| Fixture                                        | Contract                                                      | Primary Node source                                                                             |
| ---------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `exports/module-keys.ts`                       | ESM namespace keys                                            | `lib/https.js` exports                                                                          |
| `exports/named-aliases.ts`                     | Named exports alias namespace values                          | `lib/https.js` exports                                                                          |
| `exports/descriptors.ts`                       | Export value types and descriptors                            | `lib/https.js` exports                                                                          |
| `classes/agent-inheritance.ts`                 | `Agent` callability and `http.Agent` inheritance              | `lib/https.js` `Agent` prototype setup; `test-https-agent-constructor.js`                       |
| `classes/agent-prototype.ts`                   | HTTPS-owned `Agent.prototype` methods and descriptors         | `lib/https.js` `Agent.prototype`                                                                |
| `classes/server-inheritance.ts`                | `Server` callability and `tls.Server` inheritance             | `lib/https.js` `Server` prototype setup                                                         |
| `classes/server-prototype.ts`                  | HTTPS-owned `Server.prototype` methods and descriptors        | `lib/https.js` `Server.prototype`                                                               |
| `agent/default-state.ts`                       | New-agent HTTPS defaults                                      | `lib/https.js` `Agent`; `test-https-agent-constructor.js`                                       |
| `agent/options-state.ts`                       | HTTPS agent option overrides                                  | `lib/https.js` `Agent`; `test-https-agent-additional-options.js`                                |
| `agent/global-default-state.ts`                | `globalAgent` identity and HTTPS defaults                     | `lib/https.js` `globalAgent`; `test-https-client-override-global-agent.js`                      |
| `agent/inherited-methods.ts`                   | Inherited agent method values                                 | `lib/https.js` `Agent` inheritance                                                              |
| `agent/detached-get-name.ts`                   | Detached `getName` receiver behavior                          | `lib/https.js` `getName`; `test-https-agent-getname.js`                                         |
| `agent/get-name-defaults.ts`                   | Default HTTPS cache key suffix                                | `test-https-agent-getname.js`                                                                   |
| `agent/get-name-servername.ts`                 | SNI key omission and separation                               | `test-https-agent-getname.js`                                                                   |
| `agent/get-name-tls-options.ts`                | TLS options participate in the cache key                      | `test-https-agent-getname.js`                                                                   |
| `agent/session-cache-defaults.ts`              | Session-cache shape and default limit                         | `lib/https.js` `_sessionCache`                                                                  |
| `agent/session-cache-disabled.ts`              | A zero limit blocks cache insertion                           | `lib/https.js` `_cacheSession`; `test-https-agent-disable-session-reuse.js`                     |
| `agent/session-cache-eviction.ts`              | FIFO eviction at the configured limit                         | `lib/https.js` `_cacheSession`; `test-https-agent-session-eviction.js`                          |
| `agent/session-cache-update.ts`                | In-place update and explicit eviction                         | `lib/https.js` `_cacheSession` and `_evictSession`                                              |
| `agent/session-receiver-validation.ts`         | Session helpers reject foreign receivers                      | `lib/https.js` session helpers                                                                  |
| `agent/create-connection-options.ts`           | Object `createConnection` overload                            | `test-https-agent-create-connection.js`                                                         |
| `agent/create-connection-port-options.ts`      | Port-plus-options overload                                    | `test-https-agent-create-connection.js`                                                         |
| `agent/create-connection-port-host-options.ts` | Port-host-options overload                                    | `test-https-agent-create-connection.js`                                                         |
| `agent/tls12-session-reuse.ts`                 | A cached TLS 1.2 session is reused                            | `test-https-agent-session-reuse.js`                                                             |
| `agent/tls12-session-cache-disabled.ts`        | Disabling the cache prevents TLS 1.2 reuse                    | `test-https-agent-disable-session-reuse.js`                                                     |
| `request/request-return.ts`                    | HTTPS `ClientRequest`, protocol, method, and agent            | `lib/https.js` `request`; `test-http-url.parse-https.request.js`                                |
| `request/global-agent.ts`                      | Requests observe a replaced `globalAgent`                     | `test-https-client-override-global-agent.js`                                                    |
| `request/protocol-validation.ts`               | HTTPS rejects an HTTP URL or option protocol                  | `test-http-url.parse-only-support-http-https-protocol.js`                                       |
| `request/string-url-overload.ts`               | String URL, option headers, path, and protocol                | `lib/https.js` `request`; `test-https-client-get-url.js`                                        |
| `request/url-object-overload.ts`               | WHATWG URL plus options                                       | `lib/https.js` `request`; `test-https-client-get-url.js`                                        |
| `request/url-options-merge.ts`                 | Options override URL host, method, and path                   | `lib/https.js` `request`; `test-https-request-arguments.js`                                     |
| `request/get-auto-end.ts`                      | `get()` ends the returned request                             | `lib/https.js` `get`                                                                            |
| `request/agent-false.ts`                       | `agent: false` creates a non-global HTTPS agent               | `lib/https.js` `request`; Bun `test-https-should-work-when-sending-request-with-agent-false.ts` |
| `request/custom-agent.ts`                      | The request retains a supplied HTTPS agent                    | `test-https-agent-servername.js`                                                                |
| `request/transport-socket.ts`                  | Client and server expose encrypted TLS sockets                | `test-https-simple.js`                                                                          |
| `server/construction-overloads.ts`             | No-arg, options, and listener construction                    | `test-https-argument-of-creating.js`                                                            |
| `server/options-validation.ts`                 | Accepted and rejected option types                            | `test-https-options-boolean-check.js`                                                           |
| `server/default-state.ts`                      | HTTPS-owned HTTP server defaults                              | `lib/https.js` `Server`                                                                         |
| `server/alpn-default.ts`                       | Default ALPN wire value is `http/1.1`                         | `test-https-argument-of-creating.js`                                                            |
| `server/alpn-callback-option.ts`               | An explicit ALPN callback suppresses the default list         | `test-https-argument-of-creating.js`                                                            |
| `server/secure-response.ts`                    | Secure request/response lifecycle and close                   | `test-https-simple.js`                                                                          |
| `tls/allow-untrusted-state.ts`                 | `rejectUnauthorized: false` exposes unauthorized socket state | `test-https-client-reject.js`                                                                   |
| `tls/reject-untrusted.ts`                      | Default verification rejects the fixed self-signed cert       | `test-https-simple.js`                                                                          |
| `tls/explicit-ca-authorized.ts`                | Explicit CA trust authorizes the HTTPS socket                 | `test-https-client-reject.js`                                                                   |
| `tls/hostname-mismatch.ts`                     | Trusted certificate with a wrong hostname is rejected         | `test-https-client-checkServerIdentity.js`                                                      |
| `tls/sni-servername.ts`                        | HTTPS forwards SNI to the server request socket               | `test-https-agent-sni.js`                                                                       |
| `tls/alpn-http1.ts`                            | HTTPS integrates ALPN with HTTP/1.1                           | `test-http2-https-fallback.js` HTTPS branch                                                     |

The categories contain 18 agent, 10 request, six server, six TLS-integration,
four class/prototype, and three export fixtures.

## Fixed certificate

Loopback cases reuse
[`../tls/fixtures/localhost-key.pem`](../tls/fixtures/localhost-key.pem) and
[`../tls/fixtures/localhost-cert.pem`](../tls/fixtures/localhost-cert.pem). They
are non-secret test material with SANs for only `localhost` and `127.0.0.1`,
valid from 2026-05-24 to 2036-05-21. OpenSSL verification produced certificate
fingerprint
`CA:42:03:02:C4:52:0B:F0:54:7A:C2:24:B6:61:EC:F5:D2:9B:E0:DB:E3:B1:94:69:7C:B1:54:19:33:65:CF:97`.
The key and certificate public keys both hash to
`dced03213293e3674d4afc0e02b863f325ed5a7c238301f2eafd6fc2bd09a06b`.

## Repeated results

All counts were stable across three complete runs. No runtime had an unstable
fixture.

| Runtime | Version                | Node-equivalent output | Stable differences | Compile failures | Crashes | Timeouts |
| ------- | ---------------------- | ---------------------: | -----------------: | ---------------: | ------: | -------: |
| Node    | 26.5.0                 |                  47/47 |                  0 |                0 |       0 |        0 |
| Perry   | `main` at branch point |                   6/47 |                 41 |                0 |       0 |        0 |
| Deno    | 2.9.3                  |                  45/47 |                  2 |                0 |       0 |        0 |
| Bun     | 1.2.18                 |                  17/47 |                 30 |                0 |       0 |        0 |

Perry matches the three `getName` cases, named export aliases, inherited method
values, and detached `getName`. Its stable differences cover the default ESM
export, constructor and prototype identity, HTTPS agent state and session cache,
`createConnection`, global/custom agent identity, server ALPN/default state, and
TLS authorization/SNI/ALPN integration.

Deno has only two differences: its own `Server.prototype.listen` property and no
TLS 1.2 session reuse on the second request. Bun's differences cluster around
the HTTPS agent cache key/session cache, configurable agent defaults,
`tls.Server` inheritance, ALPN/default server state, CA and hostname handling,
TLS socket identity, authorization state, SNI, and session reuse. The three
`createConnection` overloads throw in Bun 1.2.18; the fixtures catch and print
that result, so the comparison exits cleanly.

The focused Perry runner was measured as 6 passes and 41 diffs three times. The
first clean run used `scripts/node_suite_run.py` directly. Two repeat runs used
the same runner and binary through a temporary wrapper that removed each
completed compiler `perry_strip_*` directory to prevent local disk exhaustion;
it did not change fixture input, output, exit status, or runtime cleanup.

## Exclusions and stopping boundary

- Generic HTTP parsing, header limits, custom `IncomingMessage` and
  `ServerResponse`, request/response timeouts, `closeAllConnections`,
  `closeIdleConnections`, backpressure, truncation, and 100-continue stay in
  `node:http`.
- Certificate parsing and getters, `checkServerIdentity` pattern rules, secure
  contexts, SNI context selection, ALPN negotiation mechanics, client
  certificates, default/system CA mutation, and raw TLS session APIs stay in
  `node:tls`. This suite keeps only their HTTPS integration points.
- Address selection, local binding errors, socket ref/unref, connection-family
  state, and kernel error shapes stay in `node:net`.
- Drain, EOF, high-water marks, large writes, and stream teardown stay in
  `node:stream` or `node:http`.
- HTTP/2 sessions, fallback, unknown protocols, and secure-server behavior stay
  in `node:http2`; only the HTTP/1.1 side of HTTPS ALPN remains here.
- Generic `ClientRequest` AbortSignal behavior belongs in `node:http`. The HTTPS
  upstream cases add no transport-owned result once connection races and timer
  assertions are removed.
- Proxy environment behavior, permission flags, system certificates, Unix
  sockets, external DNS or Internet endpoints, fixed ports, expired material,
  arbitrary sleeps, scheduler/kernel races, renegotiation, key logs, large
  transfers, GC pressure, and platform crypto details are outside the portable
  deterministic boundary.
- Keep-alive socket reuse belongs to `node:http`'s agent pool. HTTPS keeps only
  its TLS session cache and deterministic TLS 1.2 reuse contracts.

After reviewing all 71 Node filename matches, Node's implementation, Deno's
selected compatibility set and dedicated cases, and Bun's implementation and
selected tests, every remaining deterministic and portable HTTPS-owned contract
is represented above. The unrepresented cases fall into a named module or
exclusion, which is the stopping criterion for this expansion.
