/// Test-only reference: original ladder, oracle for `CALLABLE_EXPORT_ARITY_TABLE`.
#[cfg(test)]
fn native_callable_export_arity_reference(module: &str, prop: &str) -> Option<u32> {
    match (module, prop) {
        // Bun global/module surface (#9599).
        (
            "bun",
            "Glob" | "Terminal" | "Transpiler" | "build" | "connect" | "file" | "fileURLToPath"
            | "gc" | "hash" | "listen" | "pathToFileURL" | "plugin" | "serve" | "sleep"
            | "sleepSync" | "stringWidth" | "stripANSI" | "which" | "zstdDecompress"
            | "zstdDecompressSync",
        ) => Some(1),
        ("bun", "deepEquals" | "generateHeapSnapshot" | "spawn" | "write") => Some(2),
        ("bun", "wrapAnsi") => Some(3),
        ("bun.ant", "getPeerPid" | "getPeerUid") => Some(1),
        ("bun.ant", "memoryPressureLevel") => Some(0),
        ("bun:jsc", "heapStats") => Some(0),
        // bun:ffi (#6562).
        ("bun:ffi", "dlopen") => Some(2),
        ("bun:ffi", "ptr" | "CString" | "CFunction" | "linkSymbols") => Some(1),
        ("bun:ffi", "JSCallback") => Some(2),
        ("bun:ffi", "toArrayBuffer" | "toBuffer") => Some(3),
        ("bun:ffi", "viewSource") => Some(2),
        ("ffi", "dlopen") => Some(2),
        ("ffi", "getRawPointer" | "toString") => Some(1),
        ("ffi", "toArrayBuffer" | "toBuffer") => Some(3),
        // #3687: node:cluster — module-method `.length` matches Node.
        ("cluster", "fork" | "disconnect" | "setupPrimary" | "setupMaster" | "Worker") => Some(1),
        ("cluster", "emit") => Some(1),
        ("cluster", "eventNames") => Some(0),
        ("cluster", "getMaxListeners") => Some(0),
        (
            "cluster",
            "on"
            | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "removeListener"
            | "off"
            | "listenerCount",
        ) => Some(2),
        ("cluster", "listeners" | "rawListeners" | "setMaxListeners") => Some(1),
        ("cluster", "removeAllListeners") => Some(1),
        // #6563: node-pty `spawn(file, args, options)`.
        ("node-pty", "spawn") => Some(3),
        ("events", "EventEmitter") => Some(1),
        ("events", "EventEmitterAsyncResource") => Some(0),
        ("events", "addAbortListener") => Some(2),
        ("events", "once") => Some(2),
        ("events", "on") => Some(2),
        ("events", "getEventListeners") => Some(2),
        ("events", "getMaxListeners") => Some(1),
        ("events", "listenerCount") => Some(2),
        ("events", "setMaxListeners") => Some(0),
        ("querystring", "unescapeBuffer" | "unescape") => Some(2),
        ("querystring", "escape") => Some(1),
        ("querystring", "stringify" | "parse") => Some(4),
        ("async_hooks", "AsyncLocalStorage") => Some(0),
        ("async_hooks", "AsyncResource") => Some(1),
        ("async_hooks", "createHook") => Some(1),
        ("async_hooks", "executionAsyncId") => Some(0),
        ("async_hooks", "triggerAsyncId") => Some(0),
        ("async_hooks", "executionAsyncResource") => Some(0),
        ("url", "URL") => Some(1),
        ("url", "URLPattern") => Some(0),
        ("tls", "getCiphers" | "getCertificateCompressionAlgorithms") => Some(0),
        ("tls", "getCACertificates" | "setDefaultCACertificates" | "createSecureContext") => {
            Some(1)
        }
        ("tls", "checkServerIdentity") => Some(2),
        ("tls", "convertALPNProtocols") => Some(2),
        ("tls", "SecureContext") => Some(1),
        // #3726: `crypto.Cipheriv` / `crypto.Decipheriv` constructor exports —
        // `(cipher, key, iv, options)` arity matches Node's length 4.
        ("crypto", "Cipheriv" | "Decipheriv") => Some(4),
        ("crypto", "X509Certificate") => Some(1),
        ("crypto", "Certificate") => Some(0),
        ("crypto", "KeyObject") => Some(2),
        ("crypto.KeyObject", "from") => Some(1),
        // #2706/#2716 and #2694: crypto module-level callable exports.
        ("crypto", "DiffieHellman") => Some(4),
        ("crypto", "DiffieHellmanGroup") => Some(1),
        ("crypto", "diffieHellman") => Some(2),
        ("crypto", "encapsulate") => Some(2),
        ("crypto", "decapsulate") => Some(3),
        ("crypto", "generateKey" | "generateKeyPair" | "generatePrime") => Some(3),
        ("crypto", "generateKeySync" | "generateKeyPairSync") => Some(2),
        ("crypto", "generatePrimeSync" | "checkPrime" | "checkPrimeSync" | "setFips") => Some(1),
        ("crypto", "secureHeapUsed") => Some(0),
        ("crypto", "hkdf") => Some(6),
        ("crypto", "hkdfSync") => Some(5),
        ("crypto", "scrypt") => Some(4),
        ("crypto", "scryptSync") => Some(3),
        ("crypto", "argon2") => Some(3),
        ("crypto", "argon2Sync") => Some(2),
        ("url", "Url") => Some(0),
        ("url", "resolveObject") => Some(2),
        ("process", "binding" | "_linkedBinding") => Some(1),
        (
            "process",
            "dlopen"
            | "_rawDebug"
            | "_debugProcess"
            | "_debugEnd"
            | "_startProfilerIdleNotifier"
            | "_stopProfilerIdleNotifier"
            | "reallyExit"
            | "_tickCallback"
            | "_getActiveHandles"
            | "_getActiveRequests"
            | "openStdin"
            | "_kill",
        ) => Some(0),
        ("process", "_fatalException") => Some(2),
        ("process", "execve") => Some(1),
        ("process", "ref" | "unref") => Some(1),
        ("process", "setSourceMapsEnabled") => Some(1),
        (
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
        ) => Some(1),
        ("inspector.NetworkResources", "put") => Some(1),
        (
            "inspector.DOMStorage",
            "domStorageItemAdded"
            | "domStorageItemRemoved"
            | "domStorageItemUpdated"
            | "domStorageItemsCleared"
            | "registerStorage",
        ) => Some(1),
        ("inspector.Session", "connect" | "connectToMainThread" | "disconnect") => Some(0),
        ("inspector.Session" | "inspector/promises.Session", "post") => Some(3),
        (
            "process",
            "setUncaughtExceptionCaptureCallback" | "addUncaughtExceptionCaptureCallback",
        ) => Some(1),
        ("process", "hasUncaughtExceptionCaptureCallback") => Some(0),
        ("fs", "_toUnixTimestamp") => Some(1),
        ("util", "isArray") => Some(1),
        ("util", "debug" | "debuglog" | "inherits") => Some(2),
        ("console", "context") => Some(1),
        ("console", "createTask") => Some(0),
        ("util", "MIMEParams") => Some(0),
        ("util", "MIMEType") => Some(1),
        ("sea", "isSea" | "getAssetKeys") => Some(0),
        ("sea", "getRawAsset") => Some(1),
        ("sea", "getAsset" | "getAssetAsBlob") => Some(2),
        ("stream", "pipeline" | "compose") => Some(0),
        ("stream", "finished") => Some(3),
        (
            "stream",
            "duplexPair"
            | "isDisturbed"
            | "isErrored"
            | "isReadable"
            | "isWritable"
            | "getDefaultHighWaterMark"
            | "_isArrayBufferView"
            | "_isUint8Array"
            | "_uint8ArrayToBuffer"
            | "isDestroyed",
        ) => Some(1),
        ("stream", "setDefaultHighWaterMark" | "addAbortSignal") => Some(2),
        ("net", "connect" | "createConnection") => Some(3),
        ("net", "createServer" | "Server") => Some(2),
        ("net", "Socket") => Some(1),
        ("net", "BlockList" | "SocketAddress") => Some(0),
        // #3720: `http2.performServerHandshake(socket[, options])` — length 1.
        ("http2", "performServerHandshake") => Some(1),
        ("http2", "getDefaultSettings") => Some(0),
        ("http2", "getPackedSettings" | "getUnpackedSettings") => Some(1),
        // #3905: Node `.length` — connect(authority,options,listener)=3,
        // createServer(options,handler)=2.
        ("http2", "connect") => Some(3),
        ("http2", "createServer" | "createSecureServer") => Some(2),
        ("http", "OutgoingMessage") => Some(1),
        // #4904: Node `.length` — Agent(options)=1, ClientRequest(input,
        // options, cb)=3, IncomingMessage(socket)=1, ServerResponse(req)=1.
        ("http", "Agent" | "IncomingMessage" | "ServerResponse") => Some(1),
        ("http", "ClientRequest") => Some(3),
        // #3697: node:https module-level exports (Node `.length`).
        ("https", "request") => Some(0),
        ("https", "get") => Some(3),
        ("https", "Agent") => Some(1),
        // #4904: http twins of the https entries above.
        ("http", "request") => Some(0),
        ("http", "get") => Some(3),
        ("stream", "destroy") => Some(2),
        // #3712: node:http module-level helper exports.
        ("http", "validateHeaderName" | "validateHeaderValue") => Some(2),
        ("http", "setMaxIdleHTTPParsers" | "setGlobalProxyFromEnv") => Some(1),
        ("http", "_connectionListener") => Some(1),
        ("module", "register" | "registerHooks") => Some(1),
        // #3904: modern V8 diagnostics/profiler exports (Node .length values).
        ("v8", "getCppHeapStatistics") => Some(0),
        (
            "v8",
            "getHeapSnapshot"
            | "isStringOneByteRepresentation"
            | "queryObjects"
            | "startCpuProfile",
        ) => Some(1),
        ("v8", "writeHeapSnapshot") => Some(2),
        // #3906: implemented top-level v8 helpers reachable as bound callables.
        ("v8", "serialize" | "deserialize") => Some(1),
        (
            "v8",
            "getHeapStatistics"
            | "getHeapSpaceStatistics"
            | "getHeapCodeStatistics"
            | "cachedDataVersionTag"
            | "GCProfiler",
        ) => Some(0),
        // #3127/#3128/#3130/#3284: node:vm no-flag export lengths.
        ("vm", "Script") => Some(1),
        ("vm", "Module") => Some(1),
        ("vm", "SourceTextModule") => Some(1),
        ("vm", "SyntheticModule") => Some(2),
        ("vm", "createContext" | "measureMemory") => Some(0),
        ("vm", "createScript" | "runInThisContext" | "compileFunction") => Some(2),
        ("vm", "runInContext" | "runInNewContext") => Some(3),
        ("vm", "isContext") => Some(1),
        ("net", "_normalizeArgs") => Some(1),
        ("net", "_createServerHandle") => Some(5),
        ("domain", "Domain" | "createDomain" | "create") => Some(0),
        ("util", "diff") => Some(2),
        ("dns" | "dns/promises", "Resolver") => Some(0),
        ("fs", "ReadStream" | "WriteStream") => Some(2),
        ("fs", "Utf8Stream") => Some(0),
        ("fs", "Dir" | "Dirent") => Some(3),
        ("fs", "Stats") => Some(18),
        ("fs", "mkdtempDisposableSync") => Some(2),
        ("fs", "openAsBlob") => Some(1),
        ("events", "init") => Some(1),
        ("repl", "Recoverable") => Some(1),
        ("repl", "REPLServer" | "start") => Some(6),
        ("wasi", "WASI") => Some(0),
        ("perf_hooks", "Performance") => Some(0),
        ("perf_hooks", "PerformanceEntry") => Some(0),
        ("perf_hooks", "PerformanceMark") => Some(1),
        ("perf_hooks", "PerformanceMeasure") => Some(0),
        ("perf_hooks", "PerformanceObserver") => Some(1),
        ("perf_hooks", "PerformanceObserverEntryList") => Some(0),
        ("perf_hooks", "PerformanceResourceTiming") => Some(0),
        // #3119/#3126/#3263 node:module helpers.
        ("module", "createRequire" | "SourceMap") => Some(1),
        ("module", "findPackageJSON" | "findSourceMap") => Some(1),
        ("module", "Module") => Some(0),
        ("module", "enableCompileCache") => Some(1),
        ("module", "flushCompileCache") => Some(0),
        ("module", "getCompileCacheDir") => Some(0),
        ("module", "getSourceMapsSupport") => Some(0),
        ("module", "_findPath") => Some(3),
        ("module", "_initPaths") => Some(0),
        ("module", "_load") => Some(3),
        ("module", "_nodeModulePaths") => Some(1),
        ("module", "_preloadModules") => Some(1),
        ("module", "_resolveFilename") => Some(4),
        ("module", "_resolveLookupPaths") => Some(2),
        ("module", "setSourceMapsSupport") => Some(1),
        ("module", "stripTypeScriptTypes") => Some(1),
        ("module", "syncBuiltinESMExports") => Some(0),
        ("module", "runMain") => Some(0),
        ("tls", "connect") => Some(4),
        ("tls", "createServer" | "Server") => Some(2),
        ("tls", "TLSSocket") => Some(2),
        ("child_process", "_forkChild") => Some(2),
        _ => None,
    }
}

