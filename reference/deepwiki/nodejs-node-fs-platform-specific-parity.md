# DeepWiki Response

Repository: `nodejs/node`
Question: For Node.js v25.9.0 fs module parity, explain the source-backed behavior for fs.Stats timestamp precision fields including bigint nanosecond fields, fs access/chmod/chown/lchown/lchmod/rm/truncate syscall error propagation and error object shape, access mode constants, callback error-first behavior, and platform-specific lchmod support. Cite relevant Node source and docs.
Search: https://deepwiki.com/search/for-nodejs-v2590-fs-module-par_74333490-c4c1-47ca-a382-03d5c1a95359

**Question**: For Node.js v25.9.0 fs module parity, explain the source-backed behavior for fs.Stats timestamp precision fields including bigint nanosecond fields, fs access/chmod/chown/lchown/lchmod/rm/truncate syscall error propagation and error object shape, access mode constants, callback error-first behavior, and platform-specific lchmod support. Cite relevant Node source and docs.

You are asking about several aspects of the Node.js `fs` module, specifically concerning `fs.Stats` timestamp precision, error handling for various `fs` operations, access mode constants, callback behavior, and platform-specific `lchmod` support. I will explain these behaviors based on the provided Node.js codebase.

## `fs.Stats` Timestamp Precision and BigInt Nanosecond Fields

