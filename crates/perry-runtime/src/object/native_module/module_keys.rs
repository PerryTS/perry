use super::*;

const ASYNC_HOOKS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"AsyncLocalStorage",
    b"AsyncResource",
    b"asyncWrapProviders",
    b"createHook",
    b"default",
    b"executionAsyncId",
    b"executionAsyncResource",
    b"triggerAsyncId",
];

const STREAM_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Duplex",
    b"PassThrough",
    b"Readable",
    b"Stream",
    b"Transform",
    b"Writable",
    b"_isArrayBufferView",
    b"_isUint8Array",
    b"_uint8ArrayToBuffer",
    b"addAbortSignal",
    b"compose",
    b"default",
    b"duplexPair",
    b"finished",
    b"getDefaultHighWaterMark",
    b"isDestroyed",
    b"isDisturbed",
    b"isErrored",
    b"isReadable",
    b"isWritable",
    b"pipeline",
    b"promises",
    b"setDefaultHighWaterMark",
];

const DNS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"ADDRCONFIG",
    b"ADDRGETNETWORKPARAMS",
    b"ALL",
    b"BADFAMILY",
    b"BADFLAGS",
    b"BADHINTS",
    b"BADNAME",
    b"BADQUERY",
    b"BADRESP",
    b"BADSTR",
    b"CANCELLED",
    b"CONNREFUSED",
    b"DESTRUCTION",
    b"EOF",
    b"FILE",
    b"FORMERR",
    b"LOADIPHLPAPI",
    b"NODATA",
    b"NOMEM",
    b"NONAME",
    b"NOTFOUND",
    b"NOTIMP",
    b"NOTINITIALIZED",
    b"REFUSED",
    b"Resolver",
    b"SERVFAIL",
    b"TIMEOUT",
    b"V4MAPPED",
    b"default",
    b"getDefaultResultOrder",
    b"getServers",
    b"lookup",
    b"lookupService",
    b"promises",
    b"resolve",
    b"resolve4",
    b"resolve6",
    b"resolveAny",
    b"resolveCaa",
    b"resolveCname",
    b"resolveMx",
    b"resolveNaptr",
    b"resolveNs",
    b"resolvePtr",
    b"resolveSoa",
    b"resolveSrv",
    b"resolveTlsa",
    b"resolveTxt",
    b"reverse",
    b"setDefaultResultOrder",
    b"setServers",
];

const DNS_PROMISES_NAMESPACE_KEYS: &[&[u8]] = &[
    b"ADDRGETNETWORKPARAMS",
    b"BADFAMILY",
    b"BADFLAGS",
    b"BADHINTS",
    b"BADNAME",
    b"BADQUERY",
    b"BADRESP",
    b"BADSTR",
    b"CANCELLED",
    b"CONNREFUSED",
    b"DESTRUCTION",
    b"EOF",
    b"FILE",
    b"FORMERR",
    b"LOADIPHLPAPI",
    b"NODATA",
    b"NOMEM",
    b"NONAME",
    b"NOTFOUND",
    b"NOTIMP",
    b"NOTINITIALIZED",
    b"REFUSED",
    b"Resolver",
    b"SERVFAIL",
    b"TIMEOUT",
    b"default",
    b"getDefaultResultOrder",
    b"getServers",
    b"lookup",
    b"lookupService",
    b"resolve",
    b"resolve4",
    b"resolve6",
    b"resolveAny",
    b"resolveCaa",
    b"resolveCname",
    b"resolveMx",
    b"resolveNaptr",
    b"resolveNs",
    b"resolvePtr",
    b"resolveSoa",
    b"resolveSrv",
    b"resolveTlsa",
    b"resolveTxt",
    b"reverse",
    b"setDefaultResultOrder",
    b"setServers",
];

const CHILD_PROCESS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"ChildProcess",
    b"_forkChild",
    b"default",
    b"exec",
    b"execFile",
    b"execFileSync",
    b"execSync",
    b"fork",
    b"spawn",
    b"spawnSync",
];

// #6563: node-pty / @lydell/node-pty — `spawn` is the entire module surface
// the target apps touch; everything else lives on the returned IPty object.
const NODE_PTY_DEFAULT_KEYS: &[&[u8]] = &[b"spawn"];

