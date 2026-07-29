# `node:net` parity evidence

## Fixed sources

The audit used these primary sources on 2026-07-26:

- Node.js 26.5.0 commit
  [`bebd1b8d`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb),
  including
  [`lib/net.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/net.js),
  [`internal/blocklist.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/blocklist.js),
  and
  [`internal/socketaddress.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/socketaddress.js).
- Deno commit
  [`34c46613`](https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807),
  including
  [`ext/node/polyfills/net.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/ext/node/polyfills/net.ts),
  [`tests/unit_node/net_test.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/unit_node/net_test.ts),
  and its Node compatibility selection.
- Bun commit
  [`44f6469e`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31),
  including
  [`src/js/node/net.ts`](https://github.com/oven-sh/bun/blob/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/src/js/node/net.ts),
  copied Node tests, and
  [`test/js/node/net`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/test/js/node/net).

Node 26.5.0 has 179 `test-net*` files: 154 parallel, 12 sequential, 7 pummel, 5
internet, and 1 async-hooks case. Deno's current config names 161 net cases
across its selected, disabled, internet, pummel, and sequential groups. Bun
carries 153 copied `test-net*` files plus focused net tests.

## Contract map

| Perry area                     | Representative Node 26.5.0 evidence                                                                                                                                        | Deno and Bun evidence                                                                                                                        |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| exports, aliases, prototypes   | `lib/net.js`, `test-net-stream.js`, `test-net-socket-constructor.js`, `test-net-normalize-args.js`                                                                         | both implementations export the Node surface; Bun's `node-net.test.ts` checks `Stream === Socket`                                            |
| `SocketAddress`, `BlockList`   | `internal/socketaddress.js`, `internal/blocklist.js`, `test-net-blocklist.js`, `test-net-server-blocklist.js`                                                              | Deno implements both classes; Bun's focused tests cover address, range, subnet, mapped IPv6, and exact prefixes                              |
| validation and coercion        | `test-net-server-options.js`, `test-net-connect-options-invalid.js`, `test-net-socket-connect-invalid-autoselectfamily*.js`, `test-net-listen-invalid-port.js`             | both copy or select the upstream validation cases; their current output differences stay visible                                             |
| server lifecycle and overloads | `test-net-listening.js`, `test-net-server-address.js`, `test-net-server-close.js`, `test-net-server-listen-options-signal.js`, `test-net-server-listen-remove-callback.js` | Deno's unit suite checks immediate port reuse and connection sockets; Bun's server suite checks listen overloads, addresses, state, and data |
| socket lifecycle and metadata  | `test-net-socket-connecting.js`, `test-net-local-address-port.js`, `test-net-remote-address-port.js`, `test-net-end-close.js`                                              | both carry the upstream cases and custom connection/event coverage                                                                           |
| transport and half-close       | `test-net-binary.js`, `test-net-allow-half-open.js`, `test-net-socket-no-halfopen-enforcer.js`, `test-net-write-connect-write.js`                                          | Deno checks isolated socket buffers; Bun has client and server `allowHalfOpen` tests                                                         |
| counters and flow control      | `test-net-bytes-stats.js`, `test-net-buffersize.js`, `test-net-sync-cork.js`, `test-net-pause-resume-connecting.js`                                                        | both carry the upstream cases; the Perry probes avoid packet-boundary claims                                                                 |
| abort and reset                | `test-net-connect-abort-controller.js`, `test-net-server-listen-options-signal.js`, `test-net-socket-reset*.js`                                                            | both select or copy the abort/reset cases                                                                                                    |

## Per-fixture traceability

`same` means byte-for-byte stdout parity with Node 26.5.0 after normalization.
`diff` means a stable runtime difference. Every row completed three identical
runs.

| Fixture                                       | Fixed Node 26.5.0 contract                                                                                  | Node | Deno |  Bun |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ---: | ---: | ---: |
| `classes/block-list-identity.ts`              | `lib/internal/blocklist.js`; `test/parallel/test-net-blocklist.js`                                          | same | same | diff |
| `classes/block-list-ipv4.ts`                  | `lib/internal/blocklist.js`; `test/parallel/test-net-blocklist.js`                                          | same | same | same |
| `classes/block-list-ipv6.ts`                  | `lib/internal/blocklist.js`; `test/parallel/test-net-blocklist.js`                                          | same | same | same |
| `classes/block-list-json.ts`                  | `lib/internal/blocklist.js`; `test/parallel/test-net-blocklist.js`                                          | same | same | diff |
| `classes/block-list-socket-address-inputs.ts` | `lib/internal/blocklist.js`; `test/parallel/test-net-blocklist.js`                                          | same | same | same |
| `classes/socket-address-defaults.ts`          | `lib/internal/socketaddress.js`; `test/parallel/test-net-blocklist.js`                                      | same | diff | diff |
| `classes/socket-address-parse.ts`             | `lib/internal/socketaddress.js`; `test/parallel/test-net-blocklist.js`                                      | same | same | same |
| `classes/socket-address-validation.ts`        | `lib/internal/socketaddress.js`; `test/parallel/test-net-blocklist.js`                                      | same | same | diff |
| `connection/abort-signal.ts`                  | `test/parallel/test-net-connect-abort-controller.js`                                                        | same | same | diff |
| `connection/address-metadata.ts`              | `test/parallel/test-net-local-address-port.js`; `test-net-remote-address-port.js`                           | same | same | same |
| `connection/backpressure.ts`                  | `test/parallel/test-net-buffersize.js`                                                                      | same | same | same |
| `connection/bytes-counters.ts`                | `test/parallel/test-net-bytes-stats.js`                                                                     | same | same | diff |
| `connection/connect-overloads.ts`             | `lib/net.js`; `test/parallel/test-net-normalize-args.js`                                                    | same | same | same |
| `connection/cork-uncork.ts`                   | `test/parallel/test-net-sync-cork.js`                                                                       | same | same | diff |
| `connection/data-roundtrip.ts`                | `test/parallel/test-net-binary.js`                                                                          | same | same | diff |
| `connection/end-callback-order.ts`            | `test/parallel/test-net-end-close.js`                                                                       | same | same | same |
| `connection/event-order.ts`                   | `test/parallel/test-net-end-close.js`                                                                       | same | same | same |
| `connection/half-close.ts`                    | `test/parallel/test-net-allow-half-open.js`                                                                 | same | same | diff |
| `connection/ipv6-loopback.ts`                 | `test/parallel/test-net-listen-ipv6only.js`                                                                 | same | same | same |
| `connection/state-transitions.ts`             | `test/parallel/test-net-socket-connecting.js`                                                               | same | same | same |
| `exports/bound-socket.ts`                     | `lib/net.js`; `test/parallel/test-net-boundsocket.js`                                                       | same | diff | diff |
| `exports/class-prototypes.ts`                 | `lib/net.js`; `test/parallel/test-net-socket-constructor.js`                                                | same | same | same |
| `exports/create-server-and-helpers.ts`        | `lib/net.js`; `test/parallel/test-net-server-options.js`                                                    | same | same | diff |
| `exports/descriptors-and-aliases.ts`          | `lib/net.js`                                                                                                | same | same | same |
| `exports/stream-alias.ts`                     | `test/parallel/test-net-stream.js`                                                                          | same | same | same |
| `ip/zone-identifiers.ts`                      | `lib/net.js` (`isIPv6`)                                                                                     | same | same | same |
| `method-values/server-async-dispose.ts`       | `lib/net.js`; `test/parallel/test-net-server-async-dispose.mjs`                                             | same | same | diff |
| `method-values/socket-server.ts`              | `lib/net.js`                                                                                                | same | same | same |
| `server/address-lifecycle.ts`                 | `test/sequential/test-net-server-address.js`                                                                | same | same | same |
| `server/get-connections.ts`                   | `lib/net.js` (`Server.getConnections`)                                                                      | same | same | same |
| `server/initial-state-and-refs.ts`            | `test/parallel/test-net-server-unref.js`; `test-net-server-address.js`                                      | same | same | same |
| `server/listen-overloads.ts`                  | `lib/net.js`; `test/parallel/test-net-normalize-args.js`                                                    | same | same | same |
| `server/pause-on-connect.ts`                  | `test/parallel/test-net-server-pause-on-connect.js`                                                         | same | same | same |
| `socket/chainable-controls.ts`                | `lib/net.js` (`Socket` control methods)                                                                     | same | same | same |
| `socket/destroy-and-reset.ts`                 | `lib/net.js`; `test/parallel/test-net-connect-reset-after-destroy.js`                                       | same | same | same |
| `socket/initial-state.ts`                     | `test/parallel/test-net-socket-connecting.js`                                                               | same | same | diff |
| `socket/type-of-service.ts`                   | `lib/net.js`; `test/parallel/test-net-socket-tos.js`                                                        | same | diff | diff |
| `validation/auto-select-family-defaults.ts`   | `test/parallel/test-net-autoselectfamily-default.js`                                                        | same | same | diff |
| `validation/connect-missing-arguments.ts`     | `test/parallel/test-net-connect-options-invalid.js`                                                         | same | same | same |
| `validation/connect-options.ts`               | `test/parallel/test-net-connect-options-invalid.js`                                                         | same | diff | diff |
| `validation/connect-port.ts`                  | `test/parallel/test-net-connect-options-port.js`                                                            | same | same | same |
| `validation/create-server-options.ts`         | `test/parallel/test-net-server-options.js`                                                                  | same | same | same |
| `validation/listen-options.ts`                | `test/parallel/test-net-server-listen-options.js`; `test/parallel/test-net-server-listen-options-signal.js` | same | same | same |
| `validation/listen-port.ts`                   | `test/parallel/test-net-listen-invalid-port.js`                                                             | same | same | diff |
| `validation/receiver-validation.ts`           | `lib/net.js`; `test/parallel/test-net-socket-constructor.js`                                                | same | same | diff |
| `validation/socket-constructor-options.ts`    | `test/parallel/test-net-socket-constructor.js`                                                              | same | diff | diff |
| `validation/socket-set-timeout.ts`            | `lib/net.js`; `test/parallel/test-net-settimeout.js`                                                        | same | same | same |

## Cross-runtime measurement

All 47 fixtures ran three times per runtime with no unstable output or timeout:

| Runtime        | Node-identical stdout | Stable differences | Runtime errors | Timeouts |
| -------------- | --------------------: | -----------------: | -------------: | -------: |
| Node.js 26.5.0 |                    47 |                  0 |              0 |        0 |
| Deno 2.9.3     |                    42 |                  5 |              0 |        0 |
| Bun 1.2.18     |                    28 |                 19 |              0 |        0 |

Stable differences include Bun's missing `BlockList.toJSON()`/`fromJSON()`, Deno
and Bun's missing `BoundSocket`, class/default differences, validation gaps, and
Bun socket counter and flow-control output. Feature checks keep missing methods
observable without turning those gaps into process crashes.

## Perry baseline

Two clean final focused runs against the exact Node 26.5.0 binary matched:

```text
net  16  47  34.0  diff=31
```

Both runs had 16 passes, 31 stable output differences, 0 compile failures, 0
Perry runtime errors, 0 crashes, and 0 timeouts. The committed `net` floor is
therefore 16/47.

The differences diagnose missing or divergent Perry support for `BoundSocket`,
descriptors and prototypes, `SocketAddress` defaults and validation, `BlockList`
JSON and class inputs, constructor/connect/listen validation, receiver checks,
abort handling, address/state metadata, byte counters, callback receivers and
order, flow-control properties, IPv6 binding, paused accepts, half-open state,
and data round trips.

## Exclusions and stopping point

- HTTP request parsing, agents, upgrades, and connection pooling stay in `http`
  and `https`.
- TLS handshakes, certificates, `TLSSocket`, and secure event order stay in
  `tls`.
- UDP send, receive, multicast, and datagram block lists stay in `dgram`.
- Shared handles, worker distribution, shared ports, and cross-worker limits
  stay in `cluster`.
- async resource types, IDs, trigger IDs, and context propagation stay in
  `async_hooks`.
- DNS result choice and lookup failures stay in `dns`; this suite keeps only
  synchronous `lookup` option validation.
- Internet, fixed-port, pummel, child-process, descriptor-transfer, Unix-socket,
  interface-specific, and kernel-error-message cases are not portable enough for
  this lane.
- Packet boundaries, drain timing, TCP reset delivery, keepalive effects, and
  auto-family winner order depend on the kernel or host. The suite keeps only
  synchronous state or validation contracts for them.
- Paused data transport and server-side abort after `listen()` lack a portable
  completion barrier in Perry: waiting for the matching close event leaves a
  live handle, while a scheduler turn can report a false pass. Those probes are
  excluded; paused accepts and connect abort still cover the deterministic
  neighboring contracts.
- `BoundSocket` bind/adoption semantics are new in Node 26.5.0 and absent from
  current Deno and Bun. The suite records the export and constructor validation
  without adding host-level descriptor tests.

The review stopped after every remaining Node/Deno/Bun net case either mapped to
a fixture above, repeated an existing contract, crossed into a neighboring
suite, or required exclusion of host, kernel, process, timing, or stress
behavior.
