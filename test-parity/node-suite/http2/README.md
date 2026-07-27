# `node:http2` granular parity suite

This suite contains 59 deterministic, single-contract fixtures. Network cases
use plaintext HTTP/2 on `127.0.0.1` with ephemeral ports and explicit event
barriers. Network fixtures close clients and servers in `finally`; the
sequential-cleanup regression resolves only after both close callbacks run.

## Pinned sources

- Node.js 26.5.0, commit `bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`:
  `lib/http2.js`, `lib/internal/http2/{core,compat,util}.js`, and the HTTP/2
  tests under `test/parallel` and `test/sequential`.
- Deno commit `34c46613cbe20450b74c0e8d4f0fd8f6f781d807`:
  `ext/node/polyfills/http2.ts`, `http2_esm.ts`,
  `internal/http2/{core,compat,constants,util}.ts`, and
  `tests/unit_node/http2_test.ts`.
- Bun commit `44f6469e0d4ae93467aa65c7e3bc9001000c7b31`: `src/js/node/http2.ts`,
  `test/js/node/http2/node-http2.test.js`, and Bun's selected copies of Node's
  HTTP/2 tests.

The main Node contracts came from `test-http2-getpackedsettings.js`,
`test-http2-createserver-options.js`, `test-http2-invalidargtypes-errors.js`,
`test-http2-request-response-proto.js`, `test-http2-connect.js`,
`test-http2-client-destroy.js`, `test-http2-session-unref.js`,
`test-http2-ping.js`, `test-http2-update-settings.js`,
`test-http2-goaway-opaquedata.js`,
`test-http2-client-request-options-errors.js`,
`test-http2-misused-pseudoheaders.js`, `test-http2-sent-headers.js`,
`test-http2-multiplex.js`, and the compat request/response tests.

## Coverage

| Category  | Fixtures | Contracts                                                                                                 |
| --------- | -------: | --------------------------------------------------------------------------------------------------------- |
| settings  |       17 | defaults, freshness, packing order, aliases, custom IDs, typed arrays, validation, and error codes        |
| session   |       13 | connect/close/destroy state, class and method surface, ref/unref, AbortSignal, settings, ping, and GOAWAY |
| exports   |        7 | exact keys, descriptors, receiver checks, function metadata, handshake helper, and compat classes         |
| plaintext |        5 | pseudoheaders, request/response bodies, response headers, and multiplexing                                |
| server    |        5 | overloads, option validation, inheritance, and ephemeral listen address                                   |
| stream    |        4 | initial state, sent headers, close validation, and trailer surface                                        |
| connect   |        3 | listener overload and authority/protocol validation                                                       |
| constants |        2 | representative protocol, settings, header, method, and status values                                      |
| headers   |        2 | sensitive-header symbol and invalid request pseudoheaders                                                 |
| compat    |        1 | HTTP/2 request method, URL, version, and pseudoheader mapping                                             |

The suite replaces five broad fixtures and removes two secure-server fixtures.
The removed secure case used a fixed port, wrote fixed `/tmp` paths without
cleanup, and waited on a timer instead of a protocol event.

## Results

Node 26.5.0 ran all 59 fixtures three times with no failures or output changes.
Each run produced the same combined stdout digest:
`ffb546784c0092a9c8224d50c3ab7665dd1bcc5c253379eda034961b79f6ceca`.

Perry ran the 57-fixture focused suite three times. The two appended fixtures
and the two fixtures changed during final review then ran in isolation three
times. All classifications stayed fixed, yielding:

```text
http2  32 pass / 59 total  (54.2%)  diff=27
```

There were no Node failures, compile failures, crashes, or timeouts. The
baseline is therefore `32/59` with no flake margin.

Deno matched Node on 54 of 59 fixtures. Its five stable differences are extra
module exports, default-settings key order, `maxHeaderSize` packing, alias
precedence/warnings, and unconditional unpacked-settings validation.

Bun matched Node on 35 of 59 fixtures. Its 24 stable differences group into
module/handshake surface, sensitive-header symbol identity, settings defaults
and packing/validation, pseudoheader validation, session state, GOAWAY stream
ID, and stream property defaults. All Deno and Bun cases exited cleanly; none
timed out.