const NODE_PTY_NAMESPACE_KEYS: &[&[u8]] = &[b"default", b"spawn"];

const CLUSTER_NAMESPACE_KEYS: &[&[u8]] = &[
    b"SCHED_NONE",
    b"SCHED_RR",
    b"Worker",
    b"_events",
    b"_eventsCount",
    b"_maxListeners",
    b"default",
    b"disconnect",
    b"fork",
    b"isMaster",
    b"isPrimary",
    b"isWorker",
    b"schedulingPolicy",
    b"settings",
    b"setupMaster",
    b"setupPrimary",
    b"workers",
];

const PROCESS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"_debugEnd",
    b"_debugProcess",
    b"_events",
    b"_eventsCount",
    b"_exiting",
    b"_fatalException",
    b"_getActiveHandles",
    b"_getActiveRequests",
    b"_kill",
    b"_linkedBinding",
    b"_maxListeners",
    b"_preload_modules",
    b"_rawDebug",
    b"_startProfilerIdleNotifier",
    b"_stopProfilerIdleNotifier",
    b"_tickCallback",
    b"abort",
    b"addListener",
    b"addUncaughtExceptionCaptureCallback",
    b"allowedNodeEnvironmentFlags",
    b"argv",
    b"argv0",
    b"arch",
    b"binding",
    b"channel",
    b"chdir",
    b"config",
    b"connected",
    b"cpuUsage",
    b"cwd",
    b"debugPort",
    b"default",
    b"disconnect",
    b"dlopen",
    b"domain",
    b"env",
    b"eventNames",
    b"execve",
    b"execArgv",
    b"execPath",
    b"features",
    b"finalization",
    b"getActiveResourcesInfo",
    b"getBuiltinModule",
    b"getMaxListeners",
    b"hrtime",
    b"kill",
    b"listenerCount",
    b"listeners",
    b"memoryUsage",
    b"moduleLoadList",
    b"nextTick",
    b"off",
    b"on",
    b"once",
    b"openStdin",
    b"pid",
    b"platform",
    b"ppid",
    b"prependListener",
    b"prependOnceListener",
    b"rawListeners",
    b"ref",
    b"release",
    b"removeAllListeners",
    b"removeListener",
    b"report",
    b"resourceUsage",
    b"reallyExit",
    b"setMaxListeners",
    b"setSourceMapsEnabled",
    b"send",
    b"sourceMapsEnabled",
    b"stderr",
    b"stdin",
    b"stdout",
    b"title",
    b"unref",
    b"uptime",
    b"version",
    b"versions",
];

const PROCESS_DEFAULT_KEYS: &[&[u8]] = &[
    b"_debugEnd",
    b"_debugProcess",
    b"_events",
    b"_eventsCount",
    b"_exiting",
    b"_fatalException",
    b"_getActiveHandles",
    b"_getActiveRequests",
    b"_kill",
    b"_linkedBinding",
    b"_maxListeners",
    b"_preload_modules",
    b"_rawDebug",
    b"_startProfilerIdleNotifier",
    b"_stopProfilerIdleNotifier",
    b"_tickCallback",
    b"abort",
    b"addListener",
    b"addUncaughtExceptionCaptureCallback",
    b"allowedNodeEnvironmentFlags",
    b"argv",
    b"argv0",
    b"arch",
    b"binding",
    b"channel",
    b"chdir",
    b"config",
    b"connected",
    b"cpuUsage",
    b"cwd",
    b"debugPort",
    b"disconnect",
    b"dlopen",
    b"domain",
    b"env",
    b"eventNames",
    b"execve",
    b"ref",
    b"unref",
    b"execArgv",
    b"execPath",
    b"features",
    b"finalization",
    b"getActiveResourcesInfo",
    b"getBuiltinModule",
    b"getMaxListeners",
    b"hrtime",
    b"kill",
    b"listenerCount",
    b"listeners",
    b"memoryUsage",
    b"moduleLoadList",
    b"nextTick",
    b"off",
    b"on",
    b"once",
    b"openStdin",
    b"pid",
    b"platform",
    b"ppid",
    b"prependListener",
    b"prependOnceListener",
    b"rawListeners",
    b"release",
    b"removeAllListeners",
    b"removeListener",
    b"report",
    b"resourceUsage",
    b"reallyExit",
    b"setMaxListeners",
    b"setSourceMapsEnabled",
    b"send",
    b"sourceMapsEnabled",
    b"stderr",
    b"stdin",
    b"stdout",
    b"title",
    b"uptime",
    b"version",
    b"versions",
];

