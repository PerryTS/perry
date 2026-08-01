use super::*;
use std::sync::atomic::Ordering;

// #6667: `node:crypto` export surface. Node 22–26 enumerate 70 own keys on the
// CJS namespace (`Object.keys(require("crypto")).length === 70`); order matches
// Node's insertion order. Every key resolves to a live value through this
// module's crypto property-resolution (callable exports, sub-namespaces,
// constants) with the single exception of `setEngine`, whose OpenSSL-engine
// implementation Perry does not ship — it is listed for enumeration/interop
// parity and reads `undefined`, mirroring the module's other yet-unimplemented
// tails. Consumed by `module_keys::native_module_enumerable_keys` and, through
// it, by `copy_native_module_exports` for object spread / `Object.assign`.
// Before this table existed, `Object.keys`/for-in/spread saw only the internal
// `__module__` sentinel, so every enumeration-based interop layer (turbopack
// `e.i`, Babel `interopRequireWildcard`, plain `{ ...crypto }`) produced an
// empty namespace — e.g. jose's HKDF fell back to `(0, ns.createHmac)(...)` on
// `undefined` and threw on the Auth.js success path.
pub(super) const CRYPTO_NAMESPACE_KEYS: &[&[u8]] = &[
    b"argon2",
    b"argon2Sync",
    b"checkPrime",
    b"checkPrimeSync",
    b"createCipheriv",
    b"createDecipheriv",
    b"createDiffieHellman",
    b"createDiffieHellmanGroup",
    b"createECDH",
    b"createHash",
    b"createHmac",
    b"createPrivateKey",
    b"createPublicKey",
    b"createSecretKey",
    b"createSign",
    b"createVerify",
    b"diffieHellman",
    b"generatePrime",
    b"generatePrimeSync",
    b"getCiphers",
    b"getCipherInfo",
    b"getCurves",
    b"getDiffieHellman",
    b"getHashes",
    b"hkdf",
    b"hkdfSync",
    b"pbkdf2",
    b"pbkdf2Sync",
    b"generateKeyPair",
    b"generateKeyPairSync",
    b"generateKey",
    b"generateKeySync",
    b"privateDecrypt",
    b"privateEncrypt",
    b"publicDecrypt",
    b"publicEncrypt",
    b"randomBytes",
    b"randomFill",
    b"randomFillSync",
    b"randomInt",
    b"randomUUID",
    b"randomUUIDv7",
    b"scrypt",
    b"scryptSync",
    b"sign",
    b"setEngine",
    b"timingSafeEqual",
    b"getFips",
    b"setFips",
    b"verify",
    b"hash",
    b"encapsulate",
    b"decapsulate",
    b"Certificate",
    b"Cipheriv",
    b"Decipheriv",
    b"DiffieHellman",
    b"DiffieHellmanGroup",
    b"ECDH",
    b"Hash",
    b"Hmac",
    b"KeyObject",
    b"Sign",
    b"Verify",
    b"X509Certificate",
    b"secureHeapUsed",
    b"constants",
    b"webcrypto",
    b"subtle",
    b"getRandomValues",
];
fn dns_lookup_flag_constant(property: &str) -> Option<f64> {
    #[cfg(unix)]
    fn ai_addrconfig() -> f64 {
        libc::AI_ADDRCONFIG as f64
    }
    #[cfg(windows)]
    fn ai_addrconfig() -> f64 {
        0x0400 as f64
    }
    #[cfg(not(any(unix, windows)))]
    fn ai_addrconfig() -> f64 {
        0x0020 as f64
    }
    #[cfg(unix)]
    fn ai_v4mapped() -> f64 {
        libc::AI_V4MAPPED as f64
    }
    #[cfg(windows)]
    fn ai_v4mapped() -> f64 {
        0x0800 as f64
    }
    #[cfg(not(any(unix, windows)))]
    fn ai_v4mapped() -> f64 {
        0x0008 as f64
    }
    #[cfg(unix)]
    fn ai_all() -> f64 {
        libc::AI_ALL as f64
    }
    #[cfg(windows)]
    fn ai_all() -> f64 {
        0x0100 as f64
    }
    #[cfg(not(any(unix, windows)))]
    fn ai_all() -> f64 {
        0x0010 as f64
    }

    match property {
        "ADDRCONFIG" => Some(ai_addrconfig()),
        "V4MAPPED" => Some(ai_v4mapped()),
        "ALL" => Some(ai_all()),
        _ => None,
    }
}

