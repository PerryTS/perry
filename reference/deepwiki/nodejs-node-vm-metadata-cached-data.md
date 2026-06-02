# DeepWiki Response

Repository: `nodejs/node`
Question: What JavaScript-visible shape and validation does Node expose for vm.Script sourceMapURL, Script cachedData, compileFunction cachedData, and SourceTextModule.createCachedData?
Search: https://deepwiki.com/search/what-javascriptvisible-shape-a_8eaa6ded-6879-458d-bab6-9669f7bd5b54

**Question**: What JavaScript-visible shape and validation does Node expose for vm.Script sourceMapURL, Script cachedData, compileFunction cachedData, and SourceTextModule.createCachedData?

Node.js exposes specific JavaScript-visible shapes and validation for `vm.Script`'s `sourceMapURL` and `cachedData`, `vm.compileFunction`'s `cachedData`, and `vm.SourceTextModule.createCachedData()`.

### `vm.Script`

The `vm.Script` class allows you to compile JavaScript code. When creating a new `vm.Script` instance, you can provide `cachedData` and retrieve `sourceMapURL` [1](#ref-q1-1) .

*   **`script.sourceMapURL`**: This property is a `string` or `undefined` [2](#ref-q1-2) . It is set if the script's source code contains a source map magic comment (e.g., `//# sourceMappingURL=sourcemap.json`) [3](#ref-q1-3) . The value will be the URL of the source map [3](#ref-q1-3) . This property is populated from the `ScriptOrigin.SourceMapUrl()` method in the C++ binding [4](#ref-q1-4) .
*   **`options.cachedData`**: When creating a `new vm.Script(code, options)` [5](#ref-q1-5) , you can provide `cachedData` as a `Buffer`, `TypedArray`, or `DataView` [6](#ref-q1-6) . This data is V8's code cache for the supplied source [7](#ref-q1-7) .
    *   **Validation**: The `cachedData` option is validated to be an instance of `Buffer`, `TypedArray`, or `DataView` [8](#ref-q1-8) . If an invalid type is provided, a `TypeError` with `ERR_INVALID_ARG_TYPE` is thrown [9](#ref-q1-9) .
    *   **`script.cachedDataRejected`**: This boolean property indicates whether the provided `cachedData` was accepted by V8 [10](#ref-q1-10) . It is `true` if rejected, `false` if accepted, and `undefined` if no `cachedData` was supplied [11](#ref-q1-11) .
*   **`script.createCachedData()`**: This method returns a `Buffer` containing V8's code cache data for the script [12](#ref-q1-12) . This cache can be used with the `cachedData` option in the `vm.Script` constructor [13](#ref-q1-13) . The code cache does not contain JavaScript observable states and is safe to be saved and reused [14](#ref-q1-14) .

### `vm.compileFunction`

The `vm.compileFunction()` method also supports `cachedData` [15](#ref-q1-15) .

*   **`options.cachedData`**: Similar to `vm.Script`, this option accepts a `Buffer`, `TypedArray`, or `DataView` [16](#ref-q1-16) . This data must have been produced by a prior call to `vm.compileFunction()` with the same `code` and `params` [17](#ref-q1-17) .
    *   **`cachedDataRejected`**: The return value of `vm.compileFunction()` includes `cachedDataRejected` with the same semantics as `vm.Script` if the `cachedData` option was passed [18](#ref-q1-18) . This is handled internally by `ContextifyFunction::CompileFunctionAndCacheResult` [19](#ref-q1-19) .

### `vm.SourceTextModule.createCachedData()`

The `vm.SourceTextModule` class, used for ECMAScript modules in VM contexts, also provides a `createCachedData()` method [20](#ref-q1-20) .

*   **`sourceTextModule.createCachedData()`**: This method returns a `Buffer` containing code cache data [21](#ref-q1-21) . This cache can be used with the `SourceTextModule` constructor's `cachedData` option [22](#ref-q1-22) . The code cache is safe to save and reuse, and it serializes metadata that V8 uses to speed up future compilations [23](#ref-q1-23) .
*   **`options.cachedData`**: When creating a `new vm.SourceTextModule(code, options)` [24](#ref-q1-24) , you can provide `cachedData` as a `Buffer`, `TypedArray`, or `DataView` [25](#ref-q1-25) . The `code` must be identical to the module from which the `cachedData` was created [26](#ref-q1-26) .
    *   **Validation**: Similar to `vm.Script`, the `cachedData` option for `vm.SourceTextModule` is validated to be an instance of `Buffer`, `TypedArray`, or `DataView` [8](#ref-q1-8) . An invalid type will result in a `TypeError` [9](#ref-q1-9) . If the provided `cachedData` is rejected by V8, an `ERR_VM_MODULE_CACHED_DATA_REJECTED` error is thrown [27](#ref-q1-27) .

## Notes

The underlying V8 `ScriptCompiler::CachedData` structure, which these JavaScript-visible options interact with, includes fields like `data`, `length`, `rejected`, and `buffer_policy` [28](#ref-q1-28) . The `rejected` field directly corresponds to the `cachedDataRejected` property exposed in JavaScript [29](#ref-q1-29) . The `ScriptCompiler::Source` object, used internally for compilation, takes ownership of the `CachedData` [30](#ref-q1-30) .

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)
- [Module System Architecture (nodejs/node)](/wiki/nodejs/node#2.4)

View this search on DeepWiki: https://deepwiki.com/search/what-javascriptvisible-shape-a_8eaa6ded-6879-458d-bab6-9669f7bd5b54

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/vm.md:83-111`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L83-L111)

```markdown
* `code` {string} The JavaScript code to compile.
* `options` {Object|string}
  * `filename` {string} Specifies the filename used in stack traces produced
    by this script. **Default:** `'evalmachine.<anonymous>'`.
  * `lineOffset` {number} Specifies the line number offset that is displayed
    in stack traces produced by this script. **Default:** `0`.
  * `columnOffset` {number} Specifies the first-line column number offset that
    is displayed in stack traces produced by this script. **Default:** `0`.
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
    source. When supplied, the `cachedDataRejected` value will be set to
    either `true` or `false` depending on acceptance of the data by V8.
  * `produceCachedData` {boolean} When `true` and no `cachedData` is present, V8
    will attempt to produce code cache data for `code`. Upon success, a
    `Buffer` with V8's code cache data will be produced and stored in the
    `cachedData` property of the returned `vm.Script` instance.
    The `cachedDataProduced` value will be set to either `true` or `false`
    depending on whether code cache data is produced successfully.
    This option is **deprecated** in favor of `script.createCachedData()`.
    **Default:** `false`.
  * `importModuleDynamically`
    {Function|vm.constants.USE\_MAIN\_CONTEXT\_DEFAULT\_LOADER}
    Used to specify how the modules should be loaded during the evaluation
    of this script when `import()` is called. This option is part of the
    experimental modules API. We do not recommend using it in a production
    environment. For detailed information, see
    [Support of dynamic `import()` in compilation APIs][].

If `options` is a string, then it specifies the filename.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/vm.md:373`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L373)

<a id="ref-q1-3"></a>
### [3] `doc/api/vm.md:376-377`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L376-L377)

```markdown
When the script is compiled from a source that contains a source map magic
comment, this property will be set to the URL of the source map.
```

<a id="ref-q1-4"></a>
### [4] `src/node_contextify.cc:1093-1096`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_contextify.cc#L1093-L1096)

```cpp
  if (self->Set(env->context(),
                env->source_map_url_string(),
                v8_script->GetSourceMappingURL())
          .IsNothing()) {
```

<a id="ref-q1-5"></a>
### [5] `doc/api/vm.md:56-66`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L56-L66)

```markdown
### `new vm.Script(code[, options])`

<!-- YAML
added: v0.3.1
changes:
  - version:
    - v21.7.0
    - v20.12.0
    pr-url: https://github.com/nodejs/node/pull/51244
    description: Added support for
                `vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER`.
```

<a id="ref-q1-6"></a>
### [6] `doc/api/vm.md:91-93`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L91-L93)

```markdown
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
    source. When supplied, the `cachedDataRejected` value will be set to
```

<a id="ref-q1-7"></a>
### [7] `doc/api/vm.md:91-94`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L91-L94)

```markdown
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
    source. When supplied, the `cachedDataRejected` value will be set to
    either `true` or `false` depending on acceptance of the data by V8.
```

<a id="ref-q1-8"></a>
### [8] `test/parallel/test-vm-module-errors.js:224-226`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-vm-module-errors.js#L224-L226)

```javascript
  [true, false, 'foo', {}, Array, function() {}].forEach((invalidArg) => {
    const message = 'The "options.cachedData" property must be an ' +
                    'instance of Buffer, TypedArray, or DataView.' +
```

<a id="ref-q1-9"></a>
### [9] `test/parallel/test-vm-module-errors.js:227-234`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-vm-module-errors.js#L227-L234)

```javascript
                    common.invalidArgTypeHelper(invalidArg);
    assert.throws(
      () => new SourceTextModule('import "foo";', { cachedData: invalidArg }),
      {
        code: 'ERR_INVALID_ARG_TYPE',
        name: 'TypeError',
        message,
      }
```

<a id="ref-q1-10"></a>
### [10] `doc/api/vm.md:125-126`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L125-L126)

```markdown
When `cachedData` is supplied to create the `vm.Script`, this value will be set
to either `true` or `false` depending on acceptance of the data by V8.
```

<a id="ref-q1-11"></a>
### [11] `doc/api/vm.md:125-128`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L125-L128)

```markdown
When `cachedData` is supplied to create the `vm.Script`, this value will be set
to either `true` or `false` depending on acceptance of the data by V8.
Otherwise the value is `undefined`.
```

<a id="ref-q1-12"></a>
### [12] `doc/api/vm.md:135-136`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L135-L136)

```markdown
* Returns: {Buffer}
```

<a id="ref-q1-13"></a>
### [13] `doc/api/vm.md:137-138`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L137-L138)

```markdown
Creates a code cache that can be used with the `Script` constructor's
`cachedData` option. Returns a `Buffer`. This method may be called at any
```

<a id="ref-q1-14"></a>
### [14] `doc/api/vm.md:140-143`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L140-L143)

```markdown

The code cache of the `Script` doesn't contain any JavaScript observable
states. The code cache is safe to be saved along side the script source and
used to construct new `Script` instances multiple times.
```

<a id="ref-q1-15"></a>
### [15] `doc/api/vm.md:1273-1276`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L1273-L1276)

```markdown
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
    source. This must be produced by a prior call to [`vm.compileFunction()`][]
    with the same `code` and `params`.
```

<a id="ref-q1-16"></a>
### [16] `doc/api/vm.md:1273-1274`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L1273-L1274)

```markdown
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
```

<a id="ref-q1-17"></a>
### [17] `doc/api/vm.md:1275-1277`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L1275-L1277)

```markdown
    source. This must be produced by a prior call to [`vm.compileFunction()`][]
    with the same `code` and `params`.
  * `produceCachedData` {boolean} Specifies whether to produce new cache data.
```

<a id="ref-q1-18"></a>
### [18] `doc/api/vm.md:1239-1242`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L1239-L1242)

```markdown
    pr-url: https://github.com/nodejs/node/pull/46320
    description: The return value now includes `cachedDataRejected`
                 with the same semantics as the `vm.Script` version
                 if the `cachedData` option was passed.
```

<a id="ref-q1-19"></a>
### [19] `src/node_contextify.cc:1584-1592`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_contextify.cc#L1584-L1592)

```cpp
  if (StoreCodeCacheResult(env,
                           result,
                           options,
                           *source,
                           produce_cached_data,
                           std::move(new_cached_data))
          .IsNothing()) {
    return {};
  }
```

<a id="ref-q1-20"></a>
### [20] `doc/api/vm.md:897-903`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L897-L903)

```markdown
### `sourceTextModule.createCachedData()`

<!-- YAML
added:
 - v13.7.0
 - v12.17.0
-->
```

<a id="ref-q1-21"></a>
### [21] `doc/api/vm.md:905`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L905)

```markdown
* Returns: {Buffer}
```

<a id="ref-q1-22"></a>
### [22] `doc/api/vm.md:907-908`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L907-L908)

```markdown
Creates a code cache that can be used with the `SourceTextModule` constructor's
`cachedData` option. Returns a `Buffer`. This method may be called any number
```

<a id="ref-q1-23"></a>
### [23] `doc/api/vm.md:911-919`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L911-L919)

```markdown
The code cache of the `SourceTextModule` doesn't contain any JavaScript
observable states. The code cache is safe to be saved along side the script
source and used to construct new `SourceTextModule` instances multiple times.

Functions in the `SourceTextModule` source can be marked as lazily compiled
and they are not compiled at construction of the `SourceTextModule`. These
functions are going to be compiled when they are invoked the first time. The
code cache serializes the metadata that V8 currently knows about the
`SourceTextModule` that it can use to speed up future compilations.
```

<a id="ref-q1-24"></a>
### [24] `doc/api/vm.md:794-795`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L794-L795)

```markdown
### `new vm.SourceTextModule(code[, options])`
```

<a id="ref-q1-25"></a>
### [25] `doc/api/vm.md:811-813`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L811-L813)

```markdown
  * `cachedData` {Buffer|TypedArray|DataView} Provides an optional `Buffer` or
    `TypedArray`, or `DataView` with V8's code cache data for the supplied
    source. The `code` must be the same as the module from which this
```

<a id="ref-q1-26"></a>
### [26] `doc/api/vm.md:813-815`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/vm.md#L813-L815)

```markdown
    source. The `code` must be the same as the module from which this
    `cachedData` was created.
  * `context` {Object} The [contextified][] object as returned by the
```

<a id="ref-q1-27"></a>
### [27] `src/module_wrap.cc:393-395`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/module_wrap.cc#L393-L395)

```cpp
          cache_rejected) {
        THROW_ERR_VM_MODULE_CACHED_DATA_REJECTED(
            realm, "cachedData buffer was rejected");
```

<a id="ref-q1-28"></a>
### [28] `deps/v8/include/v8-script.h:430-477`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/include/v8-script.h#L430-L477)

```c
  struct V8_EXPORT CachedData {
    enum BufferPolicy { BufferNotOwned, BufferOwned };

    CachedData()
        : data(nullptr),
          length(0),
          rejected(false),
          buffer_policy(BufferNotOwned) {}

    // If buffer_policy is BufferNotOwned, the caller keeps the ownership of
    // data and guarantees that it stays alive until the CachedData object is
    // destroyed. If the policy is BufferOwned, the given data will be deleted
    // (with delete[]) when the CachedData object is destroyed.
    CachedData(const uint8_t* data, int length,
               BufferPolicy buffer_policy = BufferNotOwned);
    ~CachedData();

    enum CompatibilityCheckResult {
      // Don't change order/existing values of this enum since it keys into the
      // `code_cache_reject_reason` histogram. Append-only!
      kSuccess = 0,
      kMagicNumberMismatch = 1,
      kVersionMismatch = 2,
      kSourceMismatch = 3,
      kFlagsMismatch = 5,
      kChecksumMismatch = 6,
      kInvalidHeader = 7,
      kLengthMismatch = 8,
      kReadOnlySnapshotChecksumMismatch = 9,

      // This should always point at the last real enum value.
      kLast = kReadOnlySnapshotChecksumMismatch
    };

    // Check if the CachedData can be loaded in the given isolate.
    CompatibilityCheckResult CompatibilityCheck(Isolate* isolate);

    // TODO(marja): Async compilation; add constructors which take a callback
    // which will be called when V8 no longer needs the data.
    const uint8_t* data;
    int length;
    bool rejected;
    BufferPolicy buffer_policy;

    // Prevent copying.
    CachedData(const CachedData&) = delete;
    CachedData& operator=(const CachedData&) = delete;
  };
```

<a id="ref-q1-29"></a>
### [29] `deps/v8/include/v8-script.h:471`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/include/v8-script.h#L471)

```c
    bool rejected;
```

<a id="ref-q1-30"></a>
### [30] `deps/v8/include/v8-script.h:907-918`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/include/v8-script.h#L907-L918)

```c
ScriptCompiler::Source::Source(Local<String> string, const ScriptOrigin& origin,
                               CachedData* data,
                               ConsumeCodeCacheTask* consume_cache_task)
    : source_string(string),
      resource_name(origin.ResourceName()),
      resource_line_offset(origin.LineOffset()),
      resource_column_offset(origin.ColumnOffset()),
      resource_options(origin.Options()),
      source_map_url(origin.SourceMapUrl()),
      host_defined_options(origin.GetHostDefinedOptions()),
      cached_data(data),
      consume_cache_task(consume_cache_task) {}
```
