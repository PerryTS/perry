Programs no longer abort with "runtime layout X is born, but the program's
realm does not reach it" after #13783. Layout probes (is this object or
prototype on layout X?) compare X's fixed word instead of naming its
descriptor; the runtime paths that birth a layout from a builtin the
program does reach now declare it (http/http2 server close, fetch body
reads, http2 compat responses); `createRequire(...)` results and
`process.getBuiltinModule(...)`, also through `globalThis.process`, load
modules for realm reach like `require`. Runtime-internal completions (zlib
callbacks, fs operations and stream turns, `AbortSignal.timeout`, the http
request timeout, deferred child_process/cluster/ipc/messaging emits) no
longer create a JS `Timeout`/`Immediate` handle: they keep their loop phase
and order and report the async_hooks provider type node reports for each
callback
(`ZLIB`, `FSREQCALLBACK`, `FSREQPROMISE`, `Timeout`, `PIPEWRAP`, `PROCESSWRAP`,
`TickObject`, `MESSAGEPORT`, `Test`; `Immediate` for PerformanceObserver
dispatch). A pending `AbortSignal.timeout(ms)` no longer keeps the process
alive, as in node.