const TIMERS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"setTimeout",
    b"clearTimeout",
    b"setImmediate",
    b"clearImmediate",
    b"setInterval",
    b"clearInterval",
    b"promises",
];

const OS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"EOL",
    b"arch",
    b"availableParallelism",
    b"constants",
    b"cpus",
    b"default",
    b"devNull",
    b"endianness",
    b"freemem",
    b"getPriority",
    b"homedir",
    b"hostname",
    b"loadavg",
    b"machine",
    b"networkInterfaces",
    b"platform",
    b"release",
    b"setPriority",
    b"tmpdir",
    b"totalmem",
    b"type",
    b"uptime",
    b"userInfo",
    b"version",
];

const PATH_NAMESPACE_KEYS: &[&[u8]] = &[
    b"_makeLong",
    b"basename",
    b"default",
    b"delimiter",
    b"dirname",
    b"extname",
    b"format",
    b"isAbsolute",
    b"join",
    b"matchesGlob",
    b"normalize",
    b"parse",
    b"posix",
    b"relative",
    b"resolve",
    b"sep",
    b"toNamespacedPath",
    b"win32",
];

const QUERYSTRING_NAMESPACE_KEYS: &[&[u8]] = &[
    b"decode",
    b"default",
    b"encode",
    b"escape",
    b"parse",
    b"stringify",
    b"unescape",
    b"unescapeBuffer",
];

const PUNYCODE_DEFAULT_KEYS: &[&[u8]] = &[
    b"version",
    b"ucs2",
    b"decode",
    b"encode",
    b"toASCII",
    b"toUnicode",
];

const PUNYCODE_NAMESPACE_KEYS: &[&[u8]] = &[
    b"decode",
    b"default",
    b"encode",
    b"toASCII",
    b"toUnicode",
    b"ucs2",
    b"version",
];

const PUNYCODE_UCS2_KEYS: &[&[u8]] = &[b"decode", b"encode"];

const URL_NAMESPACE_KEYS: &[&[u8]] = &[
    b"URL",
    b"URLSearchParams",
    b"URLPattern",
    b"Url",
    b"default",
    b"domainToASCII",
    b"domainToUnicode",
    b"fileURLToPath",
    b"fileURLToPathBuffer",
    b"format",
    b"parse",
    b"pathToFileURL",
    b"resolve",
    b"resolveObject",
    b"urlToHttpOptions",
];

const UTIL_NAMESPACE_KEYS: &[&[u8]] = &[
    b"_errnoException",
    b"_exceptionWithHostPort",
    b"_extend",
    b"aborted",
    b"callbackify",
    b"convertProcessSignalToExitCode",
    b"debug",
    b"debuglog",
    b"default",
    b"deprecate",
    b"diff",
    b"format",
    b"formatWithOptions",
    b"getCallSites",
    b"getSystemErrorMap",
    b"getSystemErrorName",
    b"getSystemErrorMessage",
    b"inherits",
    b"inspect",
    b"isArray",
    b"isDeepStrictEqual",
    b"promisify",
    b"stripVTControlCharacters",
    b"styleText",
    b"toUSVString",
    b"setTraceSigInt",
    b"types",
    b"parseArgs",
    b"MIMEParams",
    b"MIMEType",
    b"TextDecoder",
    b"TextEncoder",
    b"transferableAbortController",
    b"transferableAbortSignal",
];

