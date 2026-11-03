use super::*;

// #6563: node-pty / @lydell/node-pty — `spawn` is the entire module surface
// the target apps touch; everything else lives on the returned IPty object.
const NODE_PTY_DEFAULT_KEYS: &[&[u8]] = &[b"spawn"];

const NODE_PTY_NAMESPACE_KEYS: &[&[u8]] = &[b"default", b"spawn"];

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

pub(crate) fn native_module_enumerable_keys(module_name: &str) -> Option<&'static [&'static [u8]]> {
    let module_name = normalize_native_module_alias(module_name);
    match module_name {
        "fs" => Some(crate::fs::NM_FS.module_keys),
        "dgram" => Some(super::namespaces::dgram::NM_DGRAM.module_keys),
        "dgram.default" => Some(super::namespaces::dgram::NM_DGRAM_DEFAULT.module_keys),
        "async_hooks" => Some(super::namespaces::async_hooks::NM_ASYNC_HOOKS.module_keys),
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
        "sea" => Some(super::namespaces::sea::NM_SEA.module_keys),
        "sea.default" => Some(super::namespaces::sea::NM_SEA_DEFAULT.module_keys),
        "module" => Some(super::namespaces::module::NM_MODULE.module_keys),
        "domain" => Some(super::namespaces::domain::NM_DOMAIN.module_keys),
        // #3677: zlib.constants enumerates the full Z_*/BROTLI_*/ZSTD_* table.
        "zlib.constants" => Some(super::namespaces::zlib::NM_ZLIB_CONSTANTS.module_keys),
        // Deprecated path alias enumerable on the top-level and style
        // sub-namespaces, matching Node's `Object.keys(...).includes`.
        "path" => Some(super::namespaces::path::NM_PATH.module_keys),
        "path.default" => Some(super::namespaces::path::NM_PATH_DEFAULT.module_keys),
        "path.posix.default" => Some(super::namespaces::path::NM_PATH_POSIX_DEFAULT.module_keys),
        "path.win32.default" => Some(super::namespaces::path::NM_PATH_WIN32_DEFAULT.module_keys),
        "path.posix" => Some(super::namespaces::path::NM_PATH_POSIX.module_keys),
        "path.win32" => Some(super::namespaces::path::NM_PATH_WIN32.module_keys),
        "constants" => Some(deprecated_constants_namespace_keys()),
        "constants.default" => Some(super::namespaces::constants::NM_CONSTANTS_DEFAULT.module_keys),
        "dns" => Some(super::namespaces::dns::NM_DNS.module_keys),
        "dns.default" => Some(super::namespaces::dns::NM_DNS_DEFAULT.module_keys),
        "dns/promises" => Some(super::namespaces::dns::NM_DNS_PROMISES.module_keys),
        "dns/promises.default" => Some(super::namespaces::dns::NM_DNS_PROMISES_DEFAULT.module_keys),
        "child_process" => Some(super::namespaces::child_process::NM_CHILD_PROCESS.module_keys),
        "child_process.default" => {
            Some(super::namespaces::child_process::NM_CHILD_PROCESS_DEFAULT.module_keys)
        }
        "node-pty" => Some(NODE_PTY_NAMESPACE_KEYS),
        "node-pty.default" => Some(NODE_PTY_DEFAULT_KEYS),
        "cluster" => Some(super::namespaces::cluster::NM_CLUSTER.module_keys),
        "cluster.default" => Some(super::namespaces::cluster::NM_CLUSTER_DEFAULT.module_keys),
        "stream" => Some(super::namespaces::stream::NM_STREAM.module_keys),
        "process" => Some(super::namespaces::process::NM_PROCESS.module_keys),
        "process.namespace" => Some(super::namespaces::process::NM_PROCESS_NAMESPACE.module_keys),
        "process.default" => Some(super::namespaces::process::NM_PROCESS_DEFAULT.module_keys),
        "buffer" => Some(super::namespaces::buffer::NM_BUFFER.module_keys),
        "querystring" => Some(super::namespaces::querystring::NM_QUERYSTRING.module_keys),
        "querystring.default" => {
            Some(super::namespaces::querystring::NM_QUERYSTRING_DEFAULT.module_keys)
        }
        "console" => Some(super::namespaces::console::NM_CONSOLE.module_keys),
        "punycode" => Some(super::namespaces::punycode::NM_PUNYCODE.module_keys),
        "punycode.default" => Some(super::namespaces::punycode::NM_PUNYCODE_DEFAULT.module_keys),
        "punycode.ucs2" => Some(super::namespaces::punycode::NM_PUNYCODE_UCS2.module_keys),
        "inspector" => Some(super::namespaces::inspector::NM_INSPECTOR.module_keys),
        "inspector.default" => Some(super::namespaces::inspector::NM_INSPECTOR_DEFAULT.module_keys),
        "inspector/promises" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_PROMISES.module_keys)
        }
        "inspector/promises.default" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_PROMISES_DEFAULT.module_keys)
        }
        "inspector.Network" => Some(super::namespaces::inspector::NM_INSPECTOR_NETWORK.module_keys),
        "inspector.NetworkResources" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_NETWORKRESOURCES.module_keys)
        }
        "inspector.DOMStorage" => {
            Some(super::namespaces::inspector::NM_INSPECTOR_DOMSTORAGE.module_keys)
        }
        "timers" => Some(super::namespaces::timers::NM_TIMERS.module_keys),
        "os" => Some(super::namespaces::os::NM_OS.module_keys),
        "os.default" => Some(super::namespaces::os::NM_OS_DEFAULT.module_keys),
        "url" => Some(super::namespaces::url::NM_URL.module_keys),
        "url.default" => Some(super::namespaces::url::NM_URL_DEFAULT.module_keys),
        "util" => Some(super::namespaces::util::NM_UTIL.module_keys),
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
        "events" => Some(super::namespaces::events::NM_EVENTS.module_keys),
        "repl" => Some(super::namespaces::repl::NM_REPL.module_keys),
        "repl.default" => Some(super::namespaces::repl::NM_REPL_DEFAULT.module_keys),
        "worker_threads" => Some(super::namespaces::worker_threads::NM_WORKER_THREADS.module_keys),
        "vm" => Some(if crate::node_vm::vm_modules_enabled() {
            VM_MODULE_NAMESPACE_KEYS
        } else {
            VM_NAMESPACE_KEYS
        }),
        "timers/promises" => Some(super::namespaces::timers::NM_TIMERS_PROMISES.module_keys),
        "readline/promises" => Some(super::namespaces::readline::NM_READLINE_PROMISES.module_keys),
        "wasi" => Some(super::namespaces::wasi::NM_WASI.module_keys),
        "wasi.default" => Some(super::namespaces::wasi::NM_WASI_DEFAULT.module_keys),
        "zlib" => Some(super::namespaces::zlib::NM_ZLIB.module_keys),
        "tls" => Some(super::namespaces::tls::NM_TLS.module_keys),
        "crypto.constants" => Some(super::namespaces::crypto::NM_CRYPTO_CONSTANTS.module_keys),
        "crypto.subtle" => Some(super::namespaces::crypto::NM_CRYPTO_SUBTLE.module_keys),
        "crypto.webcrypto" => Some(super::namespaces::crypto::NM_CRYPTO_WEBCRYPTO.module_keys),
        "fs.constants" => Some(crate::fs::NM_FS_CONSTANTS.module_keys),
        "os.constants" => Some(super::namespaces::os::NM_OS_CONSTANTS.module_keys),
        "os.constants.signals" => Some(super::namespaces::os::NM_OS_CONSTANTS_SIGNALS.module_keys),
        "os.constants.errno" => Some(super::namespaces::os::NM_OS_CONSTANTS_ERRNO.module_keys),
        "os.constants.priority" => {
            Some(super::namespaces::os::NM_OS_CONSTANTS_PRIORITY.module_keys)
        }
        "os.constants.dlopen" => Some(super::namespaces::os::NM_OS_CONSTANTS_DLOPEN.module_keys),
        "readline" => Some(super::namespaces::readline::NM_READLINE.module_keys),
        "stream/promises" => Some(super::namespaces::stream::NM_STREAM_PROMISES.module_keys),
        "string_decoder" => Some(super::namespaces::string_decoder::NM_STRING_DECODER.module_keys),
        "v8.promiseHooks" => Some(super::namespaces::v8::NM_V8_PROMISEHOOKS.module_keys),
        "v8.startupSnapshot" => Some(super::namespaces::v8::NM_V8_STARTUPSNAPSHOT.module_keys),
        "streams" => Some(super::namespaces::streams::NM_STREAMS.module_keys),
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
