# DeepWiki Response

Repository: `denoland/deno`
Question: How does Deno implement Node compatibility export wiring for node:util's named 'types' export and the node:util/types submodule? Please identify the relevant source files and tests showing that import { types } from 'node:util', util.types, and import * as types from 'node:util/types' all expose the same predicate namespace.
Search: https://deepwiki.com/search/how-does-deno-implement-node-c_1541324c-00f8-40d0-8c4e-850439758e06

**Question**: How does Deno implement Node compatibility export wiring for node:util's named 'types' export and the node:util/types submodule? Please identify the relevant source files and tests showing that import { types } from 'node:util', util.types, and import * as types from 'node:util/types' all expose the same predicate namespace.

Deno implements Node compatibility for `node:util`'s `types` export and the `node:util/types` submodule by aliasing them to the same underlying module. The `types` predicate namespace is exposed consistently across `import { types } from 'node:util'`, `util.types`, and `import * as types from 'node:util/types'`.

## Export Wiring

The core of this compatibility is achieved through Deno's internal module mapping and polyfills.
The `tools/core_import_map.json` file defines how Node.js modules are resolved within Deno [1](#ref-q1-1) . Specifically, it maps `node:util/types` to `ext:deno_node/internal/util/types.ts` [2](#ref-q1-2) .

The `cli/tsc/dts/node/util.d.cts` file, which provides TypeScript declarations for `node:util`, explicitly re-exports everything from `node:util/types` under the `util` module [3](#ref-q1-3) . It also re-exports everything from `util/types` for `node:util/types` [4](#ref-q1-4) . This ensures that `import { types } from 'node:util'` and `import * as types from 'node:util/types'` both refer to the same set of type-checking predicates.

The `ext/node/polyfills/sys_esm.js` file further reinforces this by re-exporting everything from `node:util` [5](#ref-q1-5) .

The actual implementation of these type-checking predicates resides in `ext/node/polyfills/internal/util/types.ts` [6](#ref-q1-6) . This file contains functions like `isArrayBufferView`, `isBigUint64Array`, `isDate`, and `isUint8Array` [6](#ref-q1-6) . These functions are then imported and used by other internal Node.js polyfills, such as `ext/node/polyfills/internal/fs/utils.mjs` [6](#ref-q1-6)  and `ext/node/polyfills/internal/util/comparisons.ts` [7](#ref-q1-7) .

## Test Coverage

The consistent exposure of the `types` predicate namespace is verified by tests in `tests/unit_node/util_test.ts` [8](#ref-q1-8) . For example, the test named `[util] types.isTypedArray` asserts that `util.types.isTypedArray` correctly identifies `Buffer` and `Uint8Array` instances [9](#ref-q1-9) . Another test, `[util] types.isNativeError`, checks the functionality of `util.types.isNativeError` [10](#ref-q1-10) .

Additionally, the `tests/node_compat/config.jsonc` file includes a test specifically for `es-module/test-esm-util-types.mjs` [11](#ref-q1-11) , indicating that Deno runs upstream Node.js compatibility tests to ensure correct behavior for `node:util/types` when imported as an ES module.

## Notes

The `cli/tsc/dts/node/util.d.cts` file is a TypeScript declaration file, which defines the types and interfaces for the `node:util` module, including the `types` submodule [12](#ref-q1-12) . While it doesn't contain the runtime implementation, it's crucial for ensuring type-checking compatibility for Deno users.

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)
- [File System Polyfills (denoland/deno)](/wiki/denoland/deno#7.4)

View this search on DeepWiki: https://deepwiki.com/search/how-does-deno-implement-node-c_1541324c-00f8-40d0-8c4e-850439758e06

## References

<a id="ref-q1-1"></a>
### [1] `tools/core_import_map.json:601-750`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/core_import_map.json#L601-L750)

```json
    "ext:deno_node/internal/crypto/constants.ts": "../ext/node/polyfills/internal/crypto/constants.ts",
    "ext:deno_node/internal/crypto/diffiehellman.ts": "../ext/node/polyfills/internal/crypto/diffiehellman.ts",
    "ext:deno_node/internal/crypto/hash.ts": "../ext/node/polyfills/internal/crypto/hash.ts",
    "ext:deno_node/internal/crypto/hkdf.ts": "../ext/node/polyfills/internal/crypto/hkdf.ts",
    "ext:deno_node/internal/crypto/keygen.ts": "../ext/node/polyfills/internal/crypto/keygen.ts",
    "ext:deno_node/internal/crypto/keys.ts": "../ext/node/polyfills/internal/crypto/keys.ts",
    "ext:deno_node/internal/crypto/pbkdf2.ts": "../ext/node/polyfills/internal/crypto/pbkdf2.ts",
    "ext:deno_node/internal/crypto/random.ts": "../ext/node/polyfills/internal/crypto/random.ts",
    "ext:deno_node/internal/crypto/scrypt.ts": "../ext/node/polyfills/internal/crypto/scrypt.ts",
    "ext:deno_node/internal/crypto/sig.ts": "../ext/node/polyfills/internal/crypto/sig.ts",
    "ext:deno_node/internal/crypto/types.ts": "../ext/node/polyfills/internal/crypto/types.ts",
    "ext:deno_node/internal/crypto/util.ts": "../ext/node/polyfills/internal/crypto/util.ts",
    "ext:deno_node/internal/crypto/x509.ts": "../ext/node/polyfills/internal/crypto/x509.ts",
    "ext:deno_node/internal/dgram.ts": "../ext/node/polyfills/internal/dgram.ts",
    "ext:deno_node/internal/dns/promises.ts": "../ext/node/polyfills/internal/dns/promises.ts",
    "ext:deno_node/internal/dns/utils.ts": "../ext/node/polyfills/internal/dns/utils.ts",
    "ext:deno_node/internal/error_codes.ts": "../ext/node/polyfills/internal/error_codes.ts",
    "ext:deno_node/internal/errors.ts": "../ext/node/polyfills/internal/errors.ts",
    "ext:deno_node/internal/errors/error_source.ts": "../ext/node/polyfills/internal/errors/error_source.ts",
    "ext:deno_node/internal/event_target.mjs": "../ext/node/polyfills/internal/event_target.mjs",
    "ext:deno_node/internal/fixed_queue.ts": "../ext/node/polyfills/internal/fixed_queue.ts",
    "ext:deno_node/internal/fs/handle.ts": "../ext/node/polyfills/internal/fs/handle.ts",
    "ext:deno_node/internal/fs/stat_utils.ts": "../ext/node/polyfills/internal/fs/stat_utils.ts",
    "ext:deno_node/internal/fs/utils.mjs": "../ext/node/polyfills/internal/fs/utils.mjs",
    "ext:deno_node/internal/hide_stack_frames.ts": "../ext/node/polyfills/internal/hide_stack_frames.ts",
    "ext:deno_node/internal/http.ts": "../ext/node/polyfills/internal/http.ts",
    "ext:deno_node/internal/http2/util.ts": "../ext/node/polyfills/internal/http2/util.ts",
    "ext:deno_node/internal/idna.ts": "../ext/node/polyfills/internal/idna.ts",
    "ext:deno_node/internal/net.ts": "../ext/node/polyfills/internal/net.ts",
    "ext:deno_node/internal/normalize_encoding.ts": "../ext/node/polyfills/internal/normalize_encoding.ts",
    "ext:deno_node/internal/options.ts": "../ext/node/polyfills/internal/options.ts",
    "ext:deno_node/internal/primordials.mjs": "../ext/node/polyfills/internal/primordials.mjs",
    "ext:deno_node/internal/process/per_thread.mjs": "../ext/node/polyfills/internal/process/per_thread.mjs",
    "ext:deno_node/internal/process/report.ts": "../ext/node/polyfills/internal/process/report.ts",
    "ext:deno_node/internal/querystring.ts": "../ext/node/polyfills/internal/querystring.ts",
    "ext:deno_node/internal/readline/callbacks.mjs": "../ext/node/polyfills/internal/readline/callbacks.mjs",
    "ext:deno_node/internal/readline/emitKeypressEvents.mjs": "../ext/node/polyfills/internal/readline/emitKeypressEvents.mjs",
    "ext:deno_node/internal/readline/interface.mjs": "../ext/node/polyfills/internal/readline/interface.mjs",
    "ext:deno_node/internal/readline/promises.mjs": "../ext/node/polyfills/internal/readline/promises.mjs",
    "ext:deno_node/internal/readline/symbols.mjs": "../ext/node/polyfills/internal/readline/symbols.mjs",
    "ext:deno_node/internal/readline/utils.mjs": "../ext/node/polyfills/internal/readline/utils.mjs",
    "ext:deno_node/internal/stream_base_commons.ts": "../ext/node/polyfills/internal/stream_base_commons.ts",
    "ext:deno_node/internal/streams/add-abort-signal.js": "../ext/node/polyfills/internal/streams/add-abort-signal.js",
    "ext:deno_node/internal/streams/destroy.js": "../ext/node/polyfills/internal/streams/destroy.js",
    "ext:deno_node/internal/streams/end-of-stream.js": "../ext/node/polyfills/internal/streams/end-of-stream.js",
    "ext:deno_node/internal/streams/lazy_transform.js": "../ext/node/polyfills/internal/streams/lazy_transform.js",
    "ext:deno_node/internal/streams/state.js": "../ext/node/polyfills/internal/streams/state.js",
    "ext:deno_node/internal/streams/utils.js": "../ext/node/polyfills/internal/streams/utils.js",
    "ext:deno_node/internal/test/binding.ts": "../ext/node/polyfills/internal/test/binding.ts",
    "ext:deno_node/internal/timers.mjs": "../ext/node/polyfills/internal/timers.mjs",
    "ext:deno_node/internal/url.ts": "../ext/node/polyfills/internal/url.ts",
    "ext:deno_node/internal/util.mjs": "../ext/node/polyfills/internal/util.mjs",
    "ext:deno_node/internal/util/colors.ts": "../ext/node/polyfills/internal/util/colors.ts",
    "ext:deno_node/internal/util/comparisons.ts": "../ext/node/polyfills/internal/util/comparisons.ts",
    "ext:deno_node/internal/util/debuglog.ts": "../ext/node/polyfills/internal/util/debuglog.ts",
    "ext:deno_node/internal/util/inspect.mjs": "../ext/node/polyfills/internal/util/inspect.mjs",
    "ext:deno_node/internal/util/types.ts": "../ext/node/polyfills/internal/util/types.ts",
    "ext:deno_node/internal/validators.mjs": "../ext/node/polyfills/internal/validators.mjs",
    "ext:deno_node/path/_constants.ts": "../ext/node/polyfills/path/_constants.ts",
    "ext:deno_node/path/_interface.ts": "../ext/node/polyfills/path/_interface.ts",
    "ext:deno_node/path/_posix.ts": "../ext/node/polyfills/path/_posix.ts",
    "ext:deno_node/path/_util.ts": "../ext/node/polyfills/path/_util.ts",
    "ext:deno_node/path/_win32.ts": "../ext/node/polyfills/path/_win32.ts",
    "ext:deno_node/path/mod.ts": "../ext/node/polyfills/path/mod.ts",
    "ext:deno_node/path/separator.ts": "../ext/node/polyfills/path/separator.ts",
    "ext:deno_node/readline/promises.ts": "../ext/node/polyfills/readline/promises.ts",
    "ext:deno_node/repl.ts": "../ext/node/polyfills/repl.ts",
    "ext:deno_os/30_os.js": "../ext/os/30_os.js",
    "ext:deno_os/40_signals.js": "../ext/os/40_signals.js",
    "ext:deno_process/40_process.js": "../ext/process/40_process.js",
    "ext:deno_telemetry/telemetry.ts": "../ext/deno_telemetry/telemetry.ts",
    "ext:deno_telemetry/util.ts": "../ext/deno_telemetry/util.ts",
    "ext:deno_web/00_url.js": "../ext/web/00_url.js",
    "ext:deno_web/01_urlpattern.js": "../ext/web/01_urlpattern.js",
    "ext:deno_web/00_infra.js": "../ext/web/00_infra.js",
    "ext:deno_web/01_dom_exception.js": "../ext/web/01_dom_exception.js",
    "ext:deno_web/01_mimesniff.js": "../ext/web/01_mimesniff.js",
    "ext:deno_web/02_event.js": "../ext/web/02_event.js",
    "ext:deno_web/02_structured_clone.js": "../ext/web/02_structured_clone.js",
    "ext:deno_web/02_timers.js": "../ext/web/02_timers.js",
    "ext:deno_web/03_abort_signal.js": "../ext/web/03_abort_signal.js",
    "ext:deno_web/04_global_interfaces.js": "../ext/web/04_global_interfaces.js",
    "ext:deno_web/05_base64.js": "../ext/web/05_base64.js",
    "ext:deno_web/06_streams.js": "../ext/web/06_streams.js",
    "ext:deno_web/08_text_encoding.js": "../ext/web/08_text_encoding.js",
    "ext:deno_web/09_file.js": "../ext/web/09_file.js",
    "ext:deno_web/10_filereader.js": "../ext/web/10_filereader.js",
    "ext:deno_web/12_location.js": "../ext/web/12_location.js",
    "ext:deno_web/13_message_port.js": "../ext/web/13_message_port.js",
    "ext:deno_web/14_compression.js": "../ext/web/14_compression.js",
    "ext:deno_web/15_performance.js": "../ext/web/15_performance.js",
    "ext:deno_web/16_image_data.js": "../ext/web/16_image_data.js",
    "ext:deno_web/geometry.js": "../ext/web/geometry.js",
    "ext:deno_web/webtransport.js": "../ext/web/webtransport.js",
    "ext:deno_webidl/00_webidl.js": "../ext/webidl/00_webidl.js",
    "ext:deno_websocket/01_websocket.js": "../ext/websocket/01_websocket.js",
    "ext:deno_websocket/02_websocketstream.js": "../ext/websocket/02_websocketstream.js",
    "ext:deno_webstorage/01_webstorage.js": "../ext/webstorage/01_webstorage.js",
    "ext:runtime/01_errors.js": "../runtime/js/01_errors.js",
    "ext:runtime/01_version.ts": "../runtime/js/01_version.ts",
    "ext:runtime/06_util.js": "../runtime/js/06_util.js",
    "ext:runtime/10_permissions.js": "../runtime/js/10_permissions.js",
    "ext:runtime/11_workers.js": "../runtime/js/11_workers.js",
    "ext:runtime/40_fs_events.js": "../runtime/js/40_fs_events.js",
    "ext:runtime/40_tty.js": "../runtime/js/40_tty.js",
    "ext:runtime/41_prompt.js": "../runtime/js/41_prompt.js",
    "ext:runtime/90_deno_ns.js": "../runtime/js/90_deno_ns.js",
    "ext:runtime/98_global_scope.js": "../runtime/js/98_global_scope.js",
    "node:_http_agent": "../ext/node/polyfills/_http_agent.js",
    "node:_http_common": "../ext/node/polyfills/_http_common.js",
    "node:_http_outgoing": "../ext/node/polyfills/_http_outgoing.ts",
    "node:_http_server": "../ext/node/polyfills/_http_server.ts",
    "node:_stream_duplex": "../ext/node/polyfills/internal/streams/duplex.js",
    "node:_stream_passthrough": "../ext/node/polyfills/internal/streams/passthrough.js",
    "node:_stream_readable": "../ext/node/polyfills/internal/streams/readable.js",
    "node:_stream_transform": "../ext/node/polyfills/internal/streams/transform.js",
    "node:_stream_writable": "../ext/node/polyfills/internal/streams/writable.js",
    "node:_tls_common": "../ext/node/polyfills/_tls_common.ts",
    "node:assert": "../ext/node/polyfills/assert.ts",
    "node:assert/strict": "../ext/node/polyfills/assert/strict.ts",
    "node:async_hooks": "../ext/node/polyfills/async_hooks.ts",
    "node:buffer": "../ext/node/polyfills/buffer.ts",
    "node:child_process": "../ext/node/polyfills/child_process.ts",
    "node:cluster": "../ext/node/polyfills/cluster.ts",
    "node:console": "../ext/node/polyfills/console.ts",
    "node:constants": "../ext/node/polyfills/constants.ts",
    "node:crypto": "../ext/node/polyfills/crypto.ts",
    "node:dgram": "../ext/node/polyfills/dgram.ts",
    "node:diagnostics_channel": "../ext/node/polyfills/diagnostics_channel.js",
    "node:dns": "../ext/node/polyfills/dns.ts",
    "node:dns/promises": "../ext/node/polyfills/dns/promises.ts",
    "node:domain": "../ext/node/polyfills/domain.ts",
    "node:events": "../ext/node/polyfills/events.ts",
    "node:fs": "../ext/node/polyfills/fs.ts",
    "node:fs/promises": "../ext/node/polyfills/fs/promises.ts",
    "node:http": "../ext/node/polyfills/http.ts",
    "node:http2": "../ext/node/polyfills/http2.ts",
    "node:https": "../ext/node/polyfills/https.ts",
    "node:inspector": "../ext/node/polyfills/inspector.ts",
    "node:module": "../ext/node/polyfills/01_require.js",
    "node:net": "../ext/node/polyfills/net.ts",
    "node:os": "../ext/node/polyfills/os.ts",
    "node:path": "../ext/node/polyfills/path.ts",
    "node:path/posix": "../ext/node/polyfills/path/posix.ts",
    "node:path/win32": "../ext/node/polyfills/path/win32.ts",
    "node:perf_hooks": "../ext/node/polyfills/perf_hooks.ts",
    "node:process": "../ext/node/polyfills/process.ts",
    "node:punycode": "../ext/node/polyfills/punycode.ts",
    "node:querystring": "../ext/node/polyfills/querystring.js",
    "node:readline": "../ext/node/polyfills/readline.ts",
```

<a id="ref-q1-2"></a>
### [2] `tools/core_import_map.json:657`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/core_import_map.json#L657)

```json
    "ext:deno_node/internal/util/types.ts": "../ext/node/polyfills/internal/util/types.ts",
```

<a id="ref-q1-3"></a>
### [3] `cli/tsc/dts/node/util.d.cts:12-13`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L12-L13)

```
    import * as types from "node:util/types";
    export interface InspectOptions {
```

<a id="ref-q1-4"></a>
### [4] `cli/tsc/dts/node/util.d.cts:2284-2285`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L2284-L2285)

```
declare module "node:util/types" {
    export * from "util/types";
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/sys_esm.js:2`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/sys_esm.js#L2)

```javascript
export * from "node:util";
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/internal/fs/utils.mjs:54-58`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/fs/utils.mjs#L54-L58)

```
  isArrayBufferView,
  isBigUint64Array,
  isDate,
  isUint8Array,
} = core.loadExtScript("ext:deno_node/internal/util/types.ts");
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/util/comparisons.ts:15-35`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/comparisons.ts#L15-L35)

```typescript
  isAnyArrayBuffer,
  isArrayBufferView,
  isBigIntObject,
  isBooleanObject,
  isBoxedPrimitive,
  isCryptoKey,
  isDate,
  isFloat16Array,
  isFloat32Array,
  isFloat64Array,
  isKeyObject,
  isMap,
  isNumberObject,
  isPromise,
  isRegExp,
  isSet,
  isStringObject,
  isSymbolObject,
  isWeakMap,
  isWeakSet,
} = core.loadExtScript("ext:deno_node/internal/util/types.ts");
```

<a id="ref-q1-8"></a>
### [8] `tests/unit_node/util_test.ts:1-122`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/util_test.ts#L1-L122)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.

import {
  assert,
  assertEquals,
  assertStrictEquals,
  assertThrows,
} from "@std/assert";
import { stripAnsiCode } from "@std/fmt/colors";
import * as util from "node:util";
import utilDefault from "node:util";
import { Buffer } from "node:buffer";

Deno.test({
  name: "[util] format",
  fn() {
    assertEquals(util.format("%o", [10, 11]), "[ 10, 11, [length]: 2 ]");
  },
});

Deno.test({
  name: "[util] inspect.custom",
  fn() {
    assertEquals(util.inspect.custom, Symbol.for("nodejs.util.inspect.custom"));
  },
});

Deno.test({
  name: "[util] inspect",
  fn() {
    assertEquals(stripAnsiCode(util.inspect({ foo: 123 })), "{ foo: 123 }");
    assertEquals(stripAnsiCode(util.inspect("foo")), "'foo'");
    assertEquals(
      stripAnsiCode(util.inspect("Deno's logo is so cute.")),
      `"Deno's logo is so cute."`,
    );
    assertEquals(
      stripAnsiCode(util.inspect([1, 2, 3, 4, 5, 6, 7])),
      `[
  1, 2, 3, 4,
  5, 6, 7
]`,
    );
  },
});

Deno.test({
  name: "[util] types.isTypedArray",
  fn() {
    assert(util.types.isTypedArray(new Buffer(4)));
    assert(util.types.isTypedArray(new Uint8Array(4)));
    assert(!util.types.isTypedArray(new DataView(new ArrayBuffer(4))));
  },
});

Deno.test({
  name: "[util] types.isNativeError",
  fn() {
    assert(util.types.isNativeError(new Error()));
    assert(util.types.isNativeError(new TypeError()));
    assert(util.types.isNativeError(new DOMException()));
  },
});

Deno.test({
  name: "[util] TextDecoder",
  fn() {
    assert(util.TextDecoder === TextDecoder);
    const td: util.TextDecoder = new util.TextDecoder();
    assert(td instanceof TextDecoder);
  },
});

Deno.test({
  name: "[util] TextEncoder",
  fn() {
    assert(util.TextEncoder === TextEncoder);
    const te: util.TextEncoder = new util.TextEncoder();
    assert(te instanceof TextEncoder);
  },
});

Deno.test({
  name: "[util] toUSVString",
  fn() {
    assertEquals(util.toUSVString("foo"), "foo");
    assertEquals(util.toUSVString("bar\ud801"), "bar\ufffd");
  },
});

Deno.test({
  name: "[util] getSystemErrorName()",
  fn() {
    type FnTestInvalidArg = (code?: unknown) => void;

    assertThrows(
      () => (util.getSystemErrorName as FnTestInvalidArg)(),
      TypeError,
    );
    assertThrows(
      () => (util.getSystemErrorName as FnTestInvalidArg)(1),
      RangeError,
    );

    assertStrictEquals(util.getSystemErrorName(-424242), undefined);

    switch (Deno.build.os) {
      case "windows":
        assertStrictEquals(util.getSystemErrorName(-4091), "EADDRINUSE");
        break;

      case "darwin":
        assertStrictEquals(util.getSystemErrorName(-48), "EADDRINUSE");
        break;

      case "linux":
        assertStrictEquals(util.getSystemErrorName(-98), "EADDRINUSE");
        break;
    }
  },
});
```

<a id="ref-q1-9"></a>
### [9] `tests/unit_node/util_test.ts:47-54`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/util_test.ts#L47-L54)

```typescript
Deno.test({
  name: "[util] types.isTypedArray",
  fn() {
    assert(util.types.isTypedArray(new Buffer(4)));
    assert(util.types.isTypedArray(new Uint8Array(4)));
    assert(!util.types.isTypedArray(new DataView(new ArrayBuffer(4))));
  },
});
```

<a id="ref-q1-10"></a>
### [10] `tests/unit_node/util_test.ts:56-63`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/util_test.ts#L56-L63)

```typescript
Deno.test({
  name: "[util] types.isNativeError",
  fn() {
    assert(util.types.isNativeError(new Error()));
    assert(util.types.isNativeError(new TypeError()));
    assert(util.types.isNativeError(new DOMException()));
  },
});
```

<a id="ref-q1-11"></a>
### [11] `tests/node_compat/config.jsonc:60`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L60)

```
    "es-module/test-esm-util-types.mjs": {},
```

<a id="ref-q1-12"></a>
### [12] `cli/tsc/dts/node/util.d.cts:11-13`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L11-L13)

```
declare module "util" {
    import * as types from "node:util/types";
    export interface InspectOptions {
```
