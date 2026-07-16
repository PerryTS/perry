# `node:inspector` entry evidence

All rows use Node 26.5.0 as oracle. `pass`, `diff`, `error`, and `timeout`
describe direct stdout/exit comparison under Deno 2.9.2 and Bun 1.3.14. Perry
classifications are recorded from two identical focused release-runner passes
(4/37, 33 diffs). Post-run generated-artifact and live-process scans found no
leaked suite process or endpoint.

| Entry                                | Contract category                     | Node source basis                              |  Deno |     Bun | Perry |
| ------------------------------------ | ------------------------------------- | ---------------------------------------------- | ----: | ------: | ----: |
| `events/console-api.ts`              | console notification shape            | API events; `test-inspector-console.js`        |  pass | timeout |  diff |
| `events/listener-lifecycle.ts`       | `on`/`once`/`off` cleanup             | `lib/inspector.js` message dispatch            |  pass |    diff |  diff |
| `events/notification-order.ts`       | specific/generic/callback order       | `lib/inspector.js` `#onMessage`                |  pass | timeout |  diff |
| `events/script-parsed.ts`            | normalized script metadata            | `test-inspector-scriptparsed-context.js`       |  pass |   error |  diff |
| `lifecycle/endpoint.ts`              | isolated open/dispose/reopen/close    | `test-inspector-open-dispose.mjs`              |  diff |    diff |  diff |
| `lifecycle/main-thread-connect.ts`   | main-thread rejection                 | `test-inspector-connect-to-main-thread.js`     |  pass |   error |  pass |
| `lifecycle/method-receivers.ts`      | private receiver brands               | `lib/inspector.js` `Session`                   |  pass |    diff |  diff |
| `lifecycle/open-range-validation.ts` | port overflow validation              | `test-inspector-open-port-integer-overflow.js` |  pass |    diff |  diff |
| `lifecycle/repeated-sessions.ts`     | independent in-process sessions       | `test-inspector-multisession-js.js`            |  pass | timeout |  diff |
| `lifecycle/session-connect.ts`       | connect/disconnect/reconnect          | `test-inspector-module.js`                     |  pass |    diff |  pass |
| `network/helpers.ts`                 | Network function identity/descriptors | `test-inspector-emit-protocol-event.js`        |  diff |   error |  diff |
| `post/callback-validation.ts`        | callback type validation              | `test-inspector-module.js`                     |  pass |   error |  diff |
| `post/circular-params.ts`            | JSON serialization failure            | `lib/inspector.js` `post()`                    |  pass |    diff |  diff |
| `post/disconnected.ts`               | pre/post disconnect error             | `test-inspector-module.js`                     |  pass |    diff |  pass |
| `post/method-validation.ts`          | method type validation                | `test-inspector-module.js`                     |  pass |    diff |  diff |
| `post/overloads.ts`                  | callback second arg and null params   | `lib/inspector.js` `post()`                    |  diff |    diff |  diff |
| `post/params-validation.ts`          | params type validation                | `test-inspector-module.js`                     |  pass |    diff |  diff |
| `post/pending-disconnect.ts`         | pending callback completion/order     | API `disconnect()` contract                    | error |    diff |  diff |
| `post/unknown-command.ts`            | protocol command error                | `lib/inspector.js` `#onMessage`                |  pass |    diff |  pass |
| `protocol/debugger-metadata.ts`      | normalized Debugger ID result         | V8 Debugger protocol                           |  pass |   error |  diff |
| `protocol/enable-disable.ts`         | safe domain lifecycle                 | API profiler examples                          |  pass |   error |  diff |
| `protocol/get-script-source.ts`      | controlled exact source retrieval     | `test-inspector.js`                            |  pass |   error |  diff |
| `protocol/schema-domains.ts`         | supported-domain inventory            | V8 Schema protocol                             |  pass |   error |  diff |
| `runtime/await-promise.ts`           | `awaitPromise` resolution             | inspector API examples                         |  pass |   error |  diff |
| `runtime/exception-details.ts`       | thrown evaluation result              | inspector exception tests                      |  pass |   error |  diff |
| `runtime/get-properties-release.ts`  | properties and object release         | `test-inspector-bindings.js`                   |  pass |   error |  diff |
| `runtime/numeric-specials.ts`        | unserializable values and BigInt      | V8 Runtime protocol                            |  pass |   error |  diff |
| `runtime/object-preview.ts`          | normalized preview properties         | `test-inspector.js`                            |  pass |   error |  diff |
| `runtime/primitives.ts`              | primitive remote-object shapes        | inspector API examples                         |  pass |   error |  diff |
| `runtime/release-object-group.ts`    | group release invalidation            | V8 Runtime protocol                            |  pass |   error |  diff |
| `runtime/remote-subtypes.ts`         | stable object subtype/class metadata  | V8 Runtime protocol                            |  pass |   error |  diff |
| `runtime/return-by-value.ts`         | object/array serialization            | V8 Runtime protocol                            |  pass |   error |  diff |
| `session/callback-runtime.ts`        | callback lifecycle smoke              | `test-inspector-module.js`                     | error | timeout |  diff |
| `surface/domain-helpers.ts`          | Network/DOMStorage/Resources exports  | `lib/inspector.js`                             | error |   error |  diff |
| `surface/domain-validation.ts`       | helper params validation              | `test-inspector-emit-protocol-event.js`        |  pass |   error |  diff |
| `surface/exports.ts`                 | exact public inventory/descriptors    | `lib/inspector.js` exports                     |  diff |    diff |  diff |
| `surface/session-class.ts`           | EventEmitter/prototype descriptors    | API `Session`; `lib/inspector.js`              |  pass |    diff |  diff |
