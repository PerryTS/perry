/// Test-only reference: original ladder, oracle for `CALLABLE_EXPORT_ARITY_TABLE`.
#[cfg(test)]
fn native_callable_export_arity_reference(module: &str, prop: &str) -> Option<u32> {
    match (module, prop) {
        // bun:ffi (#6562).
        ("bun:ffi", "dlopen") => Some(2),
        ("bun:ffi", "ptr" | "CString" | "JSCallback" | "CFunction" | "linkSymbols") => Some(1),
        ("bun:ffi", "toArrayBuffer" | "toBuffer") => Some(3),
        ("bun:ffi", "viewSource") => Some(2),
        ("bun:ffi", "read") => Some(0),
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
        ("tls", "getCiphers") => Some(0),
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
        ("module", "createRequire") => Some(1),
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
static CALLABLE_EXPORT_ARITY_TABLE: &[(&str, &[(&str, u32)])] = &[
    (
        "async_hooks",
        &[
            ("AsyncLocalStorage", 0),
            ("AsyncResource", 1),
            ("createHook", 1),
            ("executionAsyncId", 0),
            ("executionAsyncResource", 0),
            ("triggerAsyncId", 0),
        ],
    ),
    (
        "bun:ffi",
        &[
            ("CFunction", 1),
            ("CString", 1),
            ("JSCallback", 1),
            ("dlopen", 2),
            ("linkSymbols", 1),
            ("ptr", 1),
            ("read", 0),
            ("toArrayBuffer", 3),
            ("toBuffer", 3),
            ("viewSource", 2),
        ],
    ),
    ("child_process", &[("_forkChild", 2)]),
    (
        "cluster",
        &[
            ("Worker", 1),
            ("addListener", 2),
            ("disconnect", 1),
            ("emit", 1),
            ("eventNames", 0),
            ("fork", 1),
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
            ("setupPrimary", 1),
        ],
    ),
    ("console", &[("context", 1), ("createTask", 0)]),
    (
        "crypto",
        &[
            ("Cipheriv", 4),
            ("Decipheriv", 4),
            ("DiffieHellman", 4),
            ("DiffieHellmanGroup", 1),
            ("KeyObject", 2),
            ("X509Certificate", 1),
            ("argon2", 3),
            ("argon2Sync", 2),
            ("checkPrime", 1),
            ("checkPrimeSync", 1),
            ("decapsulate", 3),
            ("diffieHellman", 2),
            ("encapsulate", 2),
            ("generateKey", 3),
            ("generateKeyPair", 3),
            ("generateKeyPairSync", 2),
            ("generateKeySync", 2),
            ("generatePrime", 3),
            ("generatePrimeSync", 1),
            ("hkdf", 6),
            ("hkdfSync", 5),
            ("scrypt", 4),
            ("scryptSync", 3),
            ("secureHeapUsed", 0),
            ("setFips", 1),
        ],
    ),
    ("crypto.KeyObject", &[("from", 1)]),
    ("dns", &[("Resolver", 0)]),
    ("dns/promises", &[("Resolver", 0)]),
    (
        "domain",
        &[("Domain", 0), ("create", 0), ("createDomain", 0)],
    ),
    (
        "events",
        &[
            ("EventEmitter", 1),
            ("EventEmitterAsyncResource", 0),
            ("addAbortListener", 2),
            ("getEventListeners", 2),
            ("getMaxListeners", 1),
            ("init", 1),
            ("listenerCount", 2),
            ("on", 2),
            ("once", 2),
            ("setMaxListeners", 0),
        ],
    ),
    (
        "fs",
        &[
            ("Dir", 3),
            ("Dirent", 3),
            ("ReadStream", 2),
            ("Stats", 18),
            ("Utf8Stream", 0),
            ("WriteStream", 2),
            ("_toUnixTimestamp", 1),
            ("mkdtempDisposableSync", 2),
            ("openAsBlob", 1),
        ],
    ),
    (
        "http",
        &[
            ("Agent", 1),
            ("ClientRequest", 3),
            ("IncomingMessage", 1),
            ("OutgoingMessage", 1),
            ("ServerResponse", 1),
            ("_connectionListener", 1),
            ("get", 3),
            ("request", 0),
            ("setGlobalProxyFromEnv", 1),
            ("setMaxIdleHTTPParsers", 1),
            ("validateHeaderName", 2),
            ("validateHeaderValue", 2),
        ],
    ),
    (
        "http2",
        &[
            ("connect", 3),
            ("createSecureServer", 2),
            ("createServer", 2),
            ("getDefaultSettings", 0),
            ("getPackedSettings", 1),
            ("getUnpackedSettings", 1),
            ("performServerHandshake", 1),
        ],
    ),
    ("https", &[("Agent", 1), ("get", 3), ("request", 0)]),
    (
        "inspector.DOMStorage",
        &[
            ("domStorageItemAdded", 1),
            ("domStorageItemRemoved", 1),
            ("domStorageItemUpdated", 1),
            ("domStorageItemsCleared", 1),
            ("registerStorage", 1),
        ],
    ),
    (
        "inspector.Network",
        &[
            ("dataReceived", 1),
            ("dataSent", 1),
            ("loadingFailed", 1),
            ("loadingFinished", 1),
            ("requestWillBeSent", 1),
            ("responseReceived", 1),
            ("webSocketClosed", 1),
            ("webSocketCreated", 1),
            ("webSocketHandshakeResponseReceived", 1),
        ],
    ),
    ("inspector.NetworkResources", &[("put", 1)]),
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
            ("_findPath", 3),
            ("_initPaths", 0),
            ("_load", 3),
            ("_nodeModulePaths", 1),
            ("_preloadModules", 1),
            ("_resolveFilename", 4),
            ("_resolveLookupPaths", 2),
            ("createRequire", 1),
            ("enableCompileCache", 1),
            ("flushCompileCache", 0),
            ("getCompileCacheDir", 0),
            ("getSourceMapsSupport", 0),
            ("register", 1),
            ("registerHooks", 1),
            ("runMain", 0),
            ("setSourceMapsSupport", 1),
            ("stripTypeScriptTypes", 1),
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
            ("_createServerHandle", 5),
            ("_normalizeArgs", 1),
            ("connect", 3),
            ("createConnection", 3),
            ("createServer", 2),
        ],
    ),
    ("node-pty", &[("spawn", 3)]),
    (
        "perf_hooks",
        &[
            ("Performance", 0),
            ("PerformanceEntry", 0),
            ("PerformanceMark", 1),
            ("PerformanceMeasure", 0),
            ("PerformanceObserver", 1),
            ("PerformanceObserverEntryList", 0),
            ("PerformanceResourceTiming", 0),
        ],
    ),
    (
        "process",
        &[
            ("_debugEnd", 0),
            ("_debugProcess", 0),
            ("_fatalException", 2),
            ("_getActiveHandles", 0),
            ("_getActiveRequests", 0),
            ("_kill", 0),
            ("_linkedBinding", 1),
            ("_rawDebug", 0),
            ("_startProfilerIdleNotifier", 0),
            ("_stopProfilerIdleNotifier", 0),
            ("_tickCallback", 0),
            ("addUncaughtExceptionCaptureCallback", 1),
            ("binding", 1),
            ("dlopen", 0),
            ("execve", 1),
            ("hasUncaughtExceptionCaptureCallback", 0),
            ("openStdin", 0),
            ("reallyExit", 0),
            ("ref", 1),
            ("setSourceMapsEnabled", 1),
            ("setUncaughtExceptionCaptureCallback", 1),
            ("unref", 1),
        ],
    ),
    (
        "querystring",
        &[
            ("escape", 1),
            ("parse", 4),
            ("stringify", 4),
            ("unescape", 2),
            ("unescapeBuffer", 2),
        ],
    ),
    (
        "repl",
        &[("REPLServer", 6), ("Recoverable", 1), ("start", 6)],
    ),
    (
        "sea",
        &[
            ("getAsset", 2),
            ("getAssetAsBlob", 2),
            ("getAssetKeys", 0),
            ("getRawAsset", 1),
            ("isSea", 0),
        ],
    ),
    (
        "stream",
        &[
            ("_isArrayBufferView", 1),
            ("_isUint8Array", 1),
            ("_uint8ArrayToBuffer", 1),
            ("addAbortSignal", 2),
            ("compose", 0),
            ("destroy", 2),
            ("duplexPair", 1),
            ("finished", 3),
            ("getDefaultHighWaterMark", 1),
            ("isDestroyed", 1),
            ("isDisturbed", 1),
            ("isErrored", 1),
            ("isReadable", 1),
            ("isWritable", 1),
            ("pipeline", 0),
            ("setDefaultHighWaterMark", 2),
        ],
    ),
    (
        "tls",
        &[
            ("SecureContext", 1),
            ("Server", 2),
            ("TLSSocket", 2),
            ("checkServerIdentity", 2),
            ("connect", 4),
            ("convertALPNProtocols", 2),
            ("createSecureContext", 1),
            ("createServer", 2),
            ("getCACertificates", 1),
            ("getCiphers", 0),
            ("setDefaultCACertificates", 1),
        ],
    ),
    (
        "url",
        &[
            ("URL", 1),
            ("URLPattern", 0),
            ("Url", 0),
            ("resolveObject", 2),
        ],
    ),
    (
        "util",
        &[
            ("MIMEParams", 0),
            ("MIMEType", 1),
            ("debug", 2),
            ("debuglog", 2),
            ("diff", 2),
            ("inherits", 2),
            ("isArray", 1),
        ],
    ),
    (
        "v8",
        &[
            ("GCProfiler", 0),
            ("cachedDataVersionTag", 0),
            ("deserialize", 1),
            ("getCppHeapStatistics", 0),
            ("getHeapCodeStatistics", 0),
            ("getHeapSnapshot", 1),
            ("getHeapSpaceStatistics", 0),
            ("getHeapStatistics", 0),
            ("isStringOneByteRepresentation", 1),
            ("queryObjects", 1),
            ("serialize", 1),
            ("startCpuProfile", 1),
            ("writeHeapSnapshot", 2),
        ],
    ),
    (
        "vm",
        &[
            ("Module", 1),
            ("Script", 1),
            ("SourceTextModule", 1),
            ("SyntheticModule", 2),
            ("compileFunction", 2),
            ("createContext", 0),
            ("createScript", 2),
            ("isContext", 1),
            ("measureMemory", 0),
            ("runInContext", 3),
            ("runInNewContext", 3),
            ("runInThisContext", 2),
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
