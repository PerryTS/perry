# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, how does Readable.from(iterable) deliver data and end events for node:stream Readable streams, and what source files/tests show the invariant that adding a data listener drains iterable chunks and emits end once? Focus only on the minimal event-delivery behavior useful for a PerryTS/perry #1532 fix.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_49876408-3472-41e8-8e23-cf8a103cd866

**Question**: For Node compatibility in Deno, how does Readable.from(iterable) deliver data and end events for node:stream Readable streams, and what source files/tests show the invariant that adding a data listener drains iterable chunks and emits end once? Focus only on the minimal event-delivery behavior useful for a PerryTS/perry #1532 fix.

Deno's Node.js compatibility layer implements `Readable.from(iterable)` for `node:stream` Readable streams by converting an asynchronous iterable into a Node.js `Readable` stream [1](#ref-q1-1) . When a data listener is added to this stream, it triggers a "flowing" mode, which drains chunks from the iterable and emits them as `'data'` events [2](#ref-q1-2) . Once the iterable is exhausted, an `'end'` event is emitted [3](#ref-q1-3) .

### Data and End Event Delivery

The `Readable.from` static method in Deno's Node.js compatibility layer uses an internal `from` function to create a `Readable` stream from an iterable [1](#ref-q1-1) . This `Readable` stream's behavior is governed by its internal `_readableState` [4](#ref-q1-4) .

When a `'data'` listener is attached to the `Readable` stream, the stream enters a "flowing" state [2](#ref-q1-2) . In this state, the stream continuously calls its `_read` method to pull data [5](#ref-q1-5) . The `_read` method, in turn, fetches values from the underlying iterable [6](#ref-q1-6) . Each fetched value is then emitted as a `'data'` event [7](#ref-q1-7) .

Once the iterable signals that it is `done` (i.e., `res.done` is true) [8](#ref-q1-8) , the stream's internal state is updated to `kEnded` [9](#ref-q1-9) . Subsequently, the `endReadableNT` function is scheduled to run on the next tick [10](#ref-q1-10) . This function checks if the stream has already emitted an `'end'` event and if there's no remaining data in the buffer [11](#ref-q1-11) . If these conditions are met, the `'end'` event is emitted [12](#ref-q1-12) .

### Invariant for Data Listener and End Event

The invariant that adding a data listener drains iterable chunks and emits `'end'` once is demonstrated by the internal logic of the `Readable` stream. The `on` method for the `'data'` event sets the `kDataListening` flag and, if the stream is not already flowing, calls `this.resume()` [2](#ref-q1-2) . The `resume()` method ensures that `maybeReadMore` is called, which in turn drives the `_read` loop to pull data from the source [13](#ref-q1-13) .

The `'end'` event is emitted only once because the `endReadableNT` function checks the `kEndEmitted` flag [14](#ref-q1-14) . This flag is set immediately after the `'end'` event is emitted, preventing subsequent emissions [12](#ref-q1-12) .

Relevant tests that demonstrate this behavior include:
*   `"parallel/test-readable-from.js"` [15](#ref-q1-15)
*   `"parallel/test-readable-single-end.js"` [16](#ref-q1-16)
*   `"parallel/test-stream-readable-data.js"` [17](#ref-q1-17)

These tests, part of the Node.js compatibility test suite, validate that `Readable.from` correctly handles data flow and termination events as expected in Node.js environments [18](#ref-q1-18) .

## Notes

The `Readable.from` implementation for Node.js streams in Deno is distinct from the Web Streams API's `ReadableStream.from` [19](#ref-q1-19) . While both convert iterables to readable streams, their internal mechanisms and event models differ to maintain compatibility with their respective specifications. The Node.js compatibility layer specifically focuses on emulating Node.js stream behavior, including the `'data'` and `'end'` events [20](#ref-q1-20) .

Wiki pages you might want to explore:
- [Streams API (denoland/deno)](/wiki/denoland/deno#6.2)
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_49876408-3472-41e8-8e23-cf8a103cd866

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/streams/readable.js:1826-1828`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1826-L1828)

```javascript
Readable.from = function (iterable, opts) {
  return from(Readable, iterable, opts);
};
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/internal/streams/readable.js:1214-1226`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1214-L1226)

```javascript
  if (ev === "data") {
    state[kState] |= kDataListening;

    // Update readableListening so that resume() may be a no-op
    // a few lines down. This is needed to support once('readable').
    state[kState] |= this.listenerCount("readable") > 0
      ? kReadableListening
      : 0;

    // Try start flowing on next tick if stream isn't explicitly paused.
    if ((state[kState] & (kHasFlowing | kFlowing)) !== kHasFlowing) {
      this.resume();
    }
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/internal/streams/readable.js:1792-1796`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1792-L1796)

```javascript
    (state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 &&
    state.length === 0
  ) {
    state[kState] |= kEndEmitted;
    stream.emit("end");
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/internal/streams/readable.js:363`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L363)

```javascript
  this._readableState = new ReadableState(options, this, false);
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/internal/streams/readable.js:959-966`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L959-L966)

```javascript
  while (
    (state[kState] & (kReading | kEnded)) === 0 &&
    (state.length < state.highWaterMark ||
      ((state[kState] & kFlowing) !== 0 && state.length === 0))
  ) {
    const len = state.length;
    debug("maybeReadMore read 0");
    stream.read(0);
```

<a id="ref-q1-6"></a>
### [6] `ext/web/06_streams.js:5321-5331`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/06_streams.js#L5321-L5331)

```javascript
    const stream = createReadableStream(noop, async () => {
      // deno-lint-ignore prefer-primordials
      const res = await iter.next();
      if (res.done) {
        readableStreamDefaultControllerClose(stream[_controller]);
      } else {
        readableStreamDefaultControllerEnqueue(
          stream[_controller],
          await res.value,
        );
      }
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/streams/readable.js:846-849`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L846-L849)

```javascript
  if (ret !== null && (state[kState] & (kErrorEmitted | kCloseEmitted)) === 0) {
    state[kState] |= kDataEmitted;
    this.emit("data", ret);
  }
```

<a id="ref-q1-8"></a>
### [8] `ext/web/06_streams.js:5324-5325`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/06_streams.js#L5324-L5325)

```javascript
      if (res.done) {
        readableStreamDefaultControllerClose(stream[_controller]);
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/internal/streams/readable.js:867`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L867)

```javascript
  state[kState] |= kEnded;
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/internal/streams/readable.js:1787-1796`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1787-L1796)

```javascript
function endReadableNT(state, stream) {
  debug("endReadableNT");

  // Check that we didn't get one last unshift.
  if (
    (state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 &&
    state.length === 0
  ) {
    state[kState] |= kEndEmitted;
    stream.emit("end");
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal/streams/readable.js:1792-1794`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1792-L1794)

```javascript
    (state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 &&
    state.length === 0
  ) {
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/internal/streams/readable.js:1795-1796`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1795-L1796)

```javascript
    state[kState] |= kEndEmitted;
    stream.emit("end");
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/internal/streams/readable.js:928-973`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L928-L973)

```javascript
function maybeReadMore(stream, state) {
  if ((state[kState] & (kReadingMore | kConstructed)) === kConstructed) {
    state[kState] |= kReadingMore;
    process.nextTick(maybeReadMore_, stream, state);
  }
}

function maybeReadMore_(stream, state) {
  // Attempt to read more data if we should.
  //
  // The conditions for reading more data are (one of):
  // - Not enough data buffered (state.length < state.highWaterMark). The loop
  //   is responsible for filling the buffer with enough data if such data
  //   is available. If highWaterMark is 0 and we are not in the flowing mode
  //   we should _not_ attempt to buffer any extra data. We'll get more data
  //   when the stream consumer calls read() instead.
  // - No data in the buffer, and the stream is in flowing mode. In this mode
  //   the loop below is responsible for ensuring read() is called. Failing to
  //   call read here would abort the flow and there's no other mechanism for
  //   continuing the flow if the stream consumer has just subscribed to the
  //   'data' event.
  //
  // In addition to the above conditions to keep reading data, the following
  // conditions prevent the data from being read:
  // - The stream has ended (state.ended).
  // - There is already a pending 'read' operation (state.reading). This is a
  //   case where the stream has called the implementation defined _read()
  //   method, but they are processing the call asynchronously and have _not_
  //   called push() with new data. In this case we skip performing more
  //   read()s. The execution ends in this method again after the _read() ends
  //   up calling push() with more data.
  while (
    (state[kState] & (kReading | kEnded)) === 0 &&
    (state.length < state.highWaterMark ||
      ((state[kState] & kFlowing) !== 0 && state.length === 0))
  ) {
    const len = state.length;
    debug("maybeReadMore read 0");
    stream.read(0);
    if (len === state.length) {
      // Didn't get any data, stop spinning.
      break;
    }
  }
  state[kState] &= ~kReadingMore;
}
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/internal/streams/readable.js:1792-1793`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L1792-L1793)

```javascript
    (state[kState] & (kErrored | kCloseEmitted | kEndEmitted)) === 0 &&
    state.length === 0
```

<a id="ref-q1-15"></a>
### [15] `tests/node_compat/config.jsonc:2882`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2882)

```
    "parallel/test-readable-from.js": {},
```

<a id="ref-q1-16"></a>
### [16] `tests/node_compat/config.jsonc:2884`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2884)

```
    "parallel/test-readable-single-end.js": {},
```

<a id="ref-q1-17"></a>
### [17] `tests/node_compat/config.jsonc:3251`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L3251)

```
    "parallel/test-stream-readable-data.js": {},
```

<a id="ref-q1-18"></a>
### [18] `tests/node_compat/config.jsonc:1-136`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L1-L136)

```
{
  "$schema": "./schema.json",
  "tests": {
    "abort/test-addon-register-signal-handler.js": {},
    "abort/test-addon-uv-handle-leak.js": {},
    "abort/test-zlib-invalid-internals-usage.js": {
      "ignore": true,
      "reason": "Tests Node.js internal C++ binding (internalBinding('zlib').Zlib) which is not implemented in Deno"
    },
    "client-proxy/test-use-env-proxy-cli-http.mjs": {
      "ignore": true,
      "reason": "Tests Node.js-specific CLI flags/options that are not supported in Deno"
    },
    "client-proxy/test-use-env-proxy-cli-https.mjs": {
      "ignore": true,
      "reason": "Tests Node.js-specific CLI flags/options that are not supported in Deno"
    },
    "es-module/test-cjs-prototype-pollution.js": {},
    "es-module/test-esm-assert-strict.mjs": {},
    "es-module/test-esm-child-process-fork-main.mjs": {},
    "es-module/test-esm-cjs-builtins.js": {},
    "es-module/test-esm-cjs-exports.js": {},
    "es-module/test-esm-cjs-main.js": {},
    "es-module/test-esm-cyclic-dynamic-import.mjs": {},
    "es-module/test-esm-double-encoding.mjs": {},
    "es-module/test-esm-encoded-path-native.js": {},
    "es-module/test-esm-encoded-path.mjs": {},
    "es-module/test-esm-error-cache.js": {},
    "es-module/test-esm-example-loader.mjs": {},
    "es-module/test-esm-export-not-found.mjs": {},
    "es-module/test-esm-forbidden-globals.mjs": {},
    "es-module/test-esm-fs-promises.mjs": {},
    "es-module/test-esm-import-attributes-1.mjs": {},
    "es-module/test-esm-import-attributes-2.mjs": {},
    "es-module/test-esm-import-attributes-3.mjs": {},
    "es-module/test-esm-import-json-named-export.mjs": {},
    "es-module/test-esm-import-meta-main.mjs": {},
    "es-module/test-esm-in-require-cache-2.mjs": {},
    "es-module/test-esm-in-require-cache.js": {},
    "es-module/test-esm-loader-cache-clearing.js": {},
    "es-module/test-esm-loader-dependency.mjs": {},
    "es-module/test-esm-loader-event-loop.mjs": {},
    "es-module/test-esm-nowarn-exports.mjs": {},
    "es-module/test-esm-path-posix.mjs": {},
    "es-module/test-esm-path-win32.mjs": {},
    "es-module/test-esm-prototype-pollution.mjs": {},
    "es-module/test-esm-recursive-cjs-dependencies.mjs": {},
    "es-module/test-esm-repl-imports.js": {
      "ignore": true,
      "reason": "requires `deno --interactive` flag (not yet implemented); previously passed only because the test runner did not await Node-style `done` callbacks, which masked the failing assertion"
    },
    "es-module/test-esm-require-cache.mjs": {},
    "es-module/test-esm-scope-node-modules.mjs": {},
    "es-module/test-esm-shared-loader-dep.mjs": {},
    "es-module/test-esm-shebang.mjs": {},
    "es-module/test-esm-throw-undefined.mjs": {},
    "es-module/test-esm-tla.mjs": {},
    "es-module/test-esm-type-field.mjs": {},
    "es-module/test-esm-type-main.mjs": {},
    "es-module/test-esm-util-types.mjs": {},
    "es-module/test-esm-wasm-escape-import-names.mjs": {},
    "es-module/test-esm-wasm-load-exports.mjs": {},
    "es-module/test-esm-wasm-no-code-injection.mjs": {},
    "es-module/test-esm-wasm-source-phase-dynamic.mjs": {},
    "es-module/test-esm-wasm-source-phase-no-execute.mjs": {},
    "es-module/test-esm-wasm-source-phase-static.mjs": {},
    "es-module/test-import-preload-require-cycle.js": {},
    "es-module/test-loaders-hidden-from-users.js": {},
    "es-module/test-require-as-esm-interop.mjs": {},
    "es-module/test-require-module-cycle-cjs-esm-esm.js": {},
    "es-module/test-require-module-defined-esmodule.js": {},
    "es-module/test-require-module-detect-entry-point-aou.js": {},
    "es-module/test-require-module-detect-entry-point.js": {},
    "es-module/test-require-module-dont-detect-cjs.js": {},
    "es-module/test-require-module-dynamic-import-3.js": {},
    "es-module/test-require-module-retry-import-evaluating.js": {},
    "es-module/test-require-module-dynamic-import-4.js": {},
    "es-module/test-require-module-synchronous-rejection-handling.js": {},
    "es-module/test-require-module-tla-nested.js": {},
    "es-module/test-require-module-tla-rejected.js": {},
    "es-module/test-require-module-tla-resolved.js": {},
    "es-module/test-require-module-tla-unresolved.js": {},
    "es-module/test-require-module-transpiled.js": {},
    "es-module/test-require-module-with-detection.js": {},
    "es-module/test-typescript-commonjs.mjs": {},
    "es-module/test-typescript-eval.mjs": {},
    "es-module/test-typescript-module.mjs": {},
    "es-module/test-typescript-transform.mjs": {},
    "es-module/test-typescript.mjs": {},
    "es-module/test-vm-compile-function-lineoffset.js": {},
    "es-module/test-wasm-memory-out-of-bound.js": {},
    "es-module/test-wasm-simple.js": {},
    "internet/test-dns-ipv4.js": {},
    "internet/test-dns-ipv6.js": {
      "windows": false
    },
    "internet/test-snapshot-dns-lookup.js": {
      "ignore": true,
      "reason": "Node.js snapshot/heap profiling features (--build-snapshot, --heap-prof, --heapsnapshot-near-heap-limit) are not implemented in Deno"
    },
    "internet/test-snapshot-dns-resolve.js": {
      "ignore": true,
      "reason": "Node.js snapshot/heap profiling features (--build-snapshot, --heap-prof, --heapsnapshot-near-heap-limit) are not implemented in Deno"
    },
    "module-hooks/test-async-loader-hooks-globalpreload-no-warning-with-initialize.mjs": {},
    "module-hooks/test-async-loader-hooks-never-settling-race-cjs.mjs": {
      "ignore": true,
      "reason": "Flaky timeout - never-settling hook detection not yet implemented"
    },
    "module-hooks/test-async-loader-hooks-never-settling-race-esm.mjs": {
      "ignore": true,
      "reason": "Flaky timeout in debug builds - never-settling hook detection not yet implemented"
    },
    "module-hooks/test-async-loader-hooks-no-leak-internals.mjs": {
      "ignore": true,
      "reason": "Deno provides import.meta.resolve in workers; test asserts typeof import.meta.resolve === 'undefined'"
    },
    "module-hooks/test-async-loader-hooks-with-worker-permission-allowed.mjs": {},
    "module-hooks/test-module-hooks-load-buffers.js": {},
    "module-hooks/test-module-hooks-load-context-merged-esm.mjs": {},
    "module-hooks/test-module-hooks-load-context-merged.js": {},
    "module-hooks/test-module-hooks-load-context-optional-esm.mjs": {},
    "module-hooks/test-module-hooks-load-context-optional.js": {},
    "module-hooks/test-module-hooks-load-import-cjs.js": {},
    "module-hooks/test-module-hooks-load-mock.js": {},
    "module-hooks/test-module-hooks-load-short-circuit-required-middle.js": {},
    "module-hooks/test-module-hooks-load-short-circuit-required-start.js": {},
    "module-hooks/test-module-hooks-load-short-circuit.js": {},
    "module-hooks/test-module-hooks-load-url-change-require.js": {},
    "module-hooks/test-module-hooks-resolve-builtin-builtin-require.js": {},
    "module-hooks/test-module-hooks-resolve-builtin-on-disk-require-with-prefix.js": {},
    "module-hooks/test-module-hooks-resolve-context-merged.js": {},
    "module-hooks/test-module-hooks-resolve-context-optional.js": {},
    "module-hooks/test-module-hooks-resolve-load-builtin-override-both-prefix.js": {},
    "module-hooks/test-module-hooks-resolve-load-builtin-redirect-prefix.js": {},
    "module-hooks/test-module-hooks-resolve-load-builtin-redirect.js": {},
```

<a id="ref-q1-19"></a>
### [19] `ext/web/06_streams.js:5307-5337`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/06_streams.js#L5307-L5337)

```javascript
  static from(asyncIterable) {
    const prefix = "Failed to execute 'ReadableStream.from'";
    webidl.requiredArguments(
      arguments.length,
      1,
      prefix,
    );
    asyncIterable = webidl.converters["async iterable<any>"](
      asyncIterable,
      prefix,
      "Argument 1",
    );
    const iter = asyncIterable.open();

    const stream = createReadableStream(noop, async () => {
      // deno-lint-ignore prefer-primordials
      const res = await iter.next();
      if (res.done) {
        readableStreamDefaultControllerClose(stream[_controller]);
      } else {
        readableStreamDefaultControllerEnqueue(
          stream[_controller],
          await res.value,
        );
      }
    }, async (reason) => {
      // deno-lint-ignore prefer-primordials
      await iter.return(reason);
    }, 0);
    return stream;
  }
```

<a id="ref-q1-20"></a>
### [20] `ext/node/polyfills/internal/streams/readable.js:640-642`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/readable.js#L640-L642)

```javascript

  // Iterate over current buffer to convert already stored Buffers:
  let content = "";
```
