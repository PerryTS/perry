use super::*;

// #6563: node-pty / @lydell/node-pty — `spawn` is the entire module surface
// the target apps touch; everything else lives on the returned IPty object.
static NM_NODE_PTY_DEFAULT: NativeModuleVtable = keyed(&[b"spawn"]);

static NM_NODE_PTY: NativeModuleVtable = keyed(&[b"default", b"spawn"]);

/// A vtable for a module whose keys are still a hand table beside it (no
/// item-level declaration yet).
const fn keyed(module_keys: &'static [&'static [u8]]) -> NativeModuleVtable {
    NativeModuleVtable {
        module_keys,
        ..NATIVE_MODULE_VTABLE_BASE
    }
}

/// `vm` with `PERRY_EXPERIMENTAL_VM_MODULES`: the declared `vm` keys plus the
/// module classes (the birth picks it; the flag is fixed for the process).
static NM_VM_MODULES: NativeModuleVtable = keyed(&[
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
]);

/// The deprecated `constants` module: `constants.default` plus `default`.
static NM_CONSTANTS: NativeModuleVtable = NativeModuleVtable {
    keys: |_| {
        Some(ModuleKeys {
            head: super::namespaces::constants::NM_CONSTANTS_DEFAULT.module_keys,
            tail: &[b"default"],
        })
    },
    ..NATIVE_MODULE_VTABLE_BASE
};

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

static NM_SQLITE_CONSTANTS: NativeModuleVtable = keyed(&[
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
]);

static NM_SQLITE: NativeModuleVtable = keyed(&[
    b"Database",
    b"DatabaseSync",
    b"Session",
    b"Statement",
    b"StatementSync",
    b"backup",
    b"constants",
    b"default",
]);

static NM_BUN_JSC: NativeModuleVtable = keyed(&[b"heapStats"]);

static NM_BUN: NativeModuleVtable = keyed(&[
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
]);

static NM_BUN_ANT: NativeModuleVtable =
    keyed(&[b"getPeerPid", b"getPeerUid", b"memoryPressureLevel"]);

static NM_BUN_FFI: NativeModuleVtable = keyed(&[
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
]);

static NM_FFI: NativeModuleVtable = keyed(&[
    b"dlopen",
    b"getRawPointer",
    b"toArrayBuffer",
    b"toBuffer",
    b"toString",
    b"suffix",
]);

