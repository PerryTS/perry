# Perry Runtime Parity Gap List

> **Generated** by `scripts/gen_parity_gaps.py` from `docs/runtime-parity.md`
> (the API inventory) reconciled against Perry's coverage sources. Normally
> do not edit by hand — re-run the script to refresh. **As of 2026-07-17 the
> script's worst bug (a stale single-file manifest path) is fixed** — see the
> audit note under Summary — but a dry run still undercounts by a handful of
> rows relative to this file, so it still carries a small set of
> hand-corrections instead of being freshly emitted.

This is a structured gap analysis comparing the public Node.js API surface
against the APIs Perry can dispatch. Coverage is derived from four sources:
the unimplemented-API gate manifest (`crates/perry-api-manifest/src/entries.rs`
and `entries/*.rs`, `method`/`property` rows), compound `Expr::*` HIR variants
(`crates/perry-hir/src/ir/`), `js_*` FFI exports across `perry-runtime` /
`perry-stdlib` / `perry-ext-*`, and module-gated method-dispatch literals.

> **Behavioral status.** This list counts individual API *surface* gaps, not
> behavioral pass rate. Measured against Node's own test suite
> (`scripts/node_suite_run.py` vs `test-parity/node_suite_baseline.json`),
> Perry's runtime passes **~97%**; overall Node.js/TypeScript compatibility is
> around **95%**. Heavily-used modules (`fs`, `http`/`https`/`http2`,
> `net`/`tls`, `crypto`, `stream`, `events`, `child_process`,
> `worker_threads`, `process`, `zlib`) are real, not stubs.

## Summary

Across 49 `node:*` modules: **2238 covered / 280 gap** of 2518 catalogued APIs.

