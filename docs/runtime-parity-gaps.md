# Perry Runtime Parity Gap List

> **Generated** by `scripts/gen_parity_gaps.py` from `docs/runtime-parity.md`
> (the API inventory) reconciled against Perry's coverage sources. Do not
> edit by hand — re-run the script to refresh.

This is a structured gap analysis comparing the public Node.js API surface
against the APIs Perry can dispatch. Coverage is derived from four sources:
the unimplemented-API gate manifest (`crates/perry-api-manifest/src/entries.rs`,
`method`/`property` rows), compound `Expr::*` HIR variants
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

Across 49 `node:*` modules: **2222 covered / 296 gap** of 2518 catalogued APIs.

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
| `node:url` | 40 | 9 | 49 |
| `node:fs` | 174 | 8 | 182 |
| `node:readline/promises` | 0 | 7 | 7 |
| `node:assert` | 21 | 6 | 27 |
| `node:buffer` | 102 | 6 | 108 |
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
| `node:worker_threads` | 62 | 2 | 64 |
| `node:async_hooks` | 28 | 1 | 29 |
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
| **Total** | **2222** | **296** | **2518** |

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

**Covered: 40 · Gap: 9**

- `url.toJSON()`
- `new URLSearchParams()`
- `new URLSearchParams(string)`
- `new URLSearchParams(obj)`
- `new URLSearchParams(iterable)`
- `params.entries()`
- `params.keys()`
- `params.values()`
- `params[Symbol.iterator]()`

### node:fs

**Covered: 174 · Gap: 8**

- `fs.realpath.native(path[, options], callback)`
- `fs.realpathSync.native(path[, options])`
- `stats.dev`
- `stats.ino`
- `stats.nlink`
- `stats.rdev`
- `stats.size`
- `stats.blksize`

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

**Covered: 102 · Gap: 6**

- `Buffer.allocUnsafeSlow(size)`
- `Buffer.poolSize`
- `buf[index]`
- `buf.parent`
- `new buffer.Blob([sources[, options]])`
- `new buffer.File(sources, fileName[, options])`

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

**Covered: 62 · Gap: 2**

- `new Worker(filename[, options])`
- `worker[Symbol.asyncDispose]()`

### node:async_hooks

**Covered: 28 · Gap: 1**

- `new AsyncLocalStorage()`

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

