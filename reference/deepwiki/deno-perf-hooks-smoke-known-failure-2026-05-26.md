# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests establish the node:perf_hooks surface covered by a broad smoke test: performance.now/timeOrigin, mark/measure/getEntries, clearMarks/clearMeasures, toJSON/nodeTiming, PerformanceObserver observe/disconnect/takeRecords, createHistogram, monitorEventLoopDelay, eventLoopUtilization, and timerify? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry test_parity_perf_hooks known-failure entry.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_bd5ffce5-01d4-407e-8519-53046d84ccd2

**Question**: For Node compatibility in Deno, what source files or tests establish the node:perf_hooks surface covered by a broad smoke test: performance.now/timeOrigin, mark/measure/getEntries, clearMarks/clearMeasures, toJSON/nodeTiming, PerformanceObserver observe/disconnect/takeRecords, createHistogram, monitorEventLoopDelay, eventLoopUtilization, and timerify? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry test_parity_perf_hooks known-failure entry.

The `node:perf_hooks` surface in Deno's Node compatibility layer is primarily established by the source file `ext/node/polyfills/perf_hooks.js` [1](#ref-q1-1)  and tested by `tests/unit_node/perf_hooks_test.ts` [2](#ref-q1-2) . These files cover the functionalities you mentioned, including `performance.now`/`timeOrigin`, `mark`/`measure`/`getEntries`, `clearMarks`/`clearMeasures`, `PerformanceObserver` (`observe`/`disconnect`/`takeRecords`), `monitorEventLoopDelay`, `eventLoopUtilization`, and `timerify`.