/// The vtable a namespace of `module_name` is born with.
pub(crate) fn native_module_vtable_for(module_name: &str) -> &'static NativeModuleVtable {
    let module_name = normalize_native_module_alias(module_name);
    match module_name {
        "fs" => &crate::fs::NM_FS,
        "dgram" => &super::namespaces::dgram::NM_DGRAM,
        "dgram.default" => &super::namespaces::dgram::NM_DGRAM_DEFAULT,
        "async_hooks" => &super::namespaces::async_hooks::NM_ASYNC_HOOKS,
        "async_hooks.default" => &super::namespaces::async_hooks::NM_ASYNC_HOOKS_DEFAULT,
        "assert/strict" => &super::namespaces::assert::NM_ASSERT_STRICT,
        "buffer.constants" => &super::namespaces::buffer::NM_BUFFER_CONSTANTS,
        // #6667: `require("crypto")` / `import * as crypto` enumerate the full
        // export surface so wildcard interop + object spread copy the exports.
        // The table lives beside crypto's value-resolution in `constants.rs`.
        "crypto" => &super::namespaces::crypto::NM_CRYPTO,
        "sqlite" => &NM_SQLITE,
        "sqlite.constants" => &NM_SQLITE_CONSTANTS,
        // #9599: the opt-in globalThis.Bun object and `import * as bun from
        // "bun"` share this one enumerable native-module surface.
        "bun:jsc" => &NM_BUN_JSC,
        "bun" => &NM_BUN,
        "bun.ant" => &NM_BUN_ANT,
        // bun:ffi (#6562) — stage-1 surface plus the declared-but-throwing
        // exports (their reads resolve to callables that raise the stage-1
        // error).
        "bun:ffi" => &NM_BUN_FFI,
        "ffi" | "ffi.default" => &NM_FFI,
        "sea" => &super::namespaces::sea::NM_SEA,
        "sea.default" => &super::namespaces::sea::NM_SEA_DEFAULT,
        "module" => &super::namespaces::module::NM_MODULE,
        "domain" => &super::namespaces::domain::NM_DOMAIN,
        // #3677: zlib.constants enumerates the full Z_*/BROTLI_*/ZSTD_* table.
        "zlib.constants" => &super::namespaces::zlib::NM_ZLIB_CONSTANTS,
        // Deprecated path alias enumerable on the top-level and style
        // sub-namespaces, matching Node's `Object.keys(...).includes`.
        "path" => &super::namespaces::path::NM_PATH,
        "path.default" => &super::namespaces::path::NM_PATH_DEFAULT,
        "path.posix.default" => &super::namespaces::path::NM_PATH_POSIX_DEFAULT,
        "path.win32.default" => &super::namespaces::path::NM_PATH_WIN32_DEFAULT,
        "path.posix" => &super::namespaces::path::NM_PATH_POSIX,
        "path.win32" => &super::namespaces::path::NM_PATH_WIN32,
        "constants" => &NM_CONSTANTS,
        "constants.default" => &super::namespaces::constants::NM_CONSTANTS_DEFAULT,
        "dns" => &super::namespaces::dns::NM_DNS,
        "dns.default" => &super::namespaces::dns::NM_DNS_DEFAULT,
        "dns/promises" => &super::namespaces::dns::NM_DNS_PROMISES,
        "dns/promises.default" => &super::namespaces::dns::NM_DNS_PROMISES_DEFAULT,
        "child_process" => &super::namespaces::child_process::NM_CHILD_PROCESS,
        "child_process.default" => &super::namespaces::child_process::NM_CHILD_PROCESS_DEFAULT,
        "node-pty" => &NM_NODE_PTY,
        "node-pty.default" => &NM_NODE_PTY_DEFAULT,
        "cluster" => &super::namespaces::cluster::NM_CLUSTER,
        "cluster.default" => &super::namespaces::cluster::NM_CLUSTER_DEFAULT,
        "stream" => &super::namespaces::stream::NM_STREAM,
        "process" => &super::namespaces::process::NM_PROCESS,
        "process.namespace" => &super::namespaces::process::NM_PROCESS_NAMESPACE,
        "process.default" => &super::namespaces::process::NM_PROCESS_DEFAULT,
        "buffer" => &super::namespaces::buffer::NM_BUFFER,
        "querystring" => &super::namespaces::querystring::NM_QUERYSTRING,
        "querystring.default" => &super::namespaces::querystring::NM_QUERYSTRING_DEFAULT,
        "console" => &super::namespaces::console::NM_CONSOLE,
        "punycode" => &super::namespaces::punycode::NM_PUNYCODE,
        "punycode.default" => &super::namespaces::punycode::NM_PUNYCODE_DEFAULT,
        "punycode.ucs2" => &super::namespaces::punycode::NM_PUNYCODE_UCS2,
        "inspector" => &super::namespaces::inspector::NM_INSPECTOR,
        "inspector.default" => &super::namespaces::inspector::NM_INSPECTOR_DEFAULT,
        "inspector/promises" => &super::namespaces::inspector::NM_INSPECTOR_PROMISES,
        "inspector/promises.default" => {
            &super::namespaces::inspector::NM_INSPECTOR_PROMISES_DEFAULT
        }
        "inspector.Network" => &super::namespaces::inspector::NM_INSPECTOR_NETWORK,
        "inspector.NetworkResources" => {
            &super::namespaces::inspector::NM_INSPECTOR_NETWORKRESOURCES
        }
        "inspector.DOMStorage" => &super::namespaces::inspector::NM_INSPECTOR_DOMSTORAGE,
        "timers" => &super::namespaces::timers::NM_TIMERS,
        "os" => &super::namespaces::os::NM_OS,
        "os.default" => &super::namespaces::os::NM_OS_DEFAULT,
        "url" => &super::namespaces::url::NM_URL,
        "url.default" => &super::namespaces::url::NM_URL_DEFAULT,
        "util" => &super::namespaces::util::NM_UTIL,
        "util.default" => &super::namespaces::util::NM_UTIL_DEFAULT,
        "net" => &super::namespaces::net::NM_NET,
        "http" => &super::namespaces::http::NM_HTTP,
        "https" => &super::namespaces::https::NM_HTTPS,
        "http2" | "http2.constants" => {
            crate::module_hooks::http2_vtable(module_name).unwrap_or(&NATIVE_MODULE_VTABLE_IMPL)
        }
        "tty" => &super::namespaces::tty::NM_TTY,
        "v8" => &super::namespaces::v8::NM_V8,
        "perf_hooks" => &super::namespaces::perf_hooks::NM_PERF_HOOKS,
        "perf_hooks.default" => &super::namespaces::perf_hooks::NM_PERF_HOOKS_DEFAULT,
        "perf_hooks.constants" => &super::namespaces::perf_hooks::NM_PERF_HOOKS_CONSTANTS,
        "util/types" | "util.types" => &super::namespaces::util::NM_UTIL_TYPES,
        "events" => &super::namespaces::events::NM_EVENTS,
        "repl" => &super::namespaces::repl::NM_REPL,
        "repl.default" => &super::namespaces::repl::NM_REPL_DEFAULT,
        "worker_threads" => &super::namespaces::worker_threads::NM_WORKER_THREADS,
        "vm" if crate::node_vm::vm_modules_enabled() => &NM_VM_MODULES,
        "vm" => &super::namespaces::vm::NM_VM,
        "timers/promises" => &super::namespaces::timers::NM_TIMERS_PROMISES,
        "readline/promises" => &super::namespaces::readline::NM_READLINE_PROMISES,
        "wasi" => &super::namespaces::wasi::NM_WASI,
        "wasi.default" => &super::namespaces::wasi::NM_WASI_DEFAULT,
        "zlib" => &super::namespaces::zlib::NM_ZLIB,
        "tls" => &super::namespaces::tls::NM_TLS,
        "crypto.constants" => &super::namespaces::crypto::NM_CRYPTO_CONSTANTS,
        "crypto.subtle" => &super::namespaces::crypto::NM_CRYPTO_SUBTLE,
        "crypto.webcrypto" => &super::namespaces::crypto::NM_CRYPTO_WEBCRYPTO,
        "fs.constants" => &crate::fs::NM_FS_CONSTANTS,
        "os.constants" => &super::namespaces::os::NM_OS_CONSTANTS,
        "os.constants.signals" => &super::namespaces::os::NM_OS_CONSTANTS_SIGNALS,
        "os.constants.errno" => &super::namespaces::os::NM_OS_CONSTANTS_ERRNO,
        "os.constants.priority" => &super::namespaces::os::NM_OS_CONSTANTS_PRIORITY,
        "os.constants.dlopen" => &super::namespaces::os::NM_OS_CONSTANTS_DLOPEN,
        "readline" => &super::namespaces::readline::NM_READLINE,
        "stream/promises" => &super::namespaces::stream::NM_STREAM_PROMISES,
        "string_decoder" => &super::namespaces::string_decoder::NM_STRING_DECODER,
        "v8.promiseHooks" => &super::namespaces::v8::NM_V8_PROMISEHOOKS,
        "v8.startupSnapshot" => &super::namespaces::v8::NM_V8_STARTUPSNAPSHOT,
        "streams" => &super::namespaces::streams::NM_STREAMS,
        _ => &NATIVE_MODULE_VTABLE_IMPL,
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

pub(crate) fn native_module_has_enumerable_key(
    vt: &'static NativeModuleVtable,
    module_name: &str,
    key: &str,
) -> bool {
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
    (vt.keys)(vt).is_some_and(|keys| keys.contains(key.as_bytes()))
}
