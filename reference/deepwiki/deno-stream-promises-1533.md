# DeepWiki Response

Repository: `denoland/deno`
Question: How does Deno implement the Node compatibility API surface for node:stream's promises namespace, especially stream.promises, stream.promises.pipeline, stream.promises.finished, and export wiring from node:stream and node:stream/promises? Please identify the relevant source files, behavior, and tests that are useful as an implementation reference for PerryTS/perry issue #1533.
Search: https://deepwiki.com/search/how-does-deno-implement-the-no_5750c92a-e3b2-4f42-a268-e540d4ce2a19

**Question**: How does Deno implement the Node compatibility API surface for node:stream's promises namespace, especially stream.promises, stream.promises.pipeline, stream.promises.finished, and export wiring from node:stream and node:stream/promises? Please identify the relevant source files, behavior, and tests that are useful as an implementation reference for PerryTS/perry issue #1533.

Deno implements the Node.js compatibility API for `node:stream`'s `promises` namespace by providing polyfills that mirror Node.js's behavior. The core of this implementation resides in `ext/node/polyfills/stream.ts` [1](#ref-q1-1)  and `ext/node/polyfills/stream/promises.js` [2](#ref-q1-2) , with `pipeline` and `finished` functionalities being imported from internal utility modules. The `node:stream/promises` module is explicitly listed as a supported built-in Node.js module [3](#ref-q1-3) .

### Implementation Details

The `node:stream` module in Deno exports the `promises` namespace directly [4](#ref-q1-4) . This `promises` object is imported from `node:stream/promises` [5](#ref-q1-5) .

Specifically:
*   `stream.promises`: This object is directly exposed as a getter on the `Stream` object, returning the imported `promises` module [4](#ref-q1-4) .
*   `stream.promises.pipeline`: The `pipeline` function is imported from `ext:deno_node/internal/streams/pipeline.js` [6](#ref-q1-6) . The `customPromisify` symbol is used to link the `pipeline` function to `promises.pipeline` [7](#ref-q1-7) . The actual implementation of `pipeline` handles various stream types, including iterables and Node.js streams [8](#ref-q1-8) .
*   `stream.promises.finished`: Similarly, the `eos` (end-of-stream) function, which serves as the basis for `finished`, is imported from `ext:deno_node/internal/streams/end-of-stream.js` [9](#ref-q1-9) . It is then linked to `promises.finished` using `customPromisify` [10](#ref-q1-10) .

### Export Wiring

The `node:stream` module is polyfilled by `ext/node/polyfills/stream.ts` [11](#ref-q1-11) , and `node:stream/promises` is polyfilled by `ext/node/polyfills/stream/promises.js` [12](#ref-q1-12) . These mappings are defined in the `deno_node` extension's ESM module list [13](#ref-q1-13)  and the `tools/core_import_map.json` file [14](#ref-q1-14) .

### Testing

The compatibility of `node:stream/promises` is verified through Node.js official tests included in Deno's test suite. Relevant tests can be found in `tests/node_compat/config.jsonc` [15](#ref-q1-15)  [16](#ref-q1-16) . Additionally, Deno-specific unit tests, such as `tests/unit_node/stream_test.ts`, directly test `pipeline` and `finished` from `node:stream/promises` [17](#ref-q1-17)  [18](#ref-q1-18)  and also verify the `promises` export from `node:stream` [19](#ref-q1-19) .

## Notes

The `Node.js Compatibility Layer` wiki page provides a high-level overview of how Deno implements Node.js APIs, including a mention of `node:stream` polyfills [20](#ref-q1-20) . The `ext/node/polyfills/README.md` file also lists `stream` and `stream/promises` as supported modules [21](#ref-q1-21) .

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/how-does-deno-implement-the-no_5750c92a-e3b2-4f42-a268-e540d4ce2a19

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/stream.ts:33-63`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L33-L63)

```typescript
const internalUtil = core.loadExtScript("ext:deno_node/internal/util.mjs");
const {
  promisify: { custom: customPromisify },
} = internalUtil;

import {
  promiseReturningOperators,
  streamReturningOperators,
} from "ext:deno_node/internal/streams/operators.js";

import compose from "ext:deno_node/internal/streams/compose.js";
const {
  getDefaultHighWaterMark,
  setDefaultHighWaterMark,
} = core.loadExtScript("ext:deno_node/internal/streams/state.js");
import { pipeline } from "ext:deno_node/internal/streams/pipeline.js";
const { destroyer } = core.loadExtScript(
  "ext:deno_node/internal/streams/destroy.js",
);
const { eos } = core.loadExtScript(
  "ext:deno_node/internal/streams/end-of-stream.js",
);
const { Buffer } = core.loadExtScript("ext:deno_node/internal/buffer.mjs");

import * as promises from "node:stream/promises";
const utils = core.loadExtScript("ext:deno_node/internal/streams/utils.js");
const {
  isArrayBufferView,
  isUint8Array,
} = core.loadExtScript("ext:deno_node/internal/util/types.ts");
```

<a id="ref-q1-2"></a>
### [2] `ext/node/lib.rs:457-458`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/lib.rs#L457-L458)

```rust
    "node:stream" = "stream.ts",
    "node:stream/promises" = "stream/promises.js",
```

<a id="ref-q1-3"></a>
### [3] `libs/node_resolver/builtin_modules.rs:75`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/libs/node_resolver/builtin_modules.rs#L75)

```rust
  "stream/promises",
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/stream.ts:141-148`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L141-L148)

```typescript
ObjectDefineProperty(Stream, "promises", {
  __proto__: null,
  configurable: true,
  enumerable: true,
  get() {
    return promises;
  },
});
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/stream.ts:57`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L57)

```typescript
import * as promises from "node:stream/promises";
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/stream.ts:48`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L48)

```typescript
import { pipeline } from "ext:deno_node/internal/streams/pipeline.js";
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/stream.ts:150-156`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L150-L156)

```typescript
ObjectDefineProperty(pipeline, customPromisify, {
  __proto__: null,
  enumerable: true,
  get() {
    return promises.pipeline;
  },
});
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/internal/streams/pipeline.js:366-425`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/streams/pipeline.js#L366-L425)

```javascript
        const then = ret?.then;
        if (typeof then === "function") {
          finishCount++;
          then.call(ret, (val) => {
            value = val;
            if (val != null) {
              pt.write(val);
            }
            if (end) {
              pt.end();
            }
            process.nextTick(finish);
          }, (err) => {
            pt.destroy(err);
            process.nextTick(finish, err);
          });
        } else if (isIterable(ret, true)) {
          finishCount++;
          pumpToNode(ret, pt, finish, { end });
        } else if (isReadableStream(ret) || isTransformStream(ret)) {
          const toRead = ret.readable || ret;
          finishCount++;
          pumpToNode(toRead, pt, finish, { end });
        } else {
          throw new ERR_INVALID_RETURN_VALUE(
            "AsyncIterable or Promise",
            "destination",
            ret,
          );
        }

        ret = pt;

        const { destroy, cleanup } = destroyer(ret, false, true);
        destroys.push(destroy);
        if (isLastStream) {
          lastStreamCleanup.push(cleanup);
        }
      }
    } else if (isNodeStream(stream)) {
      if (isReadableNodeStream(ret)) {
        finishCount += 2;
        const cleanup = pipe(ret, stream, finish, finishOnlyHandleError, {
          end,
        });
        if (isReadable(stream) && isLastStream) {
          lastStreamCleanup.push(cleanup);
        }
      } else if (isTransformStream(ret) || isReadableStream(ret)) {
        const toRead = ret.readable || ret;
        finishCount++;
        pumpToNode(toRead, stream, finish, { end });
      } else if (isIterable(ret)) {
        finishCount++;
        pumpToNode(ret, stream, finish, { end });
      } else {
        throw new ERR_INVALID_ARG_TYPE(
          "val",
          [
            "Readable",
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/stream.ts:52-54`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L52-L54)

```typescript
const { eos } = core.loadExtScript(
  "ext:deno_node/internal/streams/end-of-stream.js",
);
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/stream.ts:158-164`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/stream.ts#L158-L164)

```typescript
ObjectDefineProperty(eos, customPromisify, {
  __proto__: null,
  enumerable: true,
  get() {
    return promises.finished;
  },
});
```

<a id="ref-q1-11"></a>
### [11] `ext/node/lib.rs:457`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/lib.rs#L457)

```rust
    "node:stream" = "stream.ts",
```

<a id="ref-q1-12"></a>
### [12] `ext/node/lib.rs:458`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/lib.rs#L458)

```rust
    "node:stream/promises" = "stream/promises.js",
```

<a id="ref-q1-13"></a>
### [13] `ext/node/lib.rs:415-467`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/lib.rs#L415-L467)

```rust
  esm = [
    dir "polyfills",
    "02_init.js",
    "internal/streams/compose.js",
    "internal/streams/duplexify.js",
    "internal/streams/duplexpair.js",
    "internal/streams/fast-utf8-stream.js",
    "internal/streams/from.js",
    "internal/streams/lazy_transform.js",
    "internal/streams/pipeline.js",
    "_readline.mjs",
    "internal_binding/mod.ts",
    "internal/dns/promises.ts",
    "internal/fs/stat_utils.ts",
    "internal/streams/operators.js",
    "node:_stream_duplex" = "internal/streams/duplex.js",
    "node:_stream_passthrough" = "internal/streams/passthrough.js",
    "node:_stream_readable" = "internal/streams/readable.js",
    "node:_stream_transform" = "internal/streams/transform.js",
    "node:_stream_writable" = "internal/streams/writable.js",
    "node:_tls_common" = "_tls_common.ts",
    "node:_tls_wrap" = "_tls_wrap.js",
    "node:child_process" = "child_process.ts",
    "node:cluster" = "cluster.ts",
    "node:console" = "console.ts",
    "node:constants" = "constants.ts",
    "node:crypto" = "crypto.ts",
    "node:dgram" = "dgram.ts",
    "node:dns" = "dns.ts",
    "node:dns/promises" = "dns/promises.ts",
    "node:fs" = "fs.ts",
    "node:fs/promises" = "fs/promises.ts",
    "node:http" = "http.ts",
    "node:http2" = "http2.ts",
    "node:https" = "https.ts",
    "node:inspector" = "inspector.js",
    "node:inspector/promises" = "inspector/promises.js",
    "node:module" = "01_require.js",
    "node:net" = "net.ts",
    "node:process" = "process.ts",
    "node:readline" = "readline.ts",
    "node:repl" = "repl.ts",
    "node:stream" = "stream.ts",
    "node:stream/promises" = "stream/promises.js",
    "node:timers" = "timers.ts",
    "node:timers/promises" = "timers/promises.ts",
    "node:tls" = "tls.ts",
    "node:tty" = "tty.js",
    "node:url" = "url.ts",
    "node:v8" = "v8.ts",
    "node:worker_threads" = "worker_threads.ts",
    "node:zlib" = "zlib.js",
  ],
```

<a id="ref-q1-14"></a>
### [14] `tools/core_import_map.json:751-753`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/core_import_map.json#L751-L753)

```json
    "node:stream": "../ext/node/polyfills/stream.ts",
    "node:stream/consumers": "../ext/node/polyfills/stream/consumers.mjs",
    "node:stream/promises": "../ext/node/polyfills/stream/promises.js",
```

<a id="ref-q1-15"></a>
### [15] `tests/node_compat/config.jsonc:3242-3245`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L3242-L3245)

```
    "parallel/test-stream-pipeline-with-empty-string.js": {},
    "parallel/test-stream-pipeline.js": {},
    "parallel/test-stream-preprocess.js": {},
    "parallel/test-stream-promises.js": {},
```

<a id="ref-q1-16"></a>
### [16] `tests/node_compat/config.jsonc:3726-3727`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L3726-L3727)

```
    "parallel/test-webstreams-finished.js": {},
    "parallel/test-webstreams-pipeline.js": {},
```

<a id="ref-q1-17"></a>
### [17] `tests/unit_node/stream_test.ts:5`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/stream_test.ts#L5)

```typescript
import { finished, pipeline } from "node:stream/promises";
```

<a id="ref-q1-18"></a>
### [18] `tests/unit_node/stream_test.ts:16-33`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/stream_test.ts#L16-L33)

```typescript
Deno.test("stream/promises pipeline", async () => {
  const filePath = relative(
    Deno.cwd(),
    fromFileUrl(new URL("./testdata/lorem_ipsum.txt", import.meta.url)),
  );
  const input = createReadStream(filePath);
  const output = createWriteStream("lorem_ipsum.txt.copy");

  await pipeline(input, output);

  const content = Deno.readTextFileSync("lorem_ipsum.txt.copy");
  assert(content.startsWith("Lorem ipsum dolor sit amet"));
  try {
    Deno.removeSync("lorem_ipsum.txt.copy");
  } catch {
    // pass
  }
});
```

<a id="ref-q1-19"></a>
### [19] `tests/unit_node/stream_test.ts:88-100`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/stream_test.ts#L88-L100)

```typescript
Deno.test("exported `promises` from node:stream works", async () => {
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue("asd");
      controller.close();
    },
  });
  const promise = promises.finished(stream as unknown as NodeJS.ReadableStream);
  for await (const chunk of stream) {
    assertEquals(chunk, "asd");
  }
  await promise;
});
```

<a id="ref-q1-20"></a>
### [20] `Node.js Compatibility Layer:60`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/Node.js Compatibility Layer#L60)

<a id="ref-q1-21"></a>
### [21] `ext/node/polyfills/README.md:42-43`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/README.md#L42-L43)

```markdown
- [x] stream
- [x] stream/promises
```
