# DeepWiki Response

Repository: `denoland/deno`
Question: For Deno's Node compatibility layer, how does EventEmitter emit the node:events errorMonitor symbol when emit('error', err) is called? Focus on listener registration keying, order relative to regular error listeners, and whether the original error is still thrown when there are no regular error listeners.
Search: https://deepwiki.com/search/for-denos-node-compatibility-l_f2740b34-94e3-4449-a706-6bbf9777fcbf

**Question**: For Deno's Node compatibility layer, how does EventEmitter emit the node:events errorMonitor symbol when emit('error', err) is called? Focus on listener registration keying, order relative to regular error listeners, and whether the original error is still thrown when there are no regular error listeners.

In Deno's Node compatibility layer, when `emit('error', err)` is called on an `EventEmitter` instance, the `errorMonitor` symbol is emitted before any regular `'error'` listeners. The original error is still thrown if no regular `'error'` listeners are registered. [1](#ref-q1-1)

### Listener Registration and Keying [2](#ref-q1-2)
The `errorMonitor` symbol is a `unique symbol` defined as `kErrorMonitor` [3](#ref-q1-3) . It is exposed as a static property `EventEmitter.errorMonitor` [4](#ref-q1-4) . Listeners for `errorMonitor` are registered using this symbol as the event type, similar to how regular event listeners are registered for string-based event names [2](#ref-q1-2) .

### Order Relative to Regular Error Listeners [1](#ref-q1-1)
When `emit('error', ...args)` is called, the `EventEmitter.prototype.emit` method first checks if there are any listeners registered for `kErrorMonitor` [5](#ref-q1-5) . If `kErrorMonitor` listeners exist, `this.emit(kErrorMonitor, ...args)` is called *before* checking for or emitting to regular `'error'` listeners [6](#ref-q1-6) . This ensures that `errorMonitor` listeners are always invoked first. [6](#ref-q1-6)

### Error Throwing Behavior [7](#ref-q1-7)
If, after the `errorMonitor` listeners have been called, there are *no* regular `'error'` listeners registered for the `'error'` event (i.e., `events.error === undefined`), the `EventEmitter` will throw the original error [8](#ref-q1-8) . If the emitted argument is not an instance of `Error`, it will be wrapped in an `ERR_UNHANDLED_ERROR` and then thrown [9](#ref-q1-9) . This behavior is consistent with Node.js's `EventEmitter` where an `'error'` event without a listener will crash the process. [10](#ref-q1-10)

## Notes
The `EventEmitter` implementation in Deno's Node compatibility layer is located in `ext/node/polyfills/_events.mjs` [11](#ref-q1-11) . The TypeScript declaration for `errorMonitor` is found in `cli/tsc/dts/node/events.d.cts` [12](#ref-q1-12)  and `ext/node/polyfills/_events.d.ts` [13](#ref-q1-13) . The `tests/node_compat/config.jsonc` file indicates that there are compatibility tests specifically for `test-event-emitter-error-monitor.js`, confirming that this behavior is tested [14](#ref-q1-14) .

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-denos-node-compatibility-l_f2740b34-94e3-4449-a706-6bbf9777fcbf

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/_events.mjs:384-392`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L384-L392)

```
EventEmitter.prototype.emit = function emit(type, ...args) {
  let doError = type === "error";

  const events = this._events;
  if (events !== undefined) {
    if (doError && events[kErrorMonitor] !== undefined) {
      this.emit(kErrorMonitor, ...args);
    }
    doError = doError && events.error === undefined;
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/_events.mjs:468-540`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L468-L540)

```
function _addListener(target, type, listener, prepend) {
  let m;
  let events;
  let existing;

  checkListener(listener);

  events = target._events;
  if (events === undefined) {
    events = target._events = ObjectCreate(null);
    target._eventsCount = 0;
  } else {
    // To avoid recursion in the case that type === "newListener"! Before
    // adding it to the listeners, first emit "newListener".
    if (events.newListener !== undefined) {
      target.emit("newListener", type, listener.listener ?? listener);

      // Re-assign `events` because a newListener handler could have caused the
      // this._events to be assigned to a new object
      events = target._events;
    }
    existing = events[type];
  }

  if (existing === undefined) {
    // Optimize the case of one listener. Don't need the extra array object.
    events[type] = listener;
    ++target._eventsCount;
  } else {
    if (typeof existing === "function") {
      // Adding the second element, need to change to array.
      existing = events[type] = prepend
        ? [listener, existing]
        : [existing, listener];
      // If we've already got an array, just append.
    } else if (prepend) {
      ArrayPrototypeUnshift(existing, listener);
    } else {
      ArrayPrototypePush(existing, listener);
    }

    // Check for listener leak
    m = _getMaxListeners(target);
    if (m > 0 && existing.length > m && !existing.warned) {
      existing.warned = true;
      // No error code for this since it is a Warning
      // eslint-disable-next-line no-restricted-syntax
      const w = new Error(
        "Possible EventEmitter memory leak detected. " +
          `${existing.length} ${String(type)} listeners ` +
          `added to ${inspect(target, { depth: -1 })}. ` +
          `MaxListeners is ${m}.`,
      );
      w.name = "MaxListenersExceededWarning";
      w.emitter = target;
      w.type = type;
      w.count = existing.length;
      process.emitWarning(w);
    }
  }

  return target;
}

/**
 * Adds a listener to the event emitter.
 * @param {string | symbol} type
 * @param {Function} listener
 * @returns {EventEmitter}
 */
EventEmitter.prototype.addListener = function addListener(type, listener) {
  return _addListener(this, type, listener, false);
};
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/_events.mjs:88`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L88)

```
const kErrorMonitor = Symbol("events.errorMonitor");
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/_events.mjs:128`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L128)

```
EventEmitter.errorMonitor = kErrorMonitor;
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/_events.mjs:388-389`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L388-L389)

```
  if (events !== undefined) {
    if (doError && events[kErrorMonitor] !== undefined) {
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/_events.mjs:389-390`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L389-L390)

```
    if (doError && events[kErrorMonitor] !== undefined) {
      this.emit(kErrorMonitor, ...args);
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/_events.mjs:397-431`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L397-L431)

```
  // If there is no 'error' event listener then throw.
  if (doError) {
    let er;
    if (args.length > 0) {
      er = args[0];
    }
    if (er instanceof Error) {
      try {
        const capture = {};
        ErrorCaptureStackTrace(capture, EventEmitter.prototype.emit);
        // ObjectDefineProperty(er, kEnhanceStackBeforeInspector, {
        //   value: enhanceStackTrace.bind(this, er, capture),
        //   configurable: true
        // });
      } catch {
        // pass
      }

      // Note: The comments on the `throw` lines are intentional, they show
      // up in Node's output if this results in an unhandled exception.
      throw er; // Unhandled 'error' event
    }

    let stringifiedEr;
    try {
      stringifiedEr = inspect(er);
    } catch {
      stringifiedEr = er;
    }

    // At least give some kind of context to the user
    const err = new ERR_UNHANDLED_ERROR(stringifiedEr);
    err.context = er;
    throw err; // Unhandled 'error' event
  }
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/_events.mjs:397-417`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L397-L417)

```
  // If there is no 'error' event listener then throw.
  if (doError) {
    let er;
    if (args.length > 0) {
      er = args[0];
    }
    if (er instanceof Error) {
      try {
        const capture = {};
        ErrorCaptureStackTrace(capture, EventEmitter.prototype.emit);
        // ObjectDefineProperty(er, kEnhanceStackBeforeInspector, {
        //   value: enhanceStackTrace.bind(this, er, capture),
        //   configurable: true
        // });
      } catch {
        // pass
      }

      // Note: The comments on the `throw` lines are intentional, they show
      // up in Node's output if this results in an unhandled exception.
      throw er; // Unhandled 'error' event
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/_events.mjs:427-430`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L427-L430)

```
    // At least give some kind of context to the user
    const err = new ERR_UNHANDLED_ERROR(stringifiedEr);
    err.context = er;
    throw err; // Unhandled 'error' event
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/_events.mjs:417`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L417)

```
      throw er; // Unhandled 'error' event
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/_events.mjs:31-180`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.mjs#L31-L180)

```
  ArrayPrototypePush,
  ArrayPrototypeShift,
  ArrayPrototypeUnshift,
  Error,
  ErrorCaptureStackTrace,
  FunctionPrototypeCall,
  FunctionPrototypeApply,
  ObjectCreate,
  ObjectDefineProperty,
  ObjectEntries,
  ObjectGetPrototypeOf,
  ObjectSetPrototypeOf,
  ReflectOwnKeys,
  SafeMap,
  SafeSet,
  Symbol,
  SymbolFor,
  SymbolAsyncIterator,
} = primordials;

const kRejection = SymbolFor("nodejs.rejection");
const kWatermarkData = SymbolFor("nodejs.watermarkData");
const kEvents = Symbol("kEvents");

const { inspect } = core.loadExtScript(
  "ext:deno_node/internal/util/inspect.mjs",
);
const { kEmptyObject } = core.loadExtScript("ext:deno_node/internal/util.mjs");
const {
  AbortError,
  ERR_INVALID_ARG_TYPE,
  ERR_INVALID_THIS,
  ERR_UNHANDLED_ERROR,
} = core.loadExtScript("ext:deno_node/internal/errors.ts");

const lazyAsyncHooks = core.createLazyLoader("node:async_hooks");
const {
  validateAbortSignal,
  validateBoolean,
  validateFunction,
  validateInteger,
  validateNumber,
  validateObject,
  validateString,
} = core.loadExtScript("ext:deno_node/internal/validators.mjs");
const { spliceOne } = core.loadExtScript("ext:deno_node/_utils.ts");
const { nextTick } = core.loadExtScript("ext:deno_node/_process/process.ts");
const {
  eventTargetData,
  kResistStopImmediatePropagation,
} = core.loadExtScript("ext:deno_web/02_event.js");

const { addAbortListener } = core.loadExtScript(
  "ext:deno_node/internal/events/abort_listener.mjs",
);
const kFirstEventParam = Symbol("kFirstEventParam");
const kCapture = Symbol("kCapture");
const kErrorMonitor = Symbol("events.errorMonitor");
const kMaxEventTargetListeners = Symbol("events.maxEventTargetListeners");
const kMaxEventTargetListenersWarned = Symbol(
  "events.maxEventTargetListenersWarned",
);

/**
 * Creates a new `EventEmitter` instance.
 * @param {{ captureRejections?: boolean; }} [opts]
 * @returns {EventEmitter}
 */
function EventEmitter(opts) {
  FunctionPrototypeCall(EventEmitter.init, this, opts);
}
EventEmitter.on = on;
EventEmitter.once = once;
EventEmitter.getEventListeners = getEventListeners;
EventEmitter.setMaxListeners = setMaxListeners;
EventEmitter.getMaxListeners = getMaxListeners;
EventEmitter.listenerCount = listenerCount;
// Backwards-compat with node 0.10.x
EventEmitter.EventEmitter = EventEmitter;
EventEmitter.usingDomains = false;

EventEmitter.captureRejectionSymbol = kRejection;
const captureRejectionSymbol = EventEmitter.captureRejectionSymbol;
const errorMonitor = EventEmitter.errorMonitor;

ObjectDefineProperty(EventEmitter, "captureRejections", {
  get() {
    return EventEmitter.prototype[kCapture];
  },
  set(value) {
    validateBoolean(value, "EventEmitter.captureRejections");

    EventEmitter.prototype[kCapture] = value;
  },
  enumerable: true,
});

EventEmitter.errorMonitor = kErrorMonitor;

// The default for captureRejections is false
ObjectDefineProperty(EventEmitter.prototype, kCapture, {
  value: false,
  writable: true,
  enumerable: false,
});

EventEmitter.prototype._events = undefined;
EventEmitter.prototype._eventsCount = 0;
EventEmitter.prototype._maxListeners = undefined;

// By default EventEmitters will print a warning if more than 10 listeners are
// added to it. This is a useful default which helps finding memory leaks.
let defaultMaxListeners = 10;

function checkListener(listener) {
  validateFunction(listener, "listener");
}

ObjectDefineProperty(EventEmitter, "defaultMaxListeners", {
  enumerable: true,
  get: function () {
    return defaultMaxListeners;
  },
  set: function (arg) {
    validateNumber(arg, "defaultMaxListeners", 0);
    defaultMaxListeners = arg;
  },
});

Object.defineProperties(EventEmitter, {
  kMaxEventTargetListeners: {
    value: kMaxEventTargetListeners,
    enumerable: false,
    configurable: false,
    writable: false,
  },
  kMaxEventTargetListenersWarned: {
    value: kMaxEventTargetListenersWarned,
    enumerable: false,
    configurable: false,
    writable: false,
  },
});

/**
 * Sets the max listeners.
 * @param {number} n
 * @param {EventTarget[] | EventEmitter[]} [eventTargets]
 * @returns {void}
 */
```

<a id="ref-q1-12"></a>
### [12] `cli/tsc/dts/node/events.d.cts:444`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/events.d.cts#L444)

```
        static readonly errorMonitor: unique symbol;
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/_events.d.ts:8`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_events.d.ts#L8)

```typescript
export const errorMonitor: unique symbol;
```

<a id="ref-q1-14"></a>
### [14] `tests/node_compat/config.jsonc:1118`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L1118)

```
    "parallel/test-event-emitter-error-monitor.js": {},
```
