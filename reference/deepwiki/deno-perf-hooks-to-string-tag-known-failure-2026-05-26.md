# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests establish the perf_hooks performance object string tag behavior, specifically Object.prototype.toString.call(performance) producing '[object Performance]' through Symbol.toStringTag or equivalent Web Performance API wiring? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry node-suite/perf_hooks/shapes/to-string-tag known-failure entry.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_cddb1758-ebe3-4f27-a87d-2883a503822c

**Question**: For Node compatibility in Deno, what source files or tests establish the perf_hooks performance object string tag behavior, specifically Object.prototype.toString.call(performance) producing '[object Performance]' through Symbol.toStringTag or equivalent Web Performance API wiring? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry node-suite/perf_hooks/shapes/to-string-tag known-failure entry.

The `perf_hooks` `performance` object's `Symbol.toStringTag` behavior, which results in `Object.prototype.toString.call(performance)` producing `'[object Performance]'`, is established in Deno's Web Performance API implementation. The core logic for the `Performance` class, including its `Symbol.for("Deno.privateCustomInspect")` method, is defined in [1](#ref-q1-1) .

Specifically, the `Performance` class in [1](#ref-q1-1)  has a `[SymbolFor("Deno.privateCustomInspect")]` method [2](#ref-q1-2) . This method is used for custom inspection of the `performance` object, and it returns a string that includes "Performance" [3](#ref-q1-3) . While this is not directly `Symbol.toStringTag`, it serves a similar purpose for how the object is represented in inspection.

The `performance` object itself is exposed through the `ext:deno_web/15_performance.js` module [4](#ref-q1-4) . This module is then loaded by the Node.js compatibility layer's `perf_hooks` polyfill [5](#ref-q1-5) . The `performance` object from the Web API is re-exported as part of the Node.js `perf_hooks` module [6](#ref-q1-6) .

The `Performance` interface definition in Deno's shared globals also indicates that `Performance` is an `EventTarget` and defines its methods like `timeOrigin`, `clearMarks`, `measure`, and `now` [7](#ref-q1-7) .

## Notes

The prompt specifically asks about `Symbol.toStringTag` behavior. While the `Performance` class in Deno's Web API implementation does not explicitly define `Symbol.toStringTag` in the provided snippets, it does implement `Symbol.for("Deno.privateCustomInspect")` [2](#ref-q1-2) . This custom inspect function is used by `Deno.inspect()` to provide a string representation that includes "Performance" [3](#ref-q1-3) . This is the closest equivalent behavior found in the provided context for controlling the string tag of the `performance` object. The `tests/unit/console_test.ts` file also shows how `Symbol.toStringTag` is handled for other objects during stringification [8](#ref-q1-8) .

Wiki pages you might want to explore:
- [HTTP, Fetch, and WebSocket (denoland/deno)](/wiki/denoland/deno#5)
- [Storage, Cache, and Web APIs (denoland/deno)](/wiki/denoland/deno#6.4)
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_cddb1758-ebe3-4f27-a87d-2883a503822c

## References

<a id="ref-q1-1"></a>
### [1] `ext/web/15_performance.js:568-818`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L568-L818)

```javascript
class Performance extends EventTarget {
  constructor(key = null) {
    if (key != illegalConstructorKey) {
      webidl.illegalConstructor();
    }

    super();
    this[webidl.brand] = webidl.brand;
  }

  get timeOrigin() {
    webidl.assertBranded(this, PerformancePrototype);
    return timeOrigin;
  }

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
  }

  clearResourceTimings() {
    webidl.assertBranded(this, PerformancePrototype);
    performanceEntries = ArrayPrototypeFilter(
      performanceEntries,
      (entry) => entry.entryType !== "resource",
    );
  }

  setResourceTimingBufferSize(_maxSize) {
    webidl.assertBranded(this, PerformancePrototype);
    webidl.requiredArguments(
      arguments.length,
      1,
      "Failed to execute 'setResourceTimingBufferSize' on 'Performance'",
    );
    // This is a noop in Deno as we don't have resource timing entries
  }

  getEntries() {
    webidl.assertBranded(this, PerformancePrototype);
    return filterByNameType();
  }

  getEntriesByName(
    name,
    type = undefined,
  ) {
    webidl.assertBranded(this, PerformancePrototype);
    const prefix = "Failed to execute 'getEntriesByName' on 'Performance'";
    webidl.requiredArguments(arguments.length, 1, prefix);

    name = webidl.converters.DOMString(name, prefix, "Argument 1");

    if (type !== undefined) {
      type = webidl.converters.DOMString(type, prefix, "Argument 2");
    }

    return filterByNameType(name, type);
  }

  getEntriesByType(type) {
    webidl.assertBranded(this, PerformancePrototype);
    const prefix = "Failed to execute 'getEntriesByName' on 'Performance'";
    webidl.requiredArguments(arguments.length, 1, prefix);

    type = webidl.converters.DOMString(type, prefix, "Argument 1");

    return filterByNameType(undefined, type);
  }

  mark(
    markName,
    markOptions = { __proto__: null },
  ) {
    webidl.assertBranded(this, PerformancePrototype);
    const prefix = "Failed to execute 'mark' on 'Performance'";
    webidl.requiredArguments(arguments.length, 1, prefix);

    markName = webidl.converters.DOMString(markName, prefix, "Argument 1");

    markOptions = webidl.converters.PerformanceMarkOptions(
      markOptions,
      prefix,
      "Argument 2",
    );

    // 3.1.1.1 If the global object is a Window object and markName uses the
    // same name as a read only attribute in the PerformanceTiming interface,
    // throw a SyntaxError. - not implemented
    const entry = new PerformanceMark(markName, markOptions);
    ArrayPrototypePush(performanceEntries, entry);
    queuePerformanceEntry(entry);
    return entry;
  }

  measure(
    measureName,
    startOrMeasureOptions = { __proto__: null },
    endMark = undefined,
  ) {
    webidl.assertBranded(this, PerformancePrototype);
    const prefix = "Failed to execute 'measure' on 'Performance'";
    webidl.requiredArguments(arguments.length, 1, prefix);

    measureName = webidl.converters.DOMString(
      measureName,
      prefix,
      "Argument 1",
    );

    startOrMeasureOptions = webidl.converters
      ["DOMString or PerformanceMeasureOptions"](
        startOrMeasureOptions,
        prefix,
        "Argument 2",
      );

    if (endMark !== undefined) {
      endMark = webidl.converters.DOMString(endMark, prefix, "Argument 3");
    }

    if (
      startOrMeasureOptions && typeof startOrMeasureOptions === "object" &&
      ObjectKeys(startOrMeasureOptions).length > 0
    ) {
      if (endMark) {
        throw new TypeError('Options cannot be passed with "endMark"');
      }
      if (
        ReflectHas(startOrMeasureOptions, "start") &&
        ReflectHas(startOrMeasureOptions, "duration") &&
        ReflectHas(startOrMeasureOptions, "end")
      ) {
        throw new TypeError(
          'Cannot specify "start", "end", and "duration" together in options',
        );
      }
    }
    let endTime;
    if (endMark) {
      endTime = convertMarkToTimestamp(endMark);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      ReflectHas(startOrMeasureOptions, "end")
    ) {
      endTime = convertMarkToTimestamp(startOrMeasureOptions.end);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      ReflectHas(startOrMeasureOptions, "start") &&
      ReflectHas(startOrMeasureOptions, "duration")
    ) {
      const start = convertMarkToTimestamp(startOrMeasureOptions.start);
      const duration = convertMarkToTimestamp(startOrMeasureOptions.duration);
      endTime = start + duration;
    } else {
      endTime = now();
    }
    let startTime;
    if (
      typeof startOrMeasureOptions === "object" &&
      ReflectHas(startOrMeasureOptions, "start")
    ) {
      startTime = convertMarkToTimestamp(startOrMeasureOptions.start);
    } else if (
      typeof startOrMeasureOptions === "object" &&
      ReflectHas(startOrMeasureOptions, "end") &&
      ReflectHas(startOrMeasureOptions, "duration")
    ) {
      const end = convertMarkToTimestamp(startOrMeasureOptions.end);
      const duration = convertMarkToTimestamp(startOrMeasureOptions.duration);
      startTime = end - duration;
    } else if (typeof startOrMeasureOptions === "string") {
      startTime = convertMarkToTimestamp(startOrMeasureOptions);
    } else {
      startTime = 0;
    }
    const entry = new PerformanceMeasure(
      measureName,
      startTime,
      endTime - startTime,
      typeof startOrMeasureOptions === "object"
        ? startOrMeasureOptions.detail ?? null
        : null,
      illegalConstructorKey,
    );
    ArrayPrototypePush(performanceEntries, entry);
    queuePerformanceEntry(entry);
    return entry;
  }

  now() {
    webidl.assertBranded(this, PerformancePrototype);
    return now();
  }

  toJSON() {
    webidl.assertBranded(this, PerformancePrototype);
    return {
      timeOrigin: this.timeOrigin,
    };
  }

  [SymbolFor("Deno.privateCustomInspect")](inspect, inspectOptions) {
    return inspect(
      getCreateFilteredInspectProxy()({
        object: this,
        evaluate: ObjectPrototypeIsPrototypeOf(PerformancePrototype, this),
        keys: ["timeOrigin"],
      }),
      inspectOptions,
    );
  }
}
```

<a id="ref-q1-2"></a>
### [2] `ext/web/15_performance.js:808-817`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L808-L817)

```javascript
  [SymbolFor("Deno.privateCustomInspect")](inspect, inspectOptions) {
    return inspect(
      getCreateFilteredInspectProxy()({
        object: this,
        evaluate: ObjectPrototypeIsPrototypeOf(PerformancePrototype, this),
        keys: ["timeOrigin"],
      }),
      inspectOptions,
    );
  }
```

<a id="ref-q1-3"></a>
### [3] `tests/unit/performance_test.ts:209`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/performance_test.ts#L209)

```typescript
  assertStringIncludes(Deno.inspect(performance), "Performance");
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/perf_hooks.js:9`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L9)

```javascript
  performance,
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/perf_hooks.js:1-160`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L1-L160)

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
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/perf_hooks.js:219`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L219)

```javascript
    performance,
```

<a id="ref-q1-7"></a>
### [7] `cli/tsc/dts/lib.deno.shared_globals.d.ts:748-807`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/lib.deno.shared_globals.d.ts#L748-L807)

```typescript
interface Performance extends EventTarget {
  /** Returns a timestamp representing the start of the performance measurement. */
  readonly timeOrigin: number;

  /** Removes the stored timestamp with the associated name. */
  clearMarks(markName?: string): void;

  /** Removes stored timestamp with the associated name. */
  clearMeasures(measureName?: string): void;

  /** Removes all performance entries with an entryType of "resource" from the
   * performance timeline and sets the size of the performance resource data
   * buffer to zero.
   *
   * Note: Deno does not currently track resource timings, so this method has
   * no observable effect. It is provided for API compatibility.
   */
  clearResourceTimings(): void;

  /** Sets the desired size of the browser's resource timing buffer which
   * stores the "resource" performance entries.
   *
   * Note: Deno does not currently track resource timings, so this method has
   * no observable effect. It is provided for API compatibility.
   */
  setResourceTimingBufferSize(maxSize: number): void;

  getEntries(): PerformanceEntryList;
  getEntriesByName(name: string, type?: string): PerformanceEntryList;
  getEntriesByType(type: string): PerformanceEntryList;

  /** Stores a timestamp with the associated name (a "mark"). */
  mark(markName: string, options?: PerformanceMarkOptions): PerformanceMark;

  /** Stores the `DOMHighResTimeStamp` duration between two marks along with the
   * associated name (a "measure"). */
  measure(
    measureName: string,
    options?: PerformanceMeasureOptions,
  ): PerformanceMeasure;
  /** Stores the `DOMHighResTimeStamp` duration between two marks along with the
   * associated name (a "measure"). */
  measure(
    measureName: string,
    startMark?: string,
    endMark?: string,
  ): PerformanceMeasure;

  /** Returns a current time from Deno's start in fractional milliseconds.
   *
   * ```ts
   * const t = performance.now();
   * console.log(`${t} ms since start!`);
   * ```
   */
  now(): number;

  /** Returns a JSON representation of the performance object. */
  toJSON(): any;
}
```

<a id="ref-q1-8"></a>
### [8] `tests/unit/console_test.ts:369-374`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L369-L374)

```typescript
    stringify({ str: 1, [Symbol.for("sym")]: 2, [Symbol.toStringTag]: "TAG" }),
    `Object [TAG] {
  str: 1,
  Symbol(sym): 2,
  Symbol(Symbol.toStringTag): "TAG"
}`,
```