const EVENTS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"EventEmitter",
    b"EventEmitterAsyncResource",
    b"default",
    b"defaultMaxListeners",
    b"usingDomains",
    b"captureRejections",
    b"captureRejectionSymbol",
    b"errorMonitor",
    b"init",
    b"listenerCount",
    b"on",
    b"once",
    b"addAbortListener",
    b"getEventListeners",
    b"getMaxListeners",
    b"setMaxListeners",
];

const VM_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Script",
    b"createContext",
    b"createScript",
    b"runInContext",
    b"runInNewContext",
    b"runInThisContext",
    b"isContext",
    b"compileFunction",
    b"measureMemory",
    b"constants",
];

const VM_MODULE_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Script",
    b"createContext",
    b"createScript",
    b"runInContext",
    b"runInNewContext",
    b"runInThisContext",
    b"isContext",
    b"compileFunction",
    b"measureMemory",
    b"constants",
    b"Module",
    b"SourceTextModule",
    b"SyntheticModule",
];

fn deprecated_constants_namespace_keys() -> &'static [&'static [u8]] {
    use std::sync::OnceLock;
    static MERGED: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    MERGED
        .get_or_init(|| {
            let keys = super::namespaces::constants::NM_CONSTANTS_DEFAULT.module_keys;
            let mut v: Vec<&'static [u8]> = Vec::with_capacity(keys.len() + 1);
            v.extend_from_slice(keys);
            v.push(b"default");
            v
        })
        .as_slice()
}

#[cfg(test)]
mod tests {
    #[test]
    fn rtld_deepbind_key_is_platform_gated() {
        let has_rtld_deepbind = super::super::namespaces::constants::NM_CONSTANTS_DEFAULT
            .module_keys
            .iter()
            .any(|key| *key == b"RTLD_DEEPBIND");
        assert_eq!(
            has_rtld_deepbind,
            cfg!(all(target_os = "linux", target_env = "gnu"))
        );
    }
}

// Node 26.5.1 enumerable exports; deprecated top-level constants are
// non-enumerable. All enumeration and interop copies use this surface.

const SQLITE_CONSTANTS_KEYS: &[&[u8]] = &[
    b"SQLITE_CHANGESET_DATA",
    b"SQLITE_CHANGESET_NOTFOUND",
    b"SQLITE_CHANGESET_CONFLICT",
    b"SQLITE_CHANGESET_CONSTRAINT",
    b"SQLITE_CHANGESET_FOREIGN_KEY",
    b"SQLITE_CHANGESET_OMIT",
    b"SQLITE_CHANGESET_REPLACE",
    b"SQLITE_CHANGESET_ABORT",
    b"SQLITE_OK",
    b"SQLITE_DENY",
    b"SQLITE_IGNORE",
    b"SQLITE_CREATE_INDEX",
    b"SQLITE_CREATE_TABLE",
    b"SQLITE_CREATE_TEMP_INDEX",
    b"SQLITE_CREATE_TEMP_TABLE",
    b"SQLITE_CREATE_TEMP_TRIGGER",
    b"SQLITE_CREATE_TEMP_VIEW",
    b"SQLITE_CREATE_TRIGGER",
    b"SQLITE_CREATE_VIEW",
    b"SQLITE_DELETE",
    b"SQLITE_DROP_INDEX",
    b"SQLITE_DROP_TABLE",
    b"SQLITE_DROP_TEMP_INDEX",
    b"SQLITE_DROP_TEMP_TABLE",
    b"SQLITE_DROP_TEMP_TRIGGER",
    b"SQLITE_DROP_TEMP_VIEW",
    b"SQLITE_DROP_TRIGGER",
    b"SQLITE_DROP_VIEW",
    b"SQLITE_INSERT",
    b"SQLITE_PRAGMA",
    b"SQLITE_READ",
    b"SQLITE_SELECT",
    b"SQLITE_TRANSACTION",
    b"SQLITE_UPDATE",
    b"SQLITE_ATTACH",
    b"SQLITE_DETACH",
    b"SQLITE_ALTER_TABLE",
    b"SQLITE_REINDEX",
    b"SQLITE_ANALYZE",
    b"SQLITE_CREATE_VTABLE",
    b"SQLITE_DROP_VTABLE",
    b"SQLITE_FUNCTION",
    b"SQLITE_SAVEPOINT",
    b"SQLITE_COPY",
    b"SQLITE_RECURSIVE",
];

