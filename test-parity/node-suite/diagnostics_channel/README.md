# node:diagnostics_channel granular parity suite

Focused parity coverage for `node:diagnostics_channel`, ported from Node.js' `test/parallel/test-diagnostics-channel*.js` and cross-checked against Deno's Node compatibility list/polyfill behavior.

Primary upstream references:

- Node.js `test/parallel/test-diagnostics-channel-pub-sub.js`
- Node.js `test/parallel/test-diagnostics-channel-object-channel-pub-sub.js`
- Node.js `test/parallel/test-diagnostics-channel-has-subscribers.js`
- Node.js `test/parallel/test-diagnostics-channel-symbol-named.js`
- Node.js `test/parallel/test-diagnostics-channel-sync-unsubscribe.js`
- Node.js `test/parallel/test-diagnostics-channel-bind-store.js`
- Node.js `test/parallel/test-diagnostics-channel-tracing-channel-*.js`
- Node.js `test/parallel/test-console-diagnostics-channels.js`
- Deno `tests/node_compat/config.jsonc` lists the same diagnostics-channel Node tests for compatibility coverage.
- Deno `ext/node/polyfills/diagnostics_channel.js` implements the public `Channel`/`tracingChannel`/store behavior mirrored by these fixtures.

The fixtures avoid Node's internal `common` helpers and print deterministic observations instead. Some cases intentionally expose Perry gaps; they should be kept as parity targets rather than hidden.

## Coverage groups

- import forms: `node:` and prefixless, namespace/default/named exports, function identity
- channel basics: identity, `Channel` instanceof, pub/sub, unsubscribe return values, symbol names, validation, synchronous unsubscribe during publish
- store binding: `bindStore`, `unbindStore`, `runStores`, transformations, nested runs, this/argument forwarding
- tracing: construction from name and channels object, `hasSubscribers`, subscribe/unsubscribe, `traceSync`, `tracePromise`, `traceCallback`, error paths, no-subscriber fast paths
- instrumentation: console diagnostic channels for `console.log/info/debug/warn/error`
