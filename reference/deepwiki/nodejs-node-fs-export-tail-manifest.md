# DeepWiki Response

Repository: `nodejs/node`
Question: For Node.js current fs module exports, what are the enumerable node:fs namespace tail exports and related implementation/docs references for Dir, Dirent, Stats, ReadStream, WriteStream, FileReadStream, FileWriteStream, Utf8Stream, _toUnixTimestamp, mkdtempDisposableSync, openAsBlob, constants, promises, and fs/promises mkdtempDisposable/constants? Also note whether StatFs, FSWatcher, StatWatcher and FileHandle pull/pullSync/writer are namespace exports.
Search: https://deepwiki.com/search/for-nodejs-current-fs-module-e_8c9d6296-4902-4b7e-ad21-1a68a5a0b44c

**Question**: For Node.js current fs module exports, what are the enumerable node:fs namespace tail exports and related implementation/docs references for Dir, Dirent, Stats, ReadStream, WriteStream, FileReadStream, FileWriteStream, Utf8Stream, _toUnixTimestamp, mkdtempDisposableSync, openAsBlob, constants, promises, and fs/promises mkdtempDisposable/constants? Also note whether StatFs, FSWatcher, StatWatcher and FileHandle pull/pullSync/writer are namespace exports.

The `node:fs` module exports several classes and utilities directly, and also exposes `constants` and `promises` as enumerable properties. The `fs/promises` module further exports its own `constants` and `mkdtempDisposable`. [1](#ref-q1-1)

### Enumerable `node:fs` Namespace Tail Exports

The following are enumerable tail exports from the `node:fs` module:
*   `Dir`: This class is lazily defined and exported. [2](#ref-q1-2)
*   `Dirent`: This class is directly imported from `internal/fs/utils` and re-exported. [3](#ref-q1-3)  [4](#ref-q1-4)
*   `Stats`: This class is imported from `internal/fs/utils` and re-exported. [5](#ref-q1-5)  [6](#ref-q1-6)  It is also deprecated. [7](#ref-q1-7)
*   `ReadStream`: This class is lazily loaded and exported. [8](#ref-q1-8)
*   `WriteStream`: This class is lazily loaded and exported. [9](#ref-q1-9)
*   `FileReadStream`: This is a legacy name for `ReadStream` and is lazily loaded and exported. [10](#ref-q1-10)
*   `FileWriteStream`: This is a legacy name for `WriteStream` and is lazily loaded and exported. [11](#ref-q1-11)
*   `Utf8Stream`: This is lazily loaded and exported. [12](#ref-q1-12)
*   `_toUnixTimestamp`: This is exported for testing purposes. [13](#ref-q1-13)
*   `mkdtempDisposableSync`: This function is directly exported. [14](#ref-q1-14)  Documentation for this function is available. [15](#ref-q1-15)
*   `openAsBlob`: This function is directly exported. [16](#ref-q1-16)  Documentation for this function is available. [17](#ref-q1-17)
*   `constants`: This object contains commonly used file system constants and is an enumerable property of `fs`. [18](#ref-q1-18)
*   `promises`: This object provides promise-based asynchronous file system methods and is an enumerable property of `fs`. [19](#ref-q1-19)

### `fs/promises` Exports

The `fs/promises` module exports its own `constants` object [20](#ref-q1-20)  and `mkdtempDisposable` function. [21](#ref-q1-21)  The `mkdtempDisposable` function returns a Promise for an async-disposable object. [22](#ref-q1-22)

### Other `fs` Related Classes

*   `StatFs`: This class is not directly exported as a namespace tail export from `node:fs`. However, `statfs` and `statfsSync` functions are exported. [23](#ref-q1-23)  The `getStatFsFromBinding` function is used internally. [24](#ref-q1-24)
*   `FSWatcher`: This class is not a direct namespace export from `node:fs`.
*   `StatWatcher`: This class is not a direct namespace export from `node:fs`. It is exposed via `internalBinding('fs')`. [25](#ref-q1-25)
*   `FileHandle`: This class is not a direct namespace export from `node:fs`. It is created by `fsPromises.open()` [26](#ref-q1-26)  and is exposed via `internalBinding('fs')`. [27](#ref-q1-27)
    *   The `FileHandle` object does not have `pull`, `pullSync`, or `writer` methods as direct properties. It has methods like `appendFile` and `writeFile`. [28](#ref-q1-28)

## Notes

The `test/parallel/test-permission-fs-supported.js` file lists `Dir`, `Utf8Stream`, `FileReadStream`, `FileWriteStream`, `_toUnixTimestamp`, `Stats`, `ReadStream`, `WriteStream`, and `Dirent` in an `ignoreList` for permission model testing, indicating they are not functions that directly interact with the permission model. [29](#ref-q1-29)  This file also explicitly lists `mkdtempDisposableSync` and `openAsBlob` as supported APIs for permission model testing. [30](#ref-q1-30)  [31](#ref-q1-31)

Wiki pages you might want to explore:
- [Core APIs (nodejs/node)](/wiki/nodejs/node#3)

View this search on DeepWiki: https://deepwiki.com/search/for-nodejs-current-fs-module-e_8c9d6296-4902-4b7e-ad21-1a68a5a0b44c

## References

<a id="ref-q1-1"></a>
### [1] `lib/fs.js:3160-3376`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3160-L3376)

<a id="ref-q1-2"></a>
### [2] `lib/fs.js:3353-3357`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3353-L3357)

```javascript
defineLazyProperties(
  fs,
  'internal/fs/dir',
  ['Dir', 'opendir', 'opendirSync'],
);
```

<a id="ref-q1-3"></a>
### [3] `lib/fs.js:105`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L105)

```javascript
  Dirent,
```

<a id="ref-q1-4"></a>
### [4] `lib/fs.js:3303`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3303)

```javascript
  Dirent,
```

<a id="ref-q1-5"></a>
### [5] `lib/fs.js:113`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L113)

```javascript
  Stats,
```

<a id="ref-q1-6"></a>
### [6] `lib/fs.js:3304`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3304)

```javascript
  Stats,
```

<a id="ref-q1-7"></a>
### [7] `lib/internal/fs/utils.js:943`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/utils.js#L943)

```javascript
  Stats: deprecate(Stats, 'fs.Stats constructor is deprecated.', 'DEP0180'),
```

<a id="ref-q1-8"></a>
### [8] `lib/fs.js:3306-3309`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3306-L3309)

```javascript
  get ReadStream() {
    lazyLoadStreams();
    return ReadStream;
  },
```

<a id="ref-q1-9"></a>
### [9] `lib/fs.js:3315-3318`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3315-L3318)

```javascript
  get WriteStream() {
    lazyLoadStreams();
    return WriteStream;
  },
```

<a id="ref-q1-10"></a>
### [10] `lib/fs.js:3326-3329`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3326-L3329)

```javascript
  get FileReadStream() {
    lazyLoadStreams();
    return FileReadStream;
  },
```

<a id="ref-q1-11"></a>
### [11] `lib/fs.js:3335-3338`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3335-L3338)

```javascript
  get FileWriteStream() {
    lazyLoadStreams();
    return FileWriteStream;
  },
```

<a id="ref-q1-12"></a>
### [12] `lib/fs.js:3344-3346`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3344-L3346)

```javascript
  get Utf8Stream() {
    lazyLoadUtf8Stream();
    return Utf8Stream;
```

<a id="ref-q1-13"></a>
### [13] `lib/fs.js:3350`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3350)

```javascript
  _toUnixTimestamp: toUnixTimestamp,
```

<a id="ref-q1-14"></a>
### [14] `lib/fs.js:3260`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3260)

```javascript
  mkdtempDisposableSync,
```

<a id="ref-q1-15"></a>
### [15] `doc/api/fs.md:5965-5978`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L5965-L5978)

```markdown
### `fs.mkdtempDisposableSync(prefix[, options])`

<!-- YAML
added: v24.4.0
-->

* `prefix` {string|Buffer|URL}
* `options` {string|Object}
  * `encoding` {string} **Default:** `'utf8'`
* Returns: {Object} A disposable object:
  * `path` {string} The path of the created directory.
  * `remove` {Function} A function which removes the created directory.
  * `[Symbol.dispose]` {Function} The same as `remove`.
```

<a id="ref-q1-16"></a>
### [16] `lib/fs.js:3263`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3263)

```javascript
  openAsBlob,
```

<a id="ref-q1-17"></a>
### [17] `doc/api/fs.md:3651-3668`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L3651-L3668)

```markdown
### `fs.openAsBlob(path[, options])`

<!-- YAML
added: v19.8.0
changes:
  - version:
      - v24.0.0
      - v22.17.0
    pr-url: https://github.com/nodejs/node/pull/57513
    description: Marking the API stable.
-->

* `path` {string|Buffer|URL}
* `options` {Object}
  * `type` {string} An optional mime type for the blob.
* Returns: {Promise} Fulfills with a {Blob} upon success.

Returns a {Blob} whose data is backed by the given file.
```

<a id="ref-q1-18"></a>
### [18] `lib/fs.js:3360-3365`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3360-L3365)

```javascript
  constants: {
    __proto__: null,
    configurable: false,
    enumerable: true,
    value: constants,
  },
```

<a id="ref-q1-19"></a>
### [19] `lib/fs.js:3366-3374`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3366-L3374)

```javascript
  promises: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      promises ??= require('internal/fs/promises').exports;
      return promises;
    },
  },
```

<a id="ref-q1-20"></a>
### [20] `lib/internal/fs/promises.js:1333`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L1333)

```javascript
    constants,
```

<a id="ref-q1-21"></a>
### [21] `lib/internal/fs/promises.js:1328`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L1328)

```javascript
    mkdtempDisposable,
```

<a id="ref-q1-22"></a>
### [22] `doc/api/fs.md:1334-1338`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1334-L1338)

```markdown
* Returns: {Promise} Fulfills with a Promise for an async-disposable Object:
  * `path` {string} The path of the created directory.
  * `remove` {AsyncFunction} A function which removes the created directory.
  * `[Symbol.asyncDispose]` {AsyncFunction} The same as `remove`.
```

<a id="ref-q1-23"></a>
### [23] `lib/fs.js:3283-3285`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L3283-L3285)

```javascript
  statfs,
  statSync,
  statfsSync,
```

<a id="ref-q1-24"></a>
### [24] `lib/fs.js:114`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/fs.js#L114)

```javascript
  getStatFsFromBinding,
```

<a id="ref-q1-25"></a>
### [25] `typings/internalBinding/fs.d.ts:257`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/typings/internalBinding/fs.d.ts#L257)

```typescript
  StatWatcher: typeof InternalFSBinding.StatWatcher;
```

<a id="ref-q1-26"></a>
### [26] `doc/api/fs.md:157-158`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L157-L158)

```markdown

Instances of the {FileHandle} object are created by the `fsPromises.open()`
```

<a id="ref-q1-27"></a>
### [27] `typings/internalBinding/fs.d.ts:249`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/typings/internalBinding/fs.d.ts#L249)

```typescript
  FileHandle: typeof InternalFSBinding.FileHandle;
```

<a id="ref-q1-28"></a>
### [28] `doc/api/fs.md:178-205`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L178-L205)

```markdown
#### `filehandle.appendFile(data[, options])`

<!-- YAML
added: v10.0.0
changes:
  - version:
    - v21.1.0
    - v20.10.0
    pr-url: https://github.com/nodejs/node/pull/50095
    description: The `flush` option is now supported.
  - version:
      - v15.14.0
      - v14.18.0
    pr-url: https://github.com/nodejs/node/pull/37490
    description: The `data` argument supports `AsyncIterable`, `Iterable`, and `Stream`.
  - version: v14.0.0
    pr-url: https://github.com/nodejs/node/pull/31030
    description: The `data` parameter won't coerce unsupported input to
                 strings anymore.
-->

* `data` {string|Buffer|TypedArray|DataView|AsyncIterable|Iterable|Stream}
* `options` {Object|string}
  * `encoding` {string|null} **Default:** `'utf8'`
  * `signal` {AbortSignal|undefined} allows aborting an in-progress writeFile. **Default:** `undefined`
* Returns: {Promise} Fulfills with `undefined` upon success.

Alias of [`filehandle.writeFile()`][].
```

<a id="ref-q1-29"></a>
### [29] `test/parallel/test-permission-fs-supported.js:69-80`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-permission-fs-supported.js#L69-L80)

```javascript
  'Dir',
  // the Utf8Stream is implemented in terms of functions
  // on the fs module that have permission checks, so we don't
  // need to check it here.
  'Utf8Stream',
  'FileReadStream',
  'FileWriteStream',
  '_toUnixTimestamp',
  'Stats',
  'ReadStream',
  'WriteStream',
  'Dirent',
```

<a id="ref-q1-30"></a>
### [30] `test/parallel/test-permission-fs-supported.js:41`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-permission-fs-supported.js#L41)

```javascript
  'mkdtempDisposableSync',
```

<a id="ref-q1-31"></a>
### [31] `test/parallel/test-permission-fs-supported.js:39`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-permission-fs-supported.js#L39)

```javascript
  'openAsBlob',
```