Perry's 27 stable diffs group into:

- module keys, compat class identity, server inheritance, overload validation,
  connect validation, and the sensitive-header symbol;
- custom settings, `initialWindowSize` bounds, header aliases, unpacked
  validation, and default object shape;
- AbortSignal and request-option validation, live session class names, destroy
  semantics, and missing `origin`/`altsvc` methods;
- invalid pseudoheaders, multiplex order, `close()` validation, and
  `sentHeaders` before `respond()`.

## Per-fixture evidence

Each row names the primary Node 26.5.0 implementation or test used to select the
contract. Runtime columns compare exit code and stdout with Node.

| Fixture                                   | Node source                                                    | Deno  | Bun   |
| ----------------------------------------- | -------------------------------------------------------------- | ----- | ----- |
| `compat/request-properties.ts`            | `compat.js`; `test-http2-compat-serverrequest.js`              | match | match |
| `connect/invalid-authority.ts`            | `core.js`; `test-http2-connect.js`                             | match | match |
| `connect/listener-overload.ts`            | `core.js`; `test-http2-connect.js`                             | match | match |
| `connect/unsupported-protocol.ts`         | `core.js`; `test-http2-connect.js`                             | match | match |
| `constants/header-aliases.ts`             | `util.js`; `test-http2-util.js`                                | match | match |
| `constants/representative-values.ts`      | `util.js`; `test-http2-util.js`                                | match | match |
| `exports/compat-descriptors.ts`           | `compat.js`; `test-http2-request-response-proto.js`            | match | match |
| `exports/exact-module-keys.ts`            | `lib/http2.js`; `core.js`                                      | diff  | diff  |
| `exports/function-metadata.ts`            | `lib/http2.js`; `core.js`                                      | match | diff  |
| `exports/performServerHandshake.ts`       | `core.js`; `test-http2-perform-server-handshake.js`            | match | diff  |
| `exports/request-class.ts`                | `compat.js`; `test-http2-request-response-proto.js`            | match | match |
| `exports/response-class.ts`               | `compat.js`; `test-http2-request-response-proto.js`            | match | match |
| `exports/response-receiver-validation.ts` | `compat.js`; `test-http2-request-response-proto.js`            | match | match |
| `headers/invalid-request-pseudoheader.ts` | `core.js`; `test-http2-misused-pseudoheaders.js`               | match | diff  |
| `headers/sensitive-symbol.ts`             | `core.js`; `test-http2-sensitive-headers.js`                   | match | diff  |
| `plaintext/multiplex.ts`                  | `test-http2-multiplex.js`                                      | match | match |
| `plaintext/request-body.ts`               | `test-http2-compat-serverrequest.js`                           | match | match |
| `plaintext/request-pseudoheaders.ts`      | `test-http2-connect.js`; `test-http2-misused-pseudoheaders.js` | match | match |
| `plaintext/response-body.ts`              | `test-http2-client-data-end.js`                                | match | match |
| `plaintext/response-headers.ts`           | `test-http2-sent-headers.js`                                   | match | match |
| `server/create-overloads.ts`              | `core.js`; `test-http2-createserver-options.js`                | match | match |
| `server/inheritance.ts`                   | `core.js`; `test-http2-createserver-options.js`                | match | match |
| `server/invalid-options.ts`               | `core.js`; `test-http2-createserver-options.js`                | match | match |
| `server/invalid-settings-option.ts`       | `core.js`; `test-http2-createserver-options.js`                | match | match |
| `server/listen-address.ts`                | `core.js`; `test-http2-server-startup.js`                      | match | match |
| `session/aborted-request-signal.ts`       | `core.js`; `test-http2-client-request-options-errors.js`       | match | match |
| `session/class-names.ts`                  | `core.js`                                                      | match | match |
| `session/client-close-callback.ts`        | `test-http2-client-destroy.js`                                 | match | match |
| `session/client-connect-state.ts`         | `test-http2-create-client-connect.js`                          | match | match |
| `session/client-destroy-state.ts`         | `test-http2-client-destroy.js`                                 | match | diff  |
| `session/goaway-opaque-data.ts`           | `test-http2-goaway-opaquedata.js`                              | match | diff  |
| `session/method-surface.ts`               | `core.js`                                                      | match | match |
| `session/ping-echo.ts`                    | `test-http2-ping.js`                                           | match | match |
| `session/ref-unref.ts`                    | `test-http2-session-unref.js`                                  | match | match |
| `session/request-option-validation.ts`    | `test-http2-client-request-options-errors.js`                  | match | match |
| `session/sequential-session-cleanup.ts`   | `core.js`; `test-http2-create-client-connect.js`               | match | match |
| `session/settings-callback.ts`            | `test-http2-update-settings.js`                                | match | match |
| `session/settings-shape.ts`               | `test-http2-session-settings.js`                               | match | diff  |
| `settings/default-freshness.ts`           | `util.js`; `test-http2-getpackedsettings.js`                   | match | match |
| `settings/default-shape.ts`               | `util.js`; `test-http2-getpackedsettings.js`                   | diff  | diff  |
| `settings/invalid-boolean-values.ts`      | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | diff  |
| `settings/invalid-numeric-values.ts`      | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | diff  |
| `settings/numeric-boundaries.ts`          | `util.js`; `test-http2-invalidargtypes-errors.js`              | diff  | diff  |
| `settings/packed-custom.ts`               | `util.js`; `test-http2-getpackedsettings.js`                   | match | diff  |
| `settings/packed-defaults.ts`             | `util.js`; `test-http2-getpackedsettings.js`                   | match | diff  |
| `settings/packed-empty.ts`                | `util.js`; `test-http2-getpackedsettings.js`                   | match | diff  |
| `settings/packed-header-alias.ts`         | `util.js`; `test-http2-getpackedsettings.js`                   | diff  | diff  |
| `settings/packed-order.ts`                | `util.js`; `test-http2-getpackedsettings.js`                   | match | diff  |
| `settings/packed-unknown-key.ts`          | `util.js`; `test-http2-getpackedsettings.js`                   | match | diff  |
| `settings/unpacked-aliases.ts`            | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | match |
| `settings/unpacked-custom.ts`             | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | diff  |
| `settings/unpacked-invalid-length.ts`     | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | diff  |
| `settings/unpacked-invalid-type.ts`       | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | diff  |
| `settings/unpacked-typed-array.ts`        | `util.js`; `test-http2-invalidargtypes-errors.js`              | match | match |
| `settings/unpacked-validation.ts`         | `util.js`; `test-http2-invalidargtypes-errors.js`              | diff  | diff  |
| `stream/close-validation.ts`              | `core.js`; `test-http2-invalidargtypes-errors.js`              | match | match |
| `stream/initial-state.ts`                 | `core.js`; `test-http2-session-stream-state.js`                | match | diff  |
| `stream/sent-headers.ts`                  | `test-http2-sent-headers.js`                                   | match | diff  |
| `stream/trailer-surface.ts`               | `core.js`; `test-http2-trailers.js`                            | match | match |

