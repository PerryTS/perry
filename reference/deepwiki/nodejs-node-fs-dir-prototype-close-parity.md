# DeepWiki Response

Repository: `nodejs/node`
Question: How does Node fs.Dir expose path, read, close, entries, disposal symbols, and ERR_DIR_CLOSED behavior across sync, callback, promise, and async-iterator close paths? Please cite implementation and tests for prototype descriptors, own properties, Symbol.asyncIterator, Symbol.dispose, Symbol.asyncDispose, entries(), and repeated close behavior.
Search: https://deepwiki.com/search/how-does-node-fsdir-expose-pat_b595a73e-f656-46c8-b8b5-209bf0b1cfec

**Question**: How does Node fs.Dir expose path, read, close, entries, disposal symbols, and ERR_DIR_CLOSED behavior across sync, callback, promise, and async-iterator close paths? Please cite implementation and tests for prototype descriptors, own properties, Symbol.asyncIterator, Symbol.dispose, Symbol.asyncDispose, entries(), and repeated close behavior.

The Node.js `fs.Dir` class exposes directory information and operations through various methods and symbols, supporting synchronous, callback-based, promise-based, and async-iterator patterns for reading and closing directories. The `path` property provides the directory's path [1](#ref-q1-1) . Directory entries are read using `read()`, `readSync()`, or by iterating over `entries()` [2](#ref-q1-2)  [3](#ref-q1-3)  [4](#ref-q1-4) . The directory can be closed using `close()`, `closeSync()`, `Symbol.dispose`, or `Symbol.asyncDispose` [5](#ref-q1-5)  [6](#ref-q1-6)  [7](#ref-q1-7)  [8](#ref-q1-8) . Repeated attempts to read from or close a `Dir` instance after it has been closed will result in an `ERR_DIR_CLOSED` error [9](#ref-q1-9)  [10](#ref-q1-10)  [11](#ref-q1-11)  [12](#ref-q1-12) . Concurrent operations on the same `Dir` instance are prevented by `ERR_DIR_CONCURRENT_OPERATION` [13](#ref-q1-13)  [14](#ref-q1-14) .