fn cached_inspector_object(property: &str, create: impl FnOnce() -> f64) -> f64 {
    let key = format!("inspector\0object\0{property}");
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|cache| cache.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }
    let value = create();
    NATIVE_CALLABLE_EXPORTS.with(|cache| {
        cache.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

fn dns_error_alias(property: &str) -> Option<&'static str> {
    match property {
        "NODATA" => Some("ENODATA"),
        "FORMERR" => Some("EFORMERR"),
        "SERVFAIL" => Some("ESERVFAIL"),
        "NOTFOUND" => Some("ENOTFOUND"),
        "NOTIMP" => Some("ENOTIMP"),
        "REFUSED" => Some("EREFUSED"),
        "BADQUERY" => Some("EBADQUERY"),
        "BADNAME" => Some("EBADNAME"),
        "BADFAMILY" => Some("EBADFAMILY"),
        "BADRESP" => Some("EBADRESP"),
        "CONNREFUSED" => Some("ECONNREFUSED"),
        "TIMEOUT" => Some("ETIMEOUT"),
        "EOF" => Some("EOF"),
        "FILE" => Some("EFILE"),
        "NOMEM" => Some("ENOMEM"),
        "DESTRUCTION" => Some("EDESTRUCTION"),
        "BADSTR" => Some("EBADSTR"),
        "BADFLAGS" => Some("EBADFLAGS"),
        "NONAME" => Some("ENONAME"),
        "BADHINTS" => Some("EBADHINTS"),
        "NOTINITIALIZED" => Some("ENOTINITIALIZED"),
        "LOADIPHLPAPI" => Some("ELOADIPHLPAPI"),
        "ADDRGETNETWORKPARAMS" => Some("EADDRGETNETWORKPARAMS"),
        "CANCELLED" => Some("ECANCELLED"),
        _ => None,
    }
}

/// Return constant (non-method) property values for native modules.
/// Returns None for method names, which should create bound closures instead.
pub(crate) unsafe fn get_native_module_constant(
    module_name: &str,
    property: &str,
    namespace_obj: f64,
) -> Option<f64> {
    let str_val = |s: &str| -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };
    let cjs_default_base = cjs_default_base_module(module_name);
    let is_cjs_default_object = cjs_default_base.is_some();
    let module_name = cjs_default_base.unwrap_or(module_name);
    if module_name == "process.namespace" && property == "default" {
        return cjs_default_export_value("process");
    }

    // Node's `require('stream')` IS the legacy `Stream` constructor (a function
    // that also carries `.Readable`/`.Writable`/… statics), so its `.prototype`
    // is the EventEmitter-derived `Stream.prototype`. Perry models the module as
    // a namespace OBJECT, so `require('stream').prototype` was `undefined`.
    // readable-stream's `Readable.prototype.on = function (ev, fn) { var res =
    // Stream.prototype.on.call(this, ev, fn); … }` (where `Stream =
    // require('stream')`) then threw "Function.prototype.call was called on a
    // value that is not a function". Resolve `require('stream').prototype` to the
    // same legacy `Stream.prototype` the `.Stream` export carries (minted +
    // cached by `bound_native_callable_export_value("stream", "Stream")`), which
    // now exposes the EventEmitter prototype methods.
    if module_name == "stream" && property == "prototype" {
        let stream_ctor = bound_native_callable_export_value("stream", "Stream");
        let ctor_ptr = (stream_ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        if ctor_ptr != 0 {
            let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
            if !JSValue::from_bits(proto.to_bits()).is_undefined() {
                return Some(proto);
            }
        }
    }

    if property == "default" && !is_cjs_default_object && module_name != "process" {
        if let Some(value) = cjs_default_export_value(module_name) {
            return Some(value);
        }
    }

    let module_name = if module_name == "process.namespace" {
        "process"
    } else {
        module_name
    };

    // #3906/#3679: node:v8 lifecycle namespaces. `v8.startupSnapshot` /
    // `v8.promiseHooks` are object-valued exports; resolve them to dedicated
    // native-module namespace objects so `typeof === "object"` and their
    // methods dispatch through `dispatch_native_module_method`. Handled here
    // (rather than only in the codegen `js_native_module_property_by_name`
    // path) so dynamic reads — `v8["promiseHooks"]`, `const { promiseHooks } =
    // v8` — resolve to the same object instead of `undefined`.
    if module_name == "v8" && matches!(property, "startupSnapshot" | "promiseHooks") {
        let submodule = if property == "startupSnapshot" {
            "v8.startupSnapshot"
        } else {
            "v8.promiseHooks"
        };
        return Some(js_create_native_module_namespace(
            submodule.as_ptr(),
            submodule.len(),
        ));
    }

    // bun:ffi (#6562): `FFIType` is a plain enum object (cached, GC-rooted
    // in `bun_ffi::types`); `suffix` is the platform dylib suffix string.
    if module_name == "bun:ffi" {
        match property {
            "FFIType" => return Some(crate::bun_ffi::types::ffi_type_object_value()),
            "suffix" => return Some(crate::bun_ffi::types::suffix_value()),
            _ => {}
        }
    }

    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        {
            0x0100 as f64
        }
        #[cfg(target_os = "linux")]
        {
            0x20000 as f64
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            0x0100 as f64
        }
    };
    let o_creat = {
        #[cfg(unix)]
        {
            libc::O_CREAT as f64
        }
        #[cfg(not(unix))]
        {
            0x200 as f64
        }
    };
    let o_trunc = {
        #[cfg(unix)]
        {
            libc::O_TRUNC as f64
        }
        #[cfg(not(unix))]
        {
            0x400 as f64
        }
    };
    let o_append = {
        #[cfg(unix)]
        {
            libc::O_APPEND as f64
        }
        #[cfg(not(unix))]
        {
            0x8 as f64
        }
    };
    let o_excl = {
        #[cfg(unix)]
        {
            libc::O_EXCL as f64
        }
        #[cfg(not(unix))]
        {
            0x800 as f64
        }
    };

    // Helper for fs constants — shared between "fs" and "fs.constants" modules.
    // Using a nested match (module first, then property) instead of OR patterns
    // on tuples, because rustc's match optimizer can miscompile tuple OR patterns
    // by absorbing one alternative's entries into the other branch's decision tree.
    let fs_const = |prop: &str| -> Option<f64> {
        match prop {
            "F_OK" => Some(0.0),
            "R_OK" => Some(4.0),
            "W_OK" => Some(2.0),
            "X_OK" => Some(1.0),
            "O_RDONLY" => Some(0.0),
            "O_WRONLY" => Some(1.0),
            "O_RDWR" => Some(2.0),
            "O_NOFOLLOW" => Some(o_nofollow),
            "O_CREAT" => Some(o_creat),
            "O_TRUNC" => Some(o_trunc),
            "O_APPEND" => Some(o_append),
            "O_EXCL" => Some(o_excl),
            "COPYFILE_EXCL" => Some(1.0),
            "COPYFILE_FICLONE" => Some(2.0),
            "COPYFILE_FICLONE_FORCE" => Some(4.0),
            "S_IRUSR" => Some(0o400 as f64),
            "S_IWUSR" => Some(0o200 as f64),
            "S_IXUSR" => Some(0o100 as f64),
            "S_IRGRP" => Some(0o040 as f64),
            "S_IWGRP" => Some(0o020 as f64),
            "S_IXGRP" => Some(0o010 as f64),
            "S_IROTH" => Some(0o004 as f64),
            "S_IWOTH" => Some(0o002 as f64),
            "S_IXOTH" => Some(0o001 as f64),
            _ => None,
        }
    };

    // #3683: POSIX file-mode/open flags, libuv dirent/symlink/copyfile flags.
    // libuv (UV_*) values are platform-independent. S_IF* file-type masks are
    // POSIX-standard (identical on Linux/macOS). The O_* flags are OS-specific,
    // so use `libc::` on Unix for host-accurate parity with Node; the literal
    // fallbacks mirror macOS values (where Perry's primary target runs).
    let fs_const_tail = fs_const_tail_lookup;

    // #3683: `constants.defaultCoreCipherList` — OpenSSL's built-in default
    // TLS cipher list string Node exposes (informational metadata, not a
    // behavioral toggle). Matches Node's compiled-in default.
    const DEFAULT_CORE_CIPHER_LIST: &str = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA";

    // Issue #649: `os.constants.signals.SIGINT`, `os.constants.errno.ENOENT`,
    // `os.constants.priority.PRIORITY_NORMAL`, `os.constants.dlopen.RTLD_LAZY`
    // are ubiquitous in Node ecosystem code. Pre-fix every read returned
    // undefined. Use `libc::*` on Unix for byte-identical parity with Node.
    let os_signal_const = os_signal_const_lookup;

    let os_errno_const = os_errno_const_lookup;

    let os_priority_const = |prop: &str| -> Option<f64> {
        match prop {
            "PRIORITY_LOW" => Some(19.0),
            "PRIORITY_BELOW_NORMAL" => Some(10.0),
            "PRIORITY_NORMAL" => Some(0.0),
            "PRIORITY_ABOVE_NORMAL" => Some(-7.0),
            "PRIORITY_HIGH" => Some(-14.0),
            "PRIORITY_HIGHEST" => Some(-20.0),
            _ => None,
        }
    };

    let os_dlopen_const = |prop: &str| -> Option<f64> {
        #[cfg(unix)]
        {
            match prop {
                "RTLD_LAZY" => Some(libc::RTLD_LAZY as f64),
                "RTLD_NOW" => Some(libc::RTLD_NOW as f64),
                "RTLD_GLOBAL" => Some(libc::RTLD_GLOBAL as f64),
                "RTLD_LOCAL" => Some(libc::RTLD_LOCAL as f64),
                #[cfg(all(target_os = "linux", target_env = "gnu"))]
                "RTLD_DEEPBIND" => Some(libc::RTLD_DEEPBIND as f64),
                _ => None,
            }
        }
        #[cfg(not(unix))]
        {
            match prop {
                "RTLD_LAZY" => Some(1.0),
                "RTLD_NOW" => Some(2.0),
                "RTLD_GLOBAL" => Some(8.0),
                "RTLD_LOCAL" => Some(4.0),
                _ => None,
            }
        }
    };

    // Issue #649: `crypto.constants.RSA_PKCS1_PADDING` etc. OpenSSL-defined
    // stable values; hardcoded to match Node 24.x's published table.
    let crypto_const = crypto_const_lookup;

    // `zlib.constants` — the Z_*/DEFLATE/INFLATE/GZIP/BROTLI_*/ZSTD_*
    // table Node exposes on `require('node:zlib').constants`. Match the
    // JavaScript-visible table rather than blindly mirroring every zlib.h
    // macro: modern Node exposes ZLIB_VERNUM but omits Z_TREES.
    // Required by axios for its stream wiring.
    let zlib_const = zlib_const_lookup;

    let sqlite_const = sqlite_const_lookup;

    match module_name {
        // node:punycode (deprecated, #2513) — the bundled punycode.js version
        // and the `ucs2` code-point helper sub-namespace (#2607).
        "punycode" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("punycode"),
            "version" => Some(str_val(crate::punycode::PUNYCODE_VERSION)),
            "ucs2" => Some(create_sub_namespace("punycode.ucs2")),
            _ => None,
        },
        // node:perf_hooks — `performance.timeOrigin` (ms since epoch at start)
        // and the `constants.NODE_PERFORMANCE_GC_*` numeric table. Both the
        // `performance` and `constants` objects are tagged "perf_hooks", so
        // they share this arm (distinct property names, no collision).
        "perf_hooks" => match property {
            "timeOrigin" => Some(crate::perf_hooks::time_origin_ms()),
            "nodeTiming" => Some(crate::perf_hooks::js_perf_node_timing()),
            "NODE_PERFORMANCE_GC_MAJOR" => Some(4.0),
            "NODE_PERFORMANCE_GC_MINOR" => Some(1.0),
            "NODE_PERFORMANCE_GC_INCREMENTAL" => Some(8.0),
            "NODE_PERFORMANCE_GC_WEAKCB" => Some(16.0),
            "NODE_PERFORMANCE_GC_FLAGS_NO" => Some(0.0),
            "NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED" => Some(2.0),
            "NODE_PERFORMANCE_GC_FLAGS_FORCED" => Some(4.0),
            "NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING" => Some(8.0),
            "NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE" => Some(16.0),
            "NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY" => Some(32.0),
            "NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE" => Some(64.0),
            _ => None,
        },
        "module" => match property {
            "Module" => Some(bound_native_callable_export_value("module", "Module")),
            "builtinModules" => Some(crate::process::js_module_builtin_modules()),
            "constants" => Some(crate::process::js_module_constants()),
            "globalPaths" => Some(module_cjs_global_paths_value()),
            "_cache" => Some(module_cjs_cache_value()),
            "_extensions" => Some(module_cjs_extensions_value()),
            "_pathCache" => Some(module_cjs_path_cache_value()),
            "_resolveFilename"
            | "_resolveLookupPaths"
            | "_load"
            | "_findPath"
            | "_nodeModulePaths"
            | "_initPaths"
            | "_preloadModules" => Some(bound_native_callable_export_value("module", property)),
            _ => None,
        },
        "inspector" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("inspector"),
            "console" => Some(cached_inspector_object("console", || {
                crate::node_inspector::js_node_inspector_console_object()
            })),
            "Network" => Some(cached_inspector_object("Network", || {
                create_sub_namespace("inspector.Network")
            })),
            "NetworkResources" => Some(cached_inspector_object("NetworkResources", || {
                create_sub_namespace("inspector.NetworkResources")
            })),
            "DOMStorage" => Some(cached_inspector_object("DOMStorage", || {
                create_sub_namespace("inspector.DOMStorage")
            })),
            "Session" => {
                let value = bound_native_callable_export_value("inspector", "Session");
                crate::node_inspector::install_session_prototype(value, false);
                Some(value)
            }
            _ => None,
        },
        "inspector/promises" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("inspector/promises"),
            "Session" => {
                let value = bound_native_callable_export_value("inspector/promises", "Session");
                crate::node_inspector::install_session_prototype(value, true);
                Some(value)
            }
            // Node's promise entry point spreads the callback namespace and
            // replaces only Session, so read the callback export itself.
            _ => {
                let name =
                    crate::string::js_string_from_bytes(property.as_ptr(), property.len() as u32);
                let callback = cjs_default_export_value("inspector")?;
                let scope = crate::gc::RuntimeHandleScope::new();
                let callback = scope.root_nanbox_f64(callback);
                let raw = (callback.get_nanbox_u64() & crate::value::POINTER_MASK)
                    as *const crate::ObjectHeader;
                let value = crate::object::js_object_get_field_by_name_f64(raw, name);
                (value.to_bits() != crate::value::TAG_UNDEFINED).then_some(value)
            }
        },
        "process" => crate::process::process_metadata_property(property),
        "dns" => match property {
            "promises" => {
                crate::dns::dns_promises_init_servers_from_callback_if_unset();
                cjs_default_export_value("dns/promises")
            }
            _ => dns_lookup_flag_constant(property)
                .or_else(|| dns_error_alias(property).map(&str_val)),
        },
        "dns/promises" => dns_error_alias(property).map(&str_val),
        "async_hooks" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("async_hooks"),
            "asyncWrapProviders" => Some(crate::async_hooks::js_async_hooks_async_wrap_providers()),
            _ => None,
        },
        "querystring" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("querystring"),
            _ => None,
        },
        "constants" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("constants"),
            _ => fs_const(property)
                .or_else(|| fs_const_tail(property))
                .or_else(|| os_signal_const(property))
                .or_else(|| os_errno_const(property))
                .or_else(|| os_priority_const(property))
                .or_else(|| os_dlopen_const(property))
                .or_else(|| crypto_const(property))
                .or_else(|| {
                    if property == "defaultCoreCipherList" {
                        Some(str_val(DEFAULT_CORE_CIPHER_LIST))
                    } else {
                        None
                    }
                }),
        },
        "sqlite" => match property {
            "constants" => Some(create_sub_namespace("sqlite.constants")),
            "Session" => Some(sqlite_session_constructor_value()),
            "StatementSync" => Some(sqlite_statement_sync_constructor_value()),
            _ => None,
        },
        "sqlite.constants" => sqlite_const(property),
        "path" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("path"),
            "sep" => {
                if cfg!(windows) {
                    Some(str_val("\\"))
                } else {
                    Some(str_val("/"))
                }
            }
            "delimiter" => {
                if cfg!(windows) {
                    Some(str_val(";"))
                } else {
                    Some(str_val(":"))
                }
            }
            "toNamespacedPath" | "_makeLong" => Some(bound_native_callable_export_value(
                "path",
                "toNamespacedPath",
            )),
            "posix" => cjs_default_export_value("path.posix"),
            "win32" => cjs_default_export_value("path.win32"),
            _ => None,
        },
        "path.posix" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("path.posix"),
            "sep" => Some(str_val("/")),
            "delimiter" => Some(str_val(":")),
            "toNamespacedPath" | "_makeLong" => Some(bound_native_callable_export_value(
                "path.posix",
                "toNamespacedPath",
            )),
            "posix" => cjs_default_export_value("path.posix"),
            "win32" => cjs_default_export_value("path.win32"),
            _ => None,
        },
        "path.win32" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("path.win32"),
            "sep" => Some(str_val("\\")),
            "delimiter" => Some(str_val(";")),
            "toNamespacedPath" | "_makeLong" => Some(bound_native_callable_export_value(
                "path.win32",
                "toNamespacedPath",
            )),
            "posix" => cjs_default_export_value("path.posix"),
            "win32" => cjs_default_export_value("path.win32"),
            _ => None,
        },
        "fs" => match property {
            "constants" => Some(create_sub_namespace("fs.constants")),
            // #2133: `fs.promises` — populated `fs_promises` singleton so
            // `const { open } = fs.promises` (and FileHandle dispatch) work.
            "promises" => Some(unsafe {
                crate::node_submodules::js_node_submodule_namespace(
                    b"fs_promises".as_ptr(),
                    "fs_promises".len() as u32,
                )
            }),
            _ => fs_const(property).or_else(|| fs_const_tail(property)),
        },
        "fs.constants" => fs_const(property).or_else(|| fs_const_tail(property)),
        "buffer" => match property {
            "Buffer" => Some(buffer_constructor_value()),
            "Blob" => Some(js_get_global_this_builtin_value(b"Blob".as_ptr(), 4)),
            "File" => Some(js_get_global_this_builtin_value(b"File".as_ptr(), 4)),
            "constants" => Some(create_sub_namespace("buffer.constants")),
            // Match Node's common 64-bit max Buffer length value. Perry won't
            // actually allocate buffers this large, but shape/value parity lets
            // packages feature-detect the Buffer surface without falling over.
            "kMaxLength" => Some(9_007_199_254_740_991.0),
            "kStringMaxLength" => Some(536870888.0),
            "INSPECT_MAX_BYTES" => Some(50.0),
            _ => None,
        },
        "timers" => match property {
            "promises" => Some(unsafe {
                crate::node_submodules::js_node_submodule_namespace(
                    b"timers_promises".as_ptr(),
                    "timers_promises".len() as u32,
                )
            }),
            _ => None,
        },
        "buffer.constants" => match property {
            "MAX_LENGTH" => Some(9_007_199_254_740_991.0),
            "MAX_STRING_LENGTH" => Some(536870888.0),
            _ => None,
        },
        "buffer.Buffer" => match property {
            "poolSize" => Some(buffer_pool_size()),
            "name" => Some(str_val("Buffer")),
            _ => None,
        },
        "os" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("os"),
            "EOL" => {
                if cfg!(windows) {
                    Some(str_val("\r\n"))
                } else {
                    Some(str_val("\n"))
                }
            }
            "devNull" => {
                if cfg!(windows) {
                    Some(str_val("\\\\.\\nul"))
                } else {
                    Some(str_val("/dev/null"))
                }
            }
            "constants" => Some(create_cached_sub_namespace(
                "os.constants",
                &crate::object::OS_CONSTANTS_CACHE,
            )),
            _ => None,
        },
        "os.constants" => match property {
            "signals" => Some(create_cached_sub_namespace(
                "os.constants.signals",
                &crate::object::OS_CONSTANTS_SIGNALS_CACHE,
            )),
            "errno" => Some(create_cached_sub_namespace(
                "os.constants.errno",
                &crate::object::OS_CONSTANTS_ERRNO_CACHE,
            )),
            "priority" => Some(create_cached_sub_namespace(
                "os.constants.priority",
                &crate::object::OS_CONSTANTS_PRIORITY_CACHE,
            )),
            "dlopen" => Some(create_cached_sub_namespace(
                "os.constants.dlopen",
                &crate::object::OS_CONSTANTS_DLOPEN_CACHE,
            )),
            // Top-level libuv constant — sits directly on `os.constants`, not
            // inside one of the nested tables. Node's UDP socket impl uses it
            // for `SO_REUSEADDR`. Value is the published libuv flag (4).
            "UV_UDP_REUSEADDR" => Some(4.0),
            _ => None,
        },
        "os.constants.signals" => os_signal_const(property),
        "os.constants.errno" => os_errno_const(property),
        "os.constants.priority" => os_priority_const(property),
        "os.constants.dlopen" => os_dlopen_const(property),
        "util" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("util"),
            "types" => Some(create_sub_namespace("util.types")),
            "TextEncoder" => Some(crate::object::js_get_global_this_builtin_value(
                b"TextEncoder".as_ptr(),
                "TextEncoder".len(),
            )),
            "TextDecoder" => Some(crate::object::js_get_global_this_builtin_value(
                b"TextDecoder".as_ptr(),
                "TextDecoder".len(),
            )),
            _ => None,
        },
        "assert" => match property {
            "strict" => Some(create_sub_namespace("assert/strict")),
            _ => None,
        },
        "assert/strict" => match property {
            "strict" => Some(native_namespace_or_create("assert/strict", namespace_obj)),
            _ => None,
        },
        "domain" => match property {
            "_stack" | "active" => {
                let ptr = crate::value::JS_NATIVE_DOMAIN_DISPATCH.load(Ordering::SeqCst);
                if ptr.is_null() {
                    None
                } else {
                    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                        std::mem::transmute(ptr);
                    Some(dispatch(
                        property.as_ptr(),
                        property.len(),
                        std::ptr::null(),
                        0,
                    ))
                }
            }
            _ => None,
        },
        "test" => crate::node_test::property(property),
        "wasi" => match property {
            "default" => Some(native_namespace_or_create("wasi", namespace_obj)),
            _ => None,
        },
        "vm" => match property {
            "default" => Some(native_namespace_or_create("vm", namespace_obj)),
            "constants" => Some(create_sub_namespace("vm.constants")),
            "Module" | "SourceTextModule" | "SyntheticModule"
                if crate::node_vm::vm_modules_enabled() =>
            {
                Some(bound_native_callable_export_value("vm", property))
            }
            _ => None,
        },
        "vm.constants" => match property {
            "USE_MAIN_CONTEXT_DEFAULT_LOADER" => Some(crate::symbol::js_symbol_for(str_val(
                "vm_dynamic_import_main_context_default",
            ))),
            "DONT_CONTEXTIFY" => Some(crate::symbol::js_symbol_for(str_val(
                "vm_context_no_contextify",
            ))),
            _ => None,
        },
        "stream" => match property {
            "Stream" | "default" => Some(bound_native_callable_export_value("stream", "Stream")),
            "promises" => Some(unsafe {
                crate::node_submodules::js_node_submodule_namespace(
                    b"stream_promises".as_ptr(),
                    "stream_promises".len() as u32,
                )
            }),
            _ => None,
        },
        "repl" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("repl"),
            "builtinModules" => Some(crate::process::js_module_builtin_modules()),
            "REPL_MODE_SLOPPY" => Some(crate::node_repl::repl_mode_sloppy()),
            "REPL_MODE_STRICT" => Some(crate::node_repl::repl_mode_strict()),
            "Recoverable" => Some(bound_native_callable_export_value("repl", "Recoverable")),
            "REPLServer" => Some(bound_native_callable_export_value("repl", "REPLServer")),
            "start" => Some(bound_native_callable_export_value("repl", "start")),
            _ => None,
        },
        "url" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("url"),
            "URL" => Some(js_get_global_this_builtin_value(
                b"URL".as_ptr(),
                "URL".len(),
            )),
            "URLSearchParams" => Some(js_get_global_this_builtin_value(
                b"URLSearchParams".as_ptr(),
                "URLSearchParams".len(),
            )),
            "URLPattern" => Some(js_get_global_this_builtin_value(
                b"URLPattern".as_ptr(),
                "URLPattern".len(),
            )),
            _ => None,
        },
        "net" => match property {
            "Stream" => Some(bound_native_callable_export_value("net", "Socket")),
            _ => None,
        },
        "timers/promises" => match property {
            "setTimeout" | "setImmediate" | "setInterval" => Some(unsafe {
                crate::node_submodules::js_node_submodule_namespace_member(
                    b"timers_promises".as_ptr(),
                    "timers_promises".len() as u32,
                    property.as_ptr(),
                    property.len() as u32,
                )
            }),
            "scheduler" => Some(unsafe {
                crate::node_submodules::js_node_submodule_namespace_member(
                    b"timers_promises".as_ptr(),
                    "timers_promises".len() as u32,
                    b"scheduler".as_ptr(),
                    "scheduler".len() as u32,
                )
            }),
            _ => None,
        },
        "crypto" => match property {
            "constants" => Some(create_sub_namespace("crypto.constants")),
            "Certificate" => Some(create_sub_namespace("crypto.Certificate")),
            "webcrypto" => Some(webcrypto_namespace()),
            // #1366: `crypto.subtle` is the WebCrypto SubtleCrypto
            // instance. Resolve to a sub-namespace so `typeof
            // crypto.subtle === "object"` matches Node and call
            // sites that read `subtle` as a value (e.g.
            // `const s = crypto.subtle; s.digest(...)`) get an
            // object. The actual `subtle.<method>(...)` lowering
            // is handled statically by HIR (see
            // `lower/expr_call/nested_namespace.rs`).
            "subtle" => Some(subtle_crypto_namespace()),
            _ => None,
        },
        "crypto.webcrypto" => match property {
            "subtle" => Some(subtle_crypto_namespace()),
            "constructor" => Some(js_get_global_this_builtin_value(
                b"Crypto".as_ptr(),
                "Crypto".len(),
            )),
            _ => None,
        },
        "crypto.subtle" => match property {
            "constructor" => Some(js_get_global_this_builtin_value(
                b"SubtleCrypto".as_ptr(),
                "SubtleCrypto".len(),
            )),
            _ => None,
        },
        "crypto.constants" => crypto_const(property),
        "tls" => match property {
            "DEFAULT_ECDH_CURVE" => Some(str_val("auto")),
            "DEFAULT_MIN_VERSION" => Some(str_val("TLSv1.2")),
            "DEFAULT_MAX_VERSION" => Some(str_val("TLSv1.3")),
            "DEFAULT_CIPHERS" => Some(str_val(crate::tls::DEFAULT_CIPHERS)),
            "CLIENT_RENEG_LIMIT" => Some(3.0),
            "CLIENT_RENEG_WINDOW" => Some(600.0),
            "rootCertificates" => Some(crate::tls::js_tls_root_certificates()),
            _ => None,
        },
        "events" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("events"),
            "defaultMaxListeners" => Some(10.0),
            "usingDomains" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "captureRejections" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "errorMonitor" => Some(crate::symbol::js_symbol_for(str_val("events.errorMonitor"))),
            "captureRejectionSymbol" => {
                Some(crate::symbol::js_symbol_for(str_val("nodejs.rejection")))
            }
            "init" => Some(bound_native_callable_export_value("events", "init")),
            "EventEmitterAsyncResource" => Some(bound_native_callable_export_value(
                "events",
                "EventEmitterAsyncResource",
            )),
            _ => None,
        },
        // node:worker_threads value-shaped exports. `workerData` and
        // `parentPort` are dynamic for compiled Worker modules, so the
        // namespace object must agree with the named-import getter lowering.
        // Pre-fix `const { isMainThread } = require('worker_threads')` read
        // `undefined`, which made the `if (!isMainThread) common.skip(...)`
        // guard Node uses in main-thread-only tests fire under Perry, so
        // ~8 process tests in the node-core radar (#2135) were "skipping"
        // when they should have been running. (#2135)
        "worker_threads" => match property {
            "MessageChannel" | "MessagePort" | "BroadcastChannel" => {
                let global = crate::object::js_get_global_this();
                let global_obj = crate::value::js_nanbox_get_pointer(global) as *const ObjectHeader;
                if global_obj.is_null() {
                    Some(f64::from_bits(JSValue::undefined().bits()))
                } else {
                    let key = crate::string::js_string_from_bytes(
                        property.as_ptr(),
                        property.len() as u32,
                    );
                    Some(f64::from_bits(
                        js_object_get_field_by_name(global_obj, key).bits(),
                    ))
                }
            }
            "isMainThread" => Some(call_worker_threads_getter(
                &WORKER_THREADS_IS_MAIN_THREAD_GETTER,
                || f64::from_bits(JSValue::bool(true).bits()),
            )),
            "isInternalThread" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "parentPort" => Some(call_worker_threads_getter(
                &WORKER_THREADS_PARENT_PORT_GETTER,
                || f64::from_bits(crate::value::TAG_NULL),
            )),
            "workerData" => Some(call_worker_threads_getter(
                &WORKER_THREADS_WORKER_DATA_GETTER,
                || f64::from_bits(crate::value::TAG_NULL),
            )),
            "threadId" => Some(0.0),
            "threadName" => Some(call_worker_threads_getter(
                &WORKER_THREADS_THREAD_NAME_GETTER,
                || str_val(""),
            )),
            "resourceLimits" => Some(call_worker_threads_getter(
                &WORKER_THREADS_RESOURCE_LIMITS_GETTER,
                || {
                    let obj = crate::object::js_object_alloc(0, 0);
                    crate::value::js_nanbox_pointer(obj as i64)
                },
            )),
            "locks" => Some(worker_threads_locks_value()),
            "SHARE_ENV" => Some(crate::symbol::js_symbol_for(str_val(
                "nodejs.worker_threads.SHARE_ENV",
            ))),
            _ => None,
        },
        // `zlib.constants` and the top-level Z_*/DEFLATE/INFLATE shortcuts
        // Node also exposes directly on `require('node:zlib')`.
        "zlib" => match property {
            "constants" => Some(create_sub_namespace("zlib.constants")),
            "codes" => Some(zlib_codes_object()),
            _ => zlib_const(property),
        },
        "zlib.constants" => zlib_const(property),
        // Issue #912 (#909 follow-up): express reads
        // `const { METHODS } = require('node:http')` at module init and
        // immediately calls `METHODS.map(...)` — pre-fix METHODS resolved
        // to undefined and threw `TypeError: Cannot read properties of
        // undefined (reading 'map')`. Node's `http.METHODS` is a sorted
        // array of HTTP verb strings sourced from llhttp (only exposed
        // on `node:http`, not on `https`/`http2`). We materialize the
        // array once (`http_methods_array` caches the long-lived
        // pointer) and hand it back for every read.
        "http" => match property {
            "METHODS" => Some(unsafe { http_methods_array() }),
            "OutgoingMessage" => Some(bound_native_callable_export_value(
                "http",
                "OutgoingMessage",
            )),
            // #3712: Node's `http.maxHeaderSize` default is 16 KiB (16384).
            "maxHeaderSize" => Some(16384.0),
            // #3712: `http.globalAgent` is an http.Agent with protocol "http:"
            // and defaultPort 80 (distinct from https.globalAgent above).
            "globalAgent" => Some(unsafe { http_global_agent_object() }),
            // #2519: `http.STATUS_CODES` maps status codes to reason phrases.
            "STATUS_CODES" => Some(unsafe { http_status_codes_object() }),
            "WebSocket" => Some(js_get_global_this_builtin_value(
                b"WebSocket".as_ptr(),
                "WebSocket".len(),
            )),
            // #4974: `require('_http_server').kConnectionsCheckingInterval`
            // (the module aliases to "http" in cjs_wrap). Node exports a
            // Symbol used as `server[k]` to reach the connections-checking
            // interval timer; Perry represents it as a sentinel string key
            // the ext-http server handle dispatch recognizes, mirroring the
            // `@@__perry_wk_*` well-known-symbol encoding.
            "kConnectionsCheckingInterval" => {
                Some(native_string_value("@@kConnectionsCheckingInterval"))
            }
            _ => None,
        },
        "https" => match property {
            "globalAgent" => Some(unsafe { https_global_agent_object() }),
            _ => None,
        },
        // node:http2 — `constants` is a sub-namespace object (Node exposes it
        // as a single object, not loose top-level constants), so
        // `import { constants } from 'node:http2'` binds to a real object and
        // `constants.HTTP2_HEADER_PATH` resolves through `http2.constants`
        // below. The `Http2ServerRequest` / `Http2ServerResponse` /
        // `createSecureServer` exports are handled elsewhere (#1651).
        "http2" => match property {
            "constants" => Some(create_sub_namespace("http2.constants")),
            // #6468: `sensitiveHeaders` / `http2.constants` are the only http2
            // props backed by `crate::node_http2_constants`, gated behind
            // `mod-http2-constants`. This whole `"http2"` arm is only reached
            // through the http2 namespace object, which materializes on a
            // `node:http2` import — the same signal that enables the gate — so
            // the `None` fallback when the gate is off is never observed.
            #[cfg(feature = "mod-http2-constants")]
            "sensitiveHeaders" => Some(crate::node_http2_constants::sensitive_headers_symbol()),
            // `Http2ServerRequest` / `Http2ServerResponse` imported as VALUES are
            // used by libraries purely for `req instanceof Http2ServerRequest`
            // brand checks (e.g. @hono/node-server distinguishing HTTP/2 from
            // HTTP/1 requests). Resolve them to callable class values so the
            // `instanceof` RHS is a function (returns `false` for Perry's HTTP/1
            // handles) instead of `undefined` — which threw "Right-hand side of
            // 'instanceof' is not an object" and 400'd every request.
            "Http2ServerRequest" => Some(bound_native_callable_export_value(
                "http2",
                "Http2ServerRequest",
            )),
            "Http2ServerResponse" => Some(bound_native_callable_export_value(
                "http2",
                "Http2ServerResponse",
            )),
            // #3905: `import http2 from "node:http2"` — default is the module
            // namespace object.
            "default" => Some(native_namespace_or_create("http2", namespace_obj)),
            _ => None,
        },
        #[cfg(feature = "mod-http2-constants")]
        "http2.constants" => crate::node_http2_constants::constant(property),
        // node:cluster — primary-side settings and Worker handles are backed
        // by `crate::cluster`; scheduling/identity constants remain static.
        "cluster" => crate::cluster::cluster_property(property),
        // #1336: Histograms returned by perf_hooks.monitorEventLoopDelay /
        // .createHistogram expose numeric stats via property read. Perry's
        // stub doesn't record samples so every accessor reads 0; `exceeds`
        // and `count` matter for code that branches on counts before
        // computing averages.
        "perf_histogram" => match property {
            "mean" | "min" | "max" | "stddev" | "exceeds" | "count" => Some(0.0),
            "percentiles" | "percentilesBigInt" => {
                let obj = js_object_alloc(0, 0);
                Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Sorted table for `zlib_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static ZLIB_CONST_TABLE: &[(&str, f64)] = &[
    ("BROTLI_DECODE", (8) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES", (-30) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP", (-25) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES", (-21) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1", (-26) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2", (-27) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS", (-22) as f64),
    ("BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET", (-19) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1", (-9) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2", (-10) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_CL_SPACE", (-6) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT",
        (-8) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_DICTIONARY", (-12) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_DISTANCE", (-16) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE",
        (-3) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE", (-1) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE", (-7) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_1", (-14) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_2", (-15) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_RESERVED", (-2) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET",
        (-4) as f64,
    ),
    (
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME",
        (-5) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_TRANSFORM", (-11) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS", (-13) as f64),
    ("BROTLI_DECODER_ERROR_INVALID_ARGUMENTS", (-20) as f64),
    ("BROTLI_DECODER_ERROR_UNREACHABLE", (-31) as f64),
    ("BROTLI_DECODER_NEEDS_MORE_INPUT", (2) as f64),
    ("BROTLI_DECODER_NEEDS_MORE_OUTPUT", (3) as f64),
    ("BROTLI_DECODER_NO_ERROR", (0) as f64),
    (
        "BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION",
        (0) as f64,
    ),
    ("BROTLI_DECODER_PARAM_LARGE_WINDOW", (1) as f64),
    ("BROTLI_DECODER_RESULT_ERROR", (0) as f64),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT", (2) as f64),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT", (3) as f64),
    ("BROTLI_DECODER_RESULT_SUCCESS", (1) as f64),
    ("BROTLI_DECODER_SUCCESS", (1) as f64),
    ("BROTLI_DEFAULT_MODE", (0) as f64),
    ("BROTLI_DEFAULT_QUALITY", (11) as f64),
    ("BROTLI_DEFAULT_WINDOW", (22) as f64),
    ("BROTLI_ENCODE", (9) as f64),
    ("BROTLI_LARGE_MAX_WINDOW_BITS", (30) as f64),
    ("BROTLI_MAX_INPUT_BLOCK_BITS", (24) as f64),
    ("BROTLI_MAX_QUALITY", (11) as f64),
    ("BROTLI_MAX_WINDOW_BITS", (24) as f64),
    ("BROTLI_MIN_INPUT_BLOCK_BITS", (16) as f64),
    ("BROTLI_MIN_QUALITY", (0) as f64),
    ("BROTLI_MIN_WINDOW_BITS", (10) as f64),
    ("BROTLI_MODE_FONT", (2) as f64),
    ("BROTLI_MODE_GENERIC", (0) as f64),
    ("BROTLI_MODE_TEXT", (1) as f64),
    ("BROTLI_OPERATION_EMIT_METADATA", (3) as f64),
    ("BROTLI_OPERATION_FINISH", (2) as f64),
    ("BROTLI_OPERATION_FLUSH", (1) as f64),
    ("BROTLI_OPERATION_PROCESS", (0) as f64),
    ("BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING", (4) as f64),
    ("BROTLI_PARAM_LARGE_WINDOW", (6) as f64),
    ("BROTLI_PARAM_LGBLOCK", (3) as f64),
    ("BROTLI_PARAM_LGWIN", (2) as f64),
    ("BROTLI_PARAM_MODE", (0) as f64),
    ("BROTLI_PARAM_NDIRECT", (8) as f64),
    ("BROTLI_PARAM_NPOSTFIX", (7) as f64),
    ("BROTLI_PARAM_QUALITY", (1) as f64),
    ("BROTLI_PARAM_SIZE_HINT", (5) as f64),
    ("DEFLATE", (1) as f64),
    ("DEFLATERAW", (5) as f64),
    ("GUNZIP", (4) as f64),
    ("GZIP", (3) as f64),
    ("INFLATE", (2) as f64),
    ("INFLATERAW", (6) as f64),
    ("UNZIP", (7) as f64),
    ("ZLIB_VERNUM", (0x1310) as f64),
    ("ZSTD_CLEVEL_DEFAULT", (3) as f64),
    ("ZSTD_COMPRESS", (10) as f64),
    ("ZSTD_DECOMPRESS", (11) as f64),
    ("ZSTD_MAXCLEVEL", (22) as f64),
    ("ZSTD_MINCLEVEL", (-131072) as f64),
    ("ZSTD_btlazy2", (6) as f64),
    ("ZSTD_btopt", (7) as f64),
    ("ZSTD_btultra", (8) as f64),
    ("ZSTD_btultra2", (9) as f64),
    ("ZSTD_c_chainLog", (103) as f64),
    ("ZSTD_c_checksumFlag", (201) as f64),
    ("ZSTD_c_compressionLevel", (100) as f64),
    ("ZSTD_c_contentSizeFlag", (200) as f64),
    ("ZSTD_c_dictIDFlag", (202) as f64),
    ("ZSTD_c_enableLongDistanceMatching", (160) as f64),
    ("ZSTD_c_hashLog", (102) as f64),
    ("ZSTD_c_jobSize", (401) as f64),
    ("ZSTD_c_ldmBucketSizeLog", (163) as f64),
    ("ZSTD_c_ldmHashLog", (161) as f64),
    ("ZSTD_c_ldmHashRateLog", (164) as f64),
    ("ZSTD_c_ldmMinMatch", (162) as f64),
    ("ZSTD_c_minMatch", (105) as f64),
    ("ZSTD_c_nbWorkers", (400) as f64),
    ("ZSTD_c_overlapLog", (402) as f64),
    ("ZSTD_c_searchLog", (104) as f64),
    ("ZSTD_c_strategy", (107) as f64),
    ("ZSTD_c_targetLength", (106) as f64),
    ("ZSTD_c_windowLog", (101) as f64),
    ("ZSTD_d_windowLogMax", (100) as f64),
    ("ZSTD_dfast", (2) as f64),
    ("ZSTD_e_continue", (0) as f64),
    ("ZSTD_e_end", (2) as f64),
    ("ZSTD_e_flush", (1) as f64),
    ("ZSTD_error_GENERIC", (1) as f64),
    ("ZSTD_error_checksum_wrong", (22) as f64),
    ("ZSTD_error_corruption_detected", (20) as f64),
    ("ZSTD_error_dictionaryCreation_failed", (34) as f64),
    ("ZSTD_error_dictionary_corrupted", (30) as f64),
    ("ZSTD_error_dictionary_wrong", (32) as f64),
    ("ZSTD_error_dstBuffer_null", (74) as f64),
    ("ZSTD_error_dstSize_tooSmall", (70) as f64),
    ("ZSTD_error_frameParameter_unsupported", (14) as f64),
    ("ZSTD_error_frameParameter_windowTooLarge", (16) as f64),
    ("ZSTD_error_init_missing", (62) as f64),
    ("ZSTD_error_literals_headerWrong", (24) as f64),
    ("ZSTD_error_maxSymbolValue_tooLarge", (46) as f64),
    ("ZSTD_error_maxSymbolValue_tooSmall", (48) as f64),
    ("ZSTD_error_memory_allocation", (64) as f64),
    ("ZSTD_error_noForwardProgress_destFull", (80) as f64),
    ("ZSTD_error_noForwardProgress_inputEmpty", (82) as f64),
    ("ZSTD_error_no_error", (0) as f64),
    ("ZSTD_error_parameter_combination_unsupported", (41) as f64),
    ("ZSTD_error_parameter_outOfBound", (42) as f64),
    ("ZSTD_error_parameter_unsupported", (40) as f64),
    ("ZSTD_error_prefix_unknown", (10) as f64),
    ("ZSTD_error_srcSize_wrong", (72) as f64),
    ("ZSTD_error_stabilityCondition_notRespected", (50) as f64),
    ("ZSTD_error_stage_wrong", (60) as f64),
    ("ZSTD_error_tableLog_tooLarge", (44) as f64),
    ("ZSTD_error_version_unsupported", (12) as f64),
    ("ZSTD_error_workSpace_tooSmall", (66) as f64),
    ("ZSTD_fast", (1) as f64),
    ("ZSTD_greedy", (3) as f64),
    ("ZSTD_lazy", (4) as f64),
    ("ZSTD_lazy2", (5) as f64),
    ("Z_BEST_COMPRESSION", (9) as f64),
    ("Z_BEST_SPEED", (1) as f64),
    ("Z_BLOCK", (5) as f64),
    ("Z_BUF_ERROR", (-5) as f64),
    ("Z_DATA_ERROR", (-3) as f64),
    ("Z_DEFAULT_CHUNK", (16384) as f64),
    ("Z_DEFAULT_COMPRESSION", (-1) as f64),
    ("Z_DEFAULT_LEVEL", (-1) as f64),
    ("Z_DEFAULT_MEMLEVEL", (8) as f64),
    ("Z_DEFAULT_STRATEGY", (0) as f64),
    ("Z_DEFAULT_WINDOWBITS", (15) as f64),
    ("Z_ERRNO", (-1) as f64),
    ("Z_FILTERED", (1) as f64),
    ("Z_FINISH", (4) as f64),
    ("Z_FIXED", (4) as f64),
    ("Z_FULL_FLUSH", (3) as f64),
    ("Z_HUFFMAN_ONLY", (2) as f64),
    ("Z_MAX_CHUNK", (0x7fff_ffff) as f64),
    ("Z_MAX_LEVEL", (9) as f64),
    ("Z_MAX_MEMLEVEL", (9) as f64),
    ("Z_MAX_WINDOWBITS", (15) as f64),
    ("Z_MEM_ERROR", (-4) as f64),
    ("Z_MIN_CHUNK", (64) as f64),
    ("Z_MIN_LEVEL", (-1) as f64),
    ("Z_MIN_MEMLEVEL", (1) as f64),
    ("Z_MIN_WINDOWBITS", (8) as f64),
    ("Z_NEED_DICT", (2) as f64),
    ("Z_NO_COMPRESSION", (0) as f64),
    ("Z_NO_FLUSH", (0) as f64),
    ("Z_OK", (0) as f64),
    ("Z_PARTIAL_FLUSH", (1) as f64),
    ("Z_RLE", (3) as f64),
    ("Z_STREAM_END", (1) as f64),
    ("Z_STREAM_ERROR", (-2) as f64),
    ("Z_SYNC_FLUSH", (2) as f64),
    ("Z_VERSION_ERROR", (-6) as f64),
];

fn zlib_const_lookup(prop: &str) -> Option<f64> {
    let i = ZLIB_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(ZLIB_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn zlib_const_reference(prop: &str) -> Option<f64> {
    let v: i64 = match prop {
        // Compression levels
        "Z_NO_COMPRESSION" => 0,
        "Z_BEST_SPEED" => 1,
        "Z_BEST_COMPRESSION" => 9,
        "Z_DEFAULT_COMPRESSION" => -1,
        // Compression strategies
        "Z_FILTERED" => 1,
        "Z_HUFFMAN_ONLY" => 2,
        "Z_RLE" => 3,
        "Z_FIXED" => 4,
        "Z_DEFAULT_STRATEGY" => 0,
        "ZLIB_VERNUM" => 0x1310,
        // Flush values
        "Z_NO_FLUSH" => 0,
        "Z_PARTIAL_FLUSH" => 1,
        "Z_SYNC_FLUSH" => 2,
        "Z_FULL_FLUSH" => 3,
        "Z_FINISH" => 4,
        "Z_BLOCK" => 5,
        // Return codes
        "Z_OK" => 0,
        "Z_STREAM_END" => 1,
        "Z_NEED_DICT" => 2,
        "Z_ERRNO" => -1,
        "Z_STREAM_ERROR" => -2,
        "Z_DATA_ERROR" => -3,
        "Z_MEM_ERROR" => -4,
        "Z_BUF_ERROR" => -5,
        "Z_VERSION_ERROR" => -6,
        // Min/Max window bits and memlevel
        "Z_MIN_WINDOWBITS" => 8,
        "Z_MAX_WINDOWBITS" => 15,
        "Z_DEFAULT_WINDOWBITS" => 15,
        "Z_MIN_CHUNK" => 64,
        "Z_MAX_CHUNK" => 0x7fff_ffff,
        "Z_DEFAULT_CHUNK" => 16384,
        "Z_MIN_MEMLEVEL" => 1,
        "Z_MAX_MEMLEVEL" => 9,
        "Z_DEFAULT_MEMLEVEL" => 8,
        "Z_MIN_LEVEL" => -1,
        "Z_MAX_LEVEL" => 9,
        "Z_DEFAULT_LEVEL" => -1,
        // Mode (zlib stream modes — used by zlib.createDeflate etc.)
        "DEFLATE" => 1,
        "INFLATE" => 2,
        "GZIP" => 3,
        "GUNZIP" => 4,
        "DEFLATERAW" => 5,
        "INFLATERAW" => 6,
        "UNZIP" => 7,
        "BROTLI_DECODE" => 8,
        "BROTLI_ENCODE" => 9,
        "ZSTD_COMPRESS" => 10,
        "ZSTD_DECOMPRESS" => 11,
        // Brotli operation/parameter constants — match Node's
        // `zlib.constants` exactly (these are the BrotliEncoder/
        // BrotliDecoder parameter ids the underlying brotli library
        // exposes).
        "BROTLI_OPERATION_PROCESS" => 0,
        "BROTLI_OPERATION_FLUSH" => 1,
        "BROTLI_OPERATION_FINISH" => 2,
        "BROTLI_OPERATION_EMIT_METADATA" => 3,
        "BROTLI_PARAM_MODE" => 0,
        "BROTLI_MODE_GENERIC" => 0,
        "BROTLI_MODE_TEXT" => 1,
        "BROTLI_MODE_FONT" => 2,
        "BROTLI_DEFAULT_MODE" => 0,
        "BROTLI_PARAM_QUALITY" => 1,
        "BROTLI_MIN_QUALITY" => 0,
        "BROTLI_MAX_QUALITY" => 11,
        "BROTLI_DEFAULT_QUALITY" => 11,
        "BROTLI_PARAM_LGWIN" => 2,
        "BROTLI_MIN_WINDOW_BITS" => 10,
        "BROTLI_MAX_WINDOW_BITS" => 24,
        "BROTLI_LARGE_MAX_WINDOW_BITS" => 30,
        "BROTLI_DEFAULT_WINDOW" => 22,
        "BROTLI_PARAM_LGBLOCK" => 3,
        "BROTLI_MIN_INPUT_BLOCK_BITS" => 16,
        "BROTLI_MAX_INPUT_BLOCK_BITS" => 24,
        "BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING" => 4,
        "BROTLI_PARAM_SIZE_HINT" => 5,
        "BROTLI_PARAM_LARGE_WINDOW" => 6,
        "BROTLI_PARAM_NPOSTFIX" => 7,
        "BROTLI_PARAM_NDIRECT" => 8,
        "BROTLI_DECODER_RESULT_ERROR" => 0,
        "BROTLI_DECODER_RESULT_SUCCESS" => 1,
        "BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT" => 2,
        "BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT" => 3,
        "BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION" => 0,
        "BROTLI_DECODER_PARAM_LARGE_WINDOW" => 1,
        // Zstd parameter ids — match Node's `zlib.constants`.
        "ZSTD_e_continue" => 0,
        "ZSTD_e_flush" => 1,
        "ZSTD_e_end" => 2,
        "ZSTD_fast" => 1,
        "ZSTD_dfast" => 2,
        "ZSTD_greedy" => 3,
        "ZSTD_lazy" => 4,
        "ZSTD_lazy2" => 5,
        "ZSTD_btlazy2" => 6,
        "ZSTD_btopt" => 7,
        "ZSTD_btultra" => 8,
        "ZSTD_btultra2" => 9,
        "ZSTD_c_compressionLevel" => 100,
        "ZSTD_c_windowLog" => 101,
        "ZSTD_c_hashLog" => 102,
        "ZSTD_c_chainLog" => 103,
        "ZSTD_c_searchLog" => 104,
        "ZSTD_c_minMatch" => 105,
        "ZSTD_c_targetLength" => 106,
        "ZSTD_c_strategy" => 107,
        "ZSTD_c_enableLongDistanceMatching" => 160,
        "ZSTD_c_ldmHashLog" => 161,
        "ZSTD_c_ldmMinMatch" => 162,
        "ZSTD_c_ldmBucketSizeLog" => 163,
        "ZSTD_c_ldmHashRateLog" => 164,
        "ZSTD_c_contentSizeFlag" => 200,
        "ZSTD_c_checksumFlag" => 201,
        "ZSTD_c_dictIDFlag" => 202,
        "ZSTD_c_nbWorkers" => 400,
        "ZSTD_c_jobSize" => 401,
        "ZSTD_c_overlapLog" => 402,
        "ZSTD_d_windowLogMax" => 100,
        "ZSTD_CLEVEL_DEFAULT" => 3,
        "ZSTD_MINCLEVEL" => -131072,
        "ZSTD_MAXCLEVEL" => 22,
        // #3677: Brotli decoder result/error codes Node exposes on
        // `zlib.constants` (the BrotliDecoderResult / BrotliDecoderErrorCode
        // enums). Required so `Object.keys(zlib.constants)` enumeration
        // matches Node's full set and every enumerated key reads its value.
        "BROTLI_DECODER_NO_ERROR" => 0,
        "BROTLI_DECODER_SUCCESS" => 1,
        "BROTLI_DECODER_NEEDS_MORE_INPUT" => 2,
        "BROTLI_DECODER_NEEDS_MORE_OUTPUT" => 3,
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE" => -1,
        "BROTLI_DECODER_ERROR_FORMAT_RESERVED" => -2,
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE" => -3,
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET" => -4,
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME" => -5,
        "BROTLI_DECODER_ERROR_FORMAT_CL_SPACE" => -6,
        "BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE" => -7,
        "BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT" => -8,
        "BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1" => -9,
        "BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2" => -10,
        "BROTLI_DECODER_ERROR_FORMAT_TRANSFORM" => -11,
        "BROTLI_DECODER_ERROR_FORMAT_DICTIONARY" => -12,
        "BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS" => -13,
        "BROTLI_DECODER_ERROR_FORMAT_PADDING_1" => -14,
        "BROTLI_DECODER_ERROR_FORMAT_PADDING_2" => -15,
        "BROTLI_DECODER_ERROR_FORMAT_DISTANCE" => -16,
        "BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET" => -19,
        "BROTLI_DECODER_ERROR_INVALID_ARGUMENTS" => -20,
        "BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES" => -21,
        "BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS" => -22,
        "BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP" => -25,
        "BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1" => -26,
        "BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2" => -27,
        "BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES" => -30,
        "BROTLI_DECODER_ERROR_UNREACHABLE" => -31,
        // #3677: Zstd error codes (ZSTD_ErrorCode enum) Node exposes.
        "ZSTD_error_no_error" => 0,
        "ZSTD_error_GENERIC" => 1,
        "ZSTD_error_prefix_unknown" => 10,
        "ZSTD_error_version_unsupported" => 12,
        "ZSTD_error_frameParameter_unsupported" => 14,
        "ZSTD_error_frameParameter_windowTooLarge" => 16,
        "ZSTD_error_corruption_detected" => 20,
        "ZSTD_error_checksum_wrong" => 22,
        "ZSTD_error_literals_headerWrong" => 24,
        "ZSTD_error_dictionary_corrupted" => 30,
        "ZSTD_error_dictionary_wrong" => 32,
        "ZSTD_error_dictionaryCreation_failed" => 34,
        "ZSTD_error_parameter_unsupported" => 40,
        "ZSTD_error_parameter_combination_unsupported" => 41,
        "ZSTD_error_parameter_outOfBound" => 42,
        "ZSTD_error_tableLog_tooLarge" => 44,
        "ZSTD_error_maxSymbolValue_tooLarge" => 46,
        "ZSTD_error_maxSymbolValue_tooSmall" => 48,
        "ZSTD_error_stabilityCondition_notRespected" => 50,
        "ZSTD_error_stage_wrong" => 60,
        "ZSTD_error_init_missing" => 62,
        "ZSTD_error_memory_allocation" => 64,
        "ZSTD_error_workSpace_tooSmall" => 66,
        "ZSTD_error_dstSize_tooSmall" => 70,
        "ZSTD_error_srcSize_wrong" => 72,
        "ZSTD_error_dstBuffer_null" => 74,
        "ZSTD_error_noForwardProgress_destFull" => 80,
        "ZSTD_error_noForwardProgress_inputEmpty" => 82,
        _ => return None,
    };
    Some(v as f64)
}

#[cfg(test)]
mod zlib_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in ZLIB_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    zlib_const_lookup(lit),
                    zlib_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `crypto_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static CRYPTO_CONST_TABLE: &[(&str, f64)] = &[
    ("DH_CHECK_P_NOT_PRIME", (1.0) as f64),
    ("DH_CHECK_P_NOT_SAFE_PRIME", (2.0) as f64),
    ("DH_NOT_SUITABLE_GENERATOR", (8.0) as f64),
    ("DH_UNABLE_TO_CHECK_GENERATOR", (4.0) as f64),
    ("ENGINE_METHOD_ALL", (65535.0) as f64),
    ("ENGINE_METHOD_CIPHERS", (64.0) as f64),
    ("ENGINE_METHOD_DH", (4.0) as f64),
    ("ENGINE_METHOD_DIGESTS", (128.0) as f64),
    ("ENGINE_METHOD_DSA", (2.0) as f64),
    ("ENGINE_METHOD_EC", (2048.0) as f64),
    ("ENGINE_METHOD_NONE", (0.0) as f64),
    ("ENGINE_METHOD_PKEY_ASN1_METHS", (1024.0) as f64),
    ("ENGINE_METHOD_PKEY_METHS", (512.0) as f64),
    ("ENGINE_METHOD_RAND", (8.0) as f64),
    ("ENGINE_METHOD_RSA", (1.0) as f64),
    ("OPENSSL_VERSION_NUMBER", (811597840.0) as f64),
    ("POINT_CONVERSION_COMPRESSED", (2.0) as f64),
    ("POINT_CONVERSION_HYBRID", (6.0) as f64),
    ("POINT_CONVERSION_UNCOMPRESSED", (4.0) as f64),
    ("RSA_NO_PADDING", (3.0) as f64),
    ("RSA_PKCS1_OAEP_PADDING", (4.0) as f64),
    ("RSA_PKCS1_PADDING", (1.0) as f64),
    ("RSA_PKCS1_PSS_PADDING", (6.0) as f64),
    ("RSA_PSS_SALTLEN_AUTO", (-2.0) as f64),
    ("RSA_PSS_SALTLEN_DIGEST", (-1.0) as f64),
    ("RSA_PSS_SALTLEN_MAX_SIGN", (-2.0) as f64),
    ("RSA_X931_PADDING", (5.0) as f64),
    ("SSL_OP_ALL", (2147485776.0) as f64),
    ("SSL_OP_ALLOW_NO_DHE_KEX", (1024.0) as f64),
    (
        "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION",
        (262144.0) as f64,
    ),
    ("SSL_OP_CIPHER_SERVER_PREFERENCE", (4194304.0) as f64),
    ("SSL_OP_CISCO_ANYCONNECT", (32768.0) as f64),
    ("SSL_OP_COOKIE_EXCHANGE", (8192.0) as f64),
    ("SSL_OP_CRYPTOPRO_TLSEXT_BUG", (2147483648.0) as f64),
    ("SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS", (2048.0) as f64),
    ("SSL_OP_LEGACY_SERVER_CONNECT", (4.0) as f64),
    ("SSL_OP_NO_COMPRESSION", (131072.0) as f64),
    ("SSL_OP_NO_ENCRYPT_THEN_MAC", (524288.0) as f64),
    ("SSL_OP_NO_QUERY_MTU", (4096.0) as f64),
    ("SSL_OP_NO_RENEGOTIATION", (1073741824.0) as f64),
    (
        "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION",
        (65536.0) as f64,
    ),
    ("SSL_OP_NO_SSLv2", (0.0) as f64),
    ("SSL_OP_NO_SSLv3", (33554432.0) as f64),
    ("SSL_OP_NO_TICKET", (16384.0) as f64),
    ("SSL_OP_NO_TLSv1", (67108864.0) as f64),
    ("SSL_OP_NO_TLSv1_1", (268435456.0) as f64),
    ("SSL_OP_NO_TLSv1_2", (134217728.0) as f64),
    ("SSL_OP_NO_TLSv1_3", (536870912.0) as f64),
    ("SSL_OP_PRIORITIZE_CHACHA", (2097152.0) as f64),
    ("SSL_OP_TLS_ROLLBACK_BUG", (8388608.0) as f64),
    ("TLS1_1_VERSION", (770.0) as f64),
    ("TLS1_2_VERSION", (771.0) as f64),
    ("TLS1_3_VERSION", (772.0) as f64),
    ("TLS1_VERSION", (769.0) as f64),
];

fn crypto_const_lookup(prop: &str) -> Option<f64> {
    let i = CRYPTO_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(CRYPTO_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn crypto_const_reference(prop: &str) -> Option<f64> {
    match prop {
        "OPENSSL_VERSION_NUMBER" => Some(811597840.0),
        "SSL_OP_ALL" => Some(2147485776.0),
        "SSL_OP_ALLOW_NO_DHE_KEX" => Some(1024.0),
        "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION" => Some(262144.0),
        "SSL_OP_CIPHER_SERVER_PREFERENCE" => Some(4194304.0),
        "SSL_OP_CISCO_ANYCONNECT" => Some(32768.0),
        "SSL_OP_COOKIE_EXCHANGE" => Some(8192.0),
        "SSL_OP_CRYPTOPRO_TLSEXT_BUG" => Some(2147483648.0),
        "SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS" => Some(2048.0),
        "SSL_OP_LEGACY_SERVER_CONNECT" => Some(4.0),
        "SSL_OP_NO_COMPRESSION" => Some(131072.0),
        "SSL_OP_NO_ENCRYPT_THEN_MAC" => Some(524288.0),
        "SSL_OP_NO_QUERY_MTU" => Some(4096.0),
        "SSL_OP_NO_RENEGOTIATION" => Some(1073741824.0),
        "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION" => Some(65536.0),
        "SSL_OP_NO_SSLv2" => Some(0.0),
        "SSL_OP_NO_SSLv3" => Some(33554432.0),
        "SSL_OP_NO_TICKET" => Some(16384.0),
        "SSL_OP_NO_TLSv1" => Some(67108864.0),
        "SSL_OP_NO_TLSv1_1" => Some(268435456.0),
        "SSL_OP_NO_TLSv1_2" => Some(134217728.0),
        "SSL_OP_NO_TLSv1_3" => Some(536870912.0),
        "SSL_OP_PRIORITIZE_CHACHA" => Some(2097152.0),
        "SSL_OP_TLS_ROLLBACK_BUG" => Some(8388608.0),
        "ENGINE_METHOD_RSA" => Some(1.0),
        "ENGINE_METHOD_DSA" => Some(2.0),
        "ENGINE_METHOD_DH" => Some(4.0),
        "ENGINE_METHOD_RAND" => Some(8.0),
        "ENGINE_METHOD_EC" => Some(2048.0),
        "ENGINE_METHOD_CIPHERS" => Some(64.0),
        "ENGINE_METHOD_DIGESTS" => Some(128.0),
        "ENGINE_METHOD_PKEY_METHS" => Some(512.0),
        "ENGINE_METHOD_PKEY_ASN1_METHS" => Some(1024.0),
        "ENGINE_METHOD_ALL" => Some(65535.0),
        "ENGINE_METHOD_NONE" => Some(0.0),
        "DH_CHECK_P_NOT_SAFE_PRIME" => Some(2.0),
        "DH_CHECK_P_NOT_PRIME" => Some(1.0),
        "DH_UNABLE_TO_CHECK_GENERATOR" => Some(4.0),
        "DH_NOT_SUITABLE_GENERATOR" => Some(8.0),
        "RSA_PKCS1_PADDING" => Some(1.0),
        "RSA_NO_PADDING" => Some(3.0),
        "RSA_PKCS1_OAEP_PADDING" => Some(4.0),
        "RSA_X931_PADDING" => Some(5.0),
        "RSA_PKCS1_PSS_PADDING" => Some(6.0),
        "RSA_PSS_SALTLEN_DIGEST" => Some(-1.0),
        "RSA_PSS_SALTLEN_MAX_SIGN" => Some(-2.0),
        "RSA_PSS_SALTLEN_AUTO" => Some(-2.0),
        "TLS1_VERSION" => Some(769.0),
        "TLS1_1_VERSION" => Some(770.0),
        "TLS1_2_VERSION" => Some(771.0),
        "TLS1_3_VERSION" => Some(772.0),
        "POINT_CONVERSION_COMPRESSED" => Some(2.0),
        "POINT_CONVERSION_UNCOMPRESSED" => Some(4.0),
        "POINT_CONVERSION_HYBRID" => Some(6.0),
        _ => None,
    }
}

#[cfg(test)]
mod crypto_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in CRYPTO_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    crypto_const_lookup(lit),
                    crypto_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `os_errno_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
#[cfg(unix)]
static OS_ERRNO_CONST_TABLE: &[(&str, f64)] = &[
    ("E2BIG", (libc::E2BIG) as f64),
    ("EACCES", (libc::EACCES) as f64),
    ("EADDRINUSE", (libc::EADDRINUSE) as f64),
    ("EADDRNOTAVAIL", (libc::EADDRNOTAVAIL) as f64),
    ("EAFNOSUPPORT", (libc::EAFNOSUPPORT) as f64),
    ("EAGAIN", (libc::EAGAIN) as f64),
    ("EALREADY", (libc::EALREADY) as f64),
    ("EBADF", (libc::EBADF) as f64),
    ("EBADMSG", (libc::EBADMSG) as f64),
    ("EBUSY", (libc::EBUSY) as f64),
    ("ECANCELED", (libc::ECANCELED) as f64),
    ("ECHILD", (libc::ECHILD) as f64),
    ("ECONNABORTED", (libc::ECONNABORTED) as f64),
    ("ECONNREFUSED", (libc::ECONNREFUSED) as f64),
    ("ECONNRESET", (libc::ECONNRESET) as f64),
    ("EDEADLK", (libc::EDEADLK) as f64),
    ("EDESTADDRREQ", (libc::EDESTADDRREQ) as f64),
    ("EDOM", (libc::EDOM) as f64),
    ("EDQUOT", (libc::EDQUOT) as f64),
    ("EEXIST", (libc::EEXIST) as f64),
    ("EFAULT", (libc::EFAULT) as f64),
    ("EFBIG", (libc::EFBIG) as f64),
    ("EHOSTUNREACH", (libc::EHOSTUNREACH) as f64),
    ("EIDRM", (libc::EIDRM) as f64),
    ("EILSEQ", (libc::EILSEQ) as f64),
    ("EINPROGRESS", (libc::EINPROGRESS) as f64),
    ("EINTR", (libc::EINTR) as f64),
    ("EINVAL", (libc::EINVAL) as f64),
    ("EIO", (libc::EIO) as f64),
    ("EISCONN", (libc::EISCONN) as f64),
    ("EISDIR", (libc::EISDIR) as f64),
    ("ELOOP", (libc::ELOOP) as f64),
    ("EMFILE", (libc::EMFILE) as f64),
    ("EMLINK", (libc::EMLINK) as f64),
    ("EMSGSIZE", (libc::EMSGSIZE) as f64),
    ("EMULTIHOP", (libc::EMULTIHOP) as f64),
    ("ENAMETOOLONG", (libc::ENAMETOOLONG) as f64),
    ("ENETDOWN", (libc::ENETDOWN) as f64),
    ("ENETRESET", (libc::ENETRESET) as f64),
    ("ENETUNREACH", (libc::ENETUNREACH) as f64),
    ("ENFILE", (libc::ENFILE) as f64),
    ("ENOBUFS", (libc::ENOBUFS) as f64),
    ("ENODATA", (libc::ENODATA) as f64),
    ("ENODEV", (libc::ENODEV) as f64),
    ("ENOENT", (libc::ENOENT) as f64),
    ("ENOEXEC", (libc::ENOEXEC) as f64),
    ("ENOLCK", (libc::ENOLCK) as f64),
    ("ENOLINK", (libc::ENOLINK) as f64),
    ("ENOMEM", (libc::ENOMEM) as f64),
    ("ENOMSG", (libc::ENOMSG) as f64),
    ("ENOPROTOOPT", (libc::ENOPROTOOPT) as f64),
    ("ENOSPC", (libc::ENOSPC) as f64),
    ("ENOSR", (libc::ENOSR) as f64),
    ("ENOSTR", (libc::ENOSTR) as f64),
    ("ENOSYS", (libc::ENOSYS) as f64),
    ("ENOTCONN", (libc::ENOTCONN) as f64),
    ("ENOTDIR", (libc::ENOTDIR) as f64),
    ("ENOTEMPTY", (libc::ENOTEMPTY) as f64),
    ("ENOTSOCK", (libc::ENOTSOCK) as f64),
    ("ENOTSUP", (libc::ENOTSUP) as f64),
    ("ENOTTY", (libc::ENOTTY) as f64),
    ("ENXIO", (libc::ENXIO) as f64),
    ("EOPNOTSUPP", (libc::EOPNOTSUPP) as f64),
    ("EOVERFLOW", (libc::EOVERFLOW) as f64),
    ("EPERM", (libc::EPERM) as f64),
    ("EPIPE", (libc::EPIPE) as f64),
    ("EPROTO", (libc::EPROTO) as f64),
    ("EPROTONOSUPPORT", (libc::EPROTONOSUPPORT) as f64),
    ("EPROTOTYPE", (libc::EPROTOTYPE) as f64),
    ("ERANGE", (libc::ERANGE) as f64),
    ("EROFS", (libc::EROFS) as f64),
    ("ESPIPE", (libc::ESPIPE) as f64),
    ("ESRCH", (libc::ESRCH) as f64),
    ("ESTALE", (libc::ESTALE) as f64),
    ("ETIME", (libc::ETIME) as f64),
    ("ETIMEDOUT", (libc::ETIMEDOUT) as f64),
    ("ETXTBSY", (libc::ETXTBSY) as f64),
    ("EWOULDBLOCK", (libc::EWOULDBLOCK) as f64),
    ("EXDEV", (libc::EXDEV) as f64),
];
#[cfg(not(unix))]
static OS_ERRNO_CONST_TABLE: &[(&str, f64)] = &[
    ("EACCES", (13.0) as f64),
    ("EAGAIN", (11.0) as f64),
    ("EBADF", (9.0) as f64),
    ("EBUSY", (16.0) as f64),
    ("EEXIST", (17.0) as f64),
    ("EFAULT", (14.0) as f64),
    ("EINTR", (4.0) as f64),
    ("EINVAL", (22.0) as f64),
    ("EIO", (5.0) as f64),
    ("EISDIR", (21.0) as f64),
    ("EMFILE", (24.0) as f64),
    ("ENFILE", (23.0) as f64),
    ("ENODEV", (19.0) as f64),
    ("ENOENT", (2.0) as f64),
    ("ENOMEM", (12.0) as f64),
    ("ENOSPC", (28.0) as f64),
    ("ENOTDIR", (20.0) as f64),
    ("ENOTEMPTY", (41.0) as f64),
    ("EPERM", (1.0) as f64),
    ("EPIPE", (32.0) as f64),
    ("ERANGE", (34.0) as f64),
    ("EROFS", (30.0) as f64),
];

fn os_errno_const_lookup(prop: &str) -> Option<f64> {
    let i = OS_ERRNO_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(OS_ERRNO_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn os_errno_const_reference(prop: &str) -> Option<f64> {
    #[cfg(unix)]
    {
        let v: Option<i32> = match prop {
            "E2BIG" => Some(libc::E2BIG),
            "EACCES" => Some(libc::EACCES),
            "EADDRINUSE" => Some(libc::EADDRINUSE),
            "EADDRNOTAVAIL" => Some(libc::EADDRNOTAVAIL),
            "EAFNOSUPPORT" => Some(libc::EAFNOSUPPORT),
            "EAGAIN" => Some(libc::EAGAIN),
            "EALREADY" => Some(libc::EALREADY),
            "EBADF" => Some(libc::EBADF),
            "EBADMSG" => Some(libc::EBADMSG),
            "EBUSY" => Some(libc::EBUSY),
            "ECANCELED" => Some(libc::ECANCELED),
            "ECHILD" => Some(libc::ECHILD),
            "ECONNABORTED" => Some(libc::ECONNABORTED),
            "ECONNREFUSED" => Some(libc::ECONNREFUSED),
            "ECONNRESET" => Some(libc::ECONNRESET),
            "EDEADLK" => Some(libc::EDEADLK),
            "EDESTADDRREQ" => Some(libc::EDESTADDRREQ),
            "EDOM" => Some(libc::EDOM),
            "EDQUOT" => Some(libc::EDQUOT),
            "EEXIST" => Some(libc::EEXIST),
            "EFAULT" => Some(libc::EFAULT),
            "EFBIG" => Some(libc::EFBIG),
            "EHOSTUNREACH" => Some(libc::EHOSTUNREACH),
            "EIDRM" => Some(libc::EIDRM),
            "EILSEQ" => Some(libc::EILSEQ),
            "EINPROGRESS" => Some(libc::EINPROGRESS),
            "EINTR" => Some(libc::EINTR),
            "EINVAL" => Some(libc::EINVAL),
            "EIO" => Some(libc::EIO),
            "EISCONN" => Some(libc::EISCONN),
            "EISDIR" => Some(libc::EISDIR),
            "ELOOP" => Some(libc::ELOOP),
            "EMFILE" => Some(libc::EMFILE),
            "EMLINK" => Some(libc::EMLINK),
            "EMSGSIZE" => Some(libc::EMSGSIZE),
            "EMULTIHOP" => Some(libc::EMULTIHOP),
            "ENAMETOOLONG" => Some(libc::ENAMETOOLONG),
            "ENETDOWN" => Some(libc::ENETDOWN),
            "ENETRESET" => Some(libc::ENETRESET),
            "ENETUNREACH" => Some(libc::ENETUNREACH),
            "ENFILE" => Some(libc::ENFILE),
            "ENOBUFS" => Some(libc::ENOBUFS),
            "ENODATA" => Some(libc::ENODATA),
            "ENODEV" => Some(libc::ENODEV),
            "ENOENT" => Some(libc::ENOENT),
            "ENOEXEC" => Some(libc::ENOEXEC),
            "ENOLCK" => Some(libc::ENOLCK),
            "ENOLINK" => Some(libc::ENOLINK),
            "ENOMEM" => Some(libc::ENOMEM),
            "ENOMSG" => Some(libc::ENOMSG),
            "ENOPROTOOPT" => Some(libc::ENOPROTOOPT),
            "ENOSPC" => Some(libc::ENOSPC),
            "ENOSR" => Some(libc::ENOSR),
            "ENOSTR" => Some(libc::ENOSTR),
            "ENOSYS" => Some(libc::ENOSYS),
            "ENOTCONN" => Some(libc::ENOTCONN),
            "ENOTDIR" => Some(libc::ENOTDIR),
            "ENOTEMPTY" => Some(libc::ENOTEMPTY),
            "ENOTSOCK" => Some(libc::ENOTSOCK),
            "ENOTSUP" => Some(libc::ENOTSUP),
            "ENOTTY" => Some(libc::ENOTTY),
            "ENXIO" => Some(libc::ENXIO),
            "EOPNOTSUPP" => Some(libc::EOPNOTSUPP),
            "EOVERFLOW" => Some(libc::EOVERFLOW),
            "EPERM" => Some(libc::EPERM),
            "EPIPE" => Some(libc::EPIPE),
            "EPROTO" => Some(libc::EPROTO),
            "EPROTONOSUPPORT" => Some(libc::EPROTONOSUPPORT),
            "EPROTOTYPE" => Some(libc::EPROTOTYPE),
            "ERANGE" => Some(libc::ERANGE),
            "EROFS" => Some(libc::EROFS),
            "ESPIPE" => Some(libc::ESPIPE),
            "ESRCH" => Some(libc::ESRCH),
            "ESTALE" => Some(libc::ESTALE),
            "ETIME" => Some(libc::ETIME),
            "ETIMEDOUT" => Some(libc::ETIMEDOUT),
            "ETXTBSY" => Some(libc::ETXTBSY),
            "EWOULDBLOCK" => Some(libc::EWOULDBLOCK),
            "EXDEV" => Some(libc::EXDEV),
            _ => None,
        };
        v.map(|x| x as f64)
    }
    #[cfg(not(unix))]
    {
        match prop {
            "EACCES" => Some(13.0),
            "EAGAIN" => Some(11.0),
            "EBADF" => Some(9.0),
            "EBUSY" => Some(16.0),
            "EEXIST" => Some(17.0),
            "EFAULT" => Some(14.0),
            "EINTR" => Some(4.0),
            "EINVAL" => Some(22.0),
            "EIO" => Some(5.0),
            "EISDIR" => Some(21.0),
            "EMFILE" => Some(24.0),
            "ENFILE" => Some(23.0),
            "ENODEV" => Some(19.0),
            "ENOENT" => Some(2.0),
            "ENOMEM" => Some(12.0),
            "ENOSPC" => Some(28.0),
            "ENOTDIR" => Some(20.0),
            "ENOTEMPTY" => Some(41.0),
            "EPERM" => Some(1.0),
            "EPIPE" => Some(32.0),
            "ERANGE" => Some(34.0),
            "EROFS" => Some(30.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod os_errno_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in OS_ERRNO_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    os_errno_const_lookup(lit),
                    os_errno_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `os_signal_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
#[cfg(unix)]
static OS_SIGNAL_CONST_TABLE: &[(&str, f64)] = &[
    ("SIGABRT", (libc::SIGABRT) as f64),
    ("SIGALRM", (libc::SIGALRM) as f64),
    ("SIGBUS", (libc::SIGBUS) as f64),
    ("SIGCHLD", (libc::SIGCHLD) as f64),
    ("SIGCONT", (libc::SIGCONT) as f64),
    ("SIGFPE", (libc::SIGFPE) as f64),
    ("SIGHUP", (libc::SIGHUP) as f64),
    ("SIGILL", (libc::SIGILL) as f64),
    #[cfg(target_os = "macos")]
    ("SIGINFO", (29i32) as f64),
    ("SIGINT", (libc::SIGINT) as f64),
    ("SIGIO", (libc::SIGIO) as f64),
    ("SIGIOT", (libc::SIGABRT) as f64),
    ("SIGKILL", (libc::SIGKILL) as f64),
    ("SIGPIPE", (libc::SIGPIPE) as f64),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    ("SIGPOLL", (libc::SIGPOLL) as f64),
    ("SIGPROF", (libc::SIGPROF) as f64),
    #[cfg(target_os = "linux")]
    ("SIGPWR", (libc::SIGPWR) as f64),
    ("SIGQUIT", (libc::SIGQUIT) as f64),
    ("SIGSEGV", (libc::SIGSEGV) as f64),
    #[cfg(target_os = "linux")]
    ("SIGSTKFLT", (libc::SIGSTKFLT) as f64),
    ("SIGSTOP", (libc::SIGSTOP) as f64),
    ("SIGSYS", (libc::SIGSYS) as f64),
    ("SIGTERM", (libc::SIGTERM) as f64),
    ("SIGTRAP", (libc::SIGTRAP) as f64),
    ("SIGTSTP", (libc::SIGTSTP) as f64),
    ("SIGTTIN", (libc::SIGTTIN) as f64),
    ("SIGTTOU", (libc::SIGTTOU) as f64),
    ("SIGURG", (libc::SIGURG) as f64),
    ("SIGUSR1", (libc::SIGUSR1) as f64),
    ("SIGUSR2", (libc::SIGUSR2) as f64),
    ("SIGVTALRM", (libc::SIGVTALRM) as f64),
    ("SIGWINCH", (libc::SIGWINCH) as f64),
    ("SIGXCPU", (libc::SIGXCPU) as f64),
    ("SIGXFSZ", (libc::SIGXFSZ) as f64),
];
#[cfg(not(unix))]
static OS_SIGNAL_CONST_TABLE: &[(&str, f64)] = &[
    ("SIGABRT", (22.0) as f64),
    ("SIGBREAK", (21.0) as f64),
    ("SIGFPE", (8.0) as f64),
    ("SIGHUP", (1.0) as f64),
    ("SIGILL", (4.0) as f64),
    ("SIGINT", (2.0) as f64),
    ("SIGKILL", (9.0) as f64),
    ("SIGSEGV", (11.0) as f64),
    ("SIGTERM", (15.0) as f64),
];

fn os_signal_const_lookup(prop: &str) -> Option<f64> {
    let i = OS_SIGNAL_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(OS_SIGNAL_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn os_signal_const_reference(prop: &str) -> Option<f64> {
    #[cfg(unix)]
    {
        let v: Option<i32> = match prop {
            "SIGHUP" => Some(libc::SIGHUP),
            "SIGINT" => Some(libc::SIGINT),
            "SIGQUIT" => Some(libc::SIGQUIT),
            "SIGILL" => Some(libc::SIGILL),
            "SIGTRAP" => Some(libc::SIGTRAP),
            "SIGABRT" => Some(libc::SIGABRT),
            "SIGIOT" => Some(libc::SIGABRT),
            "SIGBUS" => Some(libc::SIGBUS),
            "SIGFPE" => Some(libc::SIGFPE),
            "SIGKILL" => Some(libc::SIGKILL),
            "SIGUSR1" => Some(libc::SIGUSR1),
            "SIGSEGV" => Some(libc::SIGSEGV),
            "SIGUSR2" => Some(libc::SIGUSR2),
            "SIGPIPE" => Some(libc::SIGPIPE),
            "SIGALRM" => Some(libc::SIGALRM),
            "SIGTERM" => Some(libc::SIGTERM),
            "SIGCHLD" => Some(libc::SIGCHLD),
            #[cfg(target_os = "linux")]
            "SIGSTKFLT" => Some(libc::SIGSTKFLT),
            "SIGCONT" => Some(libc::SIGCONT),
            "SIGSTOP" => Some(libc::SIGSTOP),
            "SIGTSTP" => Some(libc::SIGTSTP),
            "SIGTTIN" => Some(libc::SIGTTIN),
            "SIGTTOU" => Some(libc::SIGTTOU),
            "SIGURG" => Some(libc::SIGURG),
            "SIGXCPU" => Some(libc::SIGXCPU),
            "SIGXFSZ" => Some(libc::SIGXFSZ),
            "SIGVTALRM" => Some(libc::SIGVTALRM),
            "SIGPROF" => Some(libc::SIGPROF),
            "SIGWINCH" => Some(libc::SIGWINCH),
            "SIGIO" => Some(libc::SIGIO),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            "SIGPOLL" => Some(libc::SIGPOLL),
            #[cfg(target_os = "linux")]
            "SIGPWR" => Some(libc::SIGPWR),
            "SIGSYS" => Some(libc::SIGSYS),
            #[cfg(target_os = "macos")]
            "SIGINFO" => Some(29i32),
            _ => None,
        };
        v.map(|x| x as f64)
    }
    #[cfg(not(unix))]
    {
        match prop {
            "SIGHUP" => Some(1.0),
            "SIGINT" => Some(2.0),
            "SIGILL" => Some(4.0),
            "SIGABRT" => Some(22.0),
            "SIGFPE" => Some(8.0),
            "SIGKILL" => Some(9.0),
            "SIGSEGV" => Some(11.0),
            "SIGTERM" => Some(15.0),
            "SIGBREAK" => Some(21.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod os_signal_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in OS_SIGNAL_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    os_signal_const_lookup(lit),
                    os_signal_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `fs_const_tail`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static FS_CONST_TAIL_TABLE: &[(&str, f64)] = &[
    #[cfg(target_os = "linux")]
    ("O_DIRECT", (libc::O_DIRECT as i64) as f64),
    #[cfg(unix)]
    ("O_DIRECTORY", (libc::O_DIRECTORY as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("O_DIRECTORY", (0x10000) as f64),
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    ("O_DSYNC", (0x400000) as f64),
    #[cfg(all(
        all(unix, not(any(target_os = "macos", target_os = "ios"))),
        not(any(any(target_os = "macos", target_os = "ios")))
    ))]
    ("O_DSYNC", (libc::O_DSYNC as i64) as f64),
    #[cfg(target_os = "linux")]
    ("O_NOATIME", (libc::O_NOATIME as i64) as f64),
    #[cfg(unix)]
    ("O_NOCTTY", (libc::O_NOCTTY as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("O_NOCTTY", (0) as f64),
    #[cfg(unix)]
    ("O_NONBLOCK", (libc::O_NONBLOCK as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("O_NONBLOCK", (0x800) as f64),
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    ("O_SYMLINK", (0x200000) as f64),
    #[cfg(unix)]
    ("O_SYNC", (libc::O_SYNC as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("O_SYNC", (0x101000) as f64),
    #[cfg(unix)]
    ("S_IFBLK", (libc::S_IFBLK as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFBLK", (0x6000) as f64),
    #[cfg(unix)]
    ("S_IFCHR", (libc::S_IFCHR as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFCHR", (0x2000) as f64),
    #[cfg(unix)]
    ("S_IFDIR", (libc::S_IFDIR as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFDIR", (0x4000) as f64),
    #[cfg(unix)]
    ("S_IFIFO", (libc::S_IFIFO as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFIFO", (0x1000) as f64),
    #[cfg(unix)]
    ("S_IFLNK", (libc::S_IFLNK as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFLNK", (0xA000) as f64),
    #[cfg(unix)]
    ("S_IFMT", (libc::S_IFMT as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFMT", (0xF000) as f64),
    #[cfg(unix)]
    ("S_IFREG", (libc::S_IFREG as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFREG", (0x8000) as f64),
    #[cfg(unix)]
    ("S_IFSOCK", (libc::S_IFSOCK as i64) as f64),
    #[cfg(all(not(unix), not(any(unix))))]
    ("S_IFSOCK", (0xC000) as f64),
    ("S_IRWXG", (0o070) as f64),
    ("S_IRWXO", (0o007) as f64),
    ("S_IRWXU", (0o700) as f64),
    ("UV_DIRENT_BLOCK", (7) as f64),
    ("UV_DIRENT_CHAR", (6) as f64),
    ("UV_DIRENT_DIR", (2) as f64),
    ("UV_DIRENT_FIFO", (4) as f64),
    ("UV_DIRENT_FILE", (1) as f64),
    ("UV_DIRENT_LINK", (3) as f64),
    ("UV_DIRENT_SOCKET", (5) as f64),
    ("UV_DIRENT_UNKNOWN", (0) as f64),
    ("UV_FS_COPYFILE_EXCL", (1) as f64),
    ("UV_FS_COPYFILE_FICLONE", (2) as f64),
    ("UV_FS_COPYFILE_FICLONE_FORCE", (4) as f64),
    #[cfg(windows)]
    ("UV_FS_O_FILEMAP", (0x2000_0000) as f64),
    #[cfg(all(not(windows), not(any(windows))))]
    ("UV_FS_O_FILEMAP", (0) as f64),
    ("UV_FS_SYMLINK_DIR", (1) as f64),
    ("UV_FS_SYMLINK_JUNCTION", (2) as f64),
];

fn fs_const_tail_lookup(prop: &str) -> Option<f64> {
    let i = FS_CONST_TAIL_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(FS_CONST_TAIL_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn fs_const_tail_reference(prop: &str) -> Option<f64> {
    let v: Option<i64> = match prop {
        // libuv dirent types (uv.h `uv_dirent_type_t`).
        "UV_DIRENT_UNKNOWN" => Some(0),
        "UV_DIRENT_FILE" => Some(1),
        "UV_DIRENT_DIR" => Some(2),
        "UV_DIRENT_LINK" => Some(3),
        "UV_DIRENT_FIFO" => Some(4),
        "UV_DIRENT_SOCKET" => Some(5),
        "UV_DIRENT_CHAR" => Some(6),
        "UV_DIRENT_BLOCK" => Some(7),
        // libuv symlink flags.
        "UV_FS_SYMLINK_DIR" => Some(1),
        "UV_FS_SYMLINK_JUNCTION" => Some(2),
        // libuv copyfile flags (Node mirrors these onto fs.constants
        // COPYFILE_* too).
        "UV_FS_COPYFILE_EXCL" => Some(1),
        "UV_FS_COPYFILE_FICLONE" => Some(2),
        "UV_FS_COPYFILE_FICLONE_FORCE" => Some(4),
        // libuv filemap open flag (Windows-only; 0 elsewhere, matching Node).
        #[cfg(windows)]
        "UV_FS_O_FILEMAP" => Some(0x2000_0000),
        #[cfg(not(windows))]
        "UV_FS_O_FILEMAP" => Some(0),
        // POSIX combined rwx permission masks (stable across platforms).
        "S_IRWXU" => Some(0o700),
        "S_IRWXG" => Some(0o070),
        "S_IRWXO" => Some(0o007),
        // POSIX file-type masks (S_IFMT family) — stable across Linux/macOS.
        #[cfg(unix)]
        "S_IFMT" => Some(libc::S_IFMT as i64),
        #[cfg(unix)]
        "S_IFREG" => Some(libc::S_IFREG as i64),
        #[cfg(unix)]
        "S_IFDIR" => Some(libc::S_IFDIR as i64),
        #[cfg(unix)]
        "S_IFCHR" => Some(libc::S_IFCHR as i64),
        #[cfg(unix)]
        "S_IFBLK" => Some(libc::S_IFBLK as i64),
        #[cfg(unix)]
        "S_IFIFO" => Some(libc::S_IFIFO as i64),
        #[cfg(unix)]
        "S_IFLNK" => Some(libc::S_IFLNK as i64),
        #[cfg(unix)]
        "S_IFSOCK" => Some(libc::S_IFSOCK as i64),
        #[cfg(not(unix))]
        "S_IFMT" => Some(0xF000),
        #[cfg(not(unix))]
        "S_IFREG" => Some(0x8000),
        #[cfg(not(unix))]
        "S_IFDIR" => Some(0x4000),
        #[cfg(not(unix))]
        "S_IFCHR" => Some(0x2000),
        #[cfg(not(unix))]
        "S_IFBLK" => Some(0x6000),
        #[cfg(not(unix))]
        "S_IFIFO" => Some(0x1000),
        #[cfg(not(unix))]
        "S_IFLNK" => Some(0xA000),
        #[cfg(not(unix))]
        "S_IFSOCK" => Some(0xC000),
        // OS-specific open() flags.
        #[cfg(unix)]
        "O_DIRECTORY" => Some(libc::O_DIRECTORY as i64),
        #[cfg(unix)]
        "O_NOCTTY" => Some(libc::O_NOCTTY as i64),
        #[cfg(unix)]
        "O_NONBLOCK" => Some(libc::O_NONBLOCK as i64),
        #[cfg(unix)]
        "O_SYNC" => Some(libc::O_SYNC as i64),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        "O_DSYNC" => Some(0x400000),
        #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
        "O_DSYNC" => Some(libc::O_DSYNC as i64),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        "O_SYMLINK" => Some(0x200000),
        // Linux-only open() flags (Node returns undefined for these on
        // platforms that lack them).
        #[cfg(target_os = "linux")]
        "O_DIRECT" => Some(libc::O_DIRECT as i64),
        #[cfg(target_os = "linux")]
        "O_NOATIME" => Some(libc::O_NOATIME as i64),
        #[cfg(not(unix))]
        "O_DIRECTORY" => Some(0x10000),
        #[cfg(not(unix))]
        "O_NOCTTY" => Some(0),
        #[cfg(not(unix))]
        "O_NONBLOCK" => Some(0x800),
        #[cfg(not(unix))]
        "O_SYNC" => Some(0x101000),
        _ => None,
    };
    v.map(|n| n as f64)
}

#[cfg(test)]
mod fs_const_tail_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in FS_CONST_TAIL_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    fs_const_tail_lookup(lit),
                    fs_const_tail_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `sqlite_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static SQLITE_CONST_TABLE: &[(&str, f64)] = &[
    ("SQLITE_ALTER_TABLE", (26.0) as f64),
    ("SQLITE_ANALYZE", (28.0) as f64),
    ("SQLITE_ATTACH", (24.0) as f64),
    ("SQLITE_CHANGESET_ABORT", (2.0) as f64),
    ("SQLITE_CHANGESET_CONFLICT", (3.0) as f64),
    ("SQLITE_CHANGESET_CONSTRAINT", (4.0) as f64),
    ("SQLITE_CHANGESET_DATA", (1.0) as f64),
    ("SQLITE_CHANGESET_FOREIGN_KEY", (5.0) as f64),
    ("SQLITE_CHANGESET_NOTFOUND", (2.0) as f64),
    ("SQLITE_CHANGESET_OMIT", (0.0) as f64),
    ("SQLITE_CHANGESET_REPLACE", (1.0) as f64),
    ("SQLITE_COPY", (0.0) as f64),
    ("SQLITE_CREATE_INDEX", (1.0) as f64),
    ("SQLITE_CREATE_TABLE", (2.0) as f64),
    ("SQLITE_CREATE_TEMP_INDEX", (3.0) as f64),
    ("SQLITE_CREATE_TEMP_TABLE", (4.0) as f64),
    ("SQLITE_CREATE_TEMP_TRIGGER", (5.0) as f64),
    ("SQLITE_CREATE_TEMP_VIEW", (6.0) as f64),
    ("SQLITE_CREATE_TRIGGER", (7.0) as f64),
    ("SQLITE_CREATE_VIEW", (8.0) as f64),
    ("SQLITE_CREATE_VTABLE", (29.0) as f64),
    ("SQLITE_DELETE", (9.0) as f64),
    ("SQLITE_DENY", (1.0) as f64),
    ("SQLITE_DETACH", (25.0) as f64),
    ("SQLITE_DROP_INDEX", (10.0) as f64),
    ("SQLITE_DROP_TABLE", (11.0) as f64),
    ("SQLITE_DROP_TEMP_INDEX", (12.0) as f64),
    ("SQLITE_DROP_TEMP_TABLE", (13.0) as f64),
    ("SQLITE_DROP_TEMP_TRIGGER", (14.0) as f64),
    ("SQLITE_DROP_TEMP_VIEW", (15.0) as f64),
    ("SQLITE_DROP_TRIGGER", (16.0) as f64),
    ("SQLITE_DROP_VIEW", (17.0) as f64),
    ("SQLITE_DROP_VTABLE", (30.0) as f64),
    ("SQLITE_FUNCTION", (31.0) as f64),
    ("SQLITE_IGNORE", (2.0) as f64),
    ("SQLITE_INSERT", (18.0) as f64),
    ("SQLITE_OK", (0.0) as f64),
    ("SQLITE_PRAGMA", (19.0) as f64),
    ("SQLITE_READ", (20.0) as f64),
    ("SQLITE_RECURSIVE", (33.0) as f64),
    ("SQLITE_REINDEX", (27.0) as f64),
    ("SQLITE_SAVEPOINT", (32.0) as f64),
    ("SQLITE_SELECT", (21.0) as f64),
    ("SQLITE_TRANSACTION", (22.0) as f64),
    ("SQLITE_UPDATE", (23.0) as f64),
];

fn sqlite_const_lookup(prop: &str) -> Option<f64> {
    let i = SQLITE_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(SQLITE_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn sqlite_const_reference(prop: &str) -> Option<f64> {
    Some(match prop {
        "SQLITE_CHANGESET_DATA" => 1.0,
        "SQLITE_CHANGESET_NOTFOUND" => 2.0,
        "SQLITE_CHANGESET_CONFLICT" => 3.0,
        "SQLITE_CHANGESET_CONSTRAINT" => 4.0,
        "SQLITE_CHANGESET_FOREIGN_KEY" => 5.0,
        "SQLITE_CHANGESET_OMIT" => 0.0,
        "SQLITE_CHANGESET_REPLACE" => 1.0,
        "SQLITE_CHANGESET_ABORT" => 2.0,
        "SQLITE_OK" => 0.0,
        "SQLITE_DENY" => 1.0,
        "SQLITE_IGNORE" => 2.0,
        "SQLITE_CREATE_INDEX" => 1.0,
        "SQLITE_CREATE_TABLE" => 2.0,
        "SQLITE_CREATE_TEMP_INDEX" => 3.0,
        "SQLITE_CREATE_TEMP_TABLE" => 4.0,
        "SQLITE_CREATE_TEMP_TRIGGER" => 5.0,
        "SQLITE_CREATE_TEMP_VIEW" => 6.0,
        "SQLITE_CREATE_TRIGGER" => 7.0,
        "SQLITE_CREATE_VIEW" => 8.0,
        "SQLITE_DELETE" => 9.0,
        "SQLITE_DROP_INDEX" => 10.0,
        "SQLITE_DROP_TABLE" => 11.0,
        "SQLITE_DROP_TEMP_INDEX" => 12.0,
        "SQLITE_DROP_TEMP_TABLE" => 13.0,
        "SQLITE_DROP_TEMP_TRIGGER" => 14.0,
        "SQLITE_DROP_TEMP_VIEW" => 15.0,
        "SQLITE_DROP_TRIGGER" => 16.0,
        "SQLITE_DROP_VIEW" => 17.0,
        "SQLITE_INSERT" => 18.0,
        "SQLITE_PRAGMA" => 19.0,
        "SQLITE_READ" => 20.0,
        "SQLITE_SELECT" => 21.0,
        "SQLITE_TRANSACTION" => 22.0,
        "SQLITE_UPDATE" => 23.0,
        "SQLITE_ATTACH" => 24.0,
        "SQLITE_DETACH" => 25.0,
        "SQLITE_ALTER_TABLE" => 26.0,
        "SQLITE_REINDEX" => 27.0,
        "SQLITE_ANALYZE" => 28.0,
        "SQLITE_CREATE_VTABLE" => 29.0,
        "SQLITE_DROP_VTABLE" => 30.0,
        "SQLITE_FUNCTION" => 31.0,
        "SQLITE_SAVEPOINT" => 32.0,
        "SQLITE_COPY" => 0.0,
        "SQLITE_RECURSIVE" => 33.0,
        _ => return None,
    })
}

#[cfg(test)]
mod sqlite_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in SQLITE_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = include_str!("constants.rs");
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    sqlite_const_lookup(lit),
                    sqlite_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}
