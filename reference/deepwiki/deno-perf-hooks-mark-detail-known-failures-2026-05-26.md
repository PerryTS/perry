# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests establish perf_hooks performance.mark detail option behavior: accepting structured-cloneable circular detail values and throwing DataCloneError or equivalent for Function detail values? Focus only on public source/tests useful as reference material for revalidating stale PerryTS/perry node-suite/perf_hooks/mark/detail-circular and mark/detail-function-throws known-failure entries.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_5699bd7d-d6ff-43f1-b08e-fea62704be65

**Question**: For Node compatibility in Deno, what source files or tests establish perf_hooks performance.mark detail option behavior: accepting structured-cloneable circular detail values and throwing DataCloneError or equivalent for Function detail values? Focus only on public source/tests useful as reference material for revalidating stale PerryTS/perry node-suite/perf_hooks/mark/detail-circular and mark/detail-function-throws known-failure entries.

The behavior of `performance.mark` regarding its `detail` option, specifically accepting structured-cloneable circular values and throwing `DataCloneError` for `Function` values, is primarily established in the `ext/web/15_performance.js` source file and tested in `tests/unit/performance_test.ts`. [1](#ref-q1-1) [2](#ref-q1-2)

The `PerformanceMark` class constructor in `ext/web/15_performance.js` handles the `detail` option. [3](#ref-q1-3)

The `detail` property of a `PerformanceMark` object is set using `getStructuredClone()(detail)`. [1](#ref-q1-1)  This indicates that the `detail` value undergoes a structured cloning process. The structured cloning algorithm inherently supports circular references for structured-cloneable types and throws a `DataCloneError` for non-cloneable types like `Function` objects.

The `tests/unit/performance_test.ts` file contains tests that verify this behavior. [2](#ref-q1-2)
Specifically, the `performanceMarkDetail` test confirms that a structured-cloneable object (like `{ foo: "foo" }`) is accepted and cloned, as `assertNotStrictEquals(mark.detail, detail)` passes, indicating a deep copy. [2](#ref-q1-2)
Similarly, `performanceMarkDetailArrayBuffer` and `performanceMarkDetailSubTypedArray` tests confirm the cloning of `ArrayBuffer` and `Uint8Array` subclasses respectively. [4](#ref-q1-4) [5](#ref-q1-5)

While there isn't a direct test in the provided snippets explicitly asserting `DataCloneError` for `Function` values in `performance.mark`, the reliance on `getStructuredClone()` implies this behavior, as structured cloning specifications dictate this error for non-cloneable types. The `tests/unit/structured_clone_test.ts` file provides general tests for the `structuredClone` function, including cases where `DataCloneError` is thrown for non-transferable or detached objects, which supports the understanding of `getStructuredClone()`'s error handling. [6](#ref-q1-6)

## Notes
The `ext/node/polyfills/perf_hooks.js` file imports the `performance` object from `ext:deno_web/15_performance.js`, indicating that the Node.js `perf_hooks` module re-exports the web platform's `Performance` API. [7](#ref-q1-7)  Therefore, the behavior defined in `ext/web/15_performance.js` directly applies to `node:perf_hooks`. [7](#ref-q1-7)

Wiki pages you might want to explore:
- [HTTP, Fetch, and WebSocket (denoland/deno)](/wiki/denoland/deno#5)
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_5699bd7d-d6ff-43f1-b08e-fea62704be65

## References

<a id="ref-q1-1"></a>
### [1] `ext/web/15_performance.js:297`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L297)

```javascript
    this[_detail] = getStructuredClone()(detail);
```

<a id="ref-q1-2"></a>
### [2] `tests/unit/performance_test.ts:112-117`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/performance_test.ts#L112-L117)

```typescript
Deno.test(function performanceMarkDetail() {
  const detail = { foo: "foo" };
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, { foo: "foo" });
  assertNotStrictEquals(mark.detail, detail);
```

<a id="ref-q1-3"></a>
### [3] `ext/web/15_performance.js:273-298`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/15_performance.js#L273-L298)

```javascript
  constructor(
    name,
    options = { __proto__: null },
  ) {
    const prefix = "Failed to construct 'PerformanceMark'";
    webidl.requiredArguments(arguments.length, 1, prefix);

    name = webidl.converters.DOMString(name, prefix, "Argument 1");

    options = webidl.converters.PerformanceMarkOptions(
      options,
      prefix,
      "Argument 2",
    );

    const { detail = null, startTime = now() } = options;

    super(name, "mark", startTime, 0, illegalConstructorKey);
    this[webidl.brand] = webidl.brand;
    if (startTime < 0) {
      throw new TypeError(
        `Cannot construct PerformanceMark: startTime cannot be negative, received ${startTime}`,
      );
    }
    this[_detail] = getStructuredClone()(detail);
  }
```

<a id="ref-q1-4"></a>
### [4] `tests/unit/performance_test.ts:120-125`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/performance_test.ts#L120-L125)

```typescript
Deno.test(function performanceMarkDetailArrayBuffer() {
  const detail = new ArrayBuffer(10);
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, new ArrayBuffer(10));
  assertNotStrictEquals(mark.detail, detail);
```

<a id="ref-q1-5"></a>
### [5] `tests/unit/performance_test.ts:128-135`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/performance_test.ts#L128-L135)

```typescript
Deno.test(function performanceMarkDetailSubTypedArray() {
  class SubUint8Array extends Uint8Array {}
  const detail = new SubUint8Array([1, 2]);
  const mark = performance.mark("test", { detail });
  assert(mark instanceof PerformanceMark);
  assertEquals(mark.detail, new Uint8Array([1, 2]));
  assertNotStrictEquals(mark.detail, detail);
});
```

<a id="ref-q1-6"></a>
### [6] `tests/unit/structured_clone_test.ts:26-37`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/structured_clone_test.ts#L26-L37)

```typescript
Deno.test("correct DataCloneError message", () => {
  assertThrows(
    () => {
      const sab = new SharedArrayBuffer(1024);
      structuredClone(sab, {
        // @ts-expect-error cannot assign SharedArrayBuffer because it's not tranferable
        transfer: [sab],
      });
    },
    DOMException,
    "Value not transferable",
  );
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/perf_hooks.js:9-13`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/perf_hooks.js#L9-L13)

```javascript
  performance,
  PerformanceEntry,
  PerformanceObserver: WebPerformanceObserver,
  PerformanceObserverEntryList,
} = core.loadExtScript("ext:deno_web/15_performance.js");
```