The `fs.Stats` object in Node.js provides file system status information, including timestamps. These timestamps can be returned with nanosecond precision as `BigInt` values if the `bigint` option is set to `true` when calling `stat`, `fstat`, or `lstat` methods [1](#ref-q1-1)  [2](#ref-q1-2)  [3](#ref-q1-3) . By default, `bigint` is `false`, and numeric values are returned as standard JavaScript numbers [4](#ref-q1-4) .

The `fs.Stats` object contains fields like `atimeMs`, `mtimeMs`, `ctimeMs`, and `birthtimeMs` which represent access time, modification time, change time, and birth time in milliseconds [5](#ref-q1-5) . When `bigint` is enabled, these timestamps, along with other numeric stat values, are represented as `BigInt`s. The underlying C++ implementation uses `kATimeSec`, `kATimeNsec`, `kMTimeSec`, `kMTimeNsec`, `kCTimeSec`, `kCTimeNsec`, `kBirthTimeSec`, and `kBirthTimeNsec` to store the seconds and nanoseconds components of these timestamps [6](#ref-q1-6) .

For example, when `fs.stat` is called with `{ bigint: true }`, the `binding.stat` function is invoked with the `bigint` flag [7](#ref-q1-7) . Similarly, `fs.promises.stat` also supports the `bigint` option [8](#ref-q1-8) . The `test-fs-stat-bigint.js` test file demonstrates how `bigint` stats are retrieved and verified [9](#ref-q1-9) .

## `fs` Syscall Error Propagation and Error Object Shape

Node.js `fs` module operations, including `access`, `chmod`, `chown`, `lchown`, `lchmod`, `rm`, and `truncate`, propagate errors consistently. Asynchronous callback-based APIs follow the "error-first" callback pattern, where the first argument to the callback is an `Error` object if an error occurred, or `null` otherwise [10](#ref-q1-10)  [11](#ref-q1-11)  [12](#ref-q1-12) . Synchronous versions of these functions throw an `Error` object if an operation fails [13](#ref-q1-13) . Promise-based APIs reject with an `Error` object [14](#ref-q1-14) .

The error objects typically have a `code` property, which is one of the [Common System Errors](https://nodejs.org/docs/latest/api/errors.html#common-system-errors), and a `syscall` property indicating the failed system call [15](#ref-q1-15) . For example, `fs.access` will invoke its callback with an `Error` object if accessibility checks fail [16](#ref-q1-16) . The `test/fixtures/permission/fs-write.js` file shows examples of `ERR_ACCESS_DENIED` being thrown or passed to callbacks for `chmod`, `chown`, and `lchown` operations when permissions are insufficient [17](#ref-q1-17)  [18](#ref-q1-18)  [19](#ref-q1-19) .

The `src/node_file.cc` file defines a `get_fs_func_name_by_type` function that maps `uv_fs_type` enums to string names, which are likely used for the `syscall` property of error objects [20](#ref-q1-20) .

## Access Mode Constants

The `fs.access` and `fs.accessSync` methods use access mode constants to specify the accessibility checks to be performed [21](#ref-q1-21)  [22](#ref-q1-22) . These constants are available under `fs.constants` [21](#ref-q1-21) .

The relevant constants are:
*   `fs.constants.F_OK`: Checks if the file exists [21](#ref-q1-21) .
*   `fs.constants.R_OK`: Checks if the file is readable [21](#ref-q1-21) .
*   `fs.constants.W_OK`: Checks if the file is writable [21](#ref-q1-21) .
*   `fs.constants.X_OK`: Checks if the file is executable [21](#ref-q1-21) .

These constants were previously available directly on the `fs` object but were deprecated in v20.8.0 and removed in v25.0.0, requiring access via `fs.constants` [23](#ref-q1-23) . The `test-fs-access.js` file confirms the type of these constants [24](#ref-q1-24) .

## Callback Error-First Behavior

Node.js `fs` asynchronous functions consistently use the error-first callback pattern. This means the callback function always receives an `Error` object as its first argument if an error occurred, and subsequent arguments contain the result of the operation [16](#ref-q1-16) . For example, `fs.access(path, mode, callback)` expects a callback with `(err)` [10](#ref-q1-10) . Similarly, `fs.chmod(path, mode, callback)` expects `(err)` [11](#ref-q1-11) . The `makeCallback` utility function is used internally to wrap user-provided callbacks to ensure this behavior [25](#ref-q1-25) .

## Platform-Specific `lchmod` Support

The `fs.lchmod` and `fs.lchmodSync` methods are platform-specific. They are only implemented on macOS [26](#ref-q1-26) . The Node.js codebase reflects this by conditionally exposing `lchmod` and `lchmodSync` based on the presence of `constants.O_SYMLINK` [27](#ref-q1-27) . If `O_SYMLINK` is undefined, indicating a platform where `lchmod` is not supported, calling `fs.promises.lchmod` will throw an `ERR_METHOD_NOT_IMPLEMENTED` error [28](#ref-q1-28) .

The `fs.lchown` and `fs.lchownSync` methods were previously deprecated but their deprecation was revoked because supporting APIs were added in libuv [29](#ref-q1-29) .

## Notes
The prompt mentions `fs.rm`, but no specific details about its error propagation or object shape were found in the provided snippets. However, based on the consistent error handling patterns across the `fs` module, it can be inferred that `fs.rm` would follow the same error-first callback pattern for asynchronous operations, throw errors for synchronous operations, and reject promises for promise-based APIs, with error objects containing `code` and `syscall` properties.

Wiki pages you might want to explore:
- [Core APIs (nodejs/node)](/wiki/nodejs/node#3)

View this search on DeepWiki: https://deepwiki.com/search/for-nodejs-v2590-fs-module-par_74333490-c4c1-47ca-a382-03d5c1a95359

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/fs.md:1719-1722`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1719-L1722)

```markdown
* `options` {Object}
  * `bigint` {boolean} Whether the numeric values in the returned
    {fs.Stats} object should be `bigint`. **Default:** `false`.
* Returns: {Promise}  Fulfills with the {fs.Stats} object for the
```

<a id="ref-q1-2"></a>
### [2] `doc/api/fs.md:3412-3415`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L3412-L3415)

```markdown
* `options` {Object}
  * `bigint` {boolean} Whether the numeric values in the returned
    {fs.Stats} object should be `bigint`. **Default:** `false`.
* `callback` {Function}
```

<a id="ref-q1-3"></a>
### [3] `doc/api/fs.md:4436-4439`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4436-L4439)

```markdown
* `options` {Object}
  * `bigint` {boolean} Whether the numeric values in the returned
    {fs.Stats} object should be `bigint`. **Default:** `false`.
* `callback` {Function}
```

<a id="ref-q1-4"></a>
### [4] `doc/api/fs.md:1721-1722`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1721-L1722)

```markdown
    {fs.Stats} object should be `bigint`. **Default:** `false`.
* Returns: {Promise}  Fulfills with the {fs.Stats} object for the
```

<a id="ref-q1-5"></a>
### [5] `doc/api/fs.md:4497-4504`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4497-L4504)

```markdown
  atimeMs: 1561174653071.963,
  mtimeMs: 1561174614583.3518,
  ctimeMs: 1561174626623.5366,
  birthtimeMs: 1561174126937.2893,
  atime: 2019-06-22T03:37:33.072Z,
  mtime: 2019-06-22T03:36:54.583Z,
  ctime: 2019-06-22T03:37:06.624Z,
  birthtime: 2019-06-22T03:28:46.937Z
```

<a id="ref-q1-6"></a>
### [6] `src/node_file.h:28-35`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.h#L28-L35)

```c
  kATimeSec,
  kATimeNsec,
  kMTimeSec,
  kMTimeNsec,
  kCTimeSec,
  kCTimeNsec,
  kBirthTimeSec,
  kBirthTimeNsec,
```

<a id="ref-q1-7"></a>
### [7] `lib/fs.js:1631-1633`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L1631-L1633)

```javascript
  const req = new FSReqCallback(options.bigint);
  req.oncomplete = callback;
  binding.stat(getValidatedPath(path), options.bigint, req);
```

<a id="ref-q1-8"></a>
### [8] `lib/internal/fs/promises.js:1029-1031`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L1029-L1031)

```javascript
async function stat(path, options = { bigint: false }) {
  const result = await PromisePrototypeThen(
    binding.stat(getValidatedPath(path), options.bigint, kUsePromises),
```

<a id="ref-q1-9"></a>
### [9] `test/parallel/test-fs-stat-bigint.js:208-209`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-stat-bigint.js#L208-L209)

```javascript
  const bigintStats = await handle.stat(common.mustNotMutateObjectDeep({ bigint: true }));
  const numStats = await handle.stat();
```

<a id="ref-q1-10"></a>
### [10] `doc/api/fs.md:2021-2022`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2021-L2022)

```markdown
  * `err` {Error}
```

<a id="ref-q1-11"></a>
### [11] `doc/api/fs.md:2303-2304`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2303-L2304)

```markdown
* `callback` {Function}
  * `err` {Error}
```

<a id="ref-q1-12"></a>
### [12] `doc/api/fs.md:2990-2991`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2990-L2991)

```markdown
* `callback` {Function}
  * `err` {Error}
```

<a id="ref-q1-13"></a>
### [13] `doc/api/fs.md:5381-5382`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L5381-L5382)

```markdown
If any of the accessibility checks fail, an `Error` will be thrown. Otherwise,
the method will return `undefined`.
```

<a id="ref-q1-14"></a>
### [14] `lib/internal/fs/promises.js:1012-1015`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L1012-L1015)

```javascript
  const result = await PromisePrototypeThen(
    binding.fstat(handle.fd, options.bigint, kUsePromises),
    undefined,
    handleErrorFromBinding,
```

<a id="ref-q1-15"></a>
### [15] `doc/api/fs.md:4446`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4446)

```markdown
In case of an error, the `err.code` will be one of [Common System Errors][].
```

<a id="ref-q1-16"></a>
### [16] `doc/api/fs.md:2031-2033`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2031-L2033)

```markdown
The final argument, `callback`, is a callback function that is invoked with
a possible error argument. If any of the accessibility checks fail, the error
argument will be an `Error` object. The following examples check if
```

<a id="ref-q1-17"></a>
### [17] `test/fixtures/permission/fs-write.js:383-388`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/permission/fs-write.js#L383-L388)

```javascript
  assert.throws(() => {
    fs.chmod(blockedFile, 0o755, common.mustNotCall());
  }, {
    code: 'ERR_ACCESS_DENIED',
    permission: 'FileSystemWrite',
  });
```

<a id="ref-q1-18"></a>
### [18] `test/fixtures/permission/fs-write.js:451-454`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/permission/fs-write.js#L451-L454)

```javascript
  fs.chown(blockedFile, 1541, 999, common.expectsError({
    code: 'ERR_ACCESS_DENIED',
    permission: 'FileSystemWrite',
  }));
```

<a id="ref-q1-19"></a>
### [19] `test/fixtures/permission/fs-write.js:476-479`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/fixtures/permission/fs-write.js#L476-L479)

```javascript
  fs.lchown(blockedFile, 1541, 999, common.expectsError({
    code: 'ERR_ACCESS_DENIED',
    permission: 'FileSystemWrite',
  }));
```

<a id="ref-q1-20"></a>
### [20] `src/node_file.cc:104-146`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L104-L146)

```cpp
static const char* get_fs_func_name_by_type(uv_fs_type req_type) {
  switch (req_type) {
#define FS_TYPE_TO_NAME(type, name)                                            \
  case UV_FS_##type:                                                           \
    return name;
    FS_TYPE_TO_NAME(OPEN, "open")
    FS_TYPE_TO_NAME(CLOSE, "close")
    FS_TYPE_TO_NAME(READ, "read")
    FS_TYPE_TO_NAME(WRITE, "write")
    FS_TYPE_TO_NAME(SENDFILE, "sendfile")
    FS_TYPE_TO_NAME(STAT, "stat")
    FS_TYPE_TO_NAME(LSTAT, "lstat")
    FS_TYPE_TO_NAME(FSTAT, "fstat")
    FS_TYPE_TO_NAME(FTRUNCATE, "ftruncate")
    FS_TYPE_TO_NAME(UTIME, "utime")
    FS_TYPE_TO_NAME(FUTIME, "futime")
    FS_TYPE_TO_NAME(ACCESS, "access")
    FS_TYPE_TO_NAME(CHMOD, "chmod")
    FS_TYPE_TO_NAME(FCHMOD, "fchmod")
    FS_TYPE_TO_NAME(FSYNC, "fsync")
    FS_TYPE_TO_NAME(FDATASYNC, "fdatasync")
    FS_TYPE_TO_NAME(UNLINK, "unlink")
    FS_TYPE_TO_NAME(RMDIR, "rmdir")
    FS_TYPE_TO_NAME(MKDIR, "mkdir")
    FS_TYPE_TO_NAME(MKDTEMP, "mkdtemp")
    FS_TYPE_TO_NAME(RENAME, "rename")
    FS_TYPE_TO_NAME(SCANDIR, "scandir")
    FS_TYPE_TO_NAME(LINK, "link")
    FS_TYPE_TO_NAME(SYMLINK, "symlink")
    FS_TYPE_TO_NAME(READLINK, "readlink")
    FS_TYPE_TO_NAME(CHOWN, "chown")
    FS_TYPE_TO_NAME(FCHOWN, "fchown")
    FS_TYPE_TO_NAME(REALPATH, "realpath")
    FS_TYPE_TO_NAME(COPYFILE, "copyfile")
    FS_TYPE_TO_NAME(LCHOWN, "lchown")
    FS_TYPE_TO_NAME(STATFS, "statfs")
    FS_TYPE_TO_NAME(MKSTEMP, "mkstemp")
    FS_TYPE_TO_NAME(LUTIME, "lutime")
#undef FS_TYPE_TO_NAME
    default:
      return "unknown";
  }
}
```

<a id="ref-q1-21"></a>
### [21] `doc/api/fs.md:2024-2029`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2024-L2029)

```markdown
The `mode` argument is an optional integer that specifies the accessibility
checks to be performed. `mode` should be either the value `fs.constants.F_OK`
or a mask consisting of the bitwise OR of any of `fs.constants.R_OK`,
`fs.constants.W_OK`, and `fs.constants.X_OK` (e.g.
`fs.constants.W_OK | fs.constants.R_OK`). Check [File access constants][] for
possible values of `mode`.
```

<a id="ref-q1-22"></a>
### [22] `doc/api/fs.md:5373-5378`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L5373-L5378)

```markdown
Synchronously tests a user's permissions for the file or directory specified
by `path`. The `mode` argument is an optional integer that specifies the
accessibility checks to be performed. `mode` should be either the value
`fs.constants.F_OK` or a mask consisting of the bitwise OR of any of
`fs.constants.R_OK`, `fs.constants.W_OK`, and `fs.constants.X_OK` (e.g.
`fs.constants.W_OK | fs.constants.R_OK`). Check [File access constants][] for
```

<a id="ref-q1-23"></a>
### [23] `doc/api/fs.md:1991-1994`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1991-L1994)

```markdown
  - version: v25.0.0
    pr-url: https://github.com/nodejs/node/pull/55862
    description: The constants `fs.F_OK`, `fs.R_OK`, `fs.W_OK` and `fs.X_OK`
                 which were present directly on `fs` are removed.
```

<a id="ref-q1-24"></a>
### [24] `test/parallel/test-fs-access.js:66-69`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-access.js#L66-L69)

```javascript
assert.strictEqual(typeof fs.constants.F_OK, 'number');
assert.strictEqual(typeof fs.constants.R_OK, 'number');
assert.strictEqual(typeof fs.constants.W_OK, 'number');
assert.strictEqual(typeof fs.constants.X_OK, 'number');
```

<a id="ref-q1-25"></a>
### [25] `lib/fs.js:221`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L221)

```javascript
  callback = makeCallback(callback);
```

<a id="ref-q1-26"></a>
### [26] `doc/api/fs.md:1153-1154`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1153-L1154)

```markdown
This method is only implemented on macOS.
```

<a id="ref-q1-27"></a>
### [27] `lib/fs.js:3248-3249`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3248-L3249)

```javascript
  lchmod: constants.O_SYMLINK !== undefined ? lchmod : undefined,
  lchmodSync: constants.O_SYMLINK !== undefined ? lchmodSync : undefined,
```

<a id="ref-q1-28"></a>
### [28] `lib/internal/fs/promises.js:1084-1086`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L1084-L1086)

```javascript
async function lchmod(path, mode) {
  if (O_SYMLINK === undefined)
    throw new ERR_METHOD_NOT_IMPLEMENTED('lchmod()');
```

<a id="ref-q1-29"></a>
### [29] `doc/api/deprecations.md:878-882`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/deprecations.md#L878-L882)

```markdown
Type: Deprecation revoked

The [`fs.lchown(path, uid, gid, callback)`][] API was deprecated. The
deprecation was revoked because the requisite supporting APIs were added in
libuv.
```
