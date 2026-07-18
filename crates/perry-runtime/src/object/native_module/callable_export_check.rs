use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

/// Whitelist of (module, property) pairs for which property-read should
/// produce a callable handle (a bound-method closure) rather than undefined.
/// Needed so `typeof tty.ReadStream === "function"` matches Node — the
/// method-call form (`tty.isatty(0)`) is already handled by a dedicated
/// codegen path, this just keeps the property-read form coherent.
///
/// Issue #894: also list `("events", "EventEmitter")` here so pino's
/// `const { EventEmitter } = require('node:events'); /* ... */
/// Object.setPrototypeOf(prototype, EventEmitter.prototype)` survives —
/// pre-fix `EventEmitter` was `undefined`, and the subsequent
/// `EventEmitter.prototype` read threw a spec TypeError at module init.
/// Returning a callable closure makes `EventEmitter` truthy and gives
/// `typeof EventEmitter === "function"` (matching Node); the chained
/// `.prototype` read on a closure pointer returns `undefined` (no method
/// dispatch table tracks `.prototype` on closures), which
/// `Object.setPrototypeOf` then ignores (Perry's runtime helper is a
/// no-op anyway). `new EventEmitter()` still routes through the dedicated
/// builtin path at lower_call/builtin.rs that allocates a real
/// `EventEmitterHandle`, so dispatch coherence is preserved.
pub(crate) fn is_native_module_callable_export(module: &str, prop: &str) -> bool {
    let module = cjs_default_base_module(module).unwrap_or(module);
    let module = assert_instance_base_module(module).unwrap_or(module);
    let prop = canonical_native_callable_property(module, prop);
    if module == "vm" && matches!(prop, "Module" | "SourceTextModule" | "SyntheticModule") {
        return crate::node_vm::vm_modules_enabled();
    }
    if module == "fs" && matches!(prop, "lchmod" | "lchmodSync") {
        return crate::fs::lchmod_is_callable_on_this_platform();
    }
    if matches!(module, "path" | "path.posix" | "path.win32")
        && matches!(
            prop,
            "join"
                | "dirname"
                | "basename"
                | "extname"
                | "resolve"
                | "isAbsolute"
                | "relative"
                | "normalize"
                | "parse"
                | "format"
                | "toNamespacedPath"
                | "matchesGlob"
        )
    {
        return true;
    }
    if matches!(module, "dns" | "dns/promises")
        && matches!(
            prop,
            "lookup"
                | "lookupService"
                | "resolve"
                | "resolve4"
                | "resolve6"
                | "resolveAny"
                | "resolveCaa"
                | "resolveCname"
                | "resolveMx"
                | "resolveNaptr"
                | "resolveNs"
                | "resolvePtr"
                | "resolveSoa"
                | "resolveSrv"
                | "resolveTlsa"
                | "resolveTxt"
                | "reverse"
                | "getServers"
                | "setServers"
                | "setDefaultResultOrder"
                | "getDefaultResultOrder"
                | "Resolver"
        )
    {
        return true;
    }

    matches!(
        (module, prop),
        // #1533: node:stream `promises` namespace exports.
        ("stream/promises", "pipeline")
            | ("stream/promises", "finished")
            | (
                "readline",
                // #3698: `createInterface` is a callable export too (the
                // named import must be function-valued, matching Node).
                "createInterface"
                    | "clearLine"
                    | "clearScreenDown"
                    | "cursorTo"
                    | "moveCursor"
                    | "emitKeypressEvents",
            )
            // #3212: node:readline/promises callable exports.
            | (
                "readline/promises",
                "createInterface" | "Interface" | "Readline",
            )
            | (
                "inspector",
                "open" | "close" | "url" | "waitForDebugger" | "Session",
            )
            | (
                "inspector.Network",
                "requestWillBeSent"
                    | "responseReceived"
                    | "loadingFinished"
                    | "loadingFailed"
                    | "dataSent"
                    | "dataReceived"
                    | "webSocketCreated"
                    | "webSocketClosed"
                    | "webSocketHandshakeResponseReceived",
            )
            | ("inspector/promises", "Session")
            | (
                "inspector.Session" | "inspector/promises.Session",
                "connect" | "connectToMainThread" | "disconnect" | "post" | "on" | "once",
            )
            // #3712: node:http module-level helper exports. `validateHeaderName`
            // / `validateHeaderValue` perform Node's HTTP-token / header-value
            // validation (throwing the matching error codes); the parser/proxy
            // setters are deterministic no-ops in Perry's runtime.
            | ("http", "validateHeaderName")
            | ("http", "validateHeaderValue")
            | ("http", "setMaxIdleHTTPParsers")
            | ("http", "setGlobalProxyFromEnv")
            | ("http", "_connectionListener")
            | ("module", "Module")
            | ("module", "createRequire")
            | ("module", "Module")
            | ("module", "findPackageJSON")
            | ("module", "findSourceMap")
            | ("module", "flushCompileCache")
            | ("module", "getCompileCacheDir")
            | ("module", "getSourceMapsSupport")
            | ("module", "_findPath")
            | ("module", "_initPaths")
            | ("module", "_load")
            | ("module", "_nodeModulePaths")
            | ("module", "_preloadModules")
            | ("module", "_resolveFilename")
            | ("module", "_resolveLookupPaths")
            | ("module", "register")
            | ("module", "registerHooks")
            | ("module", "runMain")
            | ("module", "setSourceMapsSupport")
            | ("module", "stripTypeScriptTypes")
            | ("module", "syncBuiltinESMExports")
            | ("module", "enableCompileCache")
            | ("module", "isBuiltin")
            | ("module", "SourceMap")
            | ("sqlite", "DatabaseSync")
            | ("sqlite", "Session")
            | ("sqlite", "StatementSync")
            | ("domain", "Domain")
            | ("domain", "createDomain")
            | ("domain", "create")
            | ("dgram", "createSocket")
            | ("dgram", "Socket")
            | ("process", "abort")
            | ("process", "cwd")
            | ("process", "uptime")
            | ("process", "memoryUsage")
            | ("process", "nextTick")
            | ("process", "chdir")
            | ("process", "kill")
            | ("process", "exit")
            | ("process", "umask")
            | ("process", "setSourceMapsEnabled")
            | ("process", "hasUncaughtExceptionCaptureCallback")
            | ("process", "setUncaughtExceptionCaptureCallback")
            | ("process", "addUncaughtExceptionCaptureCallback")
            | ("process", "threadCpuUsage")
            | ("process", "availableMemory")
            | ("process", "constrainedMemory")
            | ("process", "getuid")
            | ("process", "geteuid")
            | ("process", "getgid")
            | ("process", "getegid")
            | ("process", "getgroups")
            | ("process", "setuid")
            | ("process", "seteuid")
            | ("process", "setgid")
            | ("process", "setegid")
            | ("process", "setgroups")
            | ("process", "initgroups")
            | ("process", "emitWarning")
            | ("process", "on")
            | ("process", "addListener")
            | ("process", "once")
            | ("process", "prependListener")
            | ("process", "prependOnceListener")
            | ("process", "emit")
            | ("process", "listeners")
            | ("process", "rawListeners")
            | ("process", "eventNames")
            | ("process", "listenerCount")
            | ("process", "removeListener")
            | ("process", "off")
            | ("process", "removeAllListeners")
            | ("process", "setMaxListeners")
            | ("process", "getMaxListeners")
            | ("process", "getBuiltinModule")
            | ("process", "execve")
            | ("process", "ref")
            | ("process", "unref")
            | ("process", "binding")
            | ("process", "_linkedBinding")
            | ("process", "dlopen")
            | ("process", "_rawDebug")
            | ("process", "_debugProcess")
            | ("process", "_debugEnd")
            | ("process", "_startProfilerIdleNotifier")
            | ("process", "_stopProfilerIdleNotifier")
            | ("process", "reallyExit")
            | ("process", "_fatalException")
            | ("process", "_tickCallback")
            | ("process", "_getActiveHandles")
            | ("process", "_getActiveRequests")
            | ("process", "openStdin")
            | ("process", "_kill")
            | ("process", "cpuUsage")
            | ("process", "resourceUsage")
            | ("process", "getActiveResourcesInfo")
            | ("process", "hrtime")
            | ("worker_threads", "getEnvironmentData")
            | ("worker_threads", "setEnvironmentData")
            | ("worker_threads", "markAsUntransferable")
            | ("worker_threads", "isMarkedAsUntransferable")
            | ("worker_threads", "markAsUncloneable")
            | ("worker_threads", "moveMessagePortToContext")
            | ("worker_threads", "receiveMessageOnPort")
            | ("worker_threads", "postMessageToThread")
            | ("worker_threads", "Worker")
            | ("worker_threads", "MessageChannel")
            | ("worker_threads", "MessagePort")
            | ("worker_threads", "BroadcastChannel")
            | ("tty", "isatty")
            | ("tty", "ReadStream")
            | ("tty", "WriteStream")
            | ("tls", "getCiphers")
            | ("tls", "getCACertificates")
            | ("tls", "setDefaultCACertificates")
            | ("tls", "checkServerIdentity")
            | ("tls", "createSecureContext")
            | ("tls", "SecureContext")
            | ("wasi", "WASI")
            | ("net", "connect")
            | ("net", "createConnection")
            | ("net", "createServer")
            | ("net", "Server")
            | ("net", "Socket")
            | ("net", "BlockList")
            | ("net", "SocketAddress")
            | ("net", "_normalizeArgs")
            | ("net", "_createServerHandle")
            | ("tls", "connect")
            | ("tls", "createServer")
            | ("tls", "Server")
            | ("tls", "TLSSocket")
            // #1856: `child_process.ChildProcess` reads as `[Function: ChildProcess]`.
            | ("child_process", "ChildProcess")
            // #1857 / #2130: every exported function reads as a bound-method
            // closure so `const spawn = cp.spawn; spawn(...)` (Node's canonical
            // test idiom — `const spawn = require('child_process').spawn`) and
            // `util.promisify(cp.exec)` both detect/wrap them. Method-call form
            // (`cp.spawn(...)`) already lowers through a dedicated codegen path;
            // this just keeps the value-read form coherent so it dispatches
            // through dispatch_native_module_method.
            | ("child_process", "_forkChild")
            | ("child_process", "exec")
            | ("child_process", "execFile")
            | ("child_process", "execSync")
            | ("child_process", "execFileSync")
            | ("child_process", "spawn")
            | ("child_process", "spawnSync")
            | ("child_process", "fork")
            // #6563: `const { spawn } = await import("node-pty")` — the
            // value-read form must be a callable bound-method closure.
            | ("node-pty", "spawn")
            | ("events", "EventEmitter")
            | ("events", "EventEmitterAsyncResource")
            | ("events", "on")
            | ("sqlite", "backup")
            | ("events", "once")
            | ("events", "addAbortListener")
            | ("events", "getEventListeners")
            | ("events", "getMaxListeners")
            | ("events", "listenerCount")
            | ("events", "setMaxListeners")
            | ("events", "init")
            | ("async_hooks", "AsyncLocalStorage")
            | ("async_hooks", "AsyncResource")
            | ("async_hooks", "createHook")
            | ("async_hooks", "executionAsyncId")
            | ("async_hooks", "triggerAsyncId")
            | ("async_hooks", "executionAsyncResource")
            | ("stream", "compose")
            | ("stream", "duplexPair")
            | ("stream", "pipeline")
            | ("stream", "finished")
            | ("stream", "isDisturbed")
            | ("stream", "isErrored")
            | ("stream", "isReadable")
            | ("stream", "isWritable")
            | ("stream", "getDefaultHighWaterMark")
            | ("stream", "setDefaultHighWaterMark")
            | ("stream", "addAbortSignal")
            | ("stream", "_isArrayBufferView")
            | ("stream", "_isUint8Array")
            | ("stream", "_uint8ArrayToBuffer")
            | ("stream", "isDestroyed")
            | ("stream", "Readable")
            | ("stream", "Writable")
            | ("stream", "Duplex")
            | ("stream", "Transform")
            | ("stream", "PassThrough")
            | ("stream", "Stream")
            | ("string_decoder", "StringDecoder")
            | ("assert", "Assert")
            | ("assert", "ok")
            | ("assert", "fail")
            | ("assert", "equal")
            | ("assert", "notEqual")
            | ("assert", "strictEqual")
            | ("assert", "notStrictEqual")
            | ("assert", "deepEqual")
            | ("assert", "notDeepEqual")
            | ("assert", "deepStrictEqual")
            | ("assert", "partialDeepStrictEqual")
            | ("assert", "notDeepStrictEqual")
            | ("assert", "match")
            | ("assert", "doesNotMatch")
            | ("assert", "throws")
            | ("assert", "doesNotThrow")
            | ("assert", "rejects")
            | ("assert", "doesNotReject")
            | ("assert", "ifError")
            | ("assert/strict", "Assert")
            | ("assert/strict", "ok")
            | ("assert/strict", "fail")
            | ("assert/strict", "equal")
            | ("assert/strict", "notEqual")
            | ("assert/strict", "strictEqual")
            | ("assert/strict", "notStrictEqual")
            | ("assert/strict", "deepEqual")
            | ("assert/strict", "notDeepEqual")
            | ("assert/strict", "deepStrictEqual")
            | ("assert/strict", "partialDeepStrictEqual")
            | ("assert/strict", "notDeepStrictEqual")
            | ("assert/strict", "match")
            | ("assert/strict", "doesNotMatch")
            | ("assert/strict", "throws")
            | ("assert/strict", "doesNotThrow")
            | ("assert/strict", "rejects")
            | ("assert/strict", "doesNotReject")
            | ("assert/strict", "ifError")
            | ("os", "platform")
            | ("os", "arch")
            | ("os", "hostname")
            | ("os", "homedir")
            | ("os", "tmpdir")
            | ("os", "totalmem")
            | ("os", "freemem")
            | ("os", "uptime")
            | ("os", "type")
            | ("os", "release")
            | ("os", "cpus")
            | ("os", "networkInterfaces")
            | ("os", "userInfo")
            | ("os", "availableParallelism")
            | ("os", "endianness")
            | ("os", "loadavg")
            | ("os", "machine")
            | ("os", "version")
            | ("os", "getPriority")
            | ("os", "setPriority")
            | ("fs", "accessSync")
            | ("fs", "_toUnixTimestamp")
            | ("fs", "access")
            | ("fs", "appendFile")
            | ("fs", "appendFileSync")
            | ("fs", "chmodSync")
            | ("fs", "chmod")
            | ("fs", "chownSync")
            | ("fs", "chown")
            | ("fs", "copyFile")
            | ("fs", "copyFileSync")
            | ("fs", "cp")
            | ("fs", "cpSync")
            | ("fs", "createReadStream")
            | ("fs", "createWriteStream")
            | ("fs", "Dir")
            | ("fs", "Dirent")
            | ("fs", "existsSync")
            | ("fs", "exists")
            | ("fs", "FileReadStream")
            | ("fs", "FileWriteStream")
            | ("fs", "ReadStream")
            | ("fs", "Utf8Stream")
            | ("fs", "WriteStream")
            | ("fs", "closeSync")
            | ("fs", "close")
            | ("fs", "fdatasync")
            | ("fs", "fdatasyncSync")
            | ("fs", "fstatSync")
            | ("fs", "fstat")
            | ("fs", "fsync")
            | ("fs", "fsyncSync")
            | ("fs", "fchmod")
            | ("fs", "fchmodSync")
            | ("fs", "fchown")
            | ("fs", "fchownSync")
            | ("fs", "futimes")
            | ("fs", "futimesSync")
            | ("fs", "ftruncate")
            | ("fs", "ftruncateSync")
            | ("fs", "glob")
            | ("fs", "globSync")
            | ("fs", "linkSync")
            | ("fs", "link")
            | ("fs", "lchown")
            | ("fs", "lchownSync")
            | ("fs", "lutimes")
            | ("fs", "lutimesSync")
            | ("fs", "mkdir")
            | ("fs", "mkdirSync")
            | ("fs", "mkdtempDisposableSync")
            | ("fs", "mkdtempSync")
            | ("fs", "mkdtemp")
            | ("fs", "openSync")
            | ("fs", "open")
            | ("fs", "openAsBlob")
            | ("fs", "opendir")
            | ("fs", "opendirSync")
            | ("fs", "readFile")
            | ("fs", "readFileSync")
            | ("fs", "read")
            | ("fs", "readSync")
            | ("fs", "readlinkSync")
            | ("fs", "readlink")
            | ("fs", "readvSync")
            | ("fs", "readdir")
            | ("fs", "readdirSync")
            | ("fs", "realpathSync")
            | ("fs", "realpath")
            | ("fs", "rename")
            | ("fs", "renameSync")
            | ("fs", "rm")
            | ("fs", "rmSync")
            | ("fs", "rmdirSync")
            | ("fs", "rmdir")
            | ("fs", "symlinkSync")
            | ("fs", "symlink")
            | ("fs", "stat")
            | ("fs", "lstat")
            | ("fs", "statfs")
            | ("fs", "statfsSync")
            | ("fs", "statSync")
            | ("fs", "Stats")
            | ("fs", "lstatSync")
            | ("fs", "truncateSync")
            | ("fs", "truncate")
            | ("fs", "unlink")
            | ("fs", "unlinkSync")
            | ("fs", "utimes")
            | ("fs", "utimesSync")
            | ("fs", "_toUnixTimestamp")
            | ("fs", "watch")
            | ("fs", "watchFile")
            | ("fs", "unwatchFile")
            | ("fs", "writeFile")
            | ("fs", "writeFileSync")
            | ("fs", "write")
            | ("fs", "writeSync")
            | ("fs", "writev")
            | ("fs", "writevSync")
            | ("fs", "readv")
            // node:perf_hooks — the `performance` object's methods, read as
            // values (`typeof performance.mark === "function"`, `const m =
            // performance.mark`). The call form is statically lowered in
            // module_static.rs; this keeps the property-read form coherent.
            // Also the perf_hooks class exports so `typeof PerformanceObserver
            // === "function"` etc. hold.
            | ("perf_hooks", "now")
            | ("perf_hooks", "mark")
            | ("perf_hooks", "measure")
            | ("perf_hooks", "getEntries")
            | ("perf_hooks", "getEntriesByName")
            | ("perf_hooks", "getEntriesByType")
            | ("perf_hooks", "clearMarks")
            | ("perf_hooks", "clearMeasures")
            | ("perf_hooks", "eventLoopUtilization")
            | ("perf_hooks", "toJSON")
            | ("perf_hooks", "clearResourceTimings")
            | ("perf_hooks", "setResourceTimingBufferSize")
            // performance.markResourceTiming(info) records a resource entry;
            // the property also reads as a function for feature-detection
            // wrappers.
            | ("perf_hooks", "markResourceTiming")
            // performance.timerify(fn) returns a wrapper that preserves the
            // result and emits observer-visible function entries.
            | ("perf_hooks", "timerify")
            // `globalThis.crypto` is backed by the `crypto.webcrypto`
            // singleton. Its methods must read as callable bound functions
            // for feature checks and rebound calls.
            | ("crypto.webcrypto", "getRandomValues")
            | ("crypto.webcrypto", "randomUUID")
            | (
                "crypto.subtle",
                "digest"
                    | "importKey"
                    | "exportKey"
                    | "sign"
                    | "verify"
                    | "deriveBits"
                    | "deriveKey"
                    | "encrypt"
                    | "decrypt"
                    | "generateKey"
                    | "wrapKey"
                    | "unwrapKey",
            )
            | ("buffer.Buffer", "from")
            | ("buffer.Buffer", "alloc")
            | ("buffer.Buffer", "allocUnsafe")
            | ("buffer.Buffer", "allocUnsafeSlow")
            | ("buffer.Buffer", "concat")
            | ("buffer.Buffer", "of")
            | ("buffer.Buffer", "isBuffer")
            | ("buffer.Buffer", "isEncoding")
            | ("buffer.Buffer", "byteLength")
            | ("buffer.Buffer", "compare")
            | ("perf_hooks", "Performance")
            | ("perf_hooks", "PerformanceObserver")
            | ("perf_hooks", "PerformanceEntry")
            | ("perf_hooks", "PerformanceMark")
            | ("perf_hooks", "PerformanceMeasure")
            | ("perf_hooks", "PerformanceObserverEntryList")
            | ("perf_hooks", "PerformanceResourceTiming")
            | ("perf_observer", "observe")
            | ("perf_observer", "disconnect")
            | ("perf_observer", "takeRecords")
            | ("perf_observer_list", "getEntries")
            | ("perf_observer_list", "getEntriesByType")
            | ("perf_observer_list", "getEntriesByName")
            // #1336: monitorEventLoopDelay() / createHistogram() return
            // a `perf_histogram`-tagged namespace object. Property reads
            // of method names need to satisfy `typeof h.enable === "function"`.
            | ("perf_hooks", "monitorEventLoopDelay")
            | ("perf_hooks", "createHistogram")
            | ("perf_histogram", "enable")
            | ("perf_histogram", "disable")
            | ("perf_histogram", "reset")
            | ("perf_histogram", "record")
            | ("perf_histogram", "recordDelta")
            | ("perf_histogram", "add")
            | ("perf_histogram", "percentile")
            | ("perf_histogram", "percentileBigInt")
            // node:cluster — namespace property reads of these callables
            // need to satisfy `typeof cluster.fork === "function"` etc.
            // Calls dispatch through the native module method table, where
            // the primary-side settings / Worker lifecycle is implemented.
            | ("cluster", "fork")
            | ("cluster", "disconnect")
            | ("cluster", "setupPrimary")
            | ("cluster", "setupMaster")
            | ("cluster", "Worker")
            | ("buffer.Buffer", "copyBytesFrom")
            | ("buffer", "isAscii")
            | ("buffer", "isUtf8")
            | ("buffer", "atob")
            | ("buffer", "btoa")
            | ("util", "convertProcessSignalToExitCode")
            | ("util", "_errnoException")
            | ("util", "_exceptionWithHostPort")
            | ("util", "_extend")
            | ("util", "format")
            | ("util", "formatWithOptions")
            | ("util", "inspect")
            | ("util", "debug")
            | ("util", "aborted")
            | ("util", "debuglog")
            | ("util", "getCallSites")
            | ("util", "diff")
            | ("util", "getSystemErrorName")
            | ("util", "getSystemErrorMessage")
            | ("util", "getSystemErrorMap")
            | ("util", "parseEnv")
            | ("util", "transferableAbortController")
            | ("util", "transferableAbortSignal")
            | ("util", "isArray")
            | ("util", "promisify")
            | ("util", "callbackify")
            | ("util", "parseArgs")
            | ("util", "deprecate")
            | ("util", "inherits")
            | ("util", "isDeepStrictEqual")
            | ("util", "stripVTControlCharacters")
            | ("util", "styleText")
            | ("util", "toUSVString")
            | ("util", "setTraceSigInt")
            | ("util", "MIMEParams")
            | ("util", "MIMEType")
            | ("sea", "isSea")
            | ("sea", "getAsset")
            | ("sea", "getAssetAsBlob")
            | ("sea", "getRawAsset")
            | ("sea", "getAssetKeys")
            | ("zlib", "Deflate")
            | ("zlib", "DeflateRaw")
            | ("zlib", "Gzip")
            | ("zlib", "Gunzip")
            | ("zlib", "Inflate")
            | ("zlib", "InflateRaw")
            | ("zlib", "Unzip")
            | ("zlib", "BrotliCompress")
            | ("zlib", "BrotliDecompress")
            | ("zlib", "ZstdCompress")
            | ("zlib", "ZstdDecompress")
            | ("zlib", "createZstdCompress")
            | ("zlib", "createZstdDecompress")
            | ("util.types", "isArgumentsObject")
            | ("util.types", "isPromise")
            | ("util.types", "isBigIntObject")
            | ("util.types", "isArrayBuffer")
            | ("util.types", "isSharedArrayBuffer")
            | ("util.types", "isAnyArrayBuffer")
            | ("util.types", "isArrayBufferView")
            | ("util.types", "isDataView")
            | ("util.types", "isTypedArray")
            | ("util.types", "isUint8Array")
            | ("util.types", "isInt8Array")
            | ("util.types", "isInt16Array")
            | ("util.types", "isUint16Array")
            | ("util.types", "isInt32Array")
            | ("util.types", "isUint32Array")
            | ("util.types", "isFloat16Array")
            | ("util.types", "isFloat32Array")
            | ("util.types", "isFloat64Array")
            | ("util.types", "isUint8ClampedArray")
            | ("util.types", "isBigInt64Array")
            | ("util.types", "isBigUint64Array")
            | ("util.types", "isMap")
            | ("util.types", "isMapIterator")
            | ("util.types", "isProxy")
            | ("util.types", "isExternal")
            | ("util.types", "isModuleNamespaceObject")
            | ("util.types", "isSet")
            | ("util.types", "isSetIterator")
            | ("util.types", "isWeakMap")
            | ("util.types", "isWeakSet")
            | ("util.types", "isDate")
            | ("util.types", "isRegExp")
            | ("util.types", "isAsyncFunction")
            | ("util.types", "isGeneratorFunction")
            | ("util.types", "isGeneratorObject")
            | ("util.types", "isNativeError")
            | ("util.types", "isKeyObject")
            | ("util.types", "isCryptoKey")
            | ("util.types", "isNumberObject")
            | ("util.types", "isStringObject")
            | ("util.types", "isBooleanObject")
            | ("util.types", "isSymbolObject")
            | ("util.types", "isBoxedPrimitive")
            | ("util/types", "isArgumentsObject")
            | ("util/types", "isPromise")
            | ("util/types", "isBigIntObject")
            | ("timers", "setTimeout")
            | ("timers", "clearTimeout")
            | ("timers", "setInterval")
            | ("timers", "clearInterval")
            | ("timers", "setImmediate")
            | ("timers", "clearImmediate")
            | ("timers/promises", "setTimeout")
            | ("timers/promises", "setImmediate")
            | ("timers/promises", "setInterval")
            | ("util/types", "isArrayBuffer")
            | ("util/types", "isSharedArrayBuffer")
            | ("util/types", "isAnyArrayBuffer")
            | ("util/types", "isArrayBufferView")
            | ("util/types", "isDataView")
            | ("util/types", "isTypedArray")
            | ("util/types", "isUint8Array")
            | ("util/types", "isInt8Array")
            | ("util/types", "isInt16Array")
            | ("util/types", "isUint16Array")
            | ("util/types", "isInt32Array")
            | ("util/types", "isUint32Array")
            | ("util/types", "isFloat16Array")
            | ("util/types", "isFloat32Array")
            | ("util/types", "isFloat64Array")
            | ("util/types", "isUint8ClampedArray")
            | ("util/types", "isBigInt64Array")
            | ("util/types", "isBigUint64Array")
            | ("util/types", "isMap")
            | ("util/types", "isMapIterator")
            | ("util/types", "isProxy")
            | ("util/types", "isExternal")
            | ("util/types", "isModuleNamespaceObject")
            | ("util/types", "isSet")
            | ("util/types", "isSetIterator")
            | ("util/types", "isWeakMap")
            | ("util/types", "isWeakSet")
            | ("util/types", "isDate")
            | ("util/types", "isRegExp")
            | ("util/types", "isAsyncFunction")
            | ("util/types", "isGeneratorFunction")
            | ("util/types", "isGeneratorObject")
            | ("util/types", "isNativeError")
            | ("util/types", "isKeyObject")
            | ("util/types", "isCryptoKey")
            | ("util/types", "isNumberObject")
            | ("util/types", "isStringObject")
            | ("util/types", "isBooleanObject")
            | ("util/types", "isSymbolObject")
            | ("util/types", "isBoxedPrimitive")
            | ("url", "URL")
            | ("url", "URLSearchParams")
            | ("url", "URLPattern")
            | ("url", "Url")
            | ("url", "fileURLToPath")
            | ("url", "fileURLToPathBuffer")
            | ("url", "pathToFileURL")
            | ("url", "domainToASCII")
            | ("url", "domainToUnicode")
            | ("url", "urlToHttpOptions")
            | ("url", "format")
            | ("url", "parse")
            | ("url", "resolve")
            | ("url", "resolveObject")
            | ("punycode", "decode")
            | ("punycode", "encode")
            | ("punycode", "toASCII")
            | ("punycode", "toUnicode")
            | ("punycode.ucs2", "decode")
            | ("punycode.ucs2", "encode")
            | (
                "querystring",
                "unescapeBuffer" | "unescape" | "escape" | "stringify" | "parse"
            )
            | ("console", "Console")
            | ("console", "log")
            | ("console", "info")
            | ("console", "debug")
            | ("console", "error")
            | ("console", "warn")
            | ("console", "assert")
            | ("console", "dir")
            | ("console", "dirxml")
            | ("console", "trace")
            | ("console", "table")
            | ("console", "clear")
            | ("console", "count")
            | ("console", "countReset")
            | ("console", "time")
            | ("console", "timeEnd")
            | ("console", "timeLog")
            | ("console", "group")
            | ("console", "groupCollapsed")
            | ("console", "groupEnd")
            | ("console", "profile")
            | ("console", "profileEnd")
            | ("console", "timeStamp")
            | ("console", "context")
            | ("console", "createTask")
            | ("crypto", "createHash")
            | ("crypto", "Hash")
            | ("crypto", "createSign")
            | ("crypto", "Sign")
            | ("crypto", "createVerify")
            | ("crypto", "Verify")
            | ("crypto", "ECDH")
            | ("crypto", "createECDH")
            | ("crypto", "createDiffieHellman")
            | ("crypto", "DiffieHellman")
            | ("crypto", "createDiffieHellmanGroup")
            | ("crypto", "DiffieHellmanGroup")
            | ("crypto", "getDiffieHellman")
            | ("crypto", "diffieHellman")
            | ("crypto", "encapsulate")
            | ("crypto", "decapsulate")
            | ("crypto", "createPrivateKey")
            | ("crypto", "createPublicKey")
            | ("crypto", "generateKeyPairSync")
            | ("crypto", "generateKeyPair")
            | ("crypto", "generateKeySync")
            | ("crypto", "generateKey")
            | ("crypto", "createHmac")
            | ("crypto", "Hmac")
            | ("crypto", "pbkdf2Sync")
            | ("crypto", "pbkdf2")
            | ("crypto", "argon2Sync")
            | ("crypto", "argon2")
            | ("crypto", "hash")
            | ("crypto", "hkdfSync")
            | ("crypto", "hkdf")
            | ("crypto", "scryptSync")
            | ("crypto", "scrypt")
            | ("crypto", "timingSafeEqual")
            | ("crypto", "sign")
            | ("crypto", "verify")
            | ("crypto", "publicEncrypt")
            | ("crypto", "privateDecrypt")
            | ("crypto", "privateEncrypt")
            | ("crypto", "publicDecrypt")
            | ("crypto", "getHashes")
            | ("crypto", "getCiphers")
            | ("crypto", "getCipherInfo")
            | ("crypto", "getCurves")
            | ("crypto", "getFips")
            | ("crypto", "setFips")
            | ("crypto", "secureHeapUsed")
            | ("crypto", "randomBytes")
            | ("crypto", "randomUUID")
            | ("crypto", "randomUUIDv7")
            | ("crypto", "randomInt")
            | ("crypto", "generatePrime")
            | ("crypto", "generatePrimeSync")
            | ("crypto", "checkPrime")
            | ("crypto", "checkPrimeSync")
            | ("crypto", "randomFill")
            | ("crypto", "randomFillSync")
            | ("crypto", "getRandomValues")
            | ("crypto", "createCipheriv")
            | ("crypto", "createDecipheriv")
            // #3726: the constructor exports behind the factories read as
            // callable functions so `typeof crypto.Cipheriv === "function"`.
            | ("crypto", "Cipheriv")
            | ("crypto", "Decipheriv")
            | ("crypto", "X509Certificate")
            // #2565: public KeyObject constructor shape plus the supported
            // secret-key `KeyObject.from(CryptoKey)` static helper.
            | ("crypto", "KeyObject")
            | ("crypto.KeyObject", "from")
            | ("crypto", "createSecretKey")
            | ("crypto.Certificate", "verifySpkac")
            | ("crypto.Certificate", "exportPublicKey")
            | ("crypto.Certificate", "exportChallenge")
            // #3142: `(new v8.GCProfiler()).start` / `.stop` read as functions
            // so `typeof profiler.start === "function"` holds.
            | ("v8.GCProfiler", "start")
            | ("v8.GCProfiler", "stop")
            // node:zlib — sync codecs, callback codecs, stream factories and
            // class names read as callables. Needed for `util.promisify(zlib.gzip)`
            // (#1857-style hook), `const compress = zlib.gzipSync`, and
            // feature-checks like `typeof zlib.Deflate === "function"`. The call
            // path still goes through the codegen NATIVE_MODULE_TABLE for direct
            // sites; this just plugs the value-read shape.
            | ("zlib", "gzipSync")
            | ("zlib", "gunzipSync")
            | ("zlib", "deflateSync")
            | ("zlib", "inflateSync")
            | ("zlib", "deflateRawSync")
            | ("zlib", "inflateRawSync")
            | ("zlib", "unzipSync")
            | ("zlib", "brotliCompressSync")
            | ("zlib", "brotliDecompressSync")
            | ("zlib", "zstdCompressSync")
            | ("zlib", "zstdDecompressSync")
            | ("zlib", "crc32")
            | ("zlib", "gzip")
            | ("zlib", "gunzip")
            | ("zlib", "deflate")
            | ("zlib", "inflate")
            | ("zlib", "deflateRaw")
            | ("zlib", "inflateRaw")
            | ("zlib", "unzip")
            | ("zlib", "brotliCompress")
            | ("zlib", "brotliDecompress")
            | ("zlib", "zstdCompress")
            | ("zlib", "zstdDecompress")
            | ("zlib", "createGzip")
            | ("zlib", "createGunzip")
            | ("zlib", "createDeflate")
            | ("zlib", "createInflate")
            | ("zlib", "createDeflateRaw")
            | ("zlib", "createInflateRaw")
            | ("zlib", "createUnzip")
            | ("zlib", "createBrotliCompress")
            | ("zlib", "createBrotliDecompress")
            | ("zlib", "Deflate")
            | ("zlib", "DeflateRaw")
            | ("zlib", "Gzip")
            | ("zlib", "Gunzip")
            | ("zlib", "Inflate")
            | ("zlib", "InflateRaw")
            | ("zlib", "Unzip")
            | ("zlib", "BrotliCompress")
            | ("zlib", "BrotliDecompress")
            // #2533: node:http/https/http2 server factories read as callable
            // values so `const createServer = createServerHTTP` (and
            // `@hono/node-server`'s `options.createServer || createServerHTTP`)
            // produce a bound-method closure instead of undefined. The closure
            // routes back through dispatch_native_module_method → the stdlib
            // http dispatcher (external-http-server-pump). The method-call form
            // already lowers through the codegen NATIVE_MODULE_TABLE.
            | ("http", "createServer")
            | ("http", "Server")
            | ("http", "OutgoingMessage")
            // #4904: Node exposes these as constructable classes on the
            // `http` module (`new http.Agent(opts)`, `new ClientRequest(...)`,
            // `new IncomingMessage(socket)`, `new ServerResponse(req)`), and
            // tests/userland grab them as values first (`const { Agent } =
            // require('http')`). Construction routes through
            // `js_new_function_construct` → the http arm in
            // class_registry.rs → JS_NATIVE_HTTP_DISPATCH.
            | ("http", "Agent")
            | ("http", "ClientRequest")
            | ("http", "IncomingMessage")
            | ("http", "ServerResponse")
            // #4904: `const { get, request } = require('http')` — the https
            // twins below were already exported; the http side was missed.
            | ("http", "request")
            | ("http", "get")
            | ("https", "createServer")
            | ("https", "Server")
            // #3697: `https.request` / `https.get` / `https.Agent` value reads
            // (named/namespace imports) must be function-valued. The call form
            // already lowers through the codegen NATIVE_MODULE_TABLE; without
            // these the bound-value read returned `undefined`.
            | ("https", "request")
            | ("https", "get")
            | ("https", "Agent")
            | ("http2", "createServer")
            | ("http2", "createSecureServer")
            | ("http2", "Server")
            | ("http2", "getDefaultSettings")
            | ("http2", "getPackedSettings")
            | ("http2", "getUnpackedSettings")
            // #3905: `http2.connect(authority[, options][, listener])` client
            // session factory reads as a function.
            | ("http2", "connect")
            // #3720: module-level handshake helper reads as a function.
            | ("http2", "performServerHandshake")
            // #3680/#3679: node:v8 class constructors + diagnostic-control
            // helpers read as callable values (`typeof v8.Serializer ===
            // "function"`). Construction routes through new_dynamic.rs; the
            // top-level helpers are no-op callables.
            | ("v8", "Serializer")
            | ("v8", "DefaultSerializer")
            | ("v8", "Deserializer")
            | ("v8", "DefaultDeserializer")
            | ("v8", "setFlagsFromString")
            | ("v8", "takeCoverage")
            | ("v8", "stopCoverage")
            | ("v8", "setHeapSnapshotNearHeapLimit")
            // #3906: the implemented serialize/heap-introspection helpers read
            // as bound callables too, so `const s = v8.serialize` / `v8[k]`
            // (and `Object.keys(v8).map(k => v8[k])`) match Node instead of
            // returning undefined. Invocation routes through
            // dispatch_native_module_method. `GCProfiler` is a constructor
            // (construction lowers via new_dynamic.rs); the value read is a
            // function per Node.
            | ("v8", "serialize")
            | ("v8", "deserialize")
            | ("v8", "getHeapStatistics")
            | ("v8", "getHeapSpaceStatistics")
            | ("v8", "getHeapCodeStatistics")
            | ("v8", "cachedDataVersionTag")
            | ("v8", "GCProfiler")
            // #3904: modern V8 diagnostics/profiler named exports (function-valued).
            | ("v8", "getCppHeapStatistics")
            | ("v8", "getHeapSnapshot")
            | ("v8", "isStringOneByteRepresentation")
            | ("v8", "queryObjects")
            | ("v8", "startCpuProfile")
            | ("v8", "writeHeapSnapshot")
            // #3127/#3128/#3130/#3284: no-flag node:vm export shape.
            | ("vm", "Script")
            | ("vm", "createContext")
            | ("vm", "createScript")
            | ("vm", "runInContext")
            | ("vm", "runInNewContext")
            | ("vm", "runInThisContext")
            | ("vm", "isContext")
            | ("vm", "compileFunction")
            | ("vm", "measureMemory")
            // #3679: v8.startupSnapshot / v8.promiseHooks namespace methods read
            // as callable values (`typeof v8.startupSnapshot.isBuildingSnapshot
            // === "function"`). Invocation routes through
            // dispatch_native_module_method on the sub-namespace tag.
            | ("v8.startupSnapshot", "isBuildingSnapshot")
            | ("v8.startupSnapshot", "addSerializeCallback")
            | ("v8.startupSnapshot", "addDeserializeCallback")
            | ("v8.startupSnapshot", "setDeserializeMainFunction")
            | ("v8.promiseHooks", "onInit")
            | ("v8.promiseHooks", "onBefore")
            | ("v8.promiseHooks", "onAfter")
            | ("v8.promiseHooks", "onSettled")
            | ("v8.promiseHooks", "createHook")
            | ("repl", "Recoverable")
            | ("repl", "REPLServer")
            | ("repl", "start")
    )
}
