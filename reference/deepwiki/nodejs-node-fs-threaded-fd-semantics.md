# DeepWiki Response

Repository: `nodejs/node`
Question: How does Node.js implement and document fs numeric file descriptors and fs.promises FileHandle objects across worker_threads? Please cover structured clone and transfer semantics for FileHandle, whether numeric fds are valid across workers, what happens to source FileHandle.fd after transfer, relevant source files/tests, and implications for a runtime that uses thread-local fd registries.
Search: https://deepwiki.com/search/how-does-nodejs-implement-and_be1aa6af-db0b-484e-8bfe-b0b16662f732

**Question**: How does Node.js implement and document fs numeric file descriptors and fs.promises FileHandle objects across worker_threads? Please cover structured clone and transfer semantics for FileHandle, whether numeric fds are valid across workers, what happens to source FileHandle.fd after transfer, relevant source files/tests, and implications for a runtime that uses thread-local fd registries.

Node.js implements `fs.promises.FileHandle` objects to be transferable across `worker_threads` using structured cloning and transfer semantics, while numeric file descriptors (`fd`s) are not directly valid across workers [1](#ref-q1-1) . When a `FileHandle` is transferred, its underlying numeric `fd` is effectively moved, rendering the original `FileHandle.fd` unusable in the source worker [2](#ref-q1-2) .

### `FileHandle` Transfer Semantics

The `FileHandle` object in Node.js is designed to be transferable between `worker_threads` [3](#ref-q1-3)  [4](#ref-q1-4) . This is achieved through the implementation of `kTransfer` and `kDeserialize` symbols within the `FileHandle` class [5](#ref-q1-5) .

When `port.postMessage(fh, [fh])` is called to transfer a `FileHandle` instance `fh` [6](#ref-q1-6) :
1.  The `[kTransfer]()` method of the `FileHandle` is invoked [7](#ref-q1-7) .
2.  Inside `[kTransfer]()`, the internal `kHandle` (which holds the native file handle) is extracted, and the `kFd` property of the source `FileHandle` is set to `-1` [8](#ref-q1-8) . This marks the original `FileHandle` as unusable.
3.  A `DataCloneError` is thrown if the `FileHandle` is currently in use (e.g., has an ongoing read or close operation) or has multiple references [9](#ref-q1-9) . This is demonstrated in tests where attempts to transfer a `FileHandle` with a pending `readFile()` or `close()` operation fail [10](#ref-q1-10)  [11](#ref-q1-11) .
4.  The `kHandle` is then included in the `transferList` for the `postMessage` operation [12](#ref-q1-12) .
5.  In the receiving worker, a new `FileHandle` object is created, and its `[kDeserialize]()` method is called [13](#ref-q1-13) .
6.  The `[kDeserialize]()` method receives the transferred `handle` and uses it to initialize the new `FileHandle` instance, setting its `kHandle` and `kFd` properties [14](#ref-q1-14) .

The underlying C++ implementation handles the actual transfer of the file descriptor. The `FileHandle::TransferForMessaging()` method in `src/node_file.cc` is responsible for preparing the `FileHandle` for transfer [15](#ref-q1-15)  [16](#ref-q1-16) . It creates a `TransferData` object containing the numeric file descriptor (`fd_`) and marks the original `FileHandle` as closed (`closed_ = true`) [17](#ref-q1-17) . The `TransferData` object's destructor ensures that if the `fd_` is still valid (meaning it wasn't successfully transferred or deserialized), it will be closed [18](#ref-q1-18) . The `FileHandle::TransferData::Deserialize()` method then reconstructs the `FileHandle` in the target worker using the transferred `fd` [19](#ref-q1-19) .

### Numeric File Descriptors Across Workers

Numeric file descriptors (`fd`s) themselves are generally thread-local and not directly valid across different worker threads in a runtime that uses thread-local `fd` registries. Instead, Node.js transfers the *ownership* of the underlying file descriptor. When a `FileHandle` is transferred, the numeric `fd` associated with the original `FileHandle` becomes invalid in the sending thread [2](#ref-q1-2) . Any subsequent attempts to use the original `FileHandle` in the sending thread will result in an `EBADF` error (Bad File Descriptor) [20](#ref-q1-20)  [21](#ref-q1-21) . The `fd` is then associated with the new `FileHandle` object in the receiving thread.

### Documentation and Source Files

The documentation for `worker_threads` explicitly lists `FileHandle`s as one of the native (C++-backed) objects that can be transferred [22](#ref-q1-22)  [23](#ref-q1-23) . It also clarifies that after transferring, the objects are not usable on the sending side [24](#ref-q1-24) .

Key source files involved in this functionality include:
*   `lib/internal/fs/promises.js`: Defines the JavaScript `FileHandle` class and its `kTransfer` and `kDeserialize` methods [25](#ref-q1-25)  [26](#ref-q1-26) .
*   `src/node_file.h` and `src/node_file.cc`: Contain the C++ implementation of the `FileHandle` class, including `GetTransferMode()` and `TransferForMessaging()` methods that manage the underlying file descriptor during transfer [27](#ref-q1-27)  [28](#ref-q1-28) .
*   `lib/internal/worker/js_transferable.js`: Provides the `markTransferMode` function and the deserialization setup for JavaScript-based transferable objects [29](#ref-q1-29)  [30](#ref-q1-30) .
*   `test/parallel/test-worker-message-port-transfer-filehandle.js`: Contains tests that verify the transfer behavior of `FileHandle` objects, including assertions about the `fd` value before and after transfer, and error handling for in-use `FileHandle`s [31](#ref-q1-31)  [32](#ref-q1-32)  [33](#ref-q1-33) .

## Notes

The `doc/api/fs.md` file documents the `filehandle.fd` property as the numeric file descriptor managed by the `FileHandle` object [34](#ref-q1-34) . While this document describes the property, it does not detail the cross-worker transfer semantics, which are covered in the `worker_threads` documentation and implementation.

Wiki pages you might want to explore:
- [Core APIs (nodejs/node)](/wiki/nodejs/node#3)

View this search on DeepWiki: https://deepwiki.com/search/how-does-nodejs-implement-and_be1aa6af-db0b-484e-8bfe-b0b16662f732

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/worker_threads.md:1268-1273`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1268-L1273)

```markdown
`transferList` may be a list of {ArrayBuffer}, [`MessagePort`][], and
[`FileHandle`][] objects.
After transferring, they are not usable on the sending side of the channel
anymore (even if they are not contained in `value`). Unlike with
[child processes][], transferring handles such as network sockets is currently
not supported.
```

<a id="ref-q1-2"></a>
### [2] `lib/internal/fs/promises.js:387-388`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L387-L388)

```javascript
    this[kFd] = -1;
    this[kCloseReason] = 'The FileHandle has been transferred';
```

<a id="ref-q1-3"></a>
### [3] `doc/changelogs/CHANGELOG_V12.md:1266`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/changelogs/CHANGELOG_V12.md#L1266)

```markdown
* \[[`dd51ba3f93`](https://github.com/nodejs/node/commit/dd51ba3f93)] - **(SEMVER-MINOR)** **worker,fs**: make FileHandle transferable (Anna Henningsen) [#33772](https://github.com/nodejs/node/pull/33772)
```

<a id="ref-q1-4"></a>
### [4] `doc/changelogs/CHANGELOG_V14.md:3604`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/changelogs/CHANGELOG_V14.md#L3604)

```markdown
* \[[`5b1fd10048`](https://github.com/nodejs/node/commit/5b1fd10048)] - **(SEMVER-MINOR)** **worker,fs**: make FileHandle transferable (Anna Henningsen) [#33772](https://github.com/nodejs/node/pull/33772)
```

<a id="ref-q1-5"></a>
### [5] `lib/internal/fs/promises.js:118-120`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L118-L120)

```javascript
const {
  kDeserialize, kTransfer, kTransferList, markTransferMode,
} = require('internal/worker/js_transferable');
```

<a id="ref-q1-6"></a>
### [6] `test/parallel/test-worker-message-port-transfer-filehandle.js:24`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L24)

```javascript
  port1.postMessage(fh, [ fh ]);
```

<a id="ref-q1-7"></a>
### [7] `lib/internal/fs/promises.js:380`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L380)

```javascript
  [kTransfer]() {
```

<a id="ref-q1-8"></a>
### [8] `lib/internal/fs/promises.js:386-388`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L386-L388)

```javascript
    const handle = this[kHandle];
    this[kFd] = -1;
    this[kCloseReason] = 'The FileHandle has been transferred';
```

<a id="ref-q1-9"></a>
### [9] `lib/internal/fs/promises.js:381-383`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L381-L383)

```javascript
    if (this[kClosePromise] || this[kRefs] > 1) {
      throw lazyDOMException('Cannot transfer FileHandle while in use',
                             'DataCloneError');
```

<a id="ref-q1-10"></a>
### [10] `test/parallel/test-worker-message-port-transfer-filehandle.js:85-91`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L85-L91)

```javascript
  const readPromise = fh.readFile();
  assert.throws(() => {
    port1.postMessage(fh, [fh]);
  }, {
    message: 'Cannot transfer FileHandle while in use',
    name: 'DataCloneError'
  });
```

<a id="ref-q1-11"></a>
### [11] `test/parallel/test-worker-message-port-transfer-filehandle.js:103-109`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L103-L109)

```javascript
  const closePromise = fh.close();
  assert.throws(() => {
    port1.postMessage(fh, [fh]);
  }, {
    message: 'Cannot transfer FileHandle while in use',
    name: 'DataCloneError'
  });
```

<a id="ref-q1-12"></a>
### [12] `lib/internal/fs/promises.js:399`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L399)

```javascript
    return [ this[kHandle] ];
```

<a id="ref-q1-13"></a>
### [13] `lib/internal/fs/promises.js:402`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L402)

```javascript
  [kDeserialize]({ handle }) {
```

<a id="ref-q1-14"></a>
### [14] `lib/internal/fs/promises.js:403-404`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L403-L404)

```javascript
    this[kHandle] = handle;
    this[kFd] = handle.fd;
```

<a id="ref-q1-15"></a>
### [15] `src/node_file.h:378`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.h#L378)

```c
  std::unique_ptr<worker::TransferData> TransferForMessaging() override;
```

<a id="ref-q1-16"></a>
### [16] `src/node_file.cc:310-315`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L310-L315)

```cpp
std::unique_ptr<worker::TransferData> FileHandle::TransferForMessaging() {
  CHECK_NE(GetTransferMode(), TransferMode::kDisallowCloneAndTransfer);
  auto ret = std::make_unique<TransferData>(fd_);
  closed_ = true;
  return ret;
}
```

<a id="ref-q1-17"></a>
### [17] `src/node_file.cc:312-313`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L312-L313)

```cpp
  auto ret = std::make_unique<TransferData>(fd_);
  closed_ = true;
```

<a id="ref-q1-18"></a>
### [18] `src/node_file.cc:319-327`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L319-L327)

```cpp
FileHandle::TransferData::~TransferData() {
  if (fd_ > 0) {
    uv_fs_t close_req;
    CHECK_NE(fd_, -1);
    FS_SYNC_TRACE_BEGIN(close);
    CHECK_EQ(0, uv_fs_close(nullptr, &close_req, fd_, nullptr));
    FS_SYNC_TRACE_END(close);
    uv_fs_req_cleanup(&close_req);
  }
```

<a id="ref-q1-19"></a>
### [19] `src/node_file.cc:330-340`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L330-L340)

```cpp
BaseObjectPtr<BaseObject> FileHandle::TransferData::Deserialize(
    Environment* env,
    v8::Local<v8::Context> context,
    std::unique_ptr<worker::TransferData> self) {
  BindingData* bd = Realm::GetBindingData<BindingData>(context);
  if (bd == nullptr) return {};

  int fd = fd_;
  fd_ = -1;
  return BaseObjectPtr<BaseObject> { FileHandle::New(bd, fd) };
}
```

<a id="ref-q1-20"></a>
### [20] `test/parallel/test-worker-message-port-transfer-filehandle.js:33`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L33)

```javascript
  await assert.rejects(() => fh.readFile(), { code: 'EBADF' });
```

<a id="ref-q1-21"></a>
### [21] `test/parallel/test-worker-message-port-transfer-filehandle.js:72-76`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L72-L76)

```javascript
  await assert.rejects(() => fh.read(), {
    code: 'EBADF',
    message: 'The FileHandle has been transferred',
    syscall: 'read'
  });
```

<a id="ref-q1-22"></a>
### [22] `doc/api/worker_threads.md:1234`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1234)

```markdown
  * {FileHandle}s,
```

<a id="ref-q1-23"></a>
### [23] `doc/api/worker_threads.md:1268-1269`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1268-L1269)

```markdown
`transferList` may be a list of {ArrayBuffer}, [`MessagePort`][], and
[`FileHandle`][] objects.
```

<a id="ref-q1-24"></a>
### [24] `doc/api/worker_threads.md:1270-1271`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L1270-L1271)

```markdown
After transferring, they are not usable on the sending side of the channel
anymore (even if they are not contained in `value`). Unlike with
```

<a id="ref-q1-25"></a>
### [25] `lib/internal/fs/promises.js:151-163`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L151-L163)

```javascript
class FileHandle extends EventEmitter {
  /**
   * @param {InternalFSBinding.FileHandle | undefined} filehandle
   */
  constructor(filehandle) {
    super();
    markTransferMode(this, false, true);
    this[kHandle] = filehandle;
    this[kFd] = filehandle ? filehandle.fd : -1;

    this[kRefs] = 1;
    this[kClosePromise] = null;
  }
```

<a id="ref-q1-26"></a>
### [26] `lib/internal/fs/promises.js:380-405`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/promises.js#L380-L405)

```javascript
  [kTransfer]() {
    if (this[kClosePromise] || this[kRefs] > 1) {
      throw lazyDOMException('Cannot transfer FileHandle while in use',
                             'DataCloneError');
    }

    const handle = this[kHandle];
    this[kFd] = -1;
    this[kCloseReason] = 'The FileHandle has been transferred';
    this[kHandle] = null;
    this[kRefs] = 0;

    return {
      data: { handle },
      deserializeInfo: 'internal/fs/promises:FileHandle',
    };
  }

  [kTransferList]() {
    return [ this[kHandle] ];
  }

  [kDeserialize]({ handle }) {
    this[kHandle] = handle;
    this[kFd] = handle.fd;
  }
```

<a id="ref-q1-27"></a>
### [27] `src/node_file.h:377-379`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.h#L377-L379)

```c
  BaseObject::TransferMode GetTransferMode() const override;
  std::unique_ptr<worker::TransferData> TransferForMessaging() override;
```

<a id="ref-q1-28"></a>
### [28] `src/node_file.cc:304-315`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_file.cc#L304-L315)

```cpp
BaseObject::TransferMode FileHandle::GetTransferMode() const {
  return reading_ || closing_ || closed_
             ? TransferMode::kDisallowCloneAndTransfer
             : TransferMode::kTransferable;
}

std::unique_ptr<worker::TransferData> FileHandle::TransferForMessaging() {
  CHECK_NE(GetTransferMode(), TransferMode::kDisallowCloneAndTransfer);
  auto ret = std::make_unique<TransferData>(fd_);
  closed_ = true;
  return ret;
}
```

<a id="ref-q1-29"></a>
### [29] `lib/internal/worker/js_transferable.js:33-50`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/js_transferable.js#L33-L50)

```javascript
function setup() {
  // Register the handler that will be used when deserializing JS-based objects
  // from .postMessage() calls. The format of `deserializeInfo` is generally
  // 'module:Constructor', e.g. 'internal/fs/promises:FileHandle'.
  setDeserializerCreateObjectFunction((deserializeInfo) => {
    const { 0: module, 1: ctor } = StringPrototypeSplit(deserializeInfo, ':', 2);
    const Ctor = require(module)[ctor];
    if (typeof Ctor !== 'function' ||
        typeof Ctor.prototype[messaging_deserialize_symbol] !== 'function') {
      // Not one of the official errors because one should not be able to get
      // here without messing with Node.js internals.
      // eslint-disable-next-line no-restricted-syntax
      throw new Error(`Unknown deserialize spec ${deserializeInfo}`);
    }

    return new Ctor();
  });
}
```

<a id="ref-q1-30"></a>
### [30] `lib/internal/worker/js_transferable.js:91`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/worker/js_transferable.js#L91)

```javascript
function markTransferMode(obj, cloneable = false, transferable = false) {
```

<a id="ref-q1-31"></a>
### [31] `test/parallel/test-worker-message-port-transfer-filehandle.js:10-34`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L10-L34)

```javascript
  const fh = await fs.open(__filename);

  const { port1, port2 } = new MessageChannel();

  assert.throws(() => {
    port1.postMessage(fh);
  }, {
    constructor: DOMException,
    name: 'DataCloneError',
    code: 25,
  });

  // Check that transferring FileHandle instances works.
  assert.notStrictEqual(fh.fd, -1);
  port1.postMessage(fh, [ fh ]);
  assert.strictEqual(fh.fd, -1);

  const [ fh2 ] = await once(port2, 'message');
  assert.strictEqual(Object.getPrototypeOf(fh2), Object.getPrototypeOf(fh));

  assert.deepStrictEqual(await fh2.readFile(), await fs.readFile(__filename));
  await fh2.close();

  await assert.rejects(() => fh.readFile(), { code: 'EBADF' });
})().then(common.mustCall());
```

<a id="ref-q1-32"></a>
### [32] `test/parallel/test-worker-message-port-transfer-filehandle.js:79-95`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L79-L95)

```javascript
(async function() {
  // Check that a FileHandle with a read in progress cannot be transferred.
  const fh = await fs.open(__filename);

  const { port1 } = new MessageChannel();

  const readPromise = fh.readFile();
  assert.throws(() => {
    port1.postMessage(fh, [fh]);
  }, {
    message: 'Cannot transfer FileHandle while in use',
    name: 'DataCloneError'
  });

  assert.deepStrictEqual(await readPromise, await fs.readFile(__filename));
  await fh.close();
})().then(common.mustCall());
```

<a id="ref-q1-33"></a>
### [33] `test/parallel/test-worker-message-port-transfer-filehandle.js:97-111`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-worker-message-port-transfer-filehandle.js#L97-L111)

```javascript
(async function() {
  // Check that filehandles with a close in progress cannot be transferred.
  const fh = await fs.open(__filename);

  const { port1 } = new MessageChannel();

  const closePromise = fh.close();
  assert.throws(() => {
    port1.postMessage(fh, [fh]);
  }, {
    message: 'Cannot transfer FileHandle while in use',
    name: 'DataCloneError'
  });
  await closePromise;
})().then(common.mustCall());
```

<a id="ref-q1-34"></a>
### [34] `doc/api/fs.md:372-378`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L372-L378)

```markdown
#### `filehandle.fd`

<!-- YAML
added: v10.0.0
-->

* Type: {number} The numeric file descriptor managed by the {FileHandle} object.
```