/// Sorted (module → sorted (prop, arity)) table replacing the ~15 KB compiled
/// arity ladder. Same oracle-test scheme as `CALLABLE_EXPORT_TABLE`.
#[rustfmt::skip] static CALLABLE_EXPORT_ARITY_TABLE: &[(&str, &[(&str, u32)])] = &[
    (
        "async_hooks",
        &[
            ("AsyncLocalStorage", 0),
            ("AsyncResource", realm_native_async_hooks_AsyncResource_metadata!(length)),
            ("createHook", realm_native_async_hooks_createHook_metadata!(length)),
            ("executionAsyncId", 0),
            ("executionAsyncResource", 0),
            ("triggerAsyncId", 0),
        ],
    ),
    (
        "bun",
        &[
            ("Glob", 1),
            ("Terminal", 1),
            ("Transpiler", 1),
            ("build", 1),
            ("connect", 1),
            ("deepEquals", 2),
            ("file", 1),
            ("fileURLToPath", 1),
            ("gc", 1),
            ("generateHeapSnapshot", 2),
            ("hash", 1),
            ("listen", 1),
            ("pathToFileURL", 1),
            ("plugin", 1),
            ("serve", 1),
            ("sleep", 1),
            ("sleepSync", 1),
            ("spawn", 2),
            ("stringWidth", 1),
            ("stripANSI", 1),
            ("which", 1),
            ("wrapAnsi", 3),
            ("write", 2),
            ("zstdDecompress", 1),
            ("zstdDecompressSync", 1),
        ],
    ),
    (
        "bun.ant",
        &[
            ("getPeerPid", 1),
            ("getPeerUid", 1),
            ("memoryPressureLevel", 0),
        ],
    ),
    (
        "bun:ffi",
        &[
            ("CFunction", 1),
            ("CString", 1),
            ("JSCallback", 2),
            ("dlopen", 2),
            ("linkSymbols", 1),
            ("ptr", 1),
            ("toArrayBuffer", 3),
            ("toBuffer", 3),
            ("viewSource", 2),
        ],
    ),
    ("bun:jsc", &[("heapStats", 0)]),
    ("child_process", &[("_forkChild", realm_native_child_process_forkChild_metadata!(length))]),
    (
        "cluster",
        &[
            ("Worker", realm_native_cluster_Worker_metadata!(length)),
            ("addListener", 2),
            ("disconnect", realm_native_cluster_disconnect_metadata!(length)),
            ("emit", 1),
            ("eventNames", 0),
            ("fork", realm_native_cluster_fork_metadata!(length)),
            ("getMaxListeners", 0),
            ("listenerCount", 2),
            ("listeners", 1),
            ("off", 2),
            ("on", 2),
            ("once", 2),
            ("prependListener", 2),
            ("prependOnceListener", 2),
            ("rawListeners", 1),
            ("removeAllListeners", 1),
            ("removeListener", 2),
            ("setMaxListeners", 1),
            ("setupMaster", 1),
            ("setupPrimary", realm_native_cluster_setupPrimary_metadata!(length)),
        ],
    ),
    ("console", &[("context", realm_native_console_context_metadata!(length)), ("createTask", 0)]),
    (
        "crypto",
        &[
            ("Certificate", 0),
            ("Cipheriv", realm_native_crypto_Cipheriv_metadata!(length)),
            ("Decipheriv", realm_native_crypto_Decipheriv_metadata!(length)),
            ("DiffieHellman", realm_native_crypto_DiffieHellman_metadata!(length)),
            ("DiffieHellmanGroup", realm_native_crypto_DiffieHellmanGroup_metadata!(length)),
            ("KeyObject", 2),
            ("X509Certificate", realm_native_crypto_X509Certificate_metadata!(length)),
            ("argon2", realm_native_crypto_argon2_metadata!(length)),
            ("argon2Sync", realm_native_crypto_argon2Sync_metadata!(length)),
            ("checkPrime", realm_native_crypto_checkPrime_metadata!(length)),
            ("checkPrimeSync", realm_native_crypto_checkPrimeSync_metadata!(length)),
            ("decapsulate", realm_native_crypto_decapsulate_metadata!(length)),
            ("diffieHellman", realm_native_crypto_diffieHellman_metadata!(length)),
            ("encapsulate", realm_native_crypto_encapsulate_metadata!(length)),
            ("generateKey", realm_native_crypto_generateKey_metadata!(length)),
            ("generateKeyPair", realm_native_crypto_generateKeyPair_metadata!(length)),
            ("generateKeyPairSync", realm_native_crypto_generateKeyPairSync_metadata!(length)),
            ("generateKeySync", realm_native_crypto_generateKeySync_metadata!(length)),
            ("generatePrime", realm_native_crypto_generatePrime_metadata!(length)),
            ("generatePrimeSync", realm_native_crypto_generatePrimeSync_metadata!(length)),
            ("hkdf", realm_native_crypto_hkdf_metadata!(length)),
            ("hkdfSync", realm_native_crypto_hkdfSync_metadata!(length)),
            ("scrypt", realm_native_crypto_scrypt_metadata!(length)),
            ("scryptSync", realm_native_crypto_scryptSync_metadata!(length)),
            ("secureHeapUsed", 0),
            ("setFips", realm_native_crypto_setFips_metadata!(length)),
        ],
    ),
    ("crypto.KeyObject", &[("from", realm_native_crypto_KeyObject_from_metadata!(length))]),
    ("dns", &[("Resolver", 0)]),
    ("dns/promises", &[("Resolver", 0)]),
    (
        "domain",
        &[("Domain", 0), ("create", 0), ("createDomain", 0)],
    ),
    (
        "events",
        &[
            ("EventEmitter", realm_native_events_EventEmitter_metadata!(length)),
            ("EventEmitterAsyncResource", 0),
            ("addAbortListener", realm_native_events_addAbortListener_metadata!(length)),
            ("getEventListeners", realm_native_events_getEventListeners_metadata!(length)),
            ("getMaxListeners", realm_native_events_getMaxListeners_metadata!(length)),
            ("init", realm_native_events_init_metadata!(length)),
            ("listenerCount", realm_native_events_listenerCount_metadata!(length)),
            ("on", realm_native_events_on_metadata!(length)),
            ("once", realm_native_events_once_metadata!(length)),
            ("setMaxListeners", 0),
        ],
    ),
    (
        "ffi",
        &[
            ("dlopen", 2),
            ("getRawPointer", 1),
            ("toArrayBuffer", 3),
            ("toBuffer", 3),
            ("toString", 1),
        ],
    ),
    (
        "fs",
        &[
            ("Dir", realm_native_fs_Dir_metadata!(length)),
            ("Dirent", realm_native_fs_Dirent_metadata!(length)),
            ("ReadStream", realm_native_fs_ReadStream_metadata!(length)),
            ("Stats", realm_native_fs_Stats_metadata!(length)),
            ("Utf8Stream", 0),
            ("WriteStream", realm_native_fs_WriteStream_metadata!(length)),
            ("_toUnixTimestamp", realm_native_fs_toUnixTimestamp_metadata!(length)),
            ("mkdtempDisposableSync", realm_native_fs_mkdtempDisposableSync_metadata!(length)),
            ("openAsBlob", realm_native_fs_openAsBlob_metadata!(length)),
        ],
    ),
    (
        "http",
        &[
            ("Agent", realm_native_http_Agent_metadata!(length)),
            ("ClientRequest", realm_native_http_ClientRequest_metadata!(length)),
            ("IncomingMessage", realm_native_http_IncomingMessage_metadata!(length)),
            ("OutgoingMessage", realm_native_http_OutgoingMessage_metadata!(length)),
            ("ServerResponse", realm_native_http_ServerResponse_metadata!(length)),
            ("_connectionListener", realm_native_http_connectionListener_metadata!(length)),
            ("get", realm_native_http_get_metadata!(length)),
            ("request", 0),
            ("setGlobalProxyFromEnv", realm_native_http_setGlobalProxyFromEnv_metadata!(length)),
            ("setMaxIdleHTTPParsers", realm_native_http_setMaxIdleHTTPParsers_metadata!(length)),
            ("validateHeaderName", realm_native_http_validateHeaderName_metadata!(length)),
            ("validateHeaderValue", realm_native_http_validateHeaderValue_metadata!(length)),
        ],
    ),
    (
        "http2",
        &[
            ("connect", realm_native_http2_connect_metadata!(length)),
            ("createSecureServer", realm_native_http2_createSecureServer_metadata!(length)),
            ("createServer", realm_native_http2_createServer_metadata!(length)),
            ("getDefaultSettings", 0),
            ("getPackedSettings", realm_native_http2_getPackedSettings_metadata!(length)),
            ("getUnpackedSettings", realm_native_http2_getUnpackedSettings_metadata!(length)),
            ("performServerHandshake", realm_native_http2_performServerHandshake_metadata!(length)),
        ],
    ),
    ("https", &[("Agent", realm_native_https_Agent_metadata!(length)), ("get", realm_native_https_get_metadata!(length)), ("request", 0)]),
    (
        "inspector.DOMStorage",
        &[
            ("domStorageItemAdded", realm_native_inspector_DOMStorage_domStorageItemAdded_metadata!(length)),
            ("domStorageItemRemoved", realm_native_inspector_DOMStorage_domStorageItemRemoved_metadata!(length)),
            ("domStorageItemUpdated", realm_native_inspector_DOMStorage_domStorageItemUpdated_metadata!(length)),
            ("domStorageItemsCleared", realm_native_inspector_DOMStorage_domStorageItemsCleared_metadata!(length)),
            ("registerStorage", realm_native_inspector_DOMStorage_registerStorage_metadata!(length)),
        ],
    ),
    (
        "inspector.Network",
        &[
            ("dataReceived", realm_native_inspector_Network_dataReceived_metadata!(length)),
            ("dataSent", realm_native_inspector_Network_dataSent_metadata!(length)),
            ("loadingFailed", realm_native_inspector_Network_loadingFailed_metadata!(length)),
            ("loadingFinished", realm_native_inspector_Network_loadingFinished_metadata!(length)),
            ("requestWillBeSent", realm_native_inspector_Network_requestWillBeSent_metadata!(length)),
            ("responseReceived", realm_native_inspector_Network_responseReceived_metadata!(length)),
            ("webSocketClosed", realm_native_inspector_Network_webSocketClosed_metadata!(length)),
            ("webSocketCreated", realm_native_inspector_Network_webSocketCreated_metadata!(length)),
            ("webSocketHandshakeResponseReceived", realm_native_inspector_Network_webSocketHandshakeResponseReceived_metadata!(length)),
        ],
    ),
    ("inspector.NetworkResources", &[("put", realm_native_inspector_NetworkResources_put_metadata!(length))]),
    (
        "inspector.Session",
        &[
            ("connect", 0),
            ("connectToMainThread", 0),
            ("disconnect", 0),
            ("post", 3),
        ],
    ),
    ("inspector/promises.Session", &[("post", 3)]),
    (
        "module",
        &[
            ("Module", 0),
            ("SourceMap", realm_native_module_SourceMap_metadata!(length)),
            ("_findPath", realm_native_module_findPath_metadata!(length)),
            ("_initPaths", 0),
            ("_load", realm_native_module_load_metadata!(length)),
            ("_nodeModulePaths", realm_native_module_nodeModulePaths_metadata!(length)),
            ("_preloadModules", realm_native_module_preloadModules_metadata!(length)),
            ("_resolveFilename", realm_native_module_resolveFilename_metadata!(length)),
            ("_resolveLookupPaths", realm_native_module_resolveLookupPaths_metadata!(length)),
            ("createRequire", realm_native_module_createRequire_metadata!(length)),
            ("enableCompileCache", realm_native_module_enableCompileCache_metadata!(length)),
            ("findPackageJSON", realm_native_module_findPackageJSON_metadata!(length)),
            ("findSourceMap", realm_native_module_findSourceMap_metadata!(length)),
            ("flushCompileCache", 0),
            ("getCompileCacheDir", 0),
            ("getSourceMapsSupport", 0),
            ("register", realm_native_module_register_metadata!(length)),
            ("registerHooks", realm_native_module_registerHooks_metadata!(length)),
            ("runMain", 0),
            ("setSourceMapsSupport", realm_native_module_setSourceMapsSupport_metadata!(length)),
            ("stripTypeScriptTypes", realm_native_module_stripTypeScriptTypes_metadata!(length)),
            ("syncBuiltinESMExports", 0),
        ],
    ),
    (
        "net",
        &[
            ("BlockList", 0),
            ("Server", 2),
            ("Socket", 1),
            ("SocketAddress", 0),
            ("_createServerHandle", realm_native_net_createServerHandle_metadata!(length)),
            ("_normalizeArgs", realm_native_net_normalizeArgs_metadata!(length)),
            ("connect", realm_native_net_connect_metadata!(length)),
            ("createConnection", realm_native_net_createConnection_metadata!(length)),
            ("createServer", realm_native_net_createServer_metadata!(length)),
        ],
    ),
    ("node-pty", &[("spawn", 3)]),
    (
        "perf_hooks",
        &[
            ("Performance", 0),
            ("PerformanceEntry", 0),
            ("PerformanceMark", realm_native_perf_hooks_PerformanceMark_metadata!(length)),
            ("PerformanceMeasure", 0),
            ("PerformanceObserver", realm_native_perf_hooks_PerformanceObserver_metadata!(length)),
            ("PerformanceObserverEntryList", 0),
            ("PerformanceResourceTiming", 0),
        ],
    ),
    (
        "process",
        &[
            ("_debugEnd", 0),
            ("_debugProcess", 0),
            ("_fatalException", realm_native_process_fatalException_metadata!(length)),
            ("_getActiveHandles", 0),
            ("_getActiveRequests", 0),
            ("_kill", 0),
            ("_linkedBinding", realm_native_process_linkedBinding_metadata!(length)),
            ("_rawDebug", 0),
            ("_startProfilerIdleNotifier", 0),
            ("_stopProfilerIdleNotifier", 0),
            ("_tickCallback", 0),
            ("addUncaughtExceptionCaptureCallback", realm_native_process_addUncaughtExceptionCaptureCallback_metadata!(length)),
            ("binding", realm_native_process_binding_metadata!(length)),
            ("dlopen", 0),
            ("execve", realm_native_process_execve_metadata!(length)),
            ("hasUncaughtExceptionCaptureCallback", 0),
            ("openStdin", 0),
            ("reallyExit", 0),
            ("ref", realm_native_process_ref_metadata!(length)),
            ("setSourceMapsEnabled", realm_native_process_setSourceMapsEnabled_metadata!(length)),
            ("setUncaughtExceptionCaptureCallback", realm_native_process_setUncaughtExceptionCaptureCallback_metadata!(length)),
            ("unref", realm_native_process_unref_metadata!(length)),
        ],
    ),
    (
        "querystring",
        &[
            ("escape", realm_native_querystring_escape_metadata!(length)),
            ("parse", realm_native_querystring_parse_metadata!(length)),
            ("stringify", realm_native_querystring_stringify_metadata!(length)),
            ("unescape", realm_native_querystring_unescape_metadata!(length)),
            ("unescapeBuffer", realm_native_querystring_unescapeBuffer_metadata!(length)),
        ],
    ),
    (
        "repl",
        &[("REPLServer", realm_native_repl_REPLServer_metadata!(length)), ("Recoverable", realm_native_repl_Recoverable_metadata!(length)), ("start", realm_native_repl_start_metadata!(length))],
    ),
    (
        "sea",
        &[
            ("getAsset", realm_native_sea_getAsset_metadata!(length)),
            ("getAssetAsBlob", realm_native_sea_getAssetAsBlob_metadata!(length)),
            ("getAssetKeys", 0),
            ("getRawAsset", realm_native_sea_getRawAsset_metadata!(length)),
            ("isSea", 0),
        ],
    ),
    (
        "stream",
        &[
            ("_isArrayBufferView", realm_native_stream_isArrayBufferView_metadata!(length)),
            ("_isUint8Array", realm_native_stream_isUint8Array_metadata!(length)),
            ("_uint8ArrayToBuffer", realm_native_stream_uint8ArrayToBuffer_metadata!(length)),
            ("addAbortSignal", realm_native_stream_addAbortSignal_metadata!(length)),
            ("compose", 0),
            ("destroy", 2),
            ("duplexPair", realm_native_stream_duplexPair_metadata!(length)),
            ("finished", realm_native_stream_finished_metadata!(length)),
            ("getDefaultHighWaterMark", realm_native_stream_getDefaultHighWaterMark_metadata!(length)),
            ("isDestroyed", realm_native_stream_isDestroyed_metadata!(length)),
            ("isDisturbed", realm_native_stream_isDisturbed_metadata!(length)),
            ("isErrored", realm_native_stream_isErrored_metadata!(length)),
            ("isReadable", realm_native_stream_isReadable_metadata!(length)),
            ("isWritable", realm_native_stream_isWritable_metadata!(length)),
            ("pipeline", 0),
            ("setDefaultHighWaterMark", realm_native_stream_setDefaultHighWaterMark_metadata!(length)),
        ],
    ),
    (
        "tls",
        &[
            ("SecureContext", realm_native_tls_SecureContext_metadata!(length)),
            ("Server", realm_native_tls_Server_metadata!(length)),
            ("TLSSocket", realm_native_tls_TLSSocket_metadata!(length)),
            ("checkServerIdentity", realm_native_tls_checkServerIdentity_metadata!(length)),
            ("connect", realm_native_tls_connect_metadata!(length)),
            ("convertALPNProtocols", realm_native_tls_convertALPNProtocols_metadata!(length)),
            ("createSecureContext", realm_native_tls_createSecureContext_metadata!(length)),
            ("createServer", realm_native_tls_createServer_metadata!(length)),
            ("getCACertificates", realm_native_tls_getCACertificates_metadata!(length)),
            ("getCertificateCompressionAlgorithms", 0),
            ("getCiphers", 0),
            ("setDefaultCACertificates", realm_native_tls_setDefaultCACertificates_metadata!(length)),
        ],
    ),
    (
        "url",
        &[
            ("URL", 1),
            ("URLPattern", 0),
            ("Url", 0),
            ("resolveObject", realm_native_url_resolveObject_metadata!(length)),
        ],
    ),
    (
        "util",
        &[
            ("MIMEParams", 0),
            ("MIMEType", realm_native_util_MIMEType_metadata!(length)),
            ("debug", realm_native_util_debug_metadata!(length)),
            ("debuglog", realm_native_util_debuglog_metadata!(length)),
            ("diff", realm_native_util_diff_metadata!(length)),
            ("inherits", realm_native_util_inherits_metadata!(length)),
            ("isArray", realm_native_util_isArray_metadata!(length)),
        ],
    ),
    (
        "v8",
        &[
            ("GCProfiler", 0),
            ("cachedDataVersionTag", 0),
            ("deserialize", realm_native_v8_deserialize_metadata!(length)),
            ("getCppHeapStatistics", 0),
            ("getHeapCodeStatistics", 0),
            ("getHeapSnapshot", realm_native_v8_getHeapSnapshot_metadata!(length)),
            ("getHeapSpaceStatistics", 0),
            ("getHeapStatistics", 0),
            ("isStringOneByteRepresentation", realm_native_v8_isStringOneByteRepresentation_metadata!(length)),
            ("queryObjects", realm_native_v8_queryObjects_metadata!(length)),
            ("serialize", realm_native_v8_serialize_metadata!(length)),
            ("startCpuProfile", realm_native_v8_startCpuProfile_metadata!(length)),
            ("writeHeapSnapshot", realm_native_v8_writeHeapSnapshot_metadata!(length)),
        ],
    ),
    (
        "vm",
        &[
            ("Module", 1),
            ("Script", realm_native_vm_Script_metadata!(length)),
            ("SourceTextModule", 1),
            ("SyntheticModule", 2),
            ("compileFunction", realm_native_vm_compileFunction_metadata!(length)),
            ("createContext", 0),
            ("createScript", realm_native_vm_createScript_metadata!(length)),
            ("isContext", realm_native_vm_isContext_metadata!(length)),
            ("measureMemory", 0),
            ("runInContext", realm_native_vm_runInContext_metadata!(length)),
            ("runInNewContext", realm_native_vm_runInNewContext_metadata!(length)),
            ("runInThisContext", realm_native_vm_runInThisContext_metadata!(length)),
        ],
    ),
    ("wasi", &[("WASI", 0)]),
];

