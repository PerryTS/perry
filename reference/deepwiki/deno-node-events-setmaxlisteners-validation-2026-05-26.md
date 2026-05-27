# DeepWiki Response

Repository: `denoland/deno`
Question: For Deno's Node compatibility layer, how is events.setMaxListeners(n, ...eventTargets) validated and applied? Focus on the invariant for negative or invalid listener counts, multi-target behavior, and the observable RangeError code/name expected by Node-compatible tests.
Search: https://deepwiki.com/search/for-denos-node-compatibility-l_b7181110-e2d7-43fa-9585-09108a5e074e

**Question**: For Deno's Node compatibility layer, how is events.setMaxListeners(n, ...eventTargets) validated and applied? Focus on the invariant for negative or invalid listener counts, multi-target behavior, and the observable RangeError code/name expected by Node-compatible tests.

Deno's Node compatibility layer validates and applies `events.setMaxListeners(n, ...eventTargets)` by ensuring `n` is a non-negative number and then applying it to the specified event targets or as the default maximum for all new `EventEmitter` and `EventTarget` objects [1](#ref-q1-1) . Invalid listener counts (negative numbers) are caught by a `validateNumber` utility, which ensures `n` is greater than or equal to 0 [2](#ref-q1-2) . Multi-target behavior is handled by iterating through the `eventTargets` array and applying the `n` value to each target individually [3](#ref-q1-3) .

## Validation and Application of `setMaxListeners`

The `setMaxListeners` function in Deno's Node compatibility layer is defined in `ext/node/polyfills/_events.mjs` [1](#ref-q1-1) .

### Listener Count Validation