const SEA_NAMESPACE_KEYS: &[&[u8]] = &[
    b"default",
    b"getAsset",
    b"getAssetAsBlob",
    b"getAssetKeys",
    b"getRawAsset",
    b"isSea",
];

const MODULE_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Module",
    b"SourceMap",
    b"_cache",
    b"_extensions",
    b"_findPath",
    b"_initPaths",
    b"_load",
    b"_nodeModulePaths",
    b"_pathCache",
    b"_preloadModules",
    b"_resolveFilename",
    b"_resolveLookupPaths",
    b"builtinModules",
    b"constants",
    b"createRequire",
    b"default",
    b"enableCompileCache",
    b"findPackageJSON",
    b"findSourceMap",
    b"flushCompileCache",
    b"getCompileCacheDir",
    b"getSourceMapsSupport",
    b"globalPaths",
    b"isBuiltin",
    b"register",
    b"registerHooks",
    b"runMain",
    b"setSourceMapsSupport",
    b"stripTypeScriptTypes",
    b"syncBuiltinESMExports",
];

pub(crate) fn native_module_enumerable_keys(module_name: &str) -> Option<&'static [&'static [u8]]> {
    let module_name = normalize_native_module_alias(module_name);
    match module_name {
        "fs" => Some(crate::fs::NM_FS.module_keys),
        "dgram" => Some(&[b"Socket", b"createSocket", b"default"]),
        "dgram.default" => Some(super::namespaces::dgram::NM_DGRAM_DEFAULT.module_keys),
        "async_hooks" => Some(ASYNC_HOOKS_NAMESPACE_KEYS),
        "async_hooks.default" => {
            Some(super::namespaces::async_hooks::NM_ASYNC_HOOKS_DEFAULT.module_keys)
        }
        "assert/strict" => Some(super::namespaces::assert::NM_ASSERT_STRICT.module_keys),
        "buffer.constants" => Some(super::namespaces::buffer::NM_BUFFER_CONSTANTS.module_keys),
        // #6667: `require("crypto")` / `import * as crypto` enumerate the full
        // export surface so wildcard interop + object spread copy the exports.
        // The table lives beside crypto's value-resolution in `constants.rs`.
        "crypto" => Some(super::namespaces::crypto::NM_CRYPTO.module_keys),
        "sqlite" => Some(&[
            b"Database",
            b"DatabaseSync",
            b"Session",
            b"Statement",
            b"StatementSync",
            b"backup",
            b"constants",
            b"default",
        ]),
        "sqlite.constants" => Some(SQLITE_CONSTANTS_KEYS),
        // #9599: the opt-in globalThis.Bun object and `import * as bun from
        // "bun"` share this one enumerable native-module surface.
        "bun:jsc" => Some(&[b"heapStats"]),
        "bun" => Some(&[
            b"Glob",
            b"JSONL",
            b"TOML",
            b"Terminal",
            b"Transpiler",
            b"YAML",
            b"ant",
            b"build",
            b"connect",
            b"deepEquals",
            b"file",
            b"fileURLToPath",
            b"gc",
            b"generateHeapSnapshot",
            b"hash",
            b"isStandaloneExecutable",
            b"listen",
            b"pathToFileURL",
            b"plugin",
            b"semver",
            b"serve",
            b"sleep",
            b"sleepSync",
            b"spawn",
            b"stderr",
            b"stdin",
            b"stdout",
            b"stringWidth",
            b"stripANSI",
            b"version",
            b"which",
            b"wrapAnsi",
            b"write",
            b"zstdDecompress",
            b"zstdDecompressSync",
        ]),
        "bun.ant" => Some(&[b"getPeerPid", b"getPeerUid", b"memoryPressureLevel"]),
        // bun:ffi (#6562) — stage-1 surface plus the declared-but-throwing
        // exports (their reads resolve to callables that raise the stage-1
        // error).
        "bun:ffi" => Some(&[
            b"dlopen",
            b"FFIType",
            b"ptr",
            b"CString",
            b"suffix",
            b"toArrayBuffer",
            b"toBuffer",
            b"JSCallback",
            b"CFunction",
            b"linkSymbols",
            b"viewSource",
            b"read",
        ]),
        "ffi" | "ffi.default" => Some(&[
            b"dlopen",
            b"getRawPointer",
            b"toArrayBuffer",
            b"toBuffer",
            b"toString",
            b"suffix",
        ]),
        "sea" => Some(SEA_NAMESPACE_KEYS),
        "sea.default" => Some(super::namespaces::sea::NM_SEA_DEFAULT.module_keys),
        "module" => Some(MODULE_NAMESPACE_KEYS),
        "domain" => Some(super::namespaces::domain::NM_DOMAIN.module_keys),
        // #3677: zlib.constants enumerates the full Z_*/BROTLI_*/ZSTD_* table.
        "zlib.constants" => Some(super::namespaces::zlib::NM_ZLIB_CONSTANTS.module_keys),
        // Deprecated path alias enumerable on the top-level and style
        // sub-namespaces, matching Node's `Object.keys(...).includes`.
        "path" => Some(PATH_NAMESPACE_KEYS),
        "path.default" => Some(super::namespaces::path::NM_PATH_DEFAULT.module_keys),
        "path.posix.default" => Some(super::namespaces::path::NM_PATH_POSIX_DEFAULT.module_keys),
        "path.win32.default" => Some(super::namespaces::path::NM_PATH_WIN32_DEFAULT.module_keys),
        "path.posix" | "path.win32" => Some(PATH_NAMESPACE_KEYS),
        "constants" => Some(deprecated_constants_namespace_keys()),
        "constants.default" => Some(super::namespaces::constants::NM_CONSTANTS_DEFAULT.module_keys),
        "dns" => Some(DNS_NAMESPACE_KEYS),
        "dns.default" => Some(super::namespaces::dns::NM_DNS_DEFAULT.module_keys),
        "dns/promises" => Some(DNS_PROMISES_NAMESPACE_KEYS),
        "dns/promises.default" => Some(super::namespaces::dns::NM_DNS_PROMISES_DEFAULT.module_keys),
        "child_process" => Some(CHILD_PROCESS_NAMESPACE_KEYS),
        "child_process.default" => {
            Some(super::namespaces::child_process::NM_CHILD_PROCESS_DEFAULT.module_keys)
        }
        "node-pty" => Some(NODE_PTY_NAMESPACE_KEYS),
        "node-pty.default" => Some(NODE_PTY_DEFAULT_KEYS),
        "cluster" => Some(CLUSTER_NAMESPACE_KEYS),
        "cluster.default" => Some(super::namespaces::cluster::NM_CLUSTER_DEFAULT.module_keys),
        "stream" => Some(STREAM_NAMESPACE_KEYS),
        "process" => Some(super::namespaces::process::NM_PROCESS.module_keys),
        "process.namespace" => Some(PROCESS_NAMESPACE_KEYS),
        "process.default" => Some(PROCESS_DEFAULT_KEYS),
        "buffer" => Some(super::namespaces::buffer::NM_BUFFER.module_keys),
        "querystring" => Some(QUERYSTRING_NAMESPACE_KEYS),
        "querystring.default" => {
            Some(super::namespaces::querystring::NM_QUERYSTRING_DEFAULT.module_keys)
        }
        "console" => Some(super::namespaces::console::NM_CONSOLE.module_keys),
        "punycode" => Some(PUNYCODE_NAMESPACE_KEYS),
        "punycode.default" => Some(PUNYCODE_DEFAULT_KEYS),
        "punycode.ucs2" => Some(PUNYCODE_UCS2_KEYS),
        "inspector" | "inspector.default" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_DEFAULT.module_keys)
        }
        "inspector/promises" | "inspector/promises.default" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_PROMISES_DEFAULT.module_keys)
        }
        "inspector.Network" => Some(super::namespaces::inspector::NM_INSPECTOR_NETWORK.module_keys),
        "inspector.NetworkResources" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_NETWORKRESOURCES.module_keys)
        }
        "inspector.DOMStorage" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_DOMSTORAGE.module_keys)
        }
        "timers" => Some(TIMERS_NAMESPACE_KEYS),
        "os" => Some(OS_NAMESPACE_KEYS),
        "os.default" => Some(super::namespaces::os::NM_OS_DEFAULT.module_keys),
        "url" => Some(URL_NAMESPACE_KEYS),
        "url.default" => Some(super::namespaces::url::NM_URL_DEFAULT.module_keys),
        "util" => Some(UTIL_NAMESPACE_KEYS),
        "util.default" => Some(super::namespaces::util::NM_UTIL_DEFAULT.module_keys),
        "net" => Some(super::namespaces::net::NM_NET.module_keys),
        "http" => Some(super::namespaces::http::NM_HTTP.module_keys),
        "https" => Some(super::namespaces::https::NM_HTTPS.module_keys),
        "http2" | "http2.constants" => crate::module_hooks::http2_keys(module_name),
        "tty" => Some(super::namespaces::tty::NM_TTY.module_keys),
        "v8" => Some(super::namespaces::v8::NM_V8.module_keys),
        "perf_hooks" => Some(super::namespaces::perf_hooks::NM_PERF_HOOKS.module_keys),
        "perf_hooks.default" => {
            Some(super::namespaces::perf_hooks::NM_PERF_HOOKS_DEFAULT.module_keys)
        }
        "perf_hooks.constants" => {
            Some(super::namespaces::perf_hooks::NM_PERF_HOOKS_CONSTANTS.module_keys)
        }
        "util/types" | "util.types" => Some(super::namespaces::util::NM_UTIL_TYPES.module_keys),
        "events" => Some(EVENTS_NAMESPACE_KEYS),
        "repl" | "repl.default" => Some(super::namespaces::repl::NM_REPL_DEFAULT.module_keys),
        "worker_threads" => Some(super::namespaces::worker_threads::NM_WORKER_THREADS.module_keys),
        "vm" => Some(if crate::node_vm::vm_modules_enabled() {
            VM_MODULE_NAMESPACE_KEYS
        } else {
            VM_NAMESPACE_KEYS
        }),
        "timers/promises" => Some(&[b"setTimeout", b"setImmediate", b"setInterval", b"scheduler"]),
        "readline/promises" => Some(&[b"Interface", b"Readline", b"createInterface"]),
        "wasi" => Some(&[b"WASI", b"default"]),
        "wasi.default" => Some(super::namespaces::wasi::NM_WASI_DEFAULT.module_keys),
        "zlib" => Some(super::namespaces::zlib::NM_ZLIB.module_keys),
        "tls" => Some(super::namespaces::tls::NM_TLS.module_keys),
        _ => None,
    }
}