> **Audit note (2026-07-16, updated 2026-07-17):** spot-checking ~10 "gap"
> entries against the current source turned up 16 false positives (below)
> that the generator missed. Two distinct root causes:
>
> 1. `crates/perry-api-manifest/src/entries.rs` was split into
>    `entries/part_1.rs`..`part_4.rs` (2026-07-03) after this file's last
>    regeneration (2026-06-15); `scripts/gen_parity_gaps.py`'s `MANIFEST`
>    constant still pointed at the now-mostly-empty `entries.rs`, so a
>    dry run showed 1750 covered / 768 gap (e.g. `node:os` swinging from 0
>    gaps to 143) — most of the manifest was invisible to the script. **This
>    is now fixed**: the script globs `entries.rs` + `entries/*.rs` and fails
>    loudly if the parsed entry count looks implausibly low (guards against a
>    future re-split doing this again silently).
> 2. The remaining 13 of the 16 false positives are still invisible to a
>    fresh run even with the manifest path fixed — a dry run with the fixed
>    script lands at **2232 covered / 286 gap**, close to but not identical
>    to the 2238/280 below. Root cause: those 13 rows (the four
>    `new URLSearchParams(...)` overloads, `Buffer.allocUnsafeSlow`, the six
>    `stats.*` numeric fields, `new Worker(filename)`, `new AsyncLocalStorage()`)
>    are declared via the manifest's `class(...)`/`internal_method(...)`
>    macros or dispatched through code paths outside the script's four
>    recognized coverage sources (a dynamic-callee constructor path in
>    `class_registry/construct.rs`, and packed-key-array object shapes like
>    `fs`'s `STATS_KEYS_REGULAR`) — `MANIFEST_RE` only matches bare
>    `method(`/`property(` calls, not `class(`/`internal_method(`/
>    `internal_property(`/`internal_class(`. That's a separate, pre-existing
>    matcher gap, not the file-split bug, and is out of scope for the
>    manifest-path fix; the other 3 false positives (`params.entries()`/
>    `.keys()`/`.values()` on `node:url`) *are* recovered by the path fix.
>    The counts below stay hand-corrected for all 16 until the matcher gap
>    has its own fix; the rest of the list has not been re-audited and may
>    contain further stale entries.

> Web / global APIs and Bun-only APIs are tracked separately in
> `runtime-parity.md`; their coverage is curated, not recomputed here.

| Module | Covered | Gap | Total |
|--------|--------:|----:|------:|
| `node:perf_hooks` | 17 | 39 | 56 |
| `node:http2` | 68 | 34 | 102 |
| `node:test` | 59 | 34 | 93 |
| `node:util` | 84 | 21 | 105 |
| `node:tls` | 35 | 18 | 53 |
| `node:v8` | 41 | 17 | 58 |
| `node:process` | 106 | 12 | 118 |
| `node:stream/web` | 58 | 10 | 68 |
| `node:inspector` | 10 | 9 | 19 |
| `node:module` | 41 | 9 | 50 |
| `node:timers` | 8 | 9 | 17 |
| `node:url` | 47 | 2 | 49 |
| `node:fs` | 180 | 2 | 182 |
| `node:readline/promises` | 0 | 7 | 7 |
| `node:assert` | 21 | 6 | 27 |
| `node:buffer` | 103 | 5 | 108 |
| `node:cluster` | 29 | 6 | 35 |
| `node:events` | 35 | 6 | 41 |
| `node:trace_events` | 0 | 6 | 6 |
| `node:crypto` | 133 | 5 | 138 |
| `node:tty` | 15 | 4 | 19 |
| `node:https` | 21 | 3 | 24 |
| `node:child_process` | 35 | 2 | 37 |
| `node:readline` | 27 | 2 | 29 |
| `node:sqlite` | 50 | 2 | 52 |
| `node:timers/promises` | 3 | 2 | 5 |
| `node:worker_threads` | 63 | 1 | 64 |
| `node:async_hooks` | 29 | 0 | 29 |
| `node:console` | 22 | 1 | 23 |
| `node:dgram` | 27 | 1 | 28 |
| `node:fs/promises` | 60 | 1 | 61 |
| `node:http` | 140 | 1 | 141 |
| `node:net` | 77 | 1 | 78 |
| `node:stream` | 80 | 1 | 81 |
| `node:zlib` | 90 | 1 | 91 |
| `node:diagnostics_channel` | 30 | 0 | 30 |
| `node:dns` | 53 | 0 | 53 |
| `node:dns/promises` | 21 | 0 | 21 |
| `node:domain` | 10 | 0 | 10 |
| `node:os` | 209 | 0 | 209 |
| `node:path` | 16 | 0 | 16 |
| `node:punycode` | 8 | 0 | 8 |
| `node:querystring` | 7 | 0 | 7 |
| `node:repl` | 17 | 0 | 17 |
| `node:stream/consumers` | 6 | 0 | 6 |
| `node:stream/promises` | 3 | 0 | 3 |
| `node:string_decoder` | 6 | 0 | 6 |
| `node:vm` | 32 | 0 | 32 |
| `node:wasi` | 6 | 0 | 6 |
| **Total** | **2238** | **280** | **2518** |

## Per-module gaps

Only modules with at least one remaining gap are listed, in descending
gap-size order. Modules omitted here have **zero** catalogued gaps.

### node:perf_hooks

**Covered: 17 · Gap: 39**

- `performance.clearMarks([name])`
- `performance.clearMeasures([name])`
- `performance.clearResourceTimings([name])`
- `performance.getEntries()`
- `performance.getEntriesByName(name[, type])`
- `performance.getEntriesByType(type)`
- `performance.eventLoopUtilization([util1[, util2]])`
- `performance.setResourceTimingBufferSize(maxSize)`
- `performance.markResourceTiming(...)`
- `performance.toJSON()`
- `performance.nodeTiming`
- `performance.timeOrigin`
- `entry.entryType`
- `entry.flags`
- `entry.kind`
- `nodeStart`
- `v8Start`
- `environment`
- `bootstrapComplete`
- `loopStart`
- `loopExit`
- `idleTime`
- `uvMetricsInfo`
- `new PerformanceObserver(callback)`
- `PerformanceObserver.supportedEntryTypes`
- `list.getEntries()`
- `list.getEntriesByName(name[, type])`
- `list.getEntriesByType(type)`
- `histogram.mean`
- `histogram.stddev`
- `histogram.percentileBigInt(percentile)`
- `histogram.reset()`
- `histogram.enable()`
- `histogram.disable()`
- `histogram[Symbol.dispose]()`
- `histogram.record(val)`
- `histogram.recordDelta()`
- `histogram.add(other)`
- `perf_hooks.eventLoopUtilization([util1[, util2]])`

### node:http2

**Covered: 68 · Gap: 34**

- `session.originSet`
- `serverSession.altsvc(alt, originOrStream)`
- `stream.id`
- `stream.sentInfoHeaders`
- `stream.sentTrailers`
- `serverStream.pushAllowed`
- `http2Server[Symbol.asyncDispose]()`
- `http2Server.timeout`
- `http2Server.updateSettings([settings])`
- `request.authority`
- `request.complete`
- `request.httpVersion`
- `request.rawHeaders`
- `request.rawTrailers`
- `request.scheme`
- `request.trailers`
- `response.addTrailers(headers)`
- `response.appendHeader(name, value)`
- `response.createPushResponse(headers, callback)`
- `response.finished`
- `response.getHeader(name)`
- `response.getHeaderNames()`
- `response.hasHeader(name)`
- `response.removeHeader(name)`
- `response.req`
- `response.sendDate`
- `response.setHeader(name, value)`
- `response.statusCode`
- `response.statusMessage`
- `response.writableEnded`
- `response.write(chunk[, encoding][, callback])`
- `response.writeContinue()`
- `response.writeEarlyHints(hints)`
- `response.writeHead(statusCode[, statusMessage][, headers])`

### node:test

**Covered: 59 · Gap: 34**

- `t.runOnly(shouldRunOnlyTests)`
- `t.waitFor(condition[, options])`
- `t.fullName`
- `t.filePath`
- `t.passed`
- `t.attempt`
- `t.workerId`
- `s.fullName`
- `s.filePath`
- `s.passed`
- `s.attempt`
- `mock.module(specifier[, options])`
- `mock.accesses`
- `mock.accessCount()`
- `mock.resetAccesses()`
- `tap`
- `dot`
- `junit`
- `lcov`
- `'test:start'`
- `'test:plan'`
- `'test:pass'`
- `'test:fail'`
- `'test:complete'`
- `'test:diagnostic'`
- `'test:coverage'`
- `'test:enqueue'`
- `'test:dequeue'`
- `'test:watch:drained'`
- `'test:watch:restarted'`
- `'test:stderr'`
- `'test:stdout'`
- `'test:summary'`
- `'test:interrupted'`

### node:util

**Covered: 84 · Gap: 21**

- `MIMEType.prototype.params`
- `util.inspect.custom`
- `util.inspect.defaultOptions`
- `util.inspect.styles`
- `util.inspect.colors`
- `util.promisify.custom`
- `util.isArray(object)`
- `util.isBoolean(object)`
- `util.isBuffer(object)`
- `util.isError(object)`
- `util.isFunction(object)`
- `util.isNull(object)`
- `util.isNullOrUndefined(object)`
- `util.isNumber(object)`
- `util.isObject(object)`
- `util.isPrimitive(object)`
- `util.isString(object)`
- `util.isSymbol(object)`
- `util.isUndefined(object)`
- `util.print(...args)`
- `util.puts(...args)`

### node:tls

**Covered: 35 · Gap: 18**

- `tls.createSecurePair([context][, isServer][, requestCert][, rejectUnauthorized][, options])`
- `server.addContext(hostname, context)`
- `tlsSocket.localAddress`
- `tlsSocket.localPort`
- `tlsSocket.remoteAddress`
- `tlsSocket.remoteFamily`
- `tlsSocket.remotePort`
- `tlsSocket.disableRenegotiation()`
- `tlsSocket.enableTrace()`
- `tlsSocket.getEphemeralKeyInfo()`
- `tlsSocket.getFinished()`
- `tlsSocket.getPeerFinished()`
- `tlsSocket.getPeerX509Certificate()`
- `tlsSocket.getSharedSigalgs()`
- `tlsSocket.getTLSTicket()`
- `tlsSocket.getX509Certificate()`
- `tlsSocket.renegotiate(options, callback)`
- `tlsSocket.setKeyCert(context)`

### node:v8

**Covered: 41 · Gap: 17**

- `v8.startHeapProfile([options])`
- `new Serializer()`
- `transferArrayBuffer(id, arrayBuffer)`
- `_writeHostObject(object)`
- `_getDataCloneError(message)`
- `_getSharedArrayBufferId(sab)`
- `_setTreatArrayBufferViewsAsHostObjects(flag)`
- `new Deserializer(buffer)`
- `transferArrayBuffer(id, arrayBuffer)`
- `getWireFormatVersion()`
- `_readHostObject()`
- `v8.DefaultSerializer`
- `v8.DefaultDeserializer`
- `new GCProfiler()`
- `[Symbol.dispose]()`
- `[Symbol.dispose]()`
- `[Symbol.asyncDispose]()`

### node:process

**Covered: 106 · Gap: 12**

- `process.mainModule`
- `process.features.uv`
- `process.noDeprecation`
- `process.throwDeprecation`
- `process.traceDeprecation`
- `process.traceProcessWarnings`
- `'uncaughtExceptionMonitor'`
- `'unhandledRejection'`
- `'rejectionHandled'`
- `'workerMessage'`
- `'SIGWINCH'`
- `'SIGBREAK'`

### node:stream/web

**Covered: 58 · Gap: 10**

- `new ReadableStream([underlyingSource[, strategy]])`
- `new ReadableStreamDefaultReader(stream)`
- `new ReadableStreamBYOBReader(stream)`
- `new WritableStream([underlyingSink[, strategy]])`
- `new WritableStreamDefaultWriter(stream)`
- `new TransformStream([transformer[, writableStrategy[, readableStrategy]]])`
- `new ByteLengthQueuingStrategy(init)`
- `new CountQueuingStrategy(init)`
- `new TextEncoderStream()`
- `new TextDecoderStream([encoding[, options]])`

### node:inspector

**Covered: 10 · Gap: 9**

- `inspector.Network.requestWillBeSent(params)`
- `inspector.Network.responseReceived(params)`
- `inspector.Network.dataReceived(params)`
- `inspector.Network.dataSent(params)`
- `inspector.Network.loadingFinished(params)`
- `inspector.Network.loadingFailed(params)`
- `inspector.Network.webSocketCreated(params)`
- `inspector.Network.webSocketHandshakeResponseReceived(params)`
- `inspector.Network.webSocketClosed(params)`

### node:module

**Covered: 41 · Gap: 9**

- `Module.constants.compileCacheStatus`
- `module.children`
- `module.id`
- `module.loaded`
- `module.parent`
- `module.isPreloading`
- `sourceMap.payload`
- `sourceMap.findEntry(lineOffset, columnOffset)`
- `sourceMap.findOrigin(lineNumber, columnNumber)`

### node:timers

**Covered: 8 · Gap: 9**

- `immediate.unref()`
- `immediate.hasRef()`
- `immediate[Symbol.dispose]()`
- `timeout.unref()`
- `timeout.hasRef()`
- `timeout.refresh()`
- `timeout.close()`
- `timeout[Symbol.toPrimitive]()`
- `timeout[Symbol.dispose]()`

### node:url

**Covered: 47 · Gap: 2**

- `url.toJSON()`
- `params[Symbol.iterator]()`

> `new URLSearchParams()` (all four init-shape overloads) and
> `params.entries()`/`.keys()`/`.values()` are implemented
> (`crates/perry-runtime/src/url/search_params.rs`: `js_url_search_params_new_any`/
> `_new_empty`, dispatched from `object/class_registry/construct.rs`;
> `js_url_search_params_entries_arr`/`_keys_arr`/`_values_arr`) — removed
> from this list 2026-07-16. `params[Symbol.iterator]()` specifically
> wasn't found wired to a distinct dispatch path in a quick check, so it's
> left as an unverified gap.

### node:fs

**Covered: 180 · Gap: 2**

- `fs.realpath.native(path[, options], callback)`
- `fs.realpathSync.native(path[, options])`

> `stats.dev`/`.ino`/`.nlink`/`.rdev`/`.size`/`.blksize` are all populated
> fields on the `Stats` object (`crates/perry-runtime/src/fs/stats.rs`
> `STATS_KEYS_REGULAR`/`STATS_KEYS_BIGINT` include all six) — removed from
> this list 2026-07-16. The `fs.realpath.native`/`realpathSync.native`
> entries are unverified; left as-is.

### node:readline/promises

**Covered: 0 · Gap: 7**

- `readlinePromises.createInterface(options)`
- `rl.clearLine(dir)`
- `rl.clearScreenDown()`
- `rl.cursorTo(x[, y])`
- `rl.moveCursor(dx, dy)`
- `rl.commit()`
- `rl.rollback()`

### node:assert

**Covered: 21 · Gap: 6**

- `assert.CallTracker`
- `tracker.calls(fn[, exact])`
- `tracker.getCalls(fn)`
- `tracker.report()`
- `tracker.reset([fn])`
- `tracker.verify()`

### node:buffer

**Covered: 103 · Gap: 5**

- `Buffer.poolSize`
- `buf[index]`
- `buf.parent`
- `new buffer.Blob([sources[, options]])`
- `new buffer.File(sources, fileName[, options])`

> `Buffer.allocUnsafeSlow(size)` is implemented
> (`crates/perry-runtime/src/object/native_module_dispatch/dispatch_a_c.rs`
> `("buffer.Buffer", "allocUnsafeSlow")`, backed by
> `crates/perry-runtime/src/buffer/validate.rs`) — removed from this list
> 2026-07-16.

### node:cluster

**Covered: 29 · Gap: 6**

- `'message'`
- `'setup'`
- `worker.id`
- `worker.send(message[, sendHandle[, options]][, callback])`
- `'message'`
- `'error'`

### node:events

**Covered: 35 · Gap: 6**

- `EventEmitter.prototype[Symbol.for('nodejs.rejection')]()`
- `Event.prototype.composedPath()`
- `Event.prototype.initEvent(type, bubbles, cancelable)`
- `Event.prototype.preventDefault()`
- `Event.prototype.stopImmediatePropagation()`
- `Event.prototype.stopPropagation()`

### node:trace_events

**Covered: 0 · Gap: 6**

- `trace_events.createTracing(options)`
- `trace_events.getEnabledCategories()`
- `tracing.categories`
- `tracing.enabled`
- `tracing.enable()`
- `tracing.disable()`

### node:crypto

**Covered: 133 · Gap: 5**

- `crypto.setEngine(engine[, flags])`
- `crypto.fips`
- `KeyObject.from(key)`
- `new X509Certificate(buffer)`
- `x509.ca`

### node:tty

**Covered: 15 · Gap: 4**

- `readStream.isTTY`
- `readStream.fd`
- `writeStream.isTTY`
- `writeStream.fd`

### node:https

**Covered: 21 · Gap: 3**

- `agent.keepSocketAlive(socket)`
- `agent.reuseSocket(socket, request)`
- `server[Symbol.asyncDispose]()`

### node:child_process

**Covered: 35 · Gap: 2**

- `child_process.ChildProcess`
- `child_process.Stream`

### node:readline

**Covered: 27 · Gap: 2**

- `rl[Symbol.dispose]()`
- `rl.cursor`

### node:sqlite

**Covered: 50 · Gap: 2**

- `db.serialize([dbName])`
- `db.deserialize(buffer[, options])`

### node:timers/promises

**Covered: 3 · Gap: 2**

- `scheduler.wait(delay[, options])`
- `scheduler.yield()`

### node:worker_threads

**Covered: 63 · Gap: 1**

- `worker[Symbol.asyncDispose]()`

> `new Worker(filename[, options])` is implemented — dedicated
> `Expr::WorkerNew` HIR variant (`crates/perry-hir/src/lower/expr_new.rs`),
> `js_worker_threads_worker_new` runtime function — removed from this list
> 2026-07-16.

### node:console

**Covered: 22 · Gap: 1**

- `new Console(stdout[, stderr][, ignoreErrors])`

### node:dgram

**Covered: 27 · Gap: 1**

- `socket[Symbol.asyncDispose]()`

### node:fs/promises

**Covered: 60 · Gap: 1**

- `filehandle.fd`

### node:http

**Covered: 140 · Gap: 1**

- `server[Symbol.asyncDispose]()`

### node:net

**Covered: 77 · Gap: 1**

- `server[Symbol.asyncDispose]()`

### node:stream

**Covered: 80 · Gap: 1**

- `writable.writableAborted`

### node:zlib

**Covered: 90 · Gap: 1**

- `zlib.bytesRead`

## Methodology & caveats

- **Coverage = dispatchable, not byte-for-byte.** A manifest/FFI match means
  Perry can dispatch the call, not that every option/overload matches Node.
- **Module-gated dispatch.** Method-name string literals only count for
  modules that have a real implementation (a manifest entry or a
  `js_<module>_*` FFI export), so stub files naming methods in error strings
  don't read as covered.
- **Manual coverage overrides.** A few APIs are implemented in generic,
  non-module-named dispatchers (e.g. `KeyObject` property access in
  `perry-runtime/src/object/field_get_set.rs`). These are credited via an
  audited `MANUAL_COVERAGE` table in the script.
- **Constants & events** are credited as a block when the module exposes
  `constants`/`codes` or an `on`/`emit` surface, rather than per-leaf.
- `class X` declaration rows are excluded from counts.

