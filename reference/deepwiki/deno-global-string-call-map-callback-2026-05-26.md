# DeepWiki Response

Repository: `denoland/deno`
Question: In Deno's JavaScript/Node compatibility implementation or tests, what invariant applies to the call form of the global String constructor, especially when used as an Array.prototype.map callback like values.map(String)? Does String(value) perform ECMAScript ToString conversion and return ordinary string values rather than undefined? Point to relevant implementation/tests if present.
Search: https://deepwiki.com/search/in-denos-javascriptnode-compat_e47acba5-4ba1-4748-932a-32e98be1fbd3

**Question**: In Deno's JavaScript/Node compatibility implementation or tests, what invariant applies to the call form of the global String constructor, especially when used as an Array.prototype.map callback like values.map(String)? Does String(value) perform ECMAScript ToString conversion and return ordinary string values rather than undefined? Point to relevant implementation/tests if present.

In Deno's JavaScript/Node compatibility layer, the global `String` constructor, when called as a function (e.g., `String(value)`), performs an ECMAScript `ToString` conversion on its argument and returns an ordinary string value. [1](#ref-q1-1)  [2](#ref-q1-2)  This behavior is consistent with the ECMAScript specification, meaning it will not return `undefined` for any value. [3](#ref-q1-3)  This invariant holds true even when `String` is used as a callback, such as in `values.map(String)`. [4](#ref-q1-4)  [5](#ref-q1-5) 

## Implementation Details

The `String` constructor is exposed as a primordial in various Deno internal modules, including `ext/node/polyfills/internal/validators.mjs` [1](#ref-q1-1) , `ext/web/00_infra.js` [2](#ref-q1-2) , `ext/node/polyfills/string_decoder.ts` [6](#ref-q1-6) , and `ext/node/polyfills/internal/buffer.mjs` [7](#ref-q1-7) . This ensures that the standard JavaScript `String` conversion is used consistently across these compatibility layers.

### ECMAScript `ToString` Conversion

The behavior of `String(value)` is directly tied to the ECMAScript `ToString` abstract operation. [8](#ref-q1-8)  This operation converts any JavaScript value to its string representation. [9](#ref-q1-9)  For example, `String(null)` results in `"null"`, `String(1)` results in `"1"`, and `String(undefined)` results in `"undefined"`. [10](#ref-q1-10) 

A test case in `libs/core/webidl.rs` demonstrates this behavior for various types:
*   `v8::String::new(scope, "foo").unwrap()` converts to `"foo"`. [9](#ref-q1-9) 
*   `v8::Number::new(scope, 1.0)` converts to `"1"`. [11](#ref-q1-11) 
*   `v8::null(scope)` converts to `"null"`. [10](#ref-q1-10) 
*   `v8::Object::new(scope)` converts to `"[object Object]"`. [12](#ref-q1-12) 

The `String::convert` function in `libs/core/webidl.rs` is responsible for this conversion, which aligns with the ECMAScript `ToString` operation. [9](#ref-q1-9) 

## Usage as an `Array.prototype.map` Callback

When `String` is used as a callback for `Array.prototype.map`, such as `values.map(String)`, each element in the `values` array will be passed to the `String` constructor, and the result of the `ToString` conversion will be collected into a new array. [4](#ref-q1-4)  [5](#ref-q1-5)  This means that `undefined` will never be returned by the `String` constructor itself; instead, the string `"undefined"` would be returned if `undefined` were passed as an argument. [10](#ref-q1-10) 

## Notes

The `String` primordial is frequently imported and used in various Deno internal modules, indicating its consistent application for string conversions. [1](#ref-q1-1)  [2](#ref-q1-2)  [6](#ref-q1-6)  [7](#ref-q1-7)  The `ipc_serialization` test in `ext/node/ops/ipc.rs` also shows that `new String("foo")` serializes to `"foo"`, further confirming the expected string conversion behavior. [13](#ref-q1-13) 

Wiki pages you might want to explore:
- [Storage, Cache, and Web APIs (denoland/deno)](/wiki/denoland/deno#6.4)
- [Buffer and Core APIs (denoland/deno)](/wiki/denoland/deno#7.2)

View this search on DeepWiki: https://deepwiki.com/search/in-denos-javascriptnode-compat_e47acba5-4ba1-4748-932a-32e98be1fbd3

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/validators.mjs:18`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/validators.mjs#L18)

```
  String,
```

<a id="ref-q1-2"></a>
### [2] `ext/web/00_infra.js:28`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/00_infra.js#L28)

```javascript
  String,
```

<a id="ref-q1-3"></a>
### [3] `libs/core/webidl.rs:1440-1447`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1440-L1447)

```rust
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &Default::default(),
    );
    assert_eq!(converted.unwrap(), "1");
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/internal/validators.mjs:10`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/validators.mjs#L10)

```
  ArrayPrototypeMap,
```

<a id="ref-q1-5"></a>
### [5] `ext/web/00_infra.js:19`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/00_infra.js#L19)

```javascript
  ArrayPrototypeMap,
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/string_decoder.ts:52`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/string_decoder.ts#L52)

```typescript
  String,
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/buffer.mjs:42`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L42)

```
  String,
```

<a id="ref-q1-8"></a>
### [8] `libs/core/webidl.rs:1425-1437`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1425-L1437)

```rust
  fn string() {
    let mut runtime = JsRuntime::new(Default::default());
    deno_core::scope!(scope, runtime);

    let val = v8::String::new(scope, "foo").unwrap();
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &Default::default(),
    );
    assert_eq!(converted.unwrap(), "foo");
```

<a id="ref-q1-9"></a>
### [9] `libs/core/webidl.rs:1429-1437`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1429-L1437)

```rust
    let val = v8::String::new(scope, "foo").unwrap();
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &Default::default(),
    );
    assert_eq!(converted.unwrap(), "foo");
```

<a id="ref-q1-10"></a>
### [10] `libs/core/webidl.rs:1460-1467`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1460-L1467)

```rust
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &Default::default(),
    );
    assert_eq!(converted.unwrap(), "null");
```

<a id="ref-q1-11"></a>
### [11] `libs/core/webidl.rs:1439-1447`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1439-L1447)

```rust
    let val = v8::Number::new(scope, 1.0);
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &Default::default(),
    );
    assert_eq!(converted.unwrap(), "1");
```

<a id="ref-q1-12"></a>
### [12] `libs/core/webidl.rs:1481-1492`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/core/webidl.rs#L1481-L1492)

```rust
    let val = v8::Object::new(scope);
    let converted = String::convert(
      scope,
      val.into(),
      "prefix".into(),
      (|| "context".into()).into(),
      &StringOptions {
        treat_null_as_empty_string: true,
      },
    );
    assert_eq!(converted.unwrap(), "[object Object]");
```

<a id="ref-q1-13"></a>
### [13] `ext/node/ops/ipc.rs:957`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/ipc.rs#L957)

```rust
        (r#"new String("foo")"#, "\"foo\""),
```
