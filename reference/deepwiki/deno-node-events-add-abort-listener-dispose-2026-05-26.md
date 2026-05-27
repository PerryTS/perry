# DeepWiki Response

Repository: `denoland/deno`
Question: In Deno's Node compatibility implementation, what are the key invariants for node:events addAbortListener and AbortSignal abort listener disposal: should abort fire listeners only once, and should the returned Symbol.dispose function unregister without invoking the user callback? Point to the relevant implementation or tests if present.
Search: https://deepwiki.com/search/in-denos-node-compatibility-im_b6d3c83b-65dc-458f-9606-fec08adacb3d

**Question**: In Deno's Node compatibility implementation, what are the key invariants for node:events addAbortListener and AbortSignal abort listener disposal: should abort fire listeners only once, and should the returned Symbol.dispose function unregister without invoking the user callback? Point to the relevant implementation or tests if present.

In Deno's Node compatibility implementation, the `addAbortListener` function ensures that listeners are fired only once when an `AbortSignal` is aborted, and the returned `Symbol.dispose` function unregisters the listener without invoking the user callback. [1](#ref-q1-1) 

## `addAbortListener` Invariants

### Listeners Fire Only Once
The `addAbortListener` function, defined in `ext/node/polyfills/internal/events/abort_listener.mjs`, attaches a listener to an `AbortSignal`. [2](#ref-q1-2)  If the signal is already aborted, the listener is invoked asynchronously via `queueMicrotask` [3](#ref-q1-3) . Otherwise, it adds the listener to the signal with the `once: true` option, ensuring it's called only a single time when the `abort` event occurs [4](#ref-q1-4) . This behavior is consistent with the Node.js `events` module's `addAbortListener` as described in its TypeScript definition [5](#ref-q1-5) .

A test case in `tests/unit_node/events_test.ts` explicitly verifies this behavior: an `AbortController` is created, `addAbortListener` is used to attach a listener, and then the controller is aborted, confirming the listener is called [6](#ref-q1-6) . Another test in `tests/unit/abort_controller_test.ts` further confirms that an `AbortSignal`'s `abort` event listeners are called only once, even if `abort()` is called multiple times [7](#ref-q1-7) .

### `Symbol.dispose` Unregisters Without Invoking Callback
The `addAbortListener` function returns an object that implements the `Symbol.dispose` method [8](#ref-q1-8) . When `Symbol.dispose` is called, it invokes `removeEventListener` on the `AbortSignal` to unregister the listener [9](#ref-q1-9) . This action solely removes the listener from the signal and does not trigger the user-provided callback. [9](#ref-q1-9) 

This disposal mechanism is utilized in other parts of the Node.js compatibility layer, such as in `ext/node/polyfills/internal/streams/add-abort-signal.js` [10](#ref-q1-10)  and `ext/node/polyfills/internal/streams/end-of-stream.js` [11](#ref-q1-11) . For instance, in `add-abort-signal.js`, the `disposable[SymbolDispose]()` is called to clean up the abort listener when the stream ends [10](#ref-q1-10) . Similarly, in `end-of-stream.js`, the `Symbol.dispose` method is used to remove the abort listener once the stream operation is complete [11](#ref-q1-11) .

## Notes
The `addAbortListener` function is imported and re-exported in `ext/node/polyfills/events.ts` [12](#ref-q1-12)  and used in `ext/node/polyfills/http2.ts` [13](#ref-q1-13)  to integrate abort signal handling into various Node.js compatible APIs. The `Node.js Compatibility Layer` wiki page provides a high-level overview of how Node.js APIs are polyfilled in Deno [14](#ref-q1-14) .

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/in-denos-node-compatibility-im_b6d3c83b-65dc-458f-9606-fec08adacb3d

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/events/abort_listener.mjs:17-41`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L17-L41)

```
function addAbortListener(signal, listener) {
  if (signal === undefined) {
    throw new ERR_INVALID_ARG_TYPE("signal", "AbortSignal", signal);
  }
  validateAbortSignal(signal, "signal");
  validateFunction(listener, "listener");

  let removeEventListener;
  if (signal.aborted) {
    queueMicrotask(() => listener({ target: signal }));
  } else {
    signal.addEventListener("abort", listener, {
      __proto__: null,
      once: true,
    });
    removeEventListener = () => {
      signal.removeEventListener("abort", listener);
    };
  }
  return {
    __proto__: null,
    [SymbolDispose]() {
      removeEventListener?.();
    },
  };
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/internal/events/abort_listener.mjs:17-22`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L17-L22)

```
function addAbortListener(signal, listener) {
  if (signal === undefined) {
    throw new ERR_INVALID_ARG_TYPE("signal", "AbortSignal", signal);
  }
  validateAbortSignal(signal, "signal");
  validateFunction(listener, "listener");
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/internal/events/abort_listener.mjs:25-26`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L25-L26)

```
  if (signal.aborted) {
    queueMicrotask(() => listener({ target: signal }));
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/internal/events/abort_listener.mjs:28-31`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L28-L31)

```
    signal.addEventListener("abort", listener, {
      __proto__: null,
      once: true,
    });
```

<a id="ref-q1-5"></a>
### [5] `cli/tsc/dts/node/events.d.cts:404-416`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/events.d.cts#L404-L416)

```
         * Listens once to the `abort` event on the provided `signal`.
         *
         * Listening to the `abort` event on abort signals is unsafe and may
         * lead to resource leaks since another third party with the signal can
         * call `e.stopImmediatePropagation()`. Unfortunately Node.js cannot change
         * this since it would violate the web standard. Additionally, the original
         * API makes it easy to forget to remove listeners.
         *
         * This API allows safely using `AbortSignal`s in Node.js APIs by solving these
         * two issues by listening to the event such that `stopImmediatePropagation` does
         * not prevent the listener from running.
         *
         * Returns a disposable so that it may be unsubscribed from more easily.
```

<a id="ref-q1-6"></a>
### [6] `tests/unit_node/events_test.ts:38-46`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/events_test.ts#L38-L46)

```typescript
Deno.test("addAbortListener", async () => {
  const { promise, resolve } = Promise.withResolvers<void>();
  const abortController = new AbortController();
  addAbortListener(abortController.signal, () => {
    resolve();
  });
  abortController.abort();
  await promise;
});
```

<a id="ref-q1-7"></a>
### [7] `tests/unit/abort_controller_test.ts:41-53`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/abort_controller_test.ts#L41-L53)

```typescript
Deno.test(function onlyAbortsOnce() {
  const controller = new AbortController();
  const { signal } = controller;
  let called = 0;
  signal.addEventListener("abort", () => called++);
  signal.onabort = () => {
    called++;
  };
  controller.abort();
  assertEquals(called, 2);
  controller.abort();
  assertEquals(called, 2);
});
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/internal/events/abort_listener.mjs:36-41`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L36-L41)

```
  return {
    __proto__: null,
    [SymbolDispose]() {
      removeEventListener?.();
    },
  };
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/internal/events/abort_listener.mjs:38-39`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/events/abort_listener.mjs#L38-L39)

```
    [SymbolDispose]() {
      removeEventListener?.();
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/internal/streams/add-abort-signal.js:72-73`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/add-abort-signal.js#L72-L73)

```javascript
    const disposable = addAbortListener(signal, onAbort);
    eos(stream, disposable[SymbolDispose]);
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal/streams/end-of-stream.js:289-292`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/end-of-stream.js#L289-L292)

```javascript
      const disposable = addAbortListener(options.signal, abort);
      const originalCallback = callback;
      callback = once((...args) => {
        disposable[SymbolDispose]();
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/events.ts:3-17`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/events.ts#L3-L17)

```typescript
export {
  addAbortListener,
  captureRejectionSymbol,
  default,
  defaultMaxListeners,
  errorMonitor,
  EventEmitter,
  EventEmitterAsyncResource,
  getEventListeners,
  getMaxListeners,
  listenerCount,
  on,
  once,
  setMaxListeners,
} from "ext:deno_node/_events.mjs";
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/http2.ts:97-100`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/http2.ts#L97-L100)

```typescript
const { addAbortListener } = core.loadExtScript(
  "ext:deno_node/internal/events/abort_listener.mjs",
);
export { addAbortListener };
```

<a id="ref-q1-14"></a>
### [14] `ext/node/lib.rs:166-169`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/lib.rs#L166-L169)