## `path` Property
The `path` property of a `Dir` object is a getter that returns the string path of the directory it represents [1](#ref-q1-1) . It throws an `ERR_INVALID_THIS` error if invoked on an object that is not an instance of `Dir` [15](#ref-q1-15) .

## Reading Directory Entries

### `read()` (Callback and Promise)
The `read()` method allows asynchronous reading of directory entries [2](#ref-q1-2) .
- If a `callback` is provided, it uses the callback pattern [16](#ref-q1-16) . The callback is validated to be a function [17](#ref-q1-17) .
- If no `callback` is provided, it returns a Promise [18](#ref-q1-18) . This promise-based version is created using `util.promisify` on the internal `#readImpl` method [19](#ref-q1-19) .
- Both versions check if the directory is closed, throwing `ERR_DIR_CLOSED` if it is [9](#ref-q1-9) .
- Concurrent read operations are queued and processed sequentially [20](#ref-q1-20)  [21](#ref-q1-21) .

### `readSync()`
The `readSync()` method provides a synchronous way to read directory entries [3](#ref-q1-3) .
- It throws `ERR_DIR_CLOSED` if the directory is already closed [10](#ref-q1-10) .
- It throws `ERR_DIR_CONCURRENT_OPERATION` if an asynchronous operation is already in progress [13](#ref-q1-13) .

### `entries()` (Async Iterator)
The `entries()` method returns an async iterator, allowing `for await...of` loops to process directory entries [4](#ref-q1-4) .
- It internally uses the promise-based `read()` method [22](#ref-q1-22) .
- The `Dir.prototype[SymbolAsyncIterator]` is set to `Dir.prototype.entries` [23](#ref-q1-23) .
- The async iterator automatically calls `close()` when iteration finishes (e.g., `break`, `return`, or all entries are read) [24](#ref-q1-24)  [25](#ref-q1-25) . This behavior is tested in `test-fs-opendir.js` [26](#ref-q1-26)  [27](#ref-q1-27)  [28](#ref-q1-28) .

## Closing the Directory

### `close()` (Callback and Promise)
The `close()` method closes the directory handle [29](#ref-q1-29) .
- If a `callback` is provided, it uses the callback pattern [30](#ref-q1-30) .
- If no `callback` is provided, it returns a Promise [31](#ref-q1-31) . This promise-based version is created using `util.promisify` [32](#ref-q1-32) .
- If the directory is already closed, the promise version rejects with `ERR_DIR_CLOSED` [11](#ref-q1-11)  and the callback version calls the callback with `ERR_DIR_CLOSED` on `process.nextTick` [33](#ref-q1-33) .
- Concurrent close operations are queued [34](#ref-q1-34) .

### `closeSync()`
The `closeSync()` method synchronously closes the directory handle [35](#ref-q1-35) .
- It throws `ERR_DIR_CLOSED` if the directory is already closed [36](#ref-q1-36) .
- It throws `ERR_DIR_CONCURRENT_OPERATION` if an asynchronous operation is in progress [14](#ref-q1-14) .

### Disposal Symbols (`Symbol.dispose` and `Symbol.asyncDispose`)
The `Dir` class implements the `Symbol.dispose` and `Symbol.asyncDispose` symbols for explicit resource management [37](#ref-q1-37) .
- `[Symbol.dispose]()` calls `closeSync()` [38](#ref-q1-38) .
- `[Symbol.asyncDispose]()` calls the promise-based `close()` method [39](#ref-q1-39) .
- Both disposers are idempotent, meaning repeated calls do not throw errors [40](#ref-q1-40)  [41](#ref-q1-41) . This behavior is confirmed by tests [42](#ref-q1-42)  [43](#ref-q1-43) .
- The use of these symbols is encouraged, and allowing `fs.Dir` objects to be closed on garbage collection is deprecated [44](#ref-q1-44) .

## `ERR_DIR_CLOSED` Behavior
The `ERR_DIR_CLOSED` error is thrown or rejected when an operation is attempted on a `Dir` instance that has already been closed [9](#ref-q1-9)  [10](#ref-q1-10)  [11](#ref-q1-11) .
- Tests confirm this behavior for `read()`, `readSync()`, and `close()` [45](#ref-q1-45)  [46](#ref-q1-46)  [47](#ref-q1-47)  [48](#ref-q1-48)  [49](#ref-q1-49)  [50](#ref-q1-50)  [51](#ref-q1-51) .

## `ERR_DIR_CONCURRENT_OPERATION` Behavior
The `ERR_DIR_CONCURRENT_OPERATION` error is thrown when a synchronous operation (`readSync()` or `closeSync()`) is attempted while an asynchronous operation (`read()` or `close()`) is already in progress [13](#ref-q1-13)  [14](#ref-q1-14) . This prevents race conditions and ensures consistent state. This is tested in `test-fs-opendir.js` [52](#ref-q1-52) .

## Implementation Details
The `Dir` class is implemented in `lib/internal/fs/dir.js` [53](#ref-q1-53) . It uses private class fields for its internal state, such as `#handle`, `#path`, `#closed`, and `#operationQueue` [54](#ref-q1-54) . The underlying file system operations are handled by `internalBinding('fs_dir')` [55](#ref-q1-55)  which interfaces with C++ bindings in `src/node_dir.cc` [56](#ref-q1-56)  [57](#ref-q1-57) .

## Prototype Descriptors and Own Properties
The `Dir` class defines its methods and properties on its prototype.
- `path` is a getter property on the prototype [1](#ref-q1-1) .
- `read`, `readSync`, `close`, `closeSync`, and `entries` are methods on the prototype [2](#ref-q1-2)  [3](#ref-q1-3)  [5](#ref-q1-5)  [6](#ref-q1-6)  [4](#ref-q1-4) .
- `Symbol.asyncIterator` is defined on `Dir.prototype` and points to `Dir.prototype.entries` [23](#ref-q1-23) .
- `Symbol.dispose` and `Symbol.asyncDispose` are directly defined as methods on the `Dir` class prototype [7](#ref-q1-7)  [8](#ref-q1-8) .

## Repeated Close Behavior
The `close()` and `closeSync()` methods, as well as the disposal symbols, are designed to be idempotent. This means that calling them multiple times on an already closed `Dir` instance will not cause additional errors beyond the initial `ERR_DIR_CLOSED` if an operation is attempted after the first close [11](#ref-q1-11)  [36](#ref-q1-36) . The internal `#closed` flag prevents further operations <cite repo="nodejs/node"

Wiki pages you might want to explore:
- [Core APIs (nodejs/node)](/wiki/nodejs/node#3)

View this search on DeepWiki: https://deepwiki.com/search/how-does-node-fsdir-expose-pat_b595a73e-f656-46c8-b8b5-209bf0b1cfec

## References

<a id="ref-q1-1"></a>
### [1] `lib/internal/fs/dir.js:76-80`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L76-L80)

```javascript
  get path() {
    if (!(#path in this))
      throw new ERR_INVALID_THIS('Dir');
    return this.#path;
  }
```

<a id="ref-q1-2"></a>
### [2] `lib/internal/fs/dir.js:109-111`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L109-L111)

```javascript
  read(callback) {
    return arguments.length === 0 ? this.#readPromisified() : this.#readImpl(true, callback);
  }
```

<a id="ref-q1-3"></a>
### [3] `lib/internal/fs/dir.js:208`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L208)

```javascript
  readSync() {
```

<a id="ref-q1-4"></a>
### [4] `lib/internal/fs/dir.js:294-306`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L294-L306)

```javascript
  async* entries() {
    try {
      while (true) {
        const result = await this.#readPromisified();
        if (result === null) {
          break;
        }
        yield result;
      }
    } finally {
      await this.#closePromisified();
    }
  }
```

<a id="ref-q1-5"></a>
### [5] `lib/internal/fs/dir.js:243`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L243)

```javascript
  close(callback) {
```

<a id="ref-q1-6"></a>
### [6] `lib/internal/fs/dir.js:276`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L276)

```javascript
  closeSync() {
```

<a id="ref-q1-7"></a>
### [7] `lib/internal/fs/dir.js:308-311`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L308-L311)

```javascript
  [SymbolDispose]() {
    if (this.#closed) return;
    this.closeSync();
  }
```

<a id="ref-q1-8"></a>
### [8] `lib/internal/fs/dir.js:313-316`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L313-L316)

```javascript
  async [SymbolAsyncDispose]() {
    if (this.#closed) return;
    await this.#closePromisified();
  }
```

<a id="ref-q1-9"></a>
### [9] `lib/internal/fs/dir.js:114-116`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L114-L116)

```javascript
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }
```

<a id="ref-q1-10"></a>
### [10] `lib/internal/fs/dir.js:209-210`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L209-L210)

```javascript
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
```

<a id="ref-q1-11"></a>
### [11] `lib/internal/fs/dir.js:245-247`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L245-L247)

```javascript
      if (this.#closed === true) {
        return PromiseReject(new ERR_DIR_CLOSED());
      }
```

<a id="ref-q1-12"></a>
### [12] `test/parallel/test-fs-opendir.js:47-49`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L47-L49)

```javascript
const dirclosedError = {
  code: 'ERR_DIR_CLOSED'
};
```

<a id="ref-q1-13"></a>
### [13] `lib/internal/fs/dir.js:213-214`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L213-L214)

```javascript
    if (this.#operationQueue !== null) {
      throw new ERR_DIR_CONCURRENT_OPERATION();
```

<a id="ref-q1-14"></a>
### [14] `lib/internal/fs/dir.js:281-282`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L281-L282)

```javascript
    if (this.#operationQueue !== null) {
      throw new ERR_DIR_CONCURRENT_OPERATION();
```

<a id="ref-q1-15"></a>
### [15] `lib/internal/fs/dir.js:77-78`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L77-L78)

```javascript
    if (!(#path in this))
      throw new ERR_INVALID_THIS('Dir');
```

<a id="ref-q1-16"></a>
### [16] `lib/internal/fs/dir.js:110`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L110)

```javascript
    return arguments.length === 0 ? this.#readPromisified() : this.#readImpl(true, callback);
```

<a id="ref-q1-17"></a>
### [17] `lib/internal/fs/dir.js:122`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L122)

```javascript
    validateFunction(callback, 'callback');
```

<a id="ref-q1-18"></a>
### [18] `lib/internal/fs/dir.js:109-110`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L109-L110)

```javascript
  read(callback) {
    return arguments.length === 0 ? this.#readPromisified() : this.#readImpl(true, callback);
```

<a id="ref-q1-19"></a>
### [19] `lib/internal/fs/dir.js:70-71`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L70-L71)

```javascript
    this.#readPromisified = FunctionPrototypeBind(
      promisify(this.#readImpl), this, false);
```

<a id="ref-q1-20"></a>
### [20] `lib/internal/fs/dir.js:124-128`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L124-L128)

```javascript
    if (this.#operationQueue !== null) {
      ArrayPrototypePush(this.#operationQueue, () => {
        this.#readImpl(maybeSync, callback);
      });
      return;
```

<a id="ref-q1-21"></a>
### [21] `lib/internal/fs/dir.js:152-155`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L152-L155)

```javascript
        const queue = this.#operationQueue;
        this.#operationQueue = null;
        for (const op of queue) op();
      });
```

<a id="ref-q1-22"></a>
### [22] `lib/internal/fs/dir.js:297`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L297)

```javascript
        const result = await this.#readPromisified();
```

<a id="ref-q1-23"></a>
### [23] `lib/internal/fs/dir.js:319-326`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L319-L326)

```javascript
ObjectDefineProperties(Dir.prototype, {
  [SymbolAsyncIterator]: {
    __proto__: null,
    enumerable: false,
    writable: true,
    configurable: true,
    value: Dir.prototype.entries,
  },
```

<a id="ref-q1-24"></a>
### [24] `lib/internal/fs/dir.js:303-304`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L303-L304)

```javascript
    } finally {
      await this.#closePromisified();
```

<a id="ref-q1-25"></a>
### [25] `doc/api/fs.md:1433-1435`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1433-L1435)

```markdown
When using the async iterator, the {fs.Dir} object will be automatically
closed after the iterator exits.
```

<a id="ref-q1-26"></a>
### [26] `test/parallel/test-fs-opendir.js:175-182`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L175-L182)

```javascript
async function doAsyncIterBreakTest() {
  const dir = await fs.promises.opendir(testDir);
  for await (const dirent of dir) { // eslint-disable-line no-unused-vars
    break;
  }

  await assert.rejects(dir.read(), dirclosedError);
}
```

<a id="ref-q1-27"></a>
### [27] `test/parallel/test-fs-opendir.js:185-195`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L185-L195)

```javascript
async function doAsyncIterReturnTest() {
  const dir = await fs.promises.opendir(testDir);
  await (async function() {
    for await (const dirent of dir) {
      return;
    }
  })();

  await assert.rejects(dir.read(), dirclosedError);
}
doAsyncIterReturnTest().then(common.mustCall());
```

<a id="ref-q1-28"></a>
### [28] `test/parallel/test-fs-opendir.js:197-211`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L197-L211)

```javascript
async function doAsyncIterThrowTest() {
  const dir = await fs.promises.opendir(testDir);
  try {
    for await (const dirent of dir) { // eslint-disable-line no-unused-vars
      throw new Error('oh no');
    }
  } catch (err) {
    if (err.message !== 'oh no') {
      throw err;
    }
  }

  await assert.rejects(dir.read(), dirclosedError);
}
doAsyncIterThrowTest().then(common.mustCall());
```

<a id="ref-q1-29"></a>
### [29] `lib/internal/fs/dir.js:243-274`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L243-L274)

```javascript
  close(callback) {
    if (callback === undefined) {
      if (this.#closed === true) {
        return PromiseReject(new ERR_DIR_CLOSED());
      }
      return this.#closePromisified();
    }

    validateFunction(callback, 'callback');

    if (this.#closed === true) {
      process.nextTick(callback, new ERR_DIR_CLOSED());
      return;
    }

    if (this.#operationQueue !== null) {
      ArrayPrototypePush(this.#operationQueue, () => {
        this.close(callback);
      });
      return;
    }

    while (this.#handlerQueue.length > 0) {
      const handler = ArrayPrototypeShift(this.#handlerQueue);
      handler.handle.close();
    }

    this.#closed = true;
    const req = new FSReqCallback();
    req.oncomplete = callback;
    this.#handle.close(req);
  }
```

<a id="ref-q1-30"></a>
### [30] `lib/internal/fs/dir.js:251`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L251)

```javascript
    validateFunction(callback, 'callback');
```

<a id="ref-q1-31"></a>
### [31] `lib/internal/fs/dir.js:248`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L248)

```javascript
      return this.#closePromisified();
```

<a id="ref-q1-32"></a>
### [32] `lib/internal/fs/dir.js:72-73`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L72-L73)

```javascript
    this.#closePromisified = FunctionPrototypeBind(
      promisify(this.close), this);
```

<a id="ref-q1-33"></a>
### [33] `lib/internal/fs/dir.js:253-254`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L253-L254)

```javascript
    if (this.#closed === true) {
      process.nextTick(callback, new ERR_DIR_CLOSED());
```

<a id="ref-q1-34"></a>
### [34] `lib/internal/fs/dir.js:258-262`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L258-L262)

```javascript
    if (this.#operationQueue !== null) {
      ArrayPrototypePush(this.#operationQueue, () => {
        this.close(callback);
      });
      return;
```

<a id="ref-q1-35"></a>
### [35] `lib/internal/fs/dir.js:276-292`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L276-L292)

```javascript
  closeSync() {
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }

    if (this.#operationQueue !== null) {
      throw new ERR_DIR_CONCURRENT_OPERATION();
    }

    while (this.#handlerQueue.length > 0) {
      const handler = ArrayPrototypeShift(this.#handlerQueue);
      handler.handle.close();
    }

    this.#closed = true;
    this.#handle.close();
  }
```

<a id="ref-q1-36"></a>
### [36] `lib/internal/fs/dir.js:277-279`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L277-L279)

```javascript
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }
```

<a id="ref-q1-37"></a>
### [37] `lib/internal/fs/dir.js:308-316`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L308-L316)

```javascript
  [SymbolDispose]() {
    if (this.#closed) return;
    this.closeSync();
  }

  async [SymbolAsyncDispose]() {
    if (this.#closed) return;
    await this.#closePromisified();
  }
```

<a id="ref-q1-38"></a>
### [38] `lib/internal/fs/dir.js:310`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L310)

```javascript
    this.closeSync();
```

<a id="ref-q1-39"></a>
### [39] `lib/internal/fs/dir.js:315`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L315)

```javascript
    await this.#closePromisified();
```

<a id="ref-q1-40"></a>
### [40] `lib/internal/fs/dir.js:309`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L309)

```javascript
    if (this.#closed) return;
```

<a id="ref-q1-41"></a>
### [41] `lib/internal/fs/dir.js:314`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L314)

```javascript
    if (this.#closed) return;
```

<a id="ref-q1-42"></a>
### [42] `test/parallel/test-fs-promises-file-handle-dispose.js:14-15`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-promises-file-handle-dispose.js#L14-L15)

```javascript
  // Repeat invocations should not reject
  await dh[Symbol.asyncDispose]();
```

<a id="ref-q1-43"></a>
### [43] `test/parallel/test-fs-promises-file-handle-dispose.js:20-21`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-promises-file-handle-dispose.js#L20-L21)

```javascript
  // Repeat invocations should not throw
  dhSync[Symbol.dispose]();
```

<a id="ref-q1-44"></a>
### [44] `doc/api/deprecations.md:4356-4361`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/deprecations.md#L4356-L4361)

```markdown
Allowing a [`fs.Dir`][] object to be closed on garbage collection is
deprecated. In the future, doing so might result in a thrown error that will
terminate the process.

Please ensure that all `fs.Dir` objects are explicitly closed using
`Dir.prototype.close()` or `using` keyword:
```

<a id="ref-q1-45"></a>
### [45] `test/parallel/test-fs-opendir.js:80-81`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L80-L81)

```javascript
  assert.throws(() => dir.readSync(), dirclosedError);
  assert.throws(() => dir.closeSync(), dirclosedError);
```

<a id="ref-q1-46"></a>
### [46] `test/parallel/test-fs-opendir.js:181`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L181)

```javascript
  await assert.rejects(dir.read(), dirclosedError);
```

<a id="ref-q1-47"></a>
### [47] `test/parallel/test-fs-opendir.js:193`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L193)

```javascript
  await assert.rejects(dir.read(), dirclosedError);
```

<a id="ref-q1-48"></a>
### [48] `test/parallel/test-fs-opendir.js:209`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L209)

```javascript
  await assert.rejects(dir.read(), dirclosedError);
```

<a id="ref-q1-49"></a>
### [49] `test/parallel/test-fs-opendir.js:248`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L248)

```javascript
  await assert.rejects(() => dir.close(), dirclosedError);
```

<a id="ref-q1-50"></a>
### [50] `test/parallel/test-fs-opendir.js:298-301`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L298-L301)

```javascript
  dir.closeSync();
  dir.close(common.mustCall((error) => {
    assert.strictEqual(error.code, dirclosedError.code);
  }));
```

<a id="ref-q1-51"></a>
### [51] `test/parallel/test-fs-opendir.js:307-308`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L307-L308)

```javascript
  dir.closeSync();
  assert.rejects(dir.close(), dirclosedError).then(common.mustCall());
```

<a id="ref-q1-52"></a>
### [52] `test/parallel/test-fs-opendir.js:253-258`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-fs-opendir.js#L253-L258)

```javascript
async function doConcurrentAsyncAndSyncOps() {
  const dir = await fs.promises.opendir(testDir);
  const promise = dir.read();

  assert.throws(() => dir.closeSync(), dirconcurrentError);
  assert.throws(() => dir.readSync(), dirconcurrentError);
```

<a id="ref-q1-53"></a>
### [53] `lib/internal/fs/dir.js:40-317`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L40-L317)

```javascript
class Dir {
  #handle;
  #path;
  #bufferedEntries = [];
  #closed = false;
  #options;
  #readPromisified;
  #closePromisified;
  #operationQueue = null;
  #handlerQueue = [];

  constructor(handle, path, options) {
    if (handle == null) throw new ERR_MISSING_ARGS('handle');
    this.#handle = handle;
    this.#path = path;
    this.#options = {
      bufferSize: 32,
      ...getOptions(options, {
        encoding: 'utf8',
      }),
    };

    try {
      validateUint32(this.#options.bufferSize, 'options.bufferSize', true);
    } catch (validationError) {
      // Userland won't be able to close handle if we throw, so we close it first
      this.#handle.close();
      throw validationError;
    }

    this.#readPromisified = FunctionPrototypeBind(
      promisify(this.#readImpl), this, false);
    this.#closePromisified = FunctionPrototypeBind(
      promisify(this.close), this);
  }

  get path() {
    if (!(#path in this))
      throw new ERR_INVALID_THIS('Dir');
    return this.#path;
  }

  #processHandlerQueue() {
    while (this.#handlerQueue.length > 0) {
      const handler = ArrayPrototypeShift(this.#handlerQueue);
      const { handle, path } = handler;

      const result = handle.read(
        this.#options.encoding,
        this.#options.bufferSize,
      );

      if (result !== null) {
        this.#processReadResult(path, result);
        if (result.length > 0) {
          ArrayPrototypePush(this.#handlerQueue, handler);
        }
      } else {
        handle.close();
      }

      if (this.#bufferedEntries.length > 0) {
        break;
      }
    }

    return this.#bufferedEntries.length > 0;
  }

  read(callback) {
    return arguments.length === 0 ? this.#readPromisified() : this.#readImpl(true, callback);
  }

  #readImpl(maybeSync, callback) {
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }

    if (callback === undefined) {
      return this.#readPromisified();
    }

    validateFunction(callback, 'callback');

    if (this.#operationQueue !== null) {
      ArrayPrototypePush(this.#operationQueue, () => {
        this.#readImpl(maybeSync, callback);
      });
      return;
    }

    if (this.#processHandlerQueue()) {
      try {
        const dirent = ArrayPrototypeShift(this.#bufferedEntries);

        if (this.#options.recursive && dirent.isDirectory()) {
          this.#readSyncRecursive(dirent);
        }

        if (maybeSync)
          process.nextTick(callback, null, dirent);
        else
          callback(null, dirent);
        return;
      } catch (error) {
        return callback(error);
      }
    }

    const req = new FSReqCallback();
    req.oncomplete = (err, result) => {
      process.nextTick(() => {
        const queue = this.#operationQueue;
        this.#operationQueue = null;
        for (const op of queue) op();
      });

      if (err || result === null) {
        return callback(err, result);
      }

      try {
        this.#processReadResult(this.#path, result);
        const dirent = ArrayPrototypeShift(this.#bufferedEntries);
        if (this.#options.recursive && dirent.isDirectory()) {
          this.#readSyncRecursive(dirent);
        }
        callback(null, dirent);
      } catch (error) {
        callback(error);
      }
    };

    this.#operationQueue = [];
    this.#handle.read(
      this.#options.encoding,
      this.#options.bufferSize,
      req,
    );
  }

  #processReadResult(path, result) {
    for (let i = 0; i < result.length; i += 2) {
      ArrayPrototypePush(
        this.#bufferedEntries,
        getDirent(
          path,
          result[i],
          result[i + 1],
        ),
      );
    }
  }

  #readSyncRecursive(dirent) {
    const path = pathModule.join(dirent.parentPath, dirent.name);
    const handle = dirBinding.opendir(
      path,
      this.#options.encoding,
    );

    if (handle === undefined) {
      return;
    }

    ArrayPrototypePush(this.#handlerQueue, { handle, path });
  }

  readSync() {
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }

    if (this.#operationQueue !== null) {
      throw new ERR_DIR_CONCURRENT_OPERATION();
    }

    if (this.#processHandlerQueue()) {
      const dirent = ArrayPrototypeShift(this.#bufferedEntries);
      if (this.#options.recursive && dirent.isDirectory()) {
        this.#readSyncRecursive(dirent);
      }
      return dirent;
    }

    const result = this.#handle.read(
      this.#options.encoding,
      this.#options.bufferSize,
    );

    if (result === null) {
      return result;
    }

    this.#processReadResult(this.#path, result);

    const dirent = ArrayPrototypeShift(this.#bufferedEntries);
    if (this.#options.recursive && dirent.isDirectory()) {
      this.#readSyncRecursive(dirent);
    }
    return dirent;
  }

  close(callback) {
    if (callback === undefined) {
      if (this.#closed === true) {
        return PromiseReject(new ERR_DIR_CLOSED());
      }
      return this.#closePromisified();
    }

    validateFunction(callback, 'callback');

    if (this.#closed === true) {
      process.nextTick(callback, new ERR_DIR_CLOSED());
      return;
    }

    if (this.#operationQueue !== null) {
      ArrayPrototypePush(this.#operationQueue, () => {
        this.close(callback);
      });
      return;
    }

    while (this.#handlerQueue.length > 0) {
      const handler = ArrayPrototypeShift(this.#handlerQueue);
      handler.handle.close();
    }

    this.#closed = true;
    const req = new FSReqCallback();
    req.oncomplete = callback;
    this.#handle.close(req);
  }

  closeSync() {
    if (this.#closed === true) {
      throw new ERR_DIR_CLOSED();
    }

    if (this.#operationQueue !== null) {
      throw new ERR_DIR_CONCURRENT_OPERATION();
    }

    while (this.#handlerQueue.length > 0) {
      const handler = ArrayPrototypeShift(this.#handlerQueue);
      handler.handle.close();
    }

    this.#closed = true;
    this.#handle.close();
  }

  async* entries() {
    try {
      while (true) {
        const result = await this.#readPromisified();
        if (result === null) {
          break;
        }
        yield result;
      }
    } finally {
      await this.#closePromisified();
    }
  }

  [SymbolDispose]() {
    if (this.#closed) return;
    this.closeSync();
  }

  async [SymbolAsyncDispose]() {
    if (this.#closed) return;
    await this.#closePromisified();
  }
}
```

<a id="ref-q1-54"></a>
### [54] `lib/internal/fs/dir.js:41-48`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L41-L48)

```javascript
  #handle;
  #path;
  #bufferedEntries = [];
  #closed = false;
  #options;
  #readPromisified;
  #closePromisified;
  #operationQueue = null;
```

<a id="ref-q1-55"></a>
### [55] `lib/internal/fs/dir.js:16`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/internal/fs/dir.js#L16)

```javascript
const dirBinding = internalBinding('fs_dir');
```

<a id="ref-q1-56"></a>
### [56] `src/node_dir.cc:185-207`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_dir.cc#L185-L207)

```cpp
void DirHandle::Close(const FunctionCallbackInfo<Value>& args) {
  Environment* env = Environment::GetCurrent(args);

  CHECK_GE(args.Length(), 0);  // [req]

  DirHandle* dir;
  ASSIGN_OR_RETURN_UNWRAP(&dir, args.This());

  dir->closing_ = false;
  dir->closed_ = true;

  if (!args[0]->IsUndefined()) {  // close(req)
    FSReqBase* req_wrap_async = GetReqWrap(args, 0);
    CHECK_NOT_NULL(req_wrap_async);
    FS_DIR_ASYNC_TRACE_BEGIN0(UV_FS_CLOSEDIR, req_wrap_async)
    AsyncCall(env, req_wrap_async, args, "closedir", UTF8, AfterClose,
              uv_fs_closedir, dir->dir());
  } else {  // close()
    FSReqWrapSync req_wrap_sync("closedir");
    FS_DIR_SYNC_TRACE_BEGIN(closedir);
    SyncCallAndThrowOnError(env, &req_wrap_sync, uv_fs_closedir, dir->dir());
    FS_DIR_SYNC_TRACE_END(closedir);
  }
```

<a id="ref-q1-57"></a>
### [57] `src/node_dir.cc:352-393`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_dir.cc#L352-L393)

```cpp
static void OpenDir(const FunctionCallbackInfo<Value>& args) {
  Environment* env = Environment::GetCurrent(args);
  Isolate* isolate = env->isolate();

  CHECK_GE(args.Length(), 2);  // path, encoding, [callback]

  BufferValue path(isolate, args[0]);
  CHECK_NOT_NULL(*path);
  ToNamespacedPath(env, &path);

  const enum encoding encoding = ParseEncoding(isolate, args[1], UTF8);

  if (!args[2]->IsUndefined()) {  // openDir(path, encoding, req)
    FSReqBase* req_wrap_async = GetReqWrap(args, 2);
    CHECK_NOT_NULL(req_wrap_async);
    ASYNC_THROW_IF_INSUFFICIENT_PERMISSIONS(
        env,
        req_wrap_async,
        permission::PermissionScope::kFileSystemRead,
        path.ToStringView());
    FS_DIR_ASYNC_TRACE_BEGIN1(
        UV_FS_OPENDIR, req_wrap_async, "path", TRACE_STR_COPY(*path))
    AsyncCall(env, req_wrap_async, args, "opendir", encoding, AfterOpenDir,
              uv_fs_opendir, *path);
  } else {  // openDir(path, encoding)
    THROW_IF_INSUFFICIENT_PERMISSIONS(
        env, permission::PermissionScope::kFileSystemRead, path.ToStringView());
    FSReqWrapSync req_wrap_sync("opendir", *path);
    FS_DIR_SYNC_TRACE_BEGIN(opendir);
    int result =
        SyncCallAndThrowOnError(env, &req_wrap_sync, uv_fs_opendir, *path);
    FS_DIR_SYNC_TRACE_END(opendir);
    if (result < 0) {
      return;  // syscall failed, no need to continue, error is already thrown
    }

    uv_fs_t* req = &req_wrap_sync.req;
    uv_dir_t* dir = static_cast<uv_dir_t*>(req->ptr);
    DirHandle* handle = DirHandle::New(env, dir);

    args.GetReturnValue().Set(handle->object().As<Value>());
  }
```
