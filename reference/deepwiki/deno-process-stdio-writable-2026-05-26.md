# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests establish process.stdout and process.stderr stream writable semantics, specifically that process.stdout.writable and process.stderr.writable are boolean true while process.stdin.writable is not treated as a writable output stream? Focus only on public source/tests useful as reference material for a PerryTS/perry node:process stdio writable flag fix.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_bfd0377a-2f42-4f54-b60e-2deec16180f5

**Question**: For Node compatibility in Deno, what source files or tests establish process.stdout and process.stderr stream writable semantics, specifically that process.stdout.writable and process.stderr.writable are boolean true while process.stdin.writable is not treated as a writable output stream? Focus only on public source/tests useful as reference material for a PerryTS/perry node:process stdio writable flag fix.

In Deno's Node compatibility layer, `process.stdout` and `process.stderr` are established as writable streams through the `createWritableStdioStream` function in `ext/node/polyfills/_process/streams.mjs` [1](#ref-q1-1) . Conversely, `process.stdin` is initialized as a readable stream via `initStdin` in `ext/node/polyfills/process.ts` [2](#ref-q1-2)  and `ext/node/polyfills/_process/streams.mjs` [1](#ref-q1-1) .

## Writable Semantics for `process.stdout` and `process.stderr`

The writable semantics for `process.stdout` and `process.stderr` are primarily defined by the `createWritableStdioStream` function [1](#ref-q1-1) . This function creates a `Writable` stream from the `node:stream` module [3](#ref-q1-3) .

The `write` method of this stream handles the actual writing to the underlying Deno `writer` (either `io.stdout` or `io.stderr`) [4](#ref-q1-4) . It also includes error handling for `BrokenPipe` errors, mapping them to Node.js's `EPIPE` error [5](#ref-q1-5) .

The `process.stdout` and `process.stderr` objects are initialized in `ext/node/polyfills/process.ts` [6](#ref-q1-6) . If the respective Deno standard stream (`io.stdout` or `io.stderr`) is a terminal, a `TTYWriteStream` is used [7](#ref-q1-7)  [8](#ref-q1-8) . Otherwise, `createWritableStdioStream` is called [9](#ref-q1-9)  [10](#ref-q1-10) . Both `TTYWriteStream` and the stream returned by `createWritableStdioStream` are writable.

Tests confirming the behavior of `process.stdout` and `process.stderr` as writable streams can be found in `tests/unit_node/process_test.ts` [11](#ref-q1-11)  [12](#ref-q1-12) . Specifically, the test `process.stdout isn't closed when source stream ended` [13](#ref-q1-13)  demonstrates that `process.stdout` remains functional after a piped source stream ends, indicating its writable nature.

## `process.stdin` as a Non-Writable Output Stream

`process.stdin` is initialized using the `initStdin` function [2](#ref-q1-2)  [14](#ref-q1-14) . This function is responsible for setting up `process.stdin` as a readable stream, not a writable one. The `cli/tsc/dts/node/process.d.cts` file explicitly types `process.stdin` as a `ReadStream` [15](#ref-q1-15) , while `process.stdout` and `process.stderr` are typed as `WriteStream` [16](#ref-q1-16)  [17](#ref-q1-17) .

The `tests/unit_node/process_test.ts` file includes a test for `process.stdin` [18](#ref-q1-18) , which asserts its `fd` and `isTTY` properties, but does not test for writable semantics, reinforcing its role as a readable input stream.

## Notes

The `cli/tsc/dts/lib.deno.ns.d.ts` file defines `Deno.stdout.writable` as a `WritableStream<Uint8Array<ArrayBufferLike>>` [19](#ref-q1-19) , which is the underlying Deno API that `process.stdout` and `process.stderr` in the Node compatibility layer wrap. This further confirms their writable nature.

Wiki pages you might want to explore:
- [Process, TTY, and OS APIs (denoland/deno)](/wiki/denoland/deno#7.7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_bfd0377a-2f42-4f54-b60e-2deec16180f5

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/_process/streams.mjs:36-91`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_process/streams.mjs#L36-L91)

```
export function createWritableStdioStream(writer, name, warmup = false) {
  const stream = new (lazyStream().Writable)({
    emitClose: false,
    write(buf, enc, cb) {
      if (!writer) {
        this.destroy(
          new Error(`Deno.${name} is not available in this environment`),
        );
        return;
      }
      // TODO(fraidev): This try/catch is a workaround. When process.stdout
      // is a pipe (not a TTY), Node.js backs it with a real fd-based net.Socket
      // so BrokenPipe flows naturally through stream_wrap.ts as EPIPE. Deno
      // always uses createWritableStdioStream(io.stdout) regardless of pipe/TTY,
      // so BrokenPipe throws synchronously here instead. Once net.Socket supports
      // being created from a raw fd (new Socket({ fd: 1 })), process.stdout/stderr
      // should be switched to net.Socket for non-TTY cases and this can be removed.
      try {
        let data = ObjectPrototypeIsPrototypeOf(Uint8ArrayPrototype, buf)
          ? buf
          : Buffer.from(buf, enc);
        // Handle partial writes - writeSync may not write all bytes at once
        // (e.g., when stdout is a pipe and the pipe buffer is near capacity).
        // deno-lint-ignore prefer-primordials
        while (data.byteLength > 0) {
          const nwritten = writer.writeSync(data);
          // deno-lint-ignore prefer-primordials
          if (nwritten >= data.byteLength) break;
          data = TypedArrayPrototypeSlice(data, nwritten);
        }
      } catch (e) {
        if (
          ObjectPrototypeIsPrototypeOf(Deno.errors.BrokenPipe.prototype, e)
        ) {
          const err = new Error("write EPIPE");
          err.code = "EPIPE";
          err.errno = codeMap.get("EPIPE");
          err.syscall = "write";
          cb(err);
          return;
        }
        throw e;
      }
      cb();
    },
    destroy(err, cb) {
      cb(err);
      this._undestroy();

      // We need to emit 'close' anyway so that the closing
      // of the stream is observable.
      if (!this._writableState.emitClose) {
        nextTick(() => this.emit("close"));
      }
    },
  });
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/process.ts:85`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L85)

```typescript
  initStdin,
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/_process/streams.mjs:38`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_process/streams.mjs#L38)

```
    emitClose: false,
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/_process/streams.mjs:41-65`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_process/streams.mjs#L41-L65)

```
        this.destroy(
          new Error(`Deno.${name} is not available in this environment`),
        );
        return;
      }
      // TODO(fraidev): This try/catch is a workaround. When process.stdout
      // is a pipe (not a TTY), Node.js backs it with a real fd-based net.Socket
      // so BrokenPipe flows naturally through stream_wrap.ts as EPIPE. Deno
      // always uses createWritableStdioStream(io.stdout) regardless of pipe/TTY,
      // so BrokenPipe throws synchronously here instead. Once net.Socket supports
      // being created from a raw fd (new Socket({ fd: 1 })), process.stdout/stderr
      // should be switched to net.Socket for non-TTY cases and this can be removed.
      try {
        let data = ObjectPrototypeIsPrototypeOf(Uint8ArrayPrototype, buf)
          ? buf
          : Buffer.from(buf, enc);
        // Handle partial writes - writeSync may not write all bytes at once
        // (e.g., when stdout is a pipe and the pipe buffer is near capacity).
        // deno-lint-ignore prefer-primordials
        while (data.byteLength > 0) {
          const nwritten = writer.writeSync(data);
          // deno-lint-ignore prefer-primordials
          if (nwritten >= data.byteLength) break;
          data = TypedArrayPrototypeSlice(data, nwritten);
        }
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/_process/streams.mjs:67-78`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/_process/streams.mjs#L67-L78)

```
        if (
          ObjectPrototypeIsPrototypeOf(Deno.errors.BrokenPipe.prototype, e)
        ) {
          const err = new Error("write EPIPE");
          err.code = "EPIPE";
          err.errno = codeMap.get("EPIPE");
          err.syscall = "write";
          cb(err);
          return;
        }
        throw e;
      }
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/process.ts:1486-1534`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1486-L1534)

```typescript
      stdout = process.stdout = new TTYWriteStream(1);
      // For supporting legacy API we put the FD here.
      // Ref: https://github.com/nodejs/node/blob/main/lib/internal/bootstrap/switches/is_main_thread.js
      stdout.fd = 1;
      // Match Node.js: stdio streams are indestructible.
      // Libraries like mute-stream (@inquirer/prompts) call destroy()/end()
      // on process.stdout between prompts. Without this, the underlying TTY
      // handle is closed, breaking subsequent I/O.
      // _isStdio also prevents Stream.pipe() from calling end() on stdout
      // when a piped source stream ends.
      // Ref: https://github.com/nodejs/node/blob/main/lib/internal/bootstrap/switches/is_main_thread.js
      stdout._isStdio = true;
      stdout.destroySoon = stdout.destroy;
      stdout._destroy = function (err, cb) {
        cb(err);
        this._undestroy();
        if (!this._writableState.emitClose) {
          nextTick(() => this.emit("close"));
        }
      };
      addSigwinchListener(stdout);
    } else {
      stdout = process.stdout = createWritableStdioStream(
        io.stdout,
        "stdout",
      );
    }

    if (io.stderr.isTerminal()) {
      /** https://nodejs.org/api/process.html#process_process_stderr */
      stderr = process.stderr = new TTYWriteStream(2);
      // For supporting legacy API we put the FD here.
      stderr.fd = 2;
      stderr._isStdio = true;
      stderr.destroySoon = stderr.destroy;
      stderr._destroy = function (err, cb) {
        cb(err);
        this._undestroy();
        if (!this._writableState.emitClose) {
          nextTick(() => this.emit("close"));
        }
      };
      addSigwinchListener(stderr);
    } else {
      stderr = process.stderr = createWritableStdioStream(
        io.stderr,
        "stderr",
      );
    }
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/process.ts:1484-1486`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1484-L1486)

```typescript
    if (io.stdout.isTerminal()) {
      /** https://nodejs.org/api/process.html#process_process_stdout */
      stdout = process.stdout = new TTYWriteStream(1);
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/process.ts:1514-1516`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1514-L1516)

```typescript
    if (io.stderr.isTerminal()) {
      /** https://nodejs.org/api/process.html#process_process_stderr */
      stderr = process.stderr = new TTYWriteStream(2);
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/process.ts:1508-1512`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1508-L1512)

```typescript
      stdout = process.stdout = createWritableStdioStream(
        io.stdout,
        "stdout",
      );
    }
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/process.ts:1530-1534`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1530-L1534)

```typescript
      stderr = process.stderr = createWritableStdioStream(
        io.stderr,
        "stderr",
      );
    }
```

<a id="ref-q1-11"></a>
### [11] `tests/unit_node/process_test.ts:721-759`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L721-L759)

```typescript
Deno.test({
  name: "process.stdout",
  fn() {
    // @ts-ignore `Deno.stdout.rid` was soft-removed in Deno 2.
    assertEquals(process.stdout.fd, Deno.stdout.rid);
    const isTTY = Deno.stdout.isTerminal();
    assertEquals(process.stdout.isTTY, isTTY);
    const consoleSize = isTTY ? Deno.consoleSize() : undefined;
    assertEquals(process.stdout.columns, consoleSize?.columns);
    assertEquals(process.stdout.rows, consoleSize?.rows);
    assert([1, 4, 8, 24].includes(process.stdout.getColorDepth()));
    assertEquals(
      `${process.stdout.getWindowSize()}`,
      `${consoleSize && [consoleSize.columns, consoleSize.rows]}`,
    );

    if (isTTY) {
      assertStrictEquals(process.stdout.cursorTo(1, 2, () => {}), true);
      assertStrictEquals(process.stdout.moveCursor(3, 4, () => {}), true);
      assertStrictEquals(process.stdout.clearLine(1, () => {}), true);
      assertStrictEquals(process.stdout.clearScreenDown(() => {}), true);
    } else {
      assertStrictEquals(process.stdout.cursorTo, undefined);
      assertStrictEquals(process.stdout.moveCursor, undefined);
      assertStrictEquals(process.stdout.clearLine, undefined);
      assertStrictEquals(process.stdout.clearScreenDown, undefined);
    }

    // Allows overwriting `process.stdout.isTTY`
    // https://github.com/denoland/deno/issues/26123
    const original = process.stdout.isTTY;
    try {
      process.stdout.isTTY = !isTTY;
      assertEquals(process.stdout.isTTY, !isTTY);
    } finally {
      process.stdout.isTTY = original;
    }
  },
});
```

<a id="ref-q1-12"></a>
### [12] `tests/unit_node/process_test.ts:761-788`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L761-L788)

```typescript
Deno.test({
  name: "process.stderr",
  fn() {
    // @ts-ignore `Deno.stderr.rid` was soft-removed in Deno 2.
    assertEquals(process.stderr.fd, Deno.stderr.rid);
    const isTTY = Deno.stderr.isTerminal();
    assertEquals(process.stderr.isTTY, isTTY);
    const consoleSize = isTTY ? Deno.consoleSize() : undefined;
    assertEquals(process.stderr.columns, consoleSize?.columns);
    assertEquals(process.stderr.rows, consoleSize?.rows);
    assertEquals(
      `${process.stderr.getWindowSize()}`,
      `${consoleSize && [consoleSize.columns, consoleSize.rows]}`,
    );

    if (isTTY) {
      assertStrictEquals(process.stderr.cursorTo(1, 2, () => {}), true);
      assertStrictEquals(process.stderr.moveCursor(3, 4, () => {}), true);
      assertStrictEquals(process.stderr.clearLine(1, () => {}), true);
      assertStrictEquals(process.stderr.clearScreenDown(() => {}), true);
    } else {
      assertStrictEquals(process.stderr.cursorTo, undefined);
      assertStrictEquals(process.stderr.moveCursor, undefined);
      assertStrictEquals(process.stderr.clearLine, undefined);
      assertStrictEquals(process.stderr.clearScreenDown, undefined);
    }
  },
});
```

<a id="ref-q1-13"></a>
### [13] `tests/unit_node/process_test.ts:1042-1055`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L1042-L1055)

```typescript
  name: "process.stdout isn't closed when source stream ended",
  async fn() {
    const source = Readable.from(["foo", "bar"]);

    source.pipe(process.stdout);
    await once(source, "end");

    // Wait a bit to ensure that streaming is completely finished.
    await delay(10);

    // This checks if the rid 1 is still valid.
    assert(typeof process.stdout.isTTY === "boolean");
  },
});
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/process.ts:1574`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1574)

```typescript
    const newStdin = initStdin();
```

<a id="ref-q1-15"></a>
### [15] `cli/tsc/dts/node/process.d.cts:601-603`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/process.d.cts#L601-L603)

```
                stdin: ReadStream & {
                    fd: 0;
                };
```

<a id="ref-q1-16"></a>
### [16] `cli/tsc/dts/node/process.d.cts:576-578`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/process.d.cts#L576-L578)

```
                stdout: WriteStream & {
                    fd: 1;
                };
```

<a id="ref-q1-17"></a>
### [17] `cli/tsc/dts/node/process.d.cts:585-587`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/process.d.cts#L585-L587)

```
                stderr: WriteStream & {
                    fd: 2;
                };
```

<a id="ref-q1-18"></a>
### [18] `tests/unit_node/process_test.ts:524-541`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L524-L541)

```typescript
  name: "process.stdin",
  fn() {
    // @ts-ignore `Deno.stdin.rid` was soft-removed in Deno 2.
    assertEquals(process.stdin.fd, Deno.stdin.rid);
    const isTTY = Deno.stdin.isTerminal();
    assertEquals(process.stdin.isTTY, isTTY);

    // Allows overwriting `process.stdin.isTTY` (mirrors stdout/stderr from #26130)
    const original = process.stdin.isTTY;
    try {
      // @ts-ignore isTTY is defined as readonly in types but we allow setting it
      process.stdin.isTTY = !isTTY;
      assertEquals(process.stdin.isTTY, !isTTY);
    } finally {
      // @ts-ignore isTTY is defined as readonly in types but we allow setting it
      process.stdin.isTTY = original;
    }
  },
```

<a id="ref-q1-19"></a>
### [19] `cli/tsc/dts/lib.deno.ns.d.ts:2471-2472`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/lib.deno.ns.d.ts#L2471-L2472)

```typescript
    /** A writable stream interface to `stdout`. */
    readonly writable: WritableStream<Uint8Array<ArrayBufferLike>>;
```