### Source File: `ext/node/polyfills/perf_hooks.js`
This file provides the Node.js-compatible implementation for the `perf_hooks` module [1](#ref-q1-1) .

*   **`performance.now()` and `performance.timeOrigin`**: The `performance` object is loaded from Deno's web performance implementation [3](#ref-q1-3) . `timeOrigin` is exposed directly from this underlying `performance` object [4](#ref-q1-4) .
*   **`mark`/`measure`/`getEntries`, `clearMarks`/`clearMeasures`**: These functions are part of the standard Web Performance API and are inherited from Deno's `ext:deno_web/15_performance.js` [3](#ref-q1-3) . The `clearMarks` and `clearMeasures` implementations filter `performanceEntries` based on the provided name or entry type [5](#ref-q1-5) .
*   **`toJSON`/`nodeTiming`**: The `performance.nodeTiming` object is initialized as an empty object [6](#ref-q1-6) . The `toJSON` method for the `Performance` object is inherited from the web standard implementation and returns `timeOrigin` [7](#ref-q1-7) .
*   **`PerformanceObserver` (`observe`/`disconnect`/`takeRecords`)**: A Node-compatible `PerformanceObserver` class extends the Web `PerformanceObserver` [8](#ref-q1-8) . It handles Node-specific entry types in addition to web standard ones [9](#ref-q1-9) . The `observe` method dispatches to the superclass for web types and manages internal buffers for Node-specific types [10](#ref-q1-10) . The `disconnect` method calls the superclass's `disconnect` and clears Node-specific buffers [11](#ref-q1-11) . The `takeRecords` functionality is implicitly handled by the `PerformanceObserver` callback mechanism, where `getEntries` on the `PerformanceObserverEntryList` provides the records [12](#ref-q1-12) .
*   **`createHistogram`**: This function is not directly exposed in `ext/node/polyfills/perf_hooks.js`. Instead, `monitorEventLoopDelay` uses `EldHistogram` from `core.ops` [13](#ref-q1-13)  [14](#ref-q1-14) .
*   **`monitorEventLoopDelay`**: This function creates and returns an `EldHistogram` instance [15](#ref-q1-15) .
*   **`eventLoopUtilization`**: This function is currently a stub, returning `{ idle: 0, active: 0, utilization: 0 }` [16](#ref-q1-16) .
*   **`timerify`**: This function wraps a given function and returns a new function. It includes argument type validation [17](#ref-q1-17) .

### Test File: `tests/unit_node/perf_hooks_test.ts`
This file contains unit tests specifically for the `node:perf_hooks` module's compatibility with Node.js [2](#ref-q1-2) .

*   **`performance.timeOrigin`**: Tested for its type and immutability [18](#ref-q1-18) .
*   **`eventLoopUtilization`**: Tests that the returned object has `idle`, `active`, and `utilization` properties of type `number` [19](#ref-q1-19) .
*   **`monitorEventLoopDelay`**: Tests enabling and disabling the monitor, and verifies that `min` and `count` properties are greater than zero after a delay [20](#ref-q1-20) .
*   **`PerformanceObserver` (`observe`/`disconnect`/`takeRecords`)**:
    *   `supportedEntryTypes` is checked to include "mark" and "measure" [21](#ref-q1-21) .
    *   Tests observing "mark" entries, verifying that marks are captured and the observer can be disconnected [22](#ref-q1-22) .
    *   Tests observing "measure" entries [23](#ref-q1-23) .
    *   Tests that `disconnect` stops observation [24](#ref-q1-24) .
    *   Tests `takeRecords` to ensure it returns buffered entries and clears the buffer [25](#ref-q1-25) .
*   **`timerify`**: A basic test verifies that `performance.timerify` wraps a function and returns its result correctly [26](#ref-q1-26) .

### Additional Context
The `node:perf_hooks` module is mapped to `ext/node/polyfills/perf_hooks.ts` (or `.js`) via `tools/core_import_map.json` [27](#ref-q1-27) . The web-standard `performance` object and `PerformanceObserver` are sourced from `ext:deno_web/15_performance.js` [3](#ref-q1-3) .

## Notes
The `tests/node_compat/config.jsonc` file lists Node.js official compatibility tests [28](#ref-q1-28) . Specifically, `parallel/test-performance-global.js`, `parallel/test-performance-many-marks.js`, `parallel/test-performance-measure.js`, `parallel/test-performanceobserver-gc.js`, and `parallel/test-performanceobserver.js` are relevant to the `perf_hooks` surface. These are upstream Node.js tests used to validate Deno's compatibility layer [29](#ref-q1-29) . The `tests/unit/performance_test.ts` file [30](#ref-q1-30)  covers the web-standard `Performance` API, which forms the base for `node:perf_hooks`.

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_bd5ffce5-01d4-407e-8519-53046d84ccd2

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/perf_hooks.js:1-238`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L1-L238)

```javascript
// Copyright 2018-2026 the Deno authors. MIT license.

// TODO(petamoriken): enable prefer-primordials for node polyfills
// deno-lint-ignore-file prefer-primordials

(function () {
const { core } = globalThis.__bootstrap;
const {
  performance,
  PerformanceEntry,
  PerformanceObserver: WebPerformanceObserver,
  PerformanceObserverEntryList,
} = core.loadExtScript("ext:deno_web/15_performance.js");
const { EldHistogram } = core.ops;
const { ERR_INVALID_ARG_TYPE } = core.loadExtScript(
  "ext:deno_node/internal/errors.ts",
);

const constants = {
  NODE_PERFORMANCE_ENTRY_TYPE_NODE: 0,
  NODE_PERFORMANCE_ENTRY_TYPE_MARK: 1,
  NODE_PERFORMANCE_ENTRY_TYPE_MEASURE: 2,
  NODE_PERFORMANCE_ENTRY_TYPE_GC: 3,
  NODE_PERFORMANCE_ENTRY_TYPE_FUNCTION: 4,
  NODE_PERFORMANCE_ENTRY_TYPE_HTTP2: 5,
  NODE_PERFORMANCE_ENTRY_TYPE_HTTP: 6,
  NODE_PERFORMANCE_ENTRY_TYPE_DNS: 7,
  NODE_PERFORMANCE_ENTRY_TYPE_NET: 8,
};

// Entry types Node.js's PerformanceObserver supports beyond the web spec's
// "mark"/"measure". The web layer's PerformanceObserver filters these out via
// supportedEntryTypes, so this subclass tracks them in a parallel registry.
const NODE_ENTRY_TYPES = ["http2", "function", "gc", "http", "dns", "net"];

const nodeObservers = [];
const _nodeTypes = Symbol("[[nodeTypes]]");
const _nodeBuffer = Symbol("[[nodeBuffer]]");
const _nodeScheduled = Symbol("[[nodeScheduled]]");
const _nodeCallback = Symbol("[[nodeCallback]]");

function createNodeEntryList(entries) {
  return {
    getEntries() {
      return entries.slice();
    },
    getEntriesByType(type) {
      return entries.filter((e) => e.entryType === type);
    },
    getEntriesByName(name, type) {
      return entries.filter((e) =>
        e.name === name && (type === undefined || e.entryType === type)
      );
    },
  };
}

// Node-compatible PerformanceObserver that throws proper Node.js errors
class PerformanceObserver extends WebPerformanceObserver {
  [_nodeTypes] = [];
  [_nodeBuffer] = [];
  [_nodeScheduled] = false;
  [_nodeCallback] = null;

  constructor(callback) {
    if (typeof callback !== "function") {
      throw new ERR_INVALID_ARG_TYPE("callback", "Function", callback);
    }
    super(callback);
    this[_nodeCallback] = callback;
  }

  static get supportedEntryTypes() {
    return [
      ...WebPerformanceObserver.supportedEntryTypes,
      ...NODE_ENTRY_TYPES,
    ];
  }

  observe(options) {
    if (typeof options !== "object" || options === null) {
      throw new ERR_INVALID_ARG_TYPE("options", "Object", options);
    }
    if (
      options.entryTypes !== undefined && !Array.isArray(options.entryTypes)
    ) {
      throw new ERR_INVALID_ARG_TYPE(
        "options.entryTypes",
        "string[]",
        options.entryTypes,
      );
    }

    const requestedTypes = options.entryTypes !== undefined
      ? options.entryTypes
      : (options.type !== undefined ? [options.type] : []);

    const webTypes = requestedTypes.filter(
      (t) => !NODE_ENTRY_TYPES.includes(t),
    );
    const nodeTypes = requestedTypes.filter(
      (t) => NODE_ENTRY_TYPES.includes(t),
    );

    if (webTypes.length > 0) {
      if (options.entryTypes !== undefined) {
        super.observe({ entryTypes: webTypes, buffered: options.buffered });
      } else if (webTypes.length === 1) {
        super.observe({ type: webTypes[0], buffered: options.buffered });
      }
    }

    if (nodeTypes.length > 0) {
      this[_nodeTypes] = nodeTypes;
      this[_nodeBuffer] = [];
      if (!nodeObservers.includes(this)) {
        nodeObservers.push(this);
      }
    }
  }

  disconnect() {
    super.disconnect();
    const idx = nodeObservers.indexOf(this);
    if (idx !== -1) nodeObservers.splice(idx, 1);
    this[_nodeTypes] = [];
    this[_nodeBuffer] = [];
  }
}

// Internal helper used by node:http2 and other modules to dispatch
// Node-only PerformanceObserver entries (e.g. `Http2Session`) that the web
// PerformanceObserver does not understand.
function enqueueNodePerformanceEntry(entry) {
  for (let i = 0; i < nodeObservers.length; i++) {
    const obs = nodeObservers[i];
    if (!obs[_nodeTypes].includes(entry.entryType)) continue;
    obs[_nodeBuffer].push(entry);
    if (obs[_nodeScheduled]) continue;
    obs[_nodeScheduled] = true;
    queueMicrotask(() => {
      obs[_nodeScheduled] = false;
      const entries = obs[_nodeBuffer];
      obs[_nodeBuffer] = [];
      if (entries.length === 0) return;
      const list = createNodeEntryList(entries);
      try {
        obs[_nodeCallback](list, obs);
      } catch (_e) {
        // Match web observer: callback errors should not crash dispatch.
      }
    });
  }
}

const eventLoopUtilization = () => {
  // TODO(@marvinhagemeister): Return actual non-stubbed values
  return { idle: 0, active: 0, utilization: 0 };
};

performance.eventLoopUtilization = eventLoopUtilization;

performance.nodeTiming = {};

const timerify = (fn, options = {}) => {
  if (typeof fn !== "function") {
    throw new ERR_INVALID_ARG_TYPE("fn", "function", fn);
  }

  if (
    options !== undefined && (typeof options !== "object" || options === null)
  ) {
    throw new ERR_INVALID_ARG_TYPE("options", "Object", options);
  }

  if (options?.histogram !== undefined) {
    if (
      typeof options.histogram !== "object" ||
      options.histogram === null ||
      typeof options.histogram.record !== "function"
    ) {
      throw new ERR_INVALID_ARG_TYPE(
        "options.histogram",
        "RecordableHistogram",
        options.histogram,
      );
    }
  }

  function timerified(...args) {
    // TODO(bartlomieju): emit PerformanceEntry with entryType 'function'
    return new.target ? new fn(...args) : fn.apply(this, args);
  }

  Object.defineProperty(timerified, "name", {
    value: `timerified ${fn.name}`,
    configurable: true,
  });
  Object.defineProperty(timerified, "length", {
    value: fn.length,
    configurable: true,
  });

  return timerified;
};

performance.timerify = timerify;
// TODO(bartlomieju):
performance.markResourceTiming = () => {};

function monitorEventLoopDelay(options = {}) {
  const { resolution = 10 } = options;

  return new EldHistogram(resolution);
}

return {
  default: {
    performance,
    PerformanceObserver,
    PerformanceObserverEntryList,
    PerformanceEntry,
    monitorEventLoopDelay,
    eventLoopUtilization,
    timerify,
    constants,
  },
  constants,
  enqueueNodePerformanceEntry,
  eventLoopUtilization,
  monitorEventLoopDelay,
  performance,
  PerformanceEntry,
  PerformanceObserver,
  PerformanceObserverEntryList,
  timerify,
};
})();
```

<a id="ref-q1-2"></a>
### [2] `tests/unit_node/perf_hooks_test.ts:1-138`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L1-L138)

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/perf_hooks.js:8-13`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L8-L13)

```javascript
const {
  performance,
  PerformanceEntry,
  PerformanceObserver: WebPerformanceObserver,
  PerformanceObserverEntryList,
} = core.loadExtScript("ext:deno_web/15_performance.js");
```

<a id="ref-q1-4"></a>
### [4] `ext/web/15_performance.js:578-581`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L578-L581)

```javascript
  get timeOrigin() {
    webidl.assertBranded(this, PerformancePrototype);
    return timeOrigin;
  }
```

<a id="ref-q1-5"></a>
### [5] `ext/web/15_performance.js:583-623`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L583-L623)

```javascript
  clearMarks(markName = undefined) {
    webidl.assertBranded(this, PerformancePrototype);
    if (markName !== undefined) {
      markName = webidl.converters.DOMString(
        markName,
        "Failed to execute 'clearMarks' on 'Performance'",
        "Argument 1",
      );

      performanceEntries = ArrayPrototypeFilter(
        performanceEntries,
        (entry) => !(entry.name === markName && entry.entryType === "mark"),
      );
    } else {
      performanceEntries = ArrayPrototypeFilter(
        performanceEntries,
        (entry) => entry.entryType !== "mark",
      );
    }
  }

  clearMeasures(measureName = undefined) {
    webidl.assertBranded(this, PerformancePrototype);
    if (measureName !== undefined) {
      measureName = webidl.converters.DOMString(
        measureName,
        "Failed to execute 'clearMeasures' on 'Performance'",
        "Argument 1",
      );

      performanceEntries = ArrayPrototypeFilter(
        performanceEntries,
        (entry) =>
          !(entry.name === measureName && entry.entryType === "measure"),
      );
    } else {
      performanceEntries = ArrayPrototypeFilter(
        performanceEntries,
        (entry) => entry.entryType !== "measure",
      );
    }
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/perf_hooks.js:163`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L163)

```javascript
performance.nodeTiming = {};
```

<a id="ref-q1-7"></a>
### [7] `cli/tsc/dts/lib.deno.shared_globals.d.ts:805-806`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/lib.deno.shared_globals.d.ts#L805-L806)

```typescript
  /** Returns a JSON representation of the performance object. */
  toJSON(): any;
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/perf_hooks.js:59-60`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L59-L60)

```javascript
class PerformanceObserver extends WebPerformanceObserver {
  [_nodeTypes] = [];
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/perf_hooks.js:98-103`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L98-L103)

```javascript
    const webTypes = requestedTypes.filter(
      (t) => !NODE_ENTRY_TYPES.includes(t),
    );
    const nodeTypes = requestedTypes.filter(
      (t) => NODE_ENTRY_TYPES.includes(t),
    );
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/perf_hooks.js:105-119`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L105-L119)

```javascript
    if (webTypes.length > 0) {
      if (options.entryTypes !== undefined) {
        super.observe({ entryTypes: webTypes, buffered: options.buffered });
      } else if (webTypes.length === 1) {
        super.observe({ type: webTypes[0], buffered: options.buffered });
      }
    }

    if (nodeTypes.length > 0) {
      this[_nodeTypes] = nodeTypes;
      this[_nodeBuffer] = [];
      if (!nodeObservers.includes(this)) {
        nodeObservers.push(this);
      }
    }
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/perf_hooks.js:122-128`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L122-L128)

```javascript
  disconnect() {
    super.disconnect();
    const idx = nodeObservers.indexOf(this);
    if (idx !== -1) nodeObservers.splice(idx, 1);
    this[_nodeTypes] = [];
    this[_nodeBuffer] = [];
  }
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/perf_hooks.js:146-147`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L146-L147)

```javascript
      const list = createNodeEntryList(entries);
      try {
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/perf_hooks.js:14`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L14)

```javascript
const { EldHistogram } = core.ops;
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/perf_hooks.js:214`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L214)

```javascript
  return new EldHistogram(resolution);
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/perf_hooks.js:211-215`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L211-L215)

```javascript
function monitorEventLoopDelay(options = {}) {
  const { resolution = 10 } = options;

  return new EldHistogram(resolution);
}
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/perf_hooks.js:156-159`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L156-L159)

```javascript
const eventLoopUtilization = () => {
  // TODO(@marvinhagemeister): Return actual non-stubbed values
  return { idle: 0, active: 0, utilization: 0 };
};
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/perf_hooks.js:165-204`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L165-L204)

```javascript
const timerify = (fn, options = {}) => {
  if (typeof fn !== "function") {
    throw new ERR_INVALID_ARG_TYPE("fn", "function", fn);
  }

  if (
    options !== undefined && (typeof options !== "object" || options === null)
  ) {
    throw new ERR_INVALID_ARG_TYPE("options", "Object", options);
  }

  if (options?.histogram !== undefined) {
    if (
      typeof options.histogram !== "object" ||
      options.histogram === null ||
      typeof options.histogram.record !== "function"
    ) {
      throw new ERR_INVALID_ARG_TYPE(
        "options.histogram",
        "RecordableHistogram",
        options.histogram,
      );
    }
  }

  function timerified(...args) {
    // TODO(bartlomieju): emit PerformanceEntry with entryType 'function'
    return new.target ? new fn(...args) : fn.apply(this, args);
  }

  Object.defineProperty(timerified, "name", {
    value: `timerified ${fn.name}`,
    configurable: true,
  });
  Object.defineProperty(timerified, "length", {
    value: fn.length,
    configurable: true,
  });

  return timerified;
```

<a id="ref-q1-18"></a>
### [18] `tests/unit_node/perf_hooks_test.ts:14-23`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L14-L23)

```typescript
Deno.test({
  name: "[perf_hooks] performance.timeOrigin",
  fn() {
    assertEquals(typeof performance.timeOrigin, "number");
    assertThrows(() => {
      // @ts-expect-error: Cannot assign to 'timeOrigin' because it is a read-only property
      performance.timeOrigin = 1;
    });
  },
});
```

<a id="ref-q1-19"></a>
### [19] `tests/unit_node/perf_hooks_test.ts:25-30`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L25-L30)

```typescript
Deno.test("[perf_hooks]: eventLoopUtilization", () => {
  const obj = performance.eventLoopUtilization();
  assertEquals(typeof obj.idle, "number");
  assertEquals(typeof obj.active, "number");
  assertEquals(typeof obj.utilization, "number");
});
```

<a id="ref-q1-20"></a>
### [20] `tests/unit_node/perf_hooks_test.ts:32-44`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L32-L44)

```typescript
Deno.test("[perf_hooks]: monitorEventLoopDelay", async () => {
  const e = monitorEventLoopDelay();
  assertEquals(e.count, 0);
  e.enable();

  await new Promise((resolve) => setTimeout(resolve, 100));

  assert(e.min > 0);
  assert(e.minBigInt > 0n);
  assert(e.count > 0);

  e.disable();
});
```

<a id="ref-q1-21"></a>
### [21] `tests/unit_node/perf_hooks_test.ts:50-54`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L50-L54)

```typescript
Deno.test("[perf_hooks]: PerformanceObserver.supportedEntryTypes", () => {
  const supported = PerformanceObserver.supportedEntryTypes;
  assert(Array.isArray(supported));
  assert(supported.includes("mark"));
  assert(supported.includes("measure"));
```

<a id="ref-q1-22"></a>
### [22] `tests/unit_node/perf_hooks_test.ts:57-77`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L57-L77)

```typescript
Deno.test("[perf_hooks]: PerformanceObserver observes marks", async () => {
  const entries: PerformanceEntry[] = [];
  const observer = new PerformanceObserver((list) => {
    entries.push(...list.getEntries());
  });
  observer.observe({ entryTypes: ["mark"] });

  performance.mark("test-mark-1");
  performance.mark("test-mark-2");

  // Wait for microtask queue to flush
  await new Promise((resolve) => setTimeout(resolve, 10));

  assertEquals(entries.length, 2);
  assertEquals(entries[0].name, "test-mark-1");
  assertEquals(entries[1].name, "test-mark-2");
  assertEquals(entries[0].entryType, "mark");

  observer.disconnect();
  performance.clearMarks();
});
```

<a id="ref-q1-23"></a>
### [23] `tests/unit_node/perf_hooks_test.ts:79-98`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L79-L98)

```typescript
Deno.test("[perf_hooks]: PerformanceObserver observes measures", async () => {
  const entries: PerformanceEntry[] = [];
  const observer = new PerformanceObserver((list) => {
    entries.push(...list.getEntries());
  });
  observer.observe({ entryTypes: ["measure"] });

  performance.mark("start");
  performance.measure("test-measure", "start");

  await new Promise((resolve) => setTimeout(resolve, 10));

  assertEquals(entries.length, 1);
  assertEquals(entries[0].name, "test-measure");
  assertEquals(entries[0].entryType, "measure");

  observer.disconnect();
  performance.clearMarks();
  performance.clearMeasures();
});
```

<a id="ref-q1-24"></a>
### [24] `tests/unit_node/perf_hooks_test.ts:100-119`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L100-L119)

```typescript
Deno.test("[perf_hooks]: PerformanceObserver disconnect stops observation", async () => {
  const entries: PerformanceEntry[] = [];
  const observer = new PerformanceObserver((list) => {
    entries.push(...list.getEntries());
  });
  observer.observe({ entryTypes: ["mark"] });

  performance.mark("before-disconnect");
  await new Promise((resolve) => setTimeout(resolve, 10));

  observer.disconnect();

  performance.mark("after-disconnect");
  await new Promise((resolve) => setTimeout(resolve, 10));

  assertEquals(entries.length, 1);
  assertEquals(entries[0].name, "before-disconnect");

  performance.clearMarks();
});
```

<a id="ref-q1-25"></a>
### [25] `tests/unit_node/perf_hooks_test.ts:121-137`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/perf_hooks_test.ts#L121-L137)

```typescript
Deno.test("[perf_hooks]: PerformanceObserver takeRecords", () => {
  const observer = new PerformanceObserver(() => {});
  observer.observe({ entryTypes: ["mark"] });

  performance.mark("take-records-test");

  const records = observer.takeRecords();
  assertEquals(records.length, 1);
  assertEquals(records[0].name, "take-records-test");

  // After takeRecords, buffer should be empty
  const secondRecords = observer.takeRecords();
  assertEquals(secondRecords.length, 0);

  observer.disconnect();
  performance.clearMarks();
});
```

<a id="ref-q1-26"></a>
### [26] `tests/unit_node/timers_test.ts:9-20`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/timers_test.ts#L9-L20)

```typescript
Deno.test("[node/perf_hooks] performance.timerify()", () => {
  function sayHello() {
    return "hello world";
  }

  const wrapped = performance.timerify(sayHello);
  const result = wrapped();

  if (result !== "hello world") {
    throw new Error(`Expected "hello world", got "${result}"`);
  }
});
```

<a id="ref-q1-27"></a>
### [27] `tools/core_import_map.json:746`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/core_import_map.json#L746)

```json
    "node:perf_hooks": "../ext/node/polyfills/perf_hooks.ts",
```

<a id="ref-q1-28"></a>
### [28] `tests/node_compat/config.jsonc:2580-2585`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2580-L2585)

```
    "parallel/test-perf-gc-crash.js": {},
    "parallel/test-performance-global.js": {},
    "parallel/test-performance-many-marks.js": {},
    "parallel/test-performance-measure.js": {},
    "parallel/test-performanceobserver-gc.js": {},
    "parallel/test-performanceobserver.js": {},
```

<a id="ref-q1-29"></a>
### [29] `tests/node_compat/config.jsonc:1-136`
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

<a id="ref-q1-30"></a>
### [30] `tests/unit/performance_test.ts:1-224`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/performance_test.ts#L1-L224)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.
import {
  assert,
  assertEquals,
  assertNotEquals,
  assertNotStrictEquals,
  assertStringIncludes,
  assertThrows,
} from "./test_util.ts";

Deno.test({ permissions: {} }, async function performanceNow() {
  const { promise, resolve } = Promise.withResolvers<void>();
  const start = performance.now();
  let totalTime = 0;
  setTimeout(() => {
    const end = performance.now();
    totalTime = end - start;
    resolve();
  }, 10);
  await promise;
  assert(totalTime >= 10);
});

Deno.test(function timeOrigin() {
  const origin = performance.timeOrigin;

  assert(origin > 0);
  assert(Date.now() >= origin);
});

Deno.test(function performanceToJSON() {
  const json = performance.toJSON();

  assert("timeOrigin" in json);
  assert(json.timeOrigin === performance.timeOrigin);
  // check there are no other keys
  assertEquals(Object.keys(json).length, 1);
});

Deno.test(function clearMarks() {
  performance.mark("a");
  performance.mark("a");
  performance.mark("b");
  performance.mark("c");

  const marksNum = performance.getEntriesByType("mark").length;

  performance.clearMarks("a");
  assertEquals(performance.getEntriesByType("mark").length, marksNum - 2);

  performance.clearMarks();
  assertEquals(performance.getEntriesByType("mark").length, 0);
});

Deno.test(function clearMeasures() {
  performance.measure("from-start");
  performance.mark("a");
  performance.measure("from-mark-a", "a");
  performance.measure("from-start");
  performance.measure("from-mark-a", "a");
  performance.mark("b");
  performance.measure("between-a-and-b", "a", "b");

  const measuresNum = performance.getEntriesByType("measure").length;

  performance.clearMeasures("from-start");
  assertEquals(performance.getEntriesByType("measure").length, measuresNum - 2);

  performance.clearMeasures();
  assertEquals(performance.getEntriesByType("measure").length, 0);

  performance.clearMarks();
});

Deno.test(function clearResourceTimings() {
  // clearResourceTimings should exist and not throw
  // Since Deno doesn't currently track resource timings, this is effectively a no-op
  performance.clearResourceTimings();
  // After clearing, there should be no resource entries
  assertEquals(performance.getEntriesByType("resource").length, 0);
});

Deno.test(function setResourceTimingBufferSize() {
  // setResourceTimingBufferSize should exist and not throw
  // Since Deno doesn't currently track resource timings, this is effectively a no-op
  performance.setResourceTimingBufferSize(100);
  performance.setResourceTimingBufferSize(0);
  // Verify it requires an argument
  assertThrows(
    () => {
      // @ts-expect-error: testing missing argument
      performance.setResourceTimingBufferSize();
    },
    TypeError,
  );
});

Deno.test(function performanceMark() {
  const mark = performance.mark("test");
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, null);
  assertEquals(mark.name, "test");
  assertEquals(mark.entryType, "mark");
  assert(mark.startTime > 0);
  assertEquals(mark.duration, 0);
  const entries = performance.getEntries();
  assert(entries[entries.length - 1] === mark);
  const markEntries = performance.getEntriesByName("test", "mark");
  assert(markEntries[markEntries.length - 1] === mark);
});

Deno.test(function performanceMarkDetail() {
  const detail = { foo: "foo" };
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, { foo: "foo" });
  assertNotStrictEquals(mark.detail, detail);
});

Deno.test(function performanceMarkDetailArrayBuffer() {
  const detail = new ArrayBuffer(10);
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, new ArrayBuffer(10));
  assertNotStrictEquals(mark.detail, detail);
});

Deno.test(function performanceMarkDetailSubTypedArray() {
  class SubUint8Array extends Uint8Array {}
  const detail = new SubUint8Array([1, 2]);
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, new Uint8Array([1, 2]));
  assertNotStrictEquals(mark.detail, detail);
});

Deno.test(function performanceMeasure() {
  const markName1 = "mark1";
  const measureName1 = "measure1";
  const measureName2 = "measure2";
  const mark1 = performance.mark(markName1);
  // Measure against the inaccurate-but-known-good wall clock
  const now = new Date().valueOf();
  return new Promise<void>((resolve, reject) => {
    setTimeout(() => {
      try {
        const later = new Date().valueOf();
        const measure1 = performance.measure(measureName1, markName1);
        const measure2 = performance.measure(
          measureName2,
          undefined,
          markName1,
        );
        assert(measure1 instanceof PerformanceMeasure);
        assertEquals(measure1.detail, null);
        assertEquals(measure1.name, measureName1);
        assertEquals(measure1.entryType, "measure");
        assert(measure1.startTime > 0);
        assertEquals(measure2.startTime, 0);
        assertEquals(mark1.startTime, measure1.startTime);
        assertEquals(mark1.startTime, measure2.duration);
        assert(
          measure1.duration >= 100,
          `duration below 100ms: ${measure1.duration}`,
        );
        assert(
          measure1.duration < (later - now) * 1.50,
          `duration exceeds 150% of wallclock time: ${measure1.duration}ms vs ${
            later - now
          }ms`,
        );
        const entries = performance.getEntries();
        assert(entries[entries.length - 1] === measure2);
        const entriesByName = performance.getEntriesByName(
          measureName1,
          "measure",
        );
        assert(entriesByName[entriesByName.length - 1] === measure1);
        const measureEntries = performance.getEntriesByType("measure");
        assert(measureEntries[measureEntries.length - 1] === measure2);
      } catch (e) {
        return reject(e);
      }
      resolve();
    }, 100);
  });
});

Deno.test(function performanceMeasureUseMostRecentMark() {
  const markName1 = "mark1";
  const measureName1 = "measure1";
  const mark1 = performance.mark(markName1);
  return new Promise<void>((resolve, reject) => {
    setTimeout(() => {
      try {
        const laterMark1 = performance.mark(markName1);
        const measure1 = performance.measure(measureName1, markName1);
        assertNotEquals(mark1.startTime, measure1.startTime);
        assertEquals(laterMark1.startTime, measure1.startTime);
      } catch (e) {
        return reject(e);
      }
      resolve();
    }, 100);
  });
});

Deno.test(function performanceCustomInspectFunction() {
  assertStringIncludes(Deno.inspect(performance), "Performance");
  assertStringIncludes(
    Deno.inspect(Performance.prototype),
    "Performance",
  );
});

Deno.test(function performanceMarkCustomInspectFunction() {
  const mark1 = performance.mark("mark1");
  assertStringIncludes(Deno.inspect(mark1), "PerformanceMark");
  assertStringIncludes(
    Deno.inspect(PerformanceMark.prototype),
    "PerformanceMark",
  );
});
```
