# DeepWiki Response

Repository: `nodejs/node`
Question: For Node.js, describe the global WebAssembly namespace surface relevant to runtime compatibility: own property names, constructor/error class names, static method names, constructor/function typeof, name and length expectations where visible in source/tests/docs, and which files/tests document or verify this surface. Keep the answer focused on implementing feature-detection parity in another runtime.
Search: https://deepwiki.com/search/for-nodejs-describe-the-global_29cbf73e-483f-49b5-87ca-b578db601608

**Question**: For Node.js, describe the global WebAssembly namespace surface relevant to runtime compatibility: own property names, constructor/error class names, static method names, constructor/function typeof, name and length expectations where visible in source/tests/docs, and which files/tests document or verify this surface. Keep the answer focused on implementing feature-detection parity in another runtime.

The global `WebAssembly` namespace in Node.js exposes several constructors, error classes, and static methods relevant for runtime compatibility. These are primarily defined and verified within the V8 engine's mjsunit tests and Web Platform Tests (WPT) included in the Node.js codebase. [1](#ref-q1-1)  [2](#ref-q1-2)

### Global `WebAssembly` Object Properties

The `WebAssembly` object itself is a data property on the global object. [1](#ref-q1-1)  It is an object, not a function, and cannot be called or constructed. [3](#ref-q1-3)

*   **Type**: `object` [4](#ref-q1-4)
*   **String Representation**: `[object WebAssembly]` [5](#ref-q1-5)
*   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [6](#ref-q1-6)  [7](#ref-q1-7)

### Error Classes

The `WebAssembly` namespace includes three error constructors that inherit from `Error`. [8](#ref-q1-8)  [9](#ref-q1-9)  [10](#ref-q1-10)

*   **`WebAssembly.CompileError`** [11](#ref-q1-11)
    *   **Type**: `function` (constructor) [12](#ref-q1-12)
    *   **`name`**: `'CompileError'` [13](#ref-q1-13)
    *   **`length`**: `1` [14](#ref-q1-14)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [15](#ref-q1-15)
*   **`WebAssembly.LinkError`** [16](#ref-q1-16)
    *   **Type**: `function` (constructor) [17](#ref-q1-17)
    *   **`name`**: `'LinkError'` [18](#ref-q1-18)
    *   **`length`**: `1` [19](#ref-q1-19)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [20](#ref-q1-20)
*   **`WebAssembly.RuntimeError`** [21](#ref-q1-21)
    *   **Type**: `function` (constructor) [22](#ref-q1-22)
    *   **`name`**: `'RuntimeError'` [23](#ref-q1-23)
    *   **`length`**: `1` [24](#ref-q1-24)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [25](#ref-q1-25)

### Constructors

The `WebAssembly` object exposes several constructors for WebAssembly types. [26](#ref-q1-26)

*   **`WebAssembly.Module`** [27](#ref-q1-27)
    *   **Type**: `function` (constructor) [28](#ref-q1-28)
    *   **`name`**: `'Module'` [29](#ref-q1-29)
    *   **`length`**: `1` [30](#ref-q1-30)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [31](#ref-q1-31)
    *   **Static Methods**:
        *   `WebAssembly.Module.exports` [32](#ref-q1-32) : `typeof: function`, `length: 1` [33](#ref-q1-33)  [34](#ref-q1-34)
        *   `WebAssembly.Module.imports` [35](#ref-q1-35) : `typeof: function`, `length: 1` [36](#ref-q1-36)  [37](#ref-q1-37)
        *   `WebAssembly.Module.customSections` [38](#ref-q1-38) : `typeof: function`, `length: 2` [39](#ref-q1-39)  [40](#ref-q1-40)
        *   All static methods have property descriptors: `writable: true`, `enumerable: true`, `configurable: true` [41](#ref-q1-41)

*   **`WebAssembly.Instance`** [42](#ref-q1-42)
    *   **Type**: `function` (constructor) [43](#ref-q1-43)
    *   **`name`**: `'Instance'` [44](#ref-q1-44)
    *   **`length`**: `1` [45](#ref-q1-45)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [46](#ref-q1-46)

*   **`WebAssembly.Memory`** [47](#ref-q1-47)
    *   **Type**: `function` (constructor) [48](#ref-q1-48)
    *   **`name`**: `'Memory'` [49](#ref-q1-49)
    *   **`length`**: `1` [50](#ref-q1-50)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [51](#ref-q1-51)

*   **`WebAssembly.Table`** [52](#ref-q1-52)
    *   **Type**: `function` (constructor) [53](#ref-q1-53)
    *   **`name`**: `'Table'` [54](#ref-q1-54)
    *   **`length`**: `1` [55](#ref-q1-55)
    *   **Property Descriptors**: `writable: true`, `enumerable: false`, `configurable: true` [56](#ref-q1-56)

*   **`WebAssembly.Global`** [57](#ref-q1-57)
    *   Verified in tests but full property descriptor details not shown in provided snippets

### Static Methods on `WebAssembly`

*   **`WebAssembly.compile`** [58](#ref-q1-58)
    *   **Type**: `function` [59](#ref-q1-59)
    *   **`name`**: `'compile'` [60](#ref-q1-60)
    *   **`length`**: `1` [61](#ref-q1-61)
    *   **Property Descriptors**: `writable: true`, `enumerable: true`, `configurable: true` [62](#ref-q1-62)

*   **`WebAssembly.validate`** [63](#ref-q1-63)
    *   **`length`**: `1` [63](#ref-q1-63)

*   **`WebAssembly.instantiate`** [64](#ref-q1-64)
    *   **`length`**: `1` [64](#ref-q1-64)

### Prototype and Instance Properties

All constructors have a `prototype` property with standard descriptors: `writable: false`, `enumerable: false`, `configurable: false`. [65](#ref-q1-65)  Each prototype has a `constructor` property pointing back to the constructor: `writable: true`, `enumerable: false`, `configurable: true`. [66](#ref-q1-66)

Instance properties like `WebAssembly.Instance.prototype.exports` and `WebAssembly.Memory.prototype.buffer` are accessor properties (getters only). [67](#ref-q1-67)  [68](#ref-q1-68)

### Documentation and Verification Files

The primary sources for this surface specification are:

1. **V8 mjsunit tests**: [69](#ref-q1-69)  — Comprehensive tests of property descriptors, constructor behavior, method signatures, and error handling
2. **Web Platform Tests (WPT)**: [70](#ref-q1-70)  — Standardized interface tests covering property descriptors, function names, and lengths
3. **Intrinsics freezing**: [71](#ref-q1-71)  — Documents which WebAssembly prototypes are frozen in Node.js

## Notes

The WebAssembly namespace surface is stable and well-documented through V8's test suite and WPT fixtures. For feature-detection parity in another runtime, you should verify:

- The `WebAssembly` object exists and is of type `object`
- All eight interface names exist as own properties: `Module`, `Instance`, `Memory`, `Table`, `Global`, `CompileError`, `LinkError`, `RuntimeError`
- All three static methods exist: `compile`, `validate`, `instantiate`
- All static methods on `Module` exist: `exports`, `imports`, `customSections`
- Constructor `length` properties match expectations (typically `1` for constructors, `1-2` for static methods)
- Error classes inherit from `Error` and have `length: 1`
- Property descriptors follow the specified patterns (non-enumerable for constructors, enumerable for methods)

Additional WebAssembly features like `Tag`, `Exception`, and `Global` are also present in the codebase but were not fully detailed in the provided test snippets.

View this search on DeepWiki: https://deepwiki.com/search/for-nodejs-describe-the-global_29cbf73e-483f-49b5-87ca-b578db601608

## References

<a id="ref-q1-1"></a>
### [1] `deps/v8/test/mjsunit/wasm/js-api.js:66-70`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L66-L70)

```javascript
let wasmDesc = Object.getOwnPropertyDescriptor(this, 'WebAssembly');
assertEq(typeof wasmDesc.value, 'object');
assertTrue(wasmDesc.writable);
assertFalse(wasmDesc.enumerable);
assertTrue(wasmDesc.configurable);
```

<a id="ref-q1-2"></a>
### [2] `test/fixtures/wpt/wasm/jsapi/interface.any.js:57-63`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L57-L63)

```javascript
  const propdesc = Object.getOwnPropertyDescriptor(this, "WebAssembly");
  assert_equals(typeof propdesc, "object");
  assert_true(propdesc.writable, "writable");
  assert_false(propdesc.enumerable, "enumerable");
  assert_true(propdesc.configurable, "configurable");
  assert_equals(propdesc.value, this.WebAssembly);
}, "WebAssembly: property descriptor");
```

<a id="ref-q1-3"></a>
### [3] `test/fixtures/wpt/wasm/jsapi/interface.any.js:66-71`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L66-L71)

```javascript
  assert_throws_js(TypeError, () => WebAssembly());
}, "WebAssembly: calling");

test(() => {
  assert_throws_js(TypeError, () => new WebAssembly());
}, "WebAssembly: constructing");
```

<a id="ref-q1-4"></a>
### [4] `deps/v8/test/mjsunit/wasm/js-api.js:67`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L67)

```javascript
assertEq(typeof wasmDesc.value, 'object');
```

<a id="ref-q1-5"></a>
### [5] `deps/v8/test/mjsunit/wasm/js-api.js:74`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L74)

```javascript
assertEq(String(WebAssembly), '[object WebAssembly]');
```

<a id="ref-q1-6"></a>
### [6] `deps/v8/test/mjsunit/wasm/js-api.js:68-70`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L68-L70)

```javascript
assertTrue(wasmDesc.writable);
assertFalse(wasmDesc.enumerable);
assertTrue(wasmDesc.configurable);
```

<a id="ref-q1-7"></a>
### [7] `test/fixtures/wpt/wasm/jsapi/interface.any.js:59-61`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L59-L61)

```javascript
  assert_true(propdesc.writable, "writable");
  assert_false(propdesc.enumerable, "enumerable");
  assert_true(propdesc.configurable, "configurable");
```

<a id="ref-q1-8"></a>
### [8] `deps/v8/test/mjsunit/wasm/js-api.js:89-90`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L89-L90)

```javascript
assertTrue(compileError instanceof CompileError);
assertTrue(compileError instanceof Error);
```

<a id="ref-q1-9"></a>
### [9] `deps/v8/test/mjsunit/wasm/js-api.js:108-109`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L108-L109)

```javascript
assertTrue(runtimeError instanceof RuntimeError);
assertTrue(runtimeError instanceof Error);
```

<a id="ref-q1-10"></a>
### [10] `deps/v8/test/mjsunit/wasm/js-api.js:127-128`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L127-L128)

```javascript
assertTrue(linkError instanceof Error);
assertFalse(linkError instanceof TypeError);
```

<a id="ref-q1-11"></a>
### [11] `deps/v8/test/mjsunit/wasm/js-api.js:76`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L76)

```javascript
// 'WebAssembly.CompileError'
```

<a id="ref-q1-12"></a>
### [12] `deps/v8/test/mjsunit/wasm/js-api.js:79`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L79)

```javascript
assertEq(typeof compileErrorDesc.value, 'function');
```

<a id="ref-q1-13"></a>
### [13] `deps/v8/test/mjsunit/wasm/js-api.js:86`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L86)

```javascript
assertEq(CompileError.name, 'CompileError');
```

<a id="ref-q1-14"></a>
### [14] `deps/v8/test/mjsunit/wasm/js-api.js:85`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L85)

```javascript
assertEq(CompileError.length, 1);
```

<a id="ref-q1-15"></a>
### [15] `deps/v8/test/mjsunit/wasm/js-api.js:80-82`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L80-L82)

```javascript
assertTrue(compileErrorDesc.writable);
assertFalse(compileErrorDesc.enumerable);
assertTrue(compileErrorDesc.configurable);
```

<a id="ref-q1-16"></a>
### [16] `deps/v8/test/mjsunit/wasm/js-api.js:115`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L115)

```javascript
let linkErrorDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'LinkError');
```

<a id="ref-q1-17"></a>
### [17] `deps/v8/test/mjsunit/wasm/js-api.js:116`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L116)

```javascript
assertEq(typeof linkErrorDesc.value, 'function');
```

<a id="ref-q1-18"></a>
### [18] `deps/v8/test/mjsunit/wasm/js-api.js:123`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L123)

```javascript
assertEq(LinkError.name, 'LinkError');
```

<a id="ref-q1-19"></a>
### [19] `deps/v8/test/mjsunit/wasm/js-api.js:122`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L122)

```javascript
assertEq(LinkError.length, 1);
```

<a id="ref-q1-20"></a>
### [20] `deps/v8/test/mjsunit/wasm/js-api.js:117-119`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L117-L119)

```javascript
assertTrue(linkErrorDesc.writable);
assertFalse(linkErrorDesc.enumerable);
assertTrue(linkErrorDesc.configurable);
```

<a id="ref-q1-21"></a>
### [21] `deps/v8/test/mjsunit/wasm/js-api.js:95`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L95)

```javascript
// 'WebAssembly.RuntimeError'
```

<a id="ref-q1-22"></a>
### [22] `deps/v8/test/mjsunit/wasm/js-api.js:98`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L98)

```javascript
assertEq(typeof runtimeErrorDesc.value, 'function');
```

<a id="ref-q1-23"></a>
### [23] `deps/v8/test/mjsunit/wasm/js-api.js:105`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L105)

```javascript
assertEq(RuntimeError.name, 'RuntimeError');
```

<a id="ref-q1-24"></a>
### [24] `deps/v8/test/mjsunit/wasm/js-api.js:104`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L104)

```javascript
assertEq(RuntimeError.length, 1);
```

<a id="ref-q1-25"></a>
### [25] `deps/v8/test/mjsunit/wasm/js-api.js:99-101`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L99-L101)

```javascript
assertTrue(runtimeErrorDesc.writable);
assertFalse(runtimeErrorDesc.enumerable);
assertTrue(runtimeErrorDesc.configurable);
```

<a id="ref-q1-26"></a>
### [26] `test/fixtures/wpt/wasm/jsapi/interface.any.js:73-82`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L73-L82)

```javascript
const interfaces = [
  "Module",
  "Instance",
  "Memory",
  "Table",
  "Global",
  "CompileError",
  "LinkError",
  "RuntimeError",
];
```

<a id="ref-q1-27"></a>
### [27] `deps/v8/test/mjsunit/wasm/js-api.js:133`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L133)

```javascript
let moduleDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'Module');
```

<a id="ref-q1-28"></a>
### [28] `deps/v8/test/mjsunit/wasm/js-api.js:134`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L134)

```javascript
assertEq(typeof moduleDesc.value, 'function');
```

<a id="ref-q1-29"></a>
### [29] `deps/v8/test/mjsunit/wasm/js-api.js:143`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L143)

```javascript
assertEq(Module.name, 'Module');
```

<a id="ref-q1-30"></a>
### [30] `deps/v8/test/mjsunit/wasm/js-api.js:142`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L142)

```javascript
assertEq(Module.length, 1);
```

<a id="ref-q1-31"></a>
### [31] `deps/v8/test/mjsunit/wasm/js-api.js:135-137`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L135-L137)

```javascript
assertTrue(moduleDesc.writable);
assertFalse(moduleDesc.enumerable);
assertTrue(moduleDesc.configurable);
```

<a id="ref-q1-32"></a>
### [32] `deps/v8/test/mjsunit/wasm/js-api.js:237`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L237)

```javascript
let moduleExportsDesc = Object.getOwnPropertyDescriptor(Module, 'exports');
```

<a id="ref-q1-33"></a>
### [33] `deps/v8/test/mjsunit/wasm/js-api.js:238`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L238)

```javascript
assertEq(typeof moduleExportsDesc.value, 'function');
```

<a id="ref-q1-34"></a>
### [34] `deps/v8/test/mjsunit/wasm/js-api.js:245`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L245)

```javascript
assertEq(moduleExports.length, 1);
```

<a id="ref-q1-35"></a>
### [35] `deps/v8/test/mjsunit/wasm/js-api.js:189`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L189)

```javascript
let moduleImportsDesc = Object.getOwnPropertyDescriptor(Module, 'imports');
```

<a id="ref-q1-36"></a>
### [36] `deps/v8/test/mjsunit/wasm/js-api.js:190`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L190)

```javascript
assertEq(typeof moduleImportsDesc.value, 'function');
```

<a id="ref-q1-37"></a>
### [37] `deps/v8/test/mjsunit/wasm/js-api.js:197`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L197)

```javascript
assertEq(moduleImports.length, 1);
```

<a id="ref-q1-38"></a>
### [38] `deps/v8/test/mjsunit/wasm/js-api.js:283-284`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L283-L284)

```javascript
let moduleCustomSectionsDesc =
    Object.getOwnPropertyDescriptor(Module, 'customSections');
```

<a id="ref-q1-39"></a>
### [39] `deps/v8/test/mjsunit/wasm/js-api.js:285`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L285)

```javascript
assertEq(typeof moduleCustomSectionsDesc.value, 'function');
```

<a id="ref-q1-40"></a>
### [40] `deps/v8/test/mjsunit/wasm/js-api.js:292`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L292)

```javascript
assertEq(moduleCustomSections.length, 2);
```

<a id="ref-q1-41"></a>
### [41] `test/fixtures/wpt/wasm/jsapi/interface.any.js:9-11`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L9-L11)

```javascript
      assert_true(propdesc.writable, "writable");
      assert_true(propdesc.enumerable, "enumerable");
      assert_true(propdesc.configurable, "configurable");
```

<a id="ref-q1-42"></a>
### [42] `deps/v8/test/mjsunit/wasm/js-api.js:341`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L341)

```javascript
let instanceDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'Instance');
```

<a id="ref-q1-43"></a>
### [43] `deps/v8/test/mjsunit/wasm/js-api.js:342`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L342)

```javascript
assertEq(typeof instanceDesc.value, 'function');
```

<a id="ref-q1-44"></a>
### [44] `deps/v8/test/mjsunit/wasm/js-api.js:351`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L351)

```javascript
assertEq(Instance.name, 'Instance');
```

<a id="ref-q1-45"></a>
### [45] `deps/v8/test/mjsunit/wasm/js-api.js:350`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L350)

```javascript
assertEq(Instance.length, 1);
```

<a id="ref-q1-46"></a>
### [46] `deps/v8/test/mjsunit/wasm/js-api.js:343-345`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L343-L345)

```javascript
assertTrue(instanceDesc.writable);
assertFalse(instanceDesc.enumerable);
assertTrue(instanceDesc.configurable);
```

<a id="ref-q1-47"></a>
### [47] `deps/v8/test/mjsunit/wasm/js-api.js:421`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L421)

```javascript
let memoryDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'Memory');
```

<a id="ref-q1-48"></a>
### [48] `deps/v8/test/mjsunit/wasm/js-api.js:422`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L422)

```javascript
assertEq(typeof memoryDesc.value, 'function');
```

<a id="ref-q1-49"></a>
### [49] `deps/v8/test/mjsunit/wasm/js-api.js:428`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L428)

```javascript
let Memory = WebAssembly.Memory;
```

<a id="ref-q1-50"></a>
### [50] `deps/v8/test/mjsunit/wasm/js-api.js:429`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L429)

```javascript
assertEq(Memory, memoryDesc.value);
```

<a id="ref-q1-51"></a>
### [51] `deps/v8/test/mjsunit/wasm/js-api.js:423-425`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L423-L425)

```javascript
assertTrue(memoryDesc.writable);
assertFalse(memoryDesc.enumerable);
assertTrue(memoryDesc.configurable);
```

<a id="ref-q1-52"></a>
### [52] `deps/v8/test/mjsunit/wasm/js-api.js:566`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L566)

```javascript
let tableDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'Table');
```

<a id="ref-q1-53"></a>
### [53] `deps/v8/test/mjsunit/wasm/js-api.js:567`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L567)

```javascript
assertEq(typeof tableDesc.value, 'function');
```

<a id="ref-q1-54"></a>
### [54] `deps/v8/test/mjsunit/wasm/js-api.js:576`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L576)

```javascript
assertEq(Table.name, 'Table');
```

<a id="ref-q1-55"></a>
### [55] `deps/v8/test/mjsunit/wasm/js-api.js:575`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L575)

```javascript
assertEq(Table.length, 1);
```

<a id="ref-q1-56"></a>
### [56] `deps/v8/test/mjsunit/wasm/js-api.js:568-570`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L568-L570)

```javascript
assertTrue(tableDesc.writable);
assertFalse(tableDesc.enumerable);
assertTrue(tableDesc.configurable);
```

<a id="ref-q1-57"></a>
### [57] `test/fixtures/wpt/wasm/jsapi/interface.any.js:78`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L78)

```javascript
  "Global",
```

<a id="ref-q1-58"></a>
### [58] `deps/v8/test/mjsunit/wasm/js-api.js:786`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L786)

```javascript
let compileDesc = Object.getOwnPropertyDescriptor(WebAssembly, 'compile');
```

<a id="ref-q1-59"></a>
### [59] `deps/v8/test/mjsunit/wasm/js-api.js:787`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L787)

```javascript
assertEq(typeof compileDesc.value, 'function');
```

<a id="ref-q1-60"></a>
### [60] `deps/v8/test/mjsunit/wasm/js-api.js:796`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L796)

```javascript
assertEq(compile.name, 'compile');
```

<a id="ref-q1-61"></a>
### [61] `deps/v8/test/mjsunit/wasm/js-api.js:795`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L795)

```javascript
assertEq(compile.length, 1);
```

<a id="ref-q1-62"></a>
### [62] `deps/v8/test/mjsunit/wasm/js-api.js:788-790`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L788-L790)

```javascript
assertTrue(compileDesc.writable);
assertTrue(compileDesc.enumerable);
assertTrue(compileDesc.configurable);
```

<a id="ref-q1-63"></a>
### [63] `test/fixtures/wpt/wasm/jsapi/interface.any.js:116`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L116)

```javascript
  ["validate", 1],
```

<a id="ref-q1-64"></a>
### [64] `test/fixtures/wpt/wasm/jsapi/interface.any.js:118`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L118)

```javascript
  ["instantiate", 1],
```

<a id="ref-q1-65"></a>
### [65] `test/fixtures/wpt/wasm/jsapi/interface.any.js:96-101`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L96-L101)

```javascript
    const propdesc = Object.getOwnPropertyDescriptor(interface_object, "prototype");
    assert_equals(typeof propdesc, "object");
    assert_false(propdesc.writable, "writable");
    assert_false(propdesc.enumerable, "enumerable");
    assert_false(propdesc.configurable, "configurable");
  }, `WebAssembly.${name}: prototype`);
```

<a id="ref-q1-66"></a>
### [66] `test/fixtures/wpt/wasm/jsapi/interface.any.js:106-112`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L106-L112)

```javascript
    const propdesc = Object.getOwnPropertyDescriptor(interface_prototype_object, "constructor");
    assert_equals(typeof propdesc, "object");
    assert_true(propdesc.writable, "writable");
    assert_false(propdesc.enumerable, "enumerable");
    assert_true(propdesc.configurable, "configurable");
    assert_equals(propdesc.value, interface_object);
  }, `WebAssembly.${name}: prototype.constructor`);
```

<a id="ref-q1-67"></a>
### [67] `test/fixtures/wpt/wasm/jsapi/interface.any.js:129-140`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L129-L140)

```javascript
test_attributes(WebAssembly.Instance.prototype, "WebAssembly.Instance", [
  ["exports", false],
]);


test_operations(WebAssembly.Memory.prototype, "WebAssembly.Memory", [
  ["grow", 1],
]);

test_attributes(WebAssembly.Memory.prototype, "WebAssembly.Memory", [
  ["buffer", false],
]);
```

<a id="ref-q1-68"></a>
### [68] `deps/v8/test/mjsunit/wasm/js-api.js:398-404`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L398-L404)

```javascript
assertEq(instanceExportsDesc.get.name, 'get exports');
assertEq(instanceExportsDesc.get.length, 0);
assertFalse(isConstructor(instanceExportsDesc.get));
assertFalse('prototype' in instanceExportsDesc.get);
assertEq(instanceExportsDesc.set, undefined);
assertTrue(instanceExportsDesc.enumerable);
assertTrue(instanceExportsDesc.configurable);
```

<a id="ref-q1-69"></a>
### [69] `deps/v8/test/mjsunit/wasm/js-api.js:1`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/test/mjsunit/wasm/js-api.js#L1)

```javascript
// Copyright 2016 the V8 project authors. All rights reserved.
```

<a id="ref-q1-70"></a>
### [70] `test/fixtures/wpt/wasm/jsapi/interface.any.js:1`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/wpt/wasm/jsapi/interface.any.js#L1)

```javascript
// META: global=window,dedicatedworker,jsshell
```

<a id="ref-q1-71"></a>
### [71] `lib/internal/freeze_intrinsics.js:350-361`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/freeze_intrinsics.js#L350-L361)

```javascript
  if (typeof WebAssembly !== 'undefined') {
    ArrayPrototypePush(intrinsicPrototypes,
                       WebAssembly.Module.prototype,
                       WebAssembly.Instance.prototype,
                       WebAssembly.Table.prototype,
                       WebAssembly.Memory.prototype,
                       WebAssembly.CompileError.prototype,
                       WebAssembly.LinkError.prototype,
                       WebAssembly.RuntimeError.prototype,
    );
    ArrayPrototypePush(intrinsics, WebAssembly);
  }
```