The first step in `setMaxListeners` is to validate the input `n` [2](#ref-q1-2) . This is done using `validateNumber(n, "setMaxListeners", 0)` [2](#ref-q1-2) , which ensures that `n` is a non-negative number. If `n` is negative or not a number, an `ERR_INVALID_ARG_TYPE` error would be thrown, consistent with Node.js behavior.

### Multi-Target Behavior

If `eventTargets` are provided, the function iterates through each target [4](#ref-q1-4) :
*   **`EventTarget` instances**: If a target is an instance of `EventTarget`, the `n` value is assigned to its internal `kMaxEventTargetListeners` symbol property [5](#ref-q1-5) . Additionally, `kMaxEventTargetListenersWarned` is set to `false` to reset any previous warnings [6](#ref-q1-6) . The `EventTarget` class itself initializes `kMaxEventTargetListeners` with `EventEmitter.defaultMaxListeners` [7](#ref-q1-7) .
*   **`EventEmitter` instances**: If a target has a `setMaxListeners` method (which `EventEmitter` instances do), that method is called with `n` [8](#ref-q1-8) . The `EventEmitter.prototype.setMaxListeners` method sets the `_maxListeners` property of the instance [9](#ref-q1-9) .
*   **Invalid Targets**: If a target is neither an `EventTarget` nor an `EventEmitter`-like object, an `ERR_INVALID_ARG_TYPE` is thrown [10](#ref-q1-10) .

### Default Max Listeners

If no `eventTargets` are provided, the `n` value is set as the `defaultMaxListeners` for all new `EventEmitter` and `EventTarget` objects [11](#ref-q1-11) . This `defaultMaxListeners` is initially `10` [12](#ref-q1-12)  and can be accessed and modified via `EventEmitter.defaultMaxListeners` [13](#ref-q1-13) .

### Observable RangeError Code/Name

While the code explicitly throws `ERR_INVALID_ARG_TYPE` for invalid arguments to `setMaxListeners` [10](#ref-q1-10) , the Node.js documentation for `events.defaultMaxListeners` mentions that a `RangeError` is thrown if the value is not a positive number [14](#ref-q1-14) . In Deno's implementation, the `validateNumber` function is responsible for this validation. If `n` is less than 0, `validateNumber` would throw an error. The specific error code `ERR_INVALID_ARG_VALUE` is typically used for out-of-range numeric values, which aligns with the `RangeError` described in Node.js documentation.

## Notes

The `events.setMaxListeners` function is exported from `ext:deno_node/_events.mjs` [15](#ref-q1-15) . The `kMaxEventTargetListeners` and `kMaxEventTargetListenersWarned` symbols are used internally to manage the maximum listener count and warning state for `EventTarget` instances [16](#ref-q1-16) . The `NodeEventTarget` class also uses `EventEmitter.setMaxListeners` internally for its own `setMaxListeners` method [17](#ref-q1-17) .

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)

View this search on DeepWiki: https://deepwiki.com/search/for-denos-node-compatibility-l_b7181110-e2d7-43fa-9585-09108a5e074e

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/_events.mjs:181-205`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L181-L205)

```
function setMaxListeners(
  n = defaultMaxListeners,
  ...eventTargets
) {
  validateNumber(n, "setMaxListeners", 0);
  if (eventTargets.length === 0) {
    defaultMaxListeners = n;
  } else {
    for (let i = 0; i < eventTargets.length; i++) {
      const target = eventTargets[i];
      if (target instanceof EventTarget) {
        target[kMaxEventTargetListeners] = n;
        target[kMaxEventTargetListenersWarned] = false;
      } else if (typeof target.setMaxListeners === "function") {
        target.setMaxListeners(n);
      } else {
        throw new ERR_INVALID_ARG_TYPE(
          "eventTargets",
          ["EventEmitter", "EventTarget"],
          target,
        );
      }
    }
  }
}
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/_events.mjs:185`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L185)

```
  validateNumber(n, "setMaxListeners", 0);
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/_events.mjs:189-203`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L189-L203)

```
    for (let i = 0; i < eventTargets.length; i++) {
      const target = eventTargets[i];
      if (target instanceof EventTarget) {
        target[kMaxEventTargetListeners] = n;
        target[kMaxEventTargetListenersWarned] = false;
      } else if (typeof target.setMaxListeners === "function") {
        target.setMaxListeners(n);
      } else {
        throw new ERR_INVALID_ARG_TYPE(
          "eventTargets",
          ["EventEmitter", "EventTarget"],
          target,
        );
      }
    }
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/_events.mjs:189-190`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L189-L190)

```
    for (let i = 0; i < eventTargets.length; i++) {
      const target = eventTargets[i];
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/_events.mjs:191-192`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L191-L192)

```
      if (target instanceof EventTarget) {
        target[kMaxEventTargetListeners] = n;
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/_events.mjs:193`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L193)

```
        target[kMaxEventTargetListenersWarned] = false;
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/event_target.mjs:499`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/event_target.mjs#L499)

```
  self[kMaxEventTargetListeners] = EventEmitter.defaultMaxListeners;
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/_events.mjs:194-195`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L194-L195)

```
      } else if (typeof target.setMaxListeners === "function") {
        target.setMaxListeners(n);
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/_events.mjs:276-280`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L276-L280)

```
EventEmitter.prototype.setMaxListeners = function setMaxListeners(n) {
  validateNumber(n, "setMaxListeners", 0);
  this._maxListeners = n;
  return this;
};
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/_events.mjs:197-202`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L197-L202)

```
        throw new ERR_INVALID_ARG_TYPE(
          "eventTargets",
          ["EventEmitter", "EventTarget"],
          target,
        );
      }
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/_events.mjs:186-187`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L186-L187)

```
  if (eventTargets.length === 0) {
    defaultMaxListeners = n;
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/_events.mjs:143`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L143)

```
let defaultMaxListeners = 10;
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/_events.mjs:150-157`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L150-L157)

```
  enumerable: true,
  get: function () {
    return defaultMaxListeners;
  },
  set: function (arg) {
    validateNumber(arg, "defaultMaxListeners", 0);
    defaultMaxListeners = arg;
  },
```

<a id="ref-q1-14"></a>
### [14] `cli/tsc/dts/node/events.d.cts:464-465`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/events.d.cts#L464-L465)

```
         * can be used. If this value is not a positive number, a `RangeError` is thrown.
         *
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/events.ts:16`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/events.ts#L16)

```typescript
  setMaxListeners,
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/_events.mjs:89-92`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L89-L92)

```
const kMaxEventTargetListeners = Symbol("events.maxEventTargetListeners");
const kMaxEventTargetListenersWarned = Symbol(
  "events.maxEventTargetListenersWarned",
);
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/internal/event_target.mjs:841-846`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/event_target.mjs#L841-L846)

```
  setMaxListeners(n) {
    if (!isNodeEventTarget(this)) {
      throw new ERR_INVALID_THIS("NodeEventTarget");
    }
    EventEmitter.setMaxListeners(n, this);
  }
```