pub(super) fn native_callable_export_arity(module: &str, prop: &str) -> Option<u32> {
    let mi = CALLABLE_EXPORT_ARITY_TABLE
        .binary_search_by(|(m, _)| (*m).cmp(module))
        .ok()?;
    let row = CALLABLE_EXPORT_ARITY_TABLE[mi].1;
    let pi = row.binary_search_by(|(p, _)| (*p).cmp(prop)).ok()?;
    Some(row[pi].1)
}

#[cfg(test)]
mod callable_export_arity_table_tests {
    use super::super::callable_exports::TLS_SOCKET_PROTOTYPE_METHODS;
    use super::super::*;
    use super::*;

    #[test]
    fn arity_table_is_sorted() {
        for w in CALLABLE_EXPORT_ARITY_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
        for (_, row) in CALLABLE_EXPORT_ARITY_TABLE {
            for w in row.windows(2) {
                assert!(w[0].0 < w[1].0);
            }
        }
    }

    #[test]
    fn arity_table_matches_reference_exhaustively() {
        let source = concat!(
            include_str!("callable_exports.rs"),
            include_str!("callable_export_arity_table.rs")
        );
        let mut literals: Vec<&str> = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('"') else { break };
            let lit = &after[..end];
            if !lit.is_empty() && lit.len() < 64 && !lit.contains('\n') {
                literals.push(lit);
            }
            rest = &after[end + 1..];
        }
        literals.sort_unstable();
        literals.dedup();
        for module in &literals {
            for prop in &literals {
                assert_eq!(
                    native_callable_export_arity(module, prop),
                    native_callable_export_arity_reference(module, prop),
                    "divergence at ({module}, {prop})"
                );
            }
        }
    }

    #[test]
    fn tls_constructor_prototypes_match_node_parent_classes() {
        let server = bound_native_callable_export_value("tls", "Server");
        let server_addr = (server.to_bits() & crate::value::POINTER_MASK) as usize;
        let server_proto = crate::closure::closure_get_dynamic_prop(server_addr, "prototype");
        assert!(tls_constructor_prototype_is_instance_of(
            server_proto,
            "EventEmitter"
        ));
        assert_eq!(
            crate::object::js_instanceof(server_proto, 0xFFFF_0076).to_bits(),
            crate::value::TAG_TRUE
        );
        let event_emitter = bound_native_callable_export_value("events", "EventEmitter");
        assert_eq!(
            crate::object::js_instanceof_dynamic(server_proto, event_emitter).to_bits(),
            crate::value::TAG_TRUE
        );

        let socket = bound_native_callable_export_value("tls", "TLSSocket");
        let socket_addr = (socket.to_bits() & crate::value::POINTER_MASK) as usize;
        let socket_proto = crate::closure::closure_get_dynamic_prop(socket_addr, "prototype");
        assert!(tls_constructor_prototype_is_instance_of(
            socket_proto,
            "Duplex"
        ));
        let socket_proto_obj =
            JSValue::from_bits(socket_proto.to_bits()).as_pointer::<ObjectHeader>();
        for &(name, length) in TLS_SOCKET_PROTOTYPE_METHODS {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let method = crate::object::js_object_get_field_by_name(socket_proto_obj, key);
            let method_addr = method.as_pointer::<crate::closure::ClosureHeader>() as usize;
            assert!(crate::closure::is_closure_ptr(method_addr), "{name}");
            assert_eq!(builtin_closure_length(method_addr), Some(length), "{name}");
        }
        assert_eq!(
            crate::object::js_instanceof(socket_proto, 0xFFFF_0073).to_bits(),
            crate::value::TAG_TRUE
        );
        let duplex = bound_native_callable_export_value("stream", "Duplex");
        assert_eq!(
            crate::object::js_instanceof_dynamic(socket_proto, duplex).to_bits(),
            crate::value::TAG_TRUE
        );
    }
}