/// Enumerability of ZLIB's read-only own data properties. Non-enumerable
/// legacy aliases are still own properties: descriptor lookup and assignment
/// must use this policy before the enumerable-export and override paths.
/// Node exposes all constants except the BROTLI_* family as legacy aliases.
/// Keep values and insertion order in the existing constants table.
pub(crate) fn zlib_readonly_property_enumerability(module_name: &str, key: &str) -> Option<bool> {
    match module_name {
        "zlib" if matches!(key, "codes" | "constants") => Some(true),
        "zlib"
            if !key.starts_with("BROTLI_")
                && super::namespaces::zlib::NM_ZLIB_CONSTANTS
                    .module_keys
                    .contains(&key.as_bytes()) =>
        {
            Some(false)
        }
        "zlib.constants"
            if super::namespaces::zlib::NM_ZLIB_CONSTANTS
                .module_keys
                .contains(&key.as_bytes()) =>
        {
            Some(true)
        }
        _ => None,
    }
}

pub(crate) fn native_module_has_enumerable_key(module_name: &str, key: &str) -> bool {
    if crate::process::worker_restrictions::worker_process_member_hidden(module_name, key) {
        return false;
    }
    if matches!(
        module_name,
        "process" | "process.namespace" | "process.default"
    ) && key == "permission"
    {
        return crate::process::process_permission_enabled();
    }
    native_module_enumerable_keys(module_name).is_some_and(|keys| keys.contains(&key.as_bytes()))
}