## Exclusions and stopping rule

- TLS, ALPN, certificates, secure-session origin behavior, and HTTP/1 fallback
  belong to `tls`/`https` until those providers can act as a stable oracle.
- Internet tests, fixed ports, sleeps, scheduler races, large payloads, memory
  pressure, GC, signals, inspector, tracing, and kernel-specific failures are
  excluded.
- Socket ownership, backpressure, generic stream state, file-descriptor
  semantics, diagnostics channels, async context, workers, and performance hooks
  belong to their own suites.
- `respondWithFile` and `respondWithFD` stay out until they can test an HTTP/2
  result without turning the fixture into an `fs` test or leaving a pending
  response in Perry.
- Actual trailer and reset exchanges, pre-connect request queueing, ORIGIN, and
  ALTSVC stay out where Perry cannot yet reach a deterministic completion
  barrier. The suite already records their method surface or validation gap;
  another timeout would add no new evidence.
- Server push and extended CONNECT are not added: push is deprecated, and the
  remaining contracts either need TLS/backend work or duplicate request and
  stream validation already covered here.

The audit stopped after reviewing Node's implementation and full HTTP/2 test
inventory plus Deno's and Bun's current implementations and selected suites. No
remaining case was deterministic, local, HTTP/2-specific, non-redundant, and
able to exit cleanly on all four runtimes.
