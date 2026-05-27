# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility, how should node:path methods such as join, resolve, and basename validate non-string path arguments? What error name and code should be observable when null, undefined, numbers, objects, or arrays are passed?
Search: https://deepwiki.com/search/for-node-compatibility-how-sho_b0173353-cfbb-480c-ba79-4295deda6f25

**Question**: For Node compatibility, how should node:path methods such as join, resolve, and basename validate non-string path arguments? What error name and code should be observable when null, undefined, numbers, objects, or arrays are passed?

For Node compatibility, `node:path` methods like `join`, `resolve`, and `basename` should validate non-string path arguments by throwing a `TypeError` with the error code `ERR_INVALID_ARG_TYPE` [1](#ref-q1-1) . This applies when `null`, `undefined`, numbers, objects, or arrays are passed as arguments [2](#ref-q1-2) .

## Argument Validation in `node:path` Methods

The `deno_node` extension, which provides Node.js API compatibility in Deno, implements argument validation for `node:path` methods.

### `assertPath` Function
The core of this validation is handled by the `assertPath` function, defined in `ext/node/polyfills/path/_util.ts` [2](#ref-q1-2) . This function checks if a given `path` argument is a string. If it is not, it throws an `ERR_INVALID_ARG_TYPE` error [3](#ref-q1-3) .

### Usage in `path.join`
For example, in the Windows-specific implementation of `path.join` (`ext/node/polyfills/path/_win32.ts`), each path segment passed to the function is validated using `assertPath` [4](#ref-q1-4) . If any segment is not a string, `assertPath` will throw the `ERR_INVALID_ARG_TYPE` error. The `cli/tsc/dts/node/path.d.cts` file also indicates that `path.join` should throw a `TypeError` if any path segment is not a string [5](#ref-q1-5) .

### Usage in `path.resolve` and `path.basename`
Similarly, `path.resolve` is also expected to throw a `TypeError` if any of its arguments are not strings [6](#ref-q1-6) . While `basename` is not explicitly shown calling `assertPath` in the provided snippets, the general Node.js compatibility guidelines suggest similar validation for path-related functions. The `ext/node/ops/require.rs` file shows Rust operations for `op_require_path_basename` and `op_require_path_resolve` which handle string inputs [7](#ref-q1-7)  [8](#ref-q1-8) , implying that non-string arguments would be caught at the JavaScript polyfill layer.

## Error Name and Code

When a non-string argument is provided to these `node:path` methods, the observable error will be a `TypeError` with the code `ERR_INVALID_ARG_TYPE` [1](#ref-q1-1) . This error is constructed by the `ERR_INVALID_ARG_TYPE` class, which extends `NodeTypeError` [1](#ref-q1-1) . The error message will indicate the argument name, the expected types (e.g., "string"), and the actual received value [9](#ref-q1-9) .

For example, tests for `fs.stat` and `fs.statSync` explicitly check for this error when `undefined` is passed as a path argument [10](#ref-q1-10)  [11](#ref-q1-11) . Similarly, `require` also throws `ERR_INVALID_ARG_TYPE` for non-string IDs [12](#ref-q1-12) .

## Notes

The `ext/node/polyfills/internal/fs/utils.mjs` file contains a `join` function that handles `string` or `Uint8Array` types for paths [13](#ref-q1-13) . If the arguments do not match these types, it also throws an `ERR_INVALID_ARG_TYPE` error, expecting "string" or "Buffer" [14](#ref-q1-14) . This `join` function is used internally for `getDirents` [15](#ref-q1-15)  and is distinct from the `node:path.join` method, which primarily expects string arguments. The user's query specifically refers to `node:path` methods, so the primary answer focuses on string validation.

Wiki pages you might want to explore:
- [deno_node Extension Architecture (denoland/deno)](/wiki/denoland/deno#7.1)
- [File System Polyfills (denoland/deno)](/wiki/denoland/deno#7.4)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-how-sho_b0173353-cfbb-480c-ba79-4295deda6f25

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/errors.ts:770-774`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/errors.ts#L770-L774)

```typescript
class ERR_INVALID_ARG_TYPE extends NodeTypeError {
  constructor(name: string, expected: string | string[], actual: unknown) {
    const msg = createInvalidArgType(name, expected);
    super("ERR_INVALID_ARG_TYPE", `${msg}.${invalidArgTypeHelper(actual)}`);
  }
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/path/_util.ts:24-28`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_util.ts#L24-L28)

```typescript
function assertPath(path: string) {
  if (typeof path !== "string") {
    throw new ERR_INVALID_ARG_TYPE("path", ["string"], path);
  }
}
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/path/_util.ts:25-27`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_util.ts#L25-L27)

```typescript
  if (typeof path !== "string") {
    throw new ERR_INVALID_ARG_TYPE("path", ["string"], path);
  }
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/path/_win32.ts:467-468`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_win32.ts#L467-L468)

```typescript
    const path = paths[i];
    assertPath(path);
```

<a id="ref-q1-5"></a>
### [5] `cli/tsc/dts/node/path.d.cts:78-81`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/path.d.cts#L78-L81)

```
             *
             * @param paths paths to join.
             * @throws {TypeError} if any of the path segments is not a string.
             */
```

<a id="ref-q1-6"></a>
### [6] `cli/tsc/dts/node/path.d.cts:93-95`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/path.d.cts#L93-L95)

```
             * @param paths A sequence of paths or path segments.
             * @throws {TypeError} if any of the arguments is not a string.
             */
```

<a id="ref-q1-7"></a>
### [7] `ext/node/ops/require.rs:409-413`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/require.rs#L409-L413)

```rust
#[string]
pub fn op_require_path_resolve(#[scoped] parts: Vec<String>) -> String {
  path_resolve(parts.iter().map(|s| s.as_str()))
    .to_string_lossy()
    .into_owned()
```

<a id="ref-q1-8"></a>
### [8] `ext/node/ops/require.rs:431-433`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/ops/require.rs#L431-L433)

```rust
pub fn op_require_path_basename(
  #[string] request: &str,
) -> Result<String, JsErrorBox> {
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/internal/errors.ts:771-773`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/errors.ts#L771-L773)

```typescript
  constructor(name: string, expected: string | string[], actual: unknown) {
    const msg = createInvalidArgType(name, expected);
    super("ERR_INVALID_ARG_TYPE", `${msg}.${invalidArgTypeHelper(actual)}`);
```

<a id="ref-q1-10"></a>
### [10] `tests/unit_node/_fs/_fs_stat_test.ts:90-103`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/_fs/_fs_stat_test.ts#L90-L103)

```typescript
    try {
      await new Promise<Stats>((resolve, reject) => {
        stat(
          // deno-lint-ignore no-explicit-any
          undefined as any,
          (err, stats) => err ? reject(err) : resolve(stats),
        );
      });
      fail();
    } catch (err) {
      assert(err instanceof TypeError);
      // deno-lint-ignore no-explicit-any
      assertEquals((err as any).code, "ERR_INVALID_ARG_TYPE");
    }
```

<a id="ref-q1-11"></a>
### [11] `tests/unit_node/_fs/_fs_stat_test.ts:107-118`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/_fs/_fs_stat_test.ts#L107-L118)

```typescript
Deno.test({
  name: "[node/fs] statSync invalid path error",
  fn() {
    try {
      // deno-lint-ignore no-explicit-any
      statSync(undefined as any);
      fail();
    } catch (err) {
      assert(err instanceof TypeError);
      // deno-lint-ignore no-explicit-any
      assertEquals((err as any).code, "ERR_INVALID_ARG_TYPE");
    }
```

<a id="ref-q1-12"></a>
### [12] `tests/unit_node/module_test.ts:179-190`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/module_test.ts#L179-L190)

```typescript
Deno.test("[node/module require] throws ERR_INVALID_ARG_TYPE for non-string id", () => {
  const require = createRequire(import.meta.url);
  const err = assertThrows(
    // @ts-expect-error testing invalid input
    () => require(123),
    TypeError,
  );
  assertEquals(
    (err as TypeError & { code?: string }).code,
    "ERR_INVALID_ARG_TYPE",
  );
});
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/internal/fs/utils.mjs:246-273`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/fs/utils.mjs#L246-L273)

```
function join(path, name) {
  if (
    (typeof path === "string" || isUint8Array(path)) &&
    name === undefined
  ) {
    return path;
  }

  if (typeof path === "string" && isUint8Array(name)) {
    const pathBuffer = Buffer.from(
      // deno-lint-ignore prefer-primordials `join` is a `node:path` function
      lazyPath().default.join(path, lazyPath().default.sep),
    );
    // Ignore lint. `concat` is a 'node:buffer' static method on `Buffer`
    // deno-lint-ignore prefer-primordials
    return Buffer.concat([pathBuffer, name]);
  }

  if (typeof path === "string" && typeof name === "string") {
    // deno-lint-ignore prefer-primordials `join` is a `node:path` function
    return lazyPath().default.join(path, name);
  }

  if (isUint8Array(path) && isUint8Array(name)) {
    // Ignore lint. `concat` is a 'node:buffer' static method on `Buffer`
    // deno-lint-ignore prefer-primordials
    return Buffer.concat([path, bufferSep, name]);
  }
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/internal/fs/utils.mjs:275-279`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/fs/utils.mjs#L275-L279)

```
  throw new ERR_INVALID_ARG_TYPE(
    "path",
    ["string", "Buffer"],
    path,
  );
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/internal/fs/utils.mjs:296`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/fs/utils.mjs#L296)

```
          filepath = join(path, name);
```
