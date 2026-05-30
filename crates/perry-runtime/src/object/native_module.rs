//! Native-module namespace machinery: allocator (`js_create_native_module_namespace`),
//! property/method bindings (`js_native_module_property_by_name`,
//! `js_native_module_bind_method`, `js_class_method_bind`), and the
//! per-module constant/sub-namespace tables consumed from
//! `dispatch_native_module_method` and `js_object_get_field_by_name`.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static NATIVE_CALLABLE_EXPORTS: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    static BUFFER_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    static NATIVE_MODULE_NAMESPACES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
}

pub fn scan_native_callable_export_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    NATIVE_CALLABLE_EXPORTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    scan_stream_event_emitter_prototype_roots_mut(visitor);
}

/// Special class ID for native module namespace objects
/// This is used to identify objects that represent native module namespaces
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFFFFFE;

static BUFFER_POOL_SIZE_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(8192f64.to_bits());

pub(crate) fn buffer_pool_size() -> f64 {
    f64::from_bits(BUFFER_POOL_SIZE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn set_buffer_pool_size(value: f64) {
    BUFFER_POOL_SIZE_BITS.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Create a native module namespace object
/// This is used for `import * as X from 'module'` patterns
/// The returned object identifies itself as an object (typeof returns "object")
/// and stores the module name for debugging purposes
///
/// module_name_ptr: pointer to the module name string bytes
/// module_name_len: length of the module name
/// Returns the object as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_create_native_module_namespace(
    module_name_ptr: *const u8,
    module_name_len: usize,
) -> f64 {
    let module_name = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(module_name_ptr, module_name_len))
            .unwrap_or("")
    };
    let module_name = normalize_native_module_alias(module_name);
    if should_cache_native_module_namespace(module_name) {
        if let Some(bits) =
            NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get(module_name).copied())
        {
            return f64::from_bits(bits);
        }
    }

    // Create an object with one field to store the module name
    let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID, 1);

    // Create a string from the module name
    let module_name_header =
        crate::string::js_string_from_bytes(module_name.as_ptr(), module_name.len() as u32);

    // Store the module name in the first field
    js_object_set_field(obj, 0, JSValue::string_ptr(module_name_header));

    // Create a keys array with one key: "__module__"
    let keys_array = crate::array::js_array_alloc(1);
    let key_bytes = b"__module__";
    let key_str = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    crate::array::js_array_push(keys_array, JSValue::string_ptr(key_str));
    js_object_set_keys(obj, keys_array);

    // Return as NaN-boxed pointer
    let value = crate::value::js_nanbox_pointer(obj as i64);
    if should_cache_native_module_namespace(module_name) {
        NATIVE_MODULE_NAMESPACES.with(|cache| {
            cache
                .borrow_mut()
                .insert(module_name.to_string(), value.to_bits());
        });
    }
    value
}

fn normalize_native_module_alias(module_name: &str) -> &str {
    match module_name {
        "sys" => {
            crate::node_submodules::emit_sys_deprecation_warning_once();
            "util"
        }
        "path/posix" => "path.posix",
        "path/win32" => "path.win32",
        _ => module_name,
    }
}

const DEPRECATED_CONSTANTS_KEYS: &[&[u8]] = &[
    b"F_OK",
    b"R_OK",
    b"W_OK",
    b"X_OK",
    b"O_RDONLY",
    b"O_WRONLY",
    b"O_RDWR",
    b"O_NOFOLLOW",
    b"O_CREAT",
    b"O_TRUNC",
    b"O_APPEND",
    b"O_EXCL",
    b"COPYFILE_EXCL",
    b"COPYFILE_FICLONE",
    b"COPYFILE_FICLONE_FORCE",
    b"S_IRUSR",
    b"S_IWUSR",
    b"S_IXUSR",
    b"S_IRGRP",
    b"S_IWGRP",
    b"S_IXGRP",
    b"S_IROTH",
    b"S_IWOTH",
    b"S_IXOTH",
    b"SIGHUP",
    b"SIGINT",
    b"SIGQUIT",
    b"SIGILL",
    b"SIGTRAP",
    b"SIGABRT",
    b"SIGIOT",
    b"SIGBUS",
    b"SIGFPE",
    b"SIGKILL",
    b"SIGUSR1",
    b"SIGSEGV",
    b"SIGUSR2",
    b"SIGPIPE",
    b"SIGALRM",
    b"SIGTERM",
    b"SIGCHLD",
    b"SIGCONT",
    b"SIGSTOP",
    b"SIGTSTP",
    b"SIGTTIN",
    b"SIGTTOU",
    b"SIGURG",
    b"SIGXCPU",
    b"SIGXFSZ",
    b"SIGVTALRM",
    b"SIGPROF",
    b"SIGWINCH",
    b"SIGIO",
    b"SIGSYS",
    b"E2BIG",
    b"EACCES",
    b"EADDRINUSE",
    b"EADDRNOTAVAIL",
    b"EAFNOSUPPORT",
    b"EAGAIN",
    b"EALREADY",
    b"EBADF",
    b"EBADMSG",
    b"EBUSY",
    b"ECANCELED",
    b"ECHILD",
    b"ECONNABORTED",
    b"ECONNREFUSED",
    b"ECONNRESET",
    b"EDEADLK",
    b"EDESTADDRREQ",
    b"EDOM",
    b"EDQUOT",
    b"EEXIST",
    b"EFAULT",
    b"EFBIG",
    b"EHOSTUNREACH",
    b"EIDRM",
    b"EILSEQ",
    b"EINPROGRESS",
    b"EINTR",
    b"EINVAL",
    b"EIO",
    b"EISCONN",
    b"EISDIR",
    b"ELOOP",
    b"EMFILE",
    b"EMLINK",
    b"EMSGSIZE",
    b"EMULTIHOP",
    b"ENAMETOOLONG",
    b"ENETDOWN",
    b"ENETRESET",
    b"ENETUNREACH",
    b"ENFILE",
    b"ENOBUFS",
    b"ENODATA",
    b"ENODEV",
    b"ENOENT",
    b"ENOEXEC",
    b"ENOLCK",
    b"ENOLINK",
    b"ENOMEM",
    b"ENOMSG",
    b"ENOPROTOOPT",
    b"ENOSPC",
    b"ENOSR",
    b"ENOSTR",
    b"ENOSYS",
    b"ENOTCONN",
    b"ENOTDIR",
    b"ENOTEMPTY",
    b"ENOTSOCK",
    b"ENOTSUP",
    b"ENOTTY",
    b"ENXIO",
    b"EOPNOTSUPP",
    b"EOVERFLOW",
    b"EPERM",
    b"EPIPE",
    b"EPROTO",
    b"EPROTONOSUPPORT",
    b"EPROTOTYPE",
    b"ERANGE",
    b"EROFS",
    b"ESPIPE",
    b"ESRCH",
    b"ESTALE",
    b"ETIME",
    b"ETIMEDOUT",
    b"ETXTBSY",
    b"EWOULDBLOCK",
    b"EXDEV",
    b"PRIORITY_LOW",
    b"PRIORITY_BELOW_NORMAL",
    b"PRIORITY_NORMAL",
    b"PRIORITY_ABOVE_NORMAL",
    b"PRIORITY_HIGH",
    b"PRIORITY_HIGHEST",
    b"RTLD_LAZY",
    b"RTLD_NOW",
    b"RTLD_GLOBAL",
    b"RTLD_LOCAL",
    b"RTLD_DEEPBIND",
    b"OPENSSL_VERSION_NUMBER",
    b"SSL_OP_ALL",
    b"SSL_OP_ALLOW_NO_DHE_KEX",
    b"SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION",
    b"SSL_OP_CIPHER_SERVER_PREFERENCE",
    b"SSL_OP_CISCO_ANYCONNECT",
    b"SSL_OP_COOKIE_EXCHANGE",
    b"SSL_OP_CRYPTOPRO_TLSEXT_BUG",
    b"SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS",
    b"SSL_OP_LEGACY_SERVER_CONNECT",
    b"SSL_OP_NO_COMPRESSION",
    b"SSL_OP_NO_ENCRYPT_THEN_MAC",
    b"SSL_OP_NO_QUERY_MTU",
    b"SSL_OP_NO_RENEGOTIATION",
    b"SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION",
    b"SSL_OP_NO_SSLv2",
    b"SSL_OP_NO_SSLv3",
    b"SSL_OP_NO_TICKET",
    b"SSL_OP_NO_TLSv1",
    b"SSL_OP_NO_TLSv1_1",
    b"SSL_OP_NO_TLSv1_2",
    b"SSL_OP_NO_TLSv1_3",
    b"SSL_OP_PRIORITIZE_CHACHA",
    b"SSL_OP_TLS_ROLLBACK_BUG",
    b"ENGINE_METHOD_RSA",
    b"ENGINE_METHOD_DSA",
    b"ENGINE_METHOD_DH",
    b"ENGINE_METHOD_RAND",
    b"ENGINE_METHOD_EC",
    b"ENGINE_METHOD_CIPHERS",
    b"ENGINE_METHOD_DIGESTS",
    b"ENGINE_METHOD_PKEY_METHS",
    b"ENGINE_METHOD_PKEY_ASN1_METHS",
    b"ENGINE_METHOD_ALL",
    b"ENGINE_METHOD_NONE",
    b"DH_CHECK_P_NOT_SAFE_PRIME",
    b"DH_CHECK_P_NOT_PRIME",
    b"DH_UNABLE_TO_CHECK_GENERATOR",
    b"DH_NOT_SUITABLE_GENERATOR",
    b"RSA_PKCS1_PADDING",
    b"RSA_NO_PADDING",
    b"RSA_PKCS1_OAEP_PADDING",
    b"RSA_X931_PADDING",
    b"RSA_PKCS1_PSS_PADDING",
    b"RSA_PSS_SALTLEN_DIGEST",
    b"RSA_PSS_SALTLEN_MAX_SIGN",
    b"RSA_PSS_SALTLEN_AUTO",
    b"TLS1_VERSION",
    b"TLS1_1_VERSION",
    b"TLS1_2_VERSION",
    b"TLS1_3_VERSION",
    b"POINT_CONVERSION_COMPRESSED",
    b"POINT_CONVERSION_UNCOMPRESSED",
    b"POINT_CONVERSION_HYBRID",
];

pub(crate) fn native_module_enumerable_keys(module_name: &str) -> Option<&'static [&'static [u8]]> {
    match module_name {
        "assert/strict" => Some(&[
            b"AssertionError",
            b"ok",
            b"fail",
            b"equal",
            b"notEqual",
            b"deepEqual",
            b"notDeepEqual",
            b"deepStrictEqual",
            b"notDeepStrictEqual",
            b"strictEqual",
            b"notStrictEqual",
            b"partialDeepStrictEqual",
            b"match",
            b"doesNotMatch",
            b"throws",
            b"rejects",
            b"doesNotThrow",
            b"doesNotReject",
            b"ifError",
            b"strict",
        ]),
        "buffer.constants" => Some(&[b"MAX_LENGTH", b"MAX_STRING_LENGTH"]),
        // Deprecated path alias enumerable on the top-level and style
        // sub-namespaces, matching Node's `Object.keys(...).includes`.
        "path" | "path.posix" | "path.win32" => Some(&[b"_makeLong"]),
        "constants" => Some(DEPRECATED_CONSTANTS_KEYS),
        "querystring" => Some(&[
            b"unescapeBuffer",
            b"unescape",
            b"escape",
            b"stringify",
            b"encode",
            b"parse",
            b"decode",
        ]),
        "util" => Some(&[
            b"callbackify",
            b"debuglog",
            b"deprecate",
            b"format",
            b"formatWithOptions",
            b"getSystemErrorMap",
            b"getSystemErrorName",
            b"getSystemErrorMessage",
            b"inherits",
            b"inspect",
            b"isArray",
            b"isDeepStrictEqual",
            b"promisify",
            b"stripVTControlCharacters",
            b"types",
            b"parseArgs",
            b"TextDecoder",
            b"TextEncoder",
        ]),
        "net" => Some(&[
            b"_createServerHandle",
            b"_normalizeArgs",
            b"connect",
            b"createConnection",
            b"createServer",
            b"isIP",
            b"isIPv4",
            b"isIPv6",
            b"Server",
            b"Socket",
            b"getDefaultAutoSelectFamily",
            b"setDefaultAutoSelectFamily",
            b"getDefaultAutoSelectFamilyAttemptTimeout",
            b"setDefaultAutoSelectFamilyAttemptTimeout",
        ]),
        _ => None,
    }
}

fn should_cache_native_module_namespace(module_name: &str) -> bool {
    matches!(
        module_name,
        "assert/strict" | "constants" | "util" | "util.types" | "path.posix" | "path.win32"
    )
}

/// #1479: read the module-name string stored in field 0 of a
/// native-module-namespace ObjectHeader. Returns `None` if the field
/// is missing, not a string, or the bytes aren't valid UTF-8. Caller
/// must have confirmed `class_id == NATIVE_MODULE_CLASS_ID` already.
///
/// # Safety
/// `obj_ptr` must point to a live `ObjectHeader` with
/// `class_id == NATIVE_MODULE_CLASS_ID` (i.e. one produced by
/// [`js_create_native_module_namespace`]).
pub(crate) unsafe fn read_native_module_name(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<String> {
    let field = crate::object::js_object_get_field(obj_ptr, 0);
    // #1781: SSO-aware — a native-module name of ≤ 5 bytes (e.g. `"fs"`,
    // `"os"`, `"tty"`, `"net"`, `"path"`) is stored as a SHORT_STRING_TAG
    // value. Pre-fix `is_string()` (STRING_TAG-only) returned None and
    // the auto-optimize sweep couldn't determine the requested module.
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(field, &mut sso_buf)?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Issue #649: codegen entry for `PropertyGet { NativeModuleRef(name),
/// property }`. `NativeModuleRef` lowers to a literal `0.0` at the codegen
/// level, so the generic PropertyGet path can't find the namespace
/// object. This helper short-circuits to the constants dispatcher; for
/// the chained case (`fs.constants.F_OK`) the inner call returns a
/// sub-namespace ObjectHeader and the outer PropertyGet goes through
/// `js_object_get_field_by_name`'s NATIVE_MODULE_CLASS_ID arm.
#[no_mangle]
pub unsafe extern "C" fn js_native_module_property_by_name(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let module_name =
        std::str::from_utf8(std::slice::from_raw_parts(module_name_ptr, module_name_len))
            .unwrap_or("");
    let module_name = normalize_native_module_alias(module_name);
    let property_name = std::str::from_utf8(std::slice::from_raw_parts(
        property_name_ptr,
        property_name_len,
    ))
    .unwrap_or("");
    // node:perf_hooks — `performance` and `constants` are object-valued
    // exports. Resolve them to a `perf_hooks`-tagged namespace object so
    // `typeof performance === "object"`, `performance.timeOrigin` (a
    // constant), `performance.now` (a callable export), and
    // `constants.NODE_PERFORMANCE_GC_*` (constants) all dispatch coherently.
    if module_name == "perf_hooks" && property_name == "performance" {
        // Singleton so `require("perf_hooks").performance` and the global
        // `performance` are the same object (Node identity guarantee, #1327).
        return crate::perf_hooks::performance_namespace();
    }
    if module_name == "perf_hooks" && property_name == "constants" {
        return js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    }
    // #1533: node:stream exposes a `promises` namespace (`await pipeline(...)`
    // / `finished(...)`). Resolve `stream.promises` to a `stream/promises`-
    // tagged namespace object so `typeof stream.promises === "object"` and
    // `stream.promises.pipeline` / `.finished` read as callable exports
    // (same dispatch the `import ... from "node:stream/promises"` form uses).
    if module_name == "stream" && property_name == "promises" {
        let submodule = "stream/promises";
        return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
    }
    // #2133: same shape for `node:fs.promises`. Route to the populated
    // `fs_promises` singleton so destructured exports + FileHandle methods
    // dispatch correctly.
    if module_name == "fs" && property_name == "promises" {
        return unsafe {
            crate::node_submodules::js_node_submodule_namespace(
                b"fs_promises".as_ptr(),
                "fs_promises".len() as u32,
            )
        };
    }

    if let Some(val) = get_native_module_constant(module_name, property_name, 0.0) {
        return val;
    }
    // For native modules whose surface includes known callable methods or
    // class exports, return a bound-method closure so `typeof` and property
    // capture (`const f = tty.isatty`) match Node's "function" shape. The
    // closure routes back through js_native_call_method when invoked. Kept
    // narrow to specific (module, property) pairs so a typo'd access still
    // returns undefined.
    if is_native_module_callable_export(module_name, property_name) {
        return bound_native_callable_export_value(module_name, property_name);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn bound_native_callable_export_value(module_name: &str, property_name: &str) -> f64 {
    let key = format!("{module_name}\0{property_name}");
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let method_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let ns = js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, ns);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_bytes.len() as i64);
    set_bound_native_closure_name(closure, property_name);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    let closure_addr = closure as usize;

    if module_name == "tty" && matches!(property_name, "ReadStream" | "WriteStream") {
        attach_tty_stream_prototype(value, property_name);
    }
    if module_name == "stream" && property_name == "Stream" {
        attach_stream_legacy_prototype(value);
    }
    if module_name == "stream"
        && matches!(
            property_name,
            "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
        )
    {
        attach_stream_constructor_prototype(value, property_name);
    }

    // `PerformanceObserver.supportedEntryTypes` is a static array on the
    // constructor. `PerformanceObserver` is a function value (a bound-method
    // closure), so hang the array off it as a dynamic property — keeps
    // `typeof PerformanceObserver === "function"` while the static read works.
    if module_name == "perf_hooks" && property_name == "PerformanceObserver" {
        let arr = crate::perf_hooks::js_perf_supported_entry_types();
        crate::closure::closure_set_dynamic_prop(closure_addr, "supportedEntryTypes", arr);
    }

    if module_name == "events" && property_name == "EventEmitter" {
        crate::closure::closure_set_dynamic_prop(closure_addr, "defaultMaxListeners", 10.0);
    }

    if module_name == "util" && property_name == "promisify" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            crate::util_promisify::promisify_custom_symbol(),
        );
    }

    NATIVE_CALLABLE_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

fn native_callable_export_arity(module: &str, prop: &str) -> Option<u32> {
    match (module, prop) {
        ("net", "createServer" | "Server") => Some(2),
        ("net", "Socket") => Some(1),
        ("net", "_normalizeArgs") => Some(1),
        ("net", "_createServerHandle") => Some(5),
        _ => None,
    }
}

extern "C" fn buffer_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    encoding_or_offset: f64,
    length: f64,
) -> f64 {
    let value_js = crate::value::JSValue::from_bits(value.to_bits());
    let buf = if value_js.is_undefined() || value_js.is_null() {
        crate::buffer::js_buffer_alloc(0, 0)
    } else if value_js.is_int32() || value_js.is_number() {
        let size = if value_js.is_int32() {
            value_js.as_int32()
        } else {
            value as i32
        };
        crate::buffer::js_buffer_alloc_unsafe(size)
    } else {
        let second = crate::value::JSValue::from_bits(encoding_or_offset.to_bits());
        let third = crate::value::JSValue::from_bits(length.to_bits());
        let second_is_offset =
            !second.is_undefined() && !second.is_null() && !second.is_any_string();
        if !third.is_undefined() || second_is_offset {
            let len = if third.is_undefined() {
                -1
            } else if third.is_int32() {
                third.as_int32()
            } else {
                length as i32
            };
            let offset = if second.is_int32() {
                second.as_int32()
            } else {
                encoding_or_offset as i32
            };
            crate::buffer::js_buffer_from_arraybuffer_slice(value.to_bits() as i64, offset, len)
        } else {
            let enc = if second.is_undefined() {
                0
            } else {
                crate::buffer::js_encoding_tag_from_value(encoding_or_offset)
            };
            crate::buffer::js_buffer_from_value(value.to_bits() as i64, enc)
        }
    };
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn buffer_prototype_method_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

const BUFFER_STATIC_METHODS: &[&str] = &[
    "from",
    "alloc",
    "allocUnsafe",
    "allocUnsafeSlow",
    "concat",
    "of",
    "isBuffer",
    "isEncoding",
    "byteLength",
    "compare",
    "copyBytesFrom",
];

const BUFFER_PROTOTYPE_METHODS: &[&str] = &[
    "toString",
    "equals",
    "subarray",
    "readUInt8",
    "write",
    "copy",
    "slice",
    "fill",
    "includes",
    "indexOf",
    "lastIndexOf",
];

pub(crate) fn buffer_constructor_value() -> f64 {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = buffer_constructor_thunk as *const u8;
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        crate::closure::js_register_closure_arity(func_ptr, 3);
        set_bound_native_closure_name(closure, "Buffer");
        let closure_addr = closure as usize;
        let value = crate::value::js_nanbox_pointer(closure as i64);

        for method in BUFFER_STATIC_METHODS {
            let method_value = bound_native_callable_export_value("buffer.Buffer", method);
            crate::closure::closure_set_dynamic_prop(closure_addr, method, method_value);
        }

        crate::closure::closure_set_dynamic_prop(closure_addr, "poolSize", buffer_pool_size());

        let proto = js_object_alloc(0, 0);
        if !proto.is_null() {
            let constructor = "constructor";
            let constructor_key =
                crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
            js_object_set_field_by_name(proto, constructor_key, value);
            super::set_builtin_property_attrs(
                proto as usize,
                constructor.to_string(),
                super::PropertyAttrs::new(true, false, true),
            );

            for method in BUFFER_PROTOTYPE_METHODS {
                let method_ptr = buffer_prototype_method_thunk as *const u8;
                let method_closure = crate::closure::js_closure_alloc(method_ptr, 0);
                if method_closure.is_null() {
                    continue;
                }
                set_bound_native_closure_name(method_closure, method);
                let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
                let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
                js_object_set_field_by_name(proto, key, method_value);
            }
            let proto_value = crate::value::js_nanbox_pointer(proto as i64);
            crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
            super::set_builtin_property_attrs(
                closure_addr,
                "prototype".to_string(),
                super::PropertyAttrs::new(true, false, false),
            );
        }

        slot.set(value.to_bits());
        value
    })
}

pub(crate) fn is_buffer_constructor_value(value: f64) -> bool {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        cached != 0 && cached == value.to_bits()
    })
}

extern "C" fn util_debuglog_logger_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn util_debuglog_logger_value() -> f64 {
    let func_ptr = util_debuglog_logger_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    set_bound_native_closure_name(closure, "debuglog");
    crate::value::js_nanbox_pointer(closure as i64)
}

fn fn_value(func_ptr: *const u8, name: &str) -> f64 {
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    set_bound_native_closure_name(closure, name);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn attach_tty_stream_prototype(constructor_value: f64, name: &str) {
    let (packed, count) = if name == "WriteStream" {
        (b"constructor\0hasColors\0getColorDepth\0".as_ptr(), 3)
    } else {
        (b"constructor\0".as_ptr(), 1)
    };
    let packed_len = if name == "WriteStream" {
        b"constructor\0hasColors\0getColorDepth\0".len()
    } else {
        b"constructor\0".len()
    };
    let shape_id = if name == "WriteStream" {
        0x7FFF_FF32
    } else {
        0x7FFF_FF31
    };
    let proto = js_object_alloc_with_shape(shape_id, count, packed, packed_len as u32);
    js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    if name == "WriteStream" {
        js_object_set_field(
            proto,
            1,
            JSValue::from_bits(
                fn_value(
                    crate::tty::js_tty_write_stream_has_colors as *const u8,
                    "hasColors",
                )
                .to_bits(),
            ),
        );
        js_object_set_field(
            proto,
            2,
            JSValue::from_bits(
                fn_value(
                    crate::tty::js_tty_write_stream_get_color_depth as *const u8,
                    "getColorDepth",
                )
                .to_bits(),
            ),
        );
    }
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize,
        "prototype",
        proto_value,
    );
}

pub(crate) unsafe fn bound_native_callable_module_and_method(
    value: f64,
) -> Option<(String, String)> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let closure = jv.as_pointer::<crate::closure::ClosureHeader>();
    if closure.is_null()
        || (*closure).type_tag != crate::closure::CLOSURE_MAGIC
        || (*closure).func_ptr != crate::closure::BOUND_METHOD_FUNC_PTR
    {
        return None;
    }
    let ns = crate::closure::js_closure_get_capture_f64(closure, 0);
    let module = get_module_name_from_namespace(ns).to_string();
    let method_ptr = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const u8;
    let method_len = crate::closure::js_closure_get_capture_ptr(closure, 2) as usize;
    if method_ptr.is_null() {
        return None;
    }
    let method = std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len))
        .ok()?
        .to_string();
    Some((module, method))
}

pub(crate) unsafe fn bound_native_callable_value_arity(value: f64) -> Option<u32> {
    let (module, method) = bound_native_callable_module_and_method(value)?;
    match (module.as_str(), method.as_str()) {
        ("console", "Console") => Some(1),
        ("util", "isArray") => Some(1),
        ("process", "getBuiltinModule") => Some(1),
        _ => native_callable_export_arity(module.as_str(), method.as_str()),
    }
}

pub(crate) fn set_bound_native_closure_name(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
) {
    let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_value = f64::from_bits(JSValue::string_ptr(ptr).bits());
    crate::closure::closure_set_dynamic_prop(closure as usize, "name", name_value);
}

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
    if module == "fs" && matches!(prop, "lchmod" | "lchmodSync") {
        return crate::fs::lchmod_is_callable_on_this_platform();
    }
    if matches!(module, "path.posix" | "path.win32")
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

    matches!(
        (module, prop),
        // #1533: node:stream `promises` namespace exports.
        ("stream/promises", "pipeline")
            | ("stream/promises", "finished")
            | ("process", "abort")
            | ("process", "cwd")
            | ("process", "uptime")
            | ("process", "memoryUsage")
            | ("process", "nextTick")
            | ("process", "chdir")
            | ("process", "kill")
            | ("process", "exit")
            | ("process", "umask")
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
            | ("process", "cpuUsage")
            | ("process", "resourceUsage")
            | ("process", "getActiveResourcesInfo")
            | ("process", "hrtime")
            | ("worker_threads", "getEnvironmentData")
            | ("worker_threads", "setEnvironmentData")
            | ("tty", "isatty")
            | ("tty", "ReadStream")
            | ("tty", "WriteStream")
            | ("net", "createServer")
            | ("net", "Server")
            | ("net", "Socket")
            | ("net", "_normalizeArgs")
            | ("net", "_createServerHandle")
            // #1856: `child_process.ChildProcess` reads as `[Function: ChildProcess]`.
            | ("child_process", "ChildProcess")
            // #1857 / #2130: every exported function reads as a bound-method
            // closure so `const spawn = cp.spawn; spawn(...)` (Node's canonical
            // test idiom — `const spawn = require('child_process').spawn`) and
            // `util.promisify(cp.exec)` both detect/wrap them. Method-call form
            // (`cp.spawn(...)`) already lowers through a dedicated codegen path;
            // this just keeps the value-read form coherent so it dispatches
            // through dispatch_native_module_method.
            | ("child_process", "exec")
            | ("child_process", "execFile")
            | ("child_process", "execSync")
            | ("child_process", "execFileSync")
            | ("child_process", "spawn")
            | ("child_process", "spawnSync")
            | ("child_process", "fork")
            | ("events", "EventEmitter")
            | ("events", "on")
            | ("stream", "compose")
            | ("stream", "duplexPair")
            | ("stream", "pipeline")
            | ("stream", "Readable")
            | ("stream", "Writable")
            | ("stream", "Duplex")
            | ("stream", "Transform")
            | ("stream", "PassThrough")
            | ("stream", "Stream")
            | ("string_decoder", "StringDecoder")
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
            | ("fs", "existsSync")
            | ("fs", "exists")
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
            | ("fs", "mkdtempSync")
            | ("fs", "mkdtemp")
            | ("fs", "openSync")
            | ("fs", "open")
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
            | ("fs", "lstatSync")
            | ("fs", "truncateSync")
            | ("fs", "truncate")
            | ("fs", "unlink")
            | ("fs", "unlinkSync")
            | ("fs", "utimes")
            | ("fs", "utimesSync")
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
            // #1478: performance.markResourceTiming(info) records a
            // PerformanceResourceTiming. Perry's runtime no-ops it but
            // the property must still read as a function for
            // feature-detection (`typeof X === "function"`) wrappers.
            | ("perf_hooks", "markResourceTiming")
            // #1335: performance.timerify(fn) wraps `fn` to record a
            // 'function' timeline entry per call. Perry currently
            // returns `fn` unchanged (no entry recorded), but the
            // property must still read as a function for
            // feature-detection.
            | ("perf_hooks", "timerify")
            // #1366: `crypto.getRandomValues` is the WebCrypto sync
            // randomness API. Perry lowers the call form via a
            // synthetic `$$cryptoFillRandom` method on the buffer
            // (see `lower/expr_call/module_static.rs`), but reading
            // it as a value (`typeof crypto.getRandomValues ===
            // "function"`, `const f = crypto.getRandomValues`)
            // needs the property-read form to be a bound-method
            // closure.
            | ("crypto", "getRandomValues")
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
            | ("perf_hooks", "PerformanceObserver")
            | ("perf_hooks", "PerformanceEntry")
            | ("perf_hooks", "PerformanceMark")
            | ("perf_hooks", "PerformanceMeasure")
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
            // The fixtures only probe types, but compiled npm code that
            // calls `cluster.fork()` would also land on the bound-method
            // dispatch (currently a stub — see runtime entries below).
            | ("cluster", "fork")
            | ("cluster", "disconnect")
            | ("cluster", "setupPrimary")
            | ("cluster", "setupMaster")
            | ("cluster", "Worker")
            | ("buffer.Buffer", "copyBytesFrom")
            | ("buffer", "atob")
            | ("buffer", "btoa")
            | ("util", "format")
            | ("util", "formatWithOptions")
            | ("util", "inspect")
            | ("util", "debuglog")
            | ("util", "getSystemErrorName")
            | ("util", "getSystemErrorMessage")
            | ("util", "getSystemErrorMap")
            | ("util", "parseEnv")
            | ("util", "isArray")
            | ("util", "promisify")
            | ("util", "callbackify")
            | ("util", "parseArgs")
            | ("util", "deprecate")
            | ("util", "inherits")
            | ("util", "isDeepStrictEqual")
            | ("util", "stripVTControlCharacters")
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
            | ("util.types", "isPromise")
            | ("util.types", "isArrayBuffer")
            | ("util.types", "isSharedArrayBuffer")
            | ("util.types", "isAnyArrayBuffer")
            | ("util.types", "isArrayBufferView")
            | ("util.types", "isTypedArray")
            | ("util.types", "isUint8Array")
            | ("util.types", "isInt8Array")
            | ("util.types", "isInt16Array")
            | ("util.types", "isUint16Array")
            | ("util.types", "isInt32Array")
            | ("util.types", "isUint32Array")
            | ("util.types", "isFloat32Array")
            | ("util.types", "isFloat64Array")
            | ("util.types", "isUint8ClampedArray")
            | ("util.types", "isBigInt64Array")
            | ("util.types", "isBigUint64Array")
            | ("util.types", "isMap")
            | ("util.types", "isMapIterator")
            | ("util.types", "isProxy")
            | ("util.types", "isSet")
            | ("util.types", "isSetIterator")
            | ("util.types", "isDate")
            | ("util.types", "isRegExp")
            | ("util.types", "isAsyncFunction")
            | ("util.types", "isGeneratorFunction")
            | ("util.types", "isGeneratorObject")
            | ("util.types", "isNativeError")
            | ("util.types", "isNumberObject")
            | ("util.types", "isStringObject")
            | ("util.types", "isBooleanObject")
            | ("util.types", "isBoxedPrimitive")
            | ("util/types", "isPromise")
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
            | ("util/types", "isTypedArray")
            | ("util/types", "isUint8Array")
            | ("util/types", "isInt8Array")
            | ("util/types", "isInt16Array")
            | ("util/types", "isUint16Array")
            | ("util/types", "isInt32Array")
            | ("util/types", "isUint32Array")
            | ("util/types", "isFloat32Array")
            | ("util/types", "isFloat64Array")
            | ("util/types", "isUint8ClampedArray")
            | ("util/types", "isBigInt64Array")
            | ("util/types", "isBigUint64Array")
            | ("util/types", "isMap")
            | ("util/types", "isMapIterator")
            | ("util/types", "isProxy")
            | ("util/types", "isSet")
            | ("util/types", "isSetIterator")
            | ("util/types", "isDate")
            | ("util/types", "isRegExp")
            | ("util/types", "isAsyncFunction")
            | ("util/types", "isGeneratorFunction")
            | ("util/types", "isGeneratorObject")
            | ("util/types", "isNativeError")
            | ("util/types", "isNumberObject")
            | ("util/types", "isStringObject")
            | ("util/types", "isBooleanObject")
            | ("util/types", "isBoxedPrimitive")
            | ("url", "URL")
            | ("url", "URLSearchParams")
            | ("url", "fileURLToPath")
            | ("url", "fileURLToPathBuffer")
            | ("url", "pathToFileURL")
            | ("url", "domainToASCII")
            | ("url", "domainToUnicode")
            | ("url", "urlToHttpOptions")
            | ("url", "format")
            | ("url", "parse")
            | ("url", "resolve")
            | ("punycode", "decode")
            | ("punycode", "encode")
            | ("punycode", "toASCII")
            | ("punycode", "toUnicode")
            | ("querystring", "unescapeBuffer")
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
            | ("crypto", "createHash")
            | ("crypto", "Hash")
            | ("crypto", "createSign")
            | ("crypto", "Sign")
            | ("crypto", "createVerify")
            | ("crypto", "Verify")
            | ("crypto", "ECDH")
            | ("crypto", "createECDH")
            | ("crypto", "createDiffieHellman")
            | ("crypto", "createDiffieHellmanGroup")
            | ("crypto", "getDiffieHellman")
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
            | ("crypto", "createSecretKey")
            | ("crypto.Certificate", "verifySpkac")
            | ("crypto.Certificate", "exportPublicKey")
            | ("crypto.Certificate", "exportChallenge")
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
            | ("https", "createServer")
            | ("https", "Server")
            | ("http2", "createServer")
            | ("http2", "createSecureServer")
            | ("http2", "Server")
    )
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    namespace_obj: f64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            property_name_ptr,
            property_name_len,
        ))
    };

    // Extract module name from the namespace object's first field
    let module_name = unsafe { get_module_name_from_namespace(namespace_obj) };

    // Check for known constant properties first
    if let Some(val) =
        unsafe { get_native_module_constant(module_name, property_name, namespace_obj) }
    {
        return val;
    }

    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }

    // Not a constant or JS-backed property. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if !is_native_module_callable_export(module_name, property_name) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let heap_name = unsafe {
        let layout = std::alloc::Layout::from_size_align(property_name_len, 1).unwrap();
        let ptr = std::alloc::alloc(layout);
        std::ptr::copy_nonoverlapping(property_name_ptr, ptr, property_name_len);
        ptr
    };

    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, namespace_obj);
    crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, property_name_len as i64);
    set_bound_native_closure_name(closure, property_name);

    crate::value::js_nanbox_pointer(closure as i64)
}

/// Build a "bound method" closure for `obj.method` PropertyGet on a known class
/// instance. The captures (instance, method_name_ptr, method_name_len) drive
/// `dispatch_bound_method` (closure.rs), which calls `js_native_call_method`
/// — that resolves the method through `CLASS_VTABLE_REGISTRY` for any class
/// registered by `js_register_class_method` at module init.
///
/// Issue #446: previously a class method reference (`let f = obj.method`,
/// `typeof obj.method`, `arr.map(obj.method)`) silently lowered to the
/// generic property-bag lookup, which doesn't store prototype methods —
/// every such read returned `undefined`, so `typeof obj.method === "undefined"`
/// and a captured method ran no body when invoked.
///
/// Method-name pointer is expected to be stable for the closure's lifetime;
/// codegen emits it from the per-module `.str.N.bytes` rodata global.
#[no_mangle]
pub extern "C" fn js_class_method_bind(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, instance);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_name_ptr as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_name_len as i64);
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            set_bound_native_closure_name(closure, name);
        }
    }
    crate::value::js_nanbox_pointer(closure as i64)
}

pub(crate) fn class_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
}

pub(crate) unsafe fn metadata_key_to_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}

pub(crate) fn class_has_own_method(class_id: u32, method_name: &str) -> bool {
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .and_then(|reg| reg.get(&class_id))
        .map(|vtable| vtable.methods.contains_key(method_name))
        .unwrap_or(false)
}

pub fn class_prototype_method_value_for_name(class_id: u32, method_name: &str) -> f64 {
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(bits) = cache.get(&(class_id, method_name.to_string())).copied() {
            return f64::from_bits(bits);
        }

        // Bounded leak: `js_class_method_bind` keeps the byte pointer for the
        // lifetime of the bound closure (it's stashed inside the closure's
        // capture frame). We leak one allocation per unique
        // `(class_id, method_name)` pair the program ever asks for, so the
        // total leak is bounded by the static set of decorated method
        // descriptors. The cache below short-circuits repeat queries.
        let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
        let class_bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64 & 0xFFFF_FFFF);
        let class_ref = f64::from_bits(class_bits);
        let value = js_class_method_bind(class_ref, leaked.as_ptr(), leaked.len());
        cache.insert((class_id, method_name.to_string()), value.to_bits());
        value
    })
}

#[no_mangle]
pub extern "C" fn js_class_prototype_method_value(class_ref: f64, method_key: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let method_name = unsafe { metadata_key_to_string(method_key) };
    let Some(method_name) = method_name else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_prototype_method_value_for_name(class_id, &method_name)
}

/// Extract the module name string from a native module namespace object.
pub(crate) unsafe fn get_module_name_from_namespace(namespace_obj: f64) -> &'static str {
    let jsval = JSValue::from_bits(namespace_obj.to_bits());
    if !jsval.is_pointer() {
        return "";
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x100000 {
        return "";
    }
    let module_field = js_object_get_field(obj as *mut _, 0);
    if !module_field.is_any_string() {
        return "";
    }
    // #1781: SSO-aware — ≤5-byte module names (fs, os, …) arrive as
    // SHORT_STRING_TAG values; route through `js_get_string_pointer_unified`
    // so SSO materializes onto the GC-managed heap (where its bytes
    // share the lifetime story the STRING_TAG path already assumes
    // for the `&'static` lie this signature carries).
    let module_f64 = f64::from_bits(module_field.bits());
    let str_ptr =
        crate::value::js_get_string_pointer_unified(module_f64) as *const crate::StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return "";
    }
    let len = (*str_ptr).byte_len as usize;
    let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
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

    // Issue #649: `os.constants.signals.SIGINT`, `os.constants.errno.ENOENT`,
    // `os.constants.priority.PRIORITY_NORMAL`, `os.constants.dlopen.RTLD_LAZY`
    // are ubiquitous in Node ecosystem code. Pre-fix every read returned
    // undefined. Use `libc::*` on Unix for byte-identical parity with Node.
    let os_signal_const = |prop: &str| -> Option<f64> {
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
    };

    let os_errno_const = |prop: &str| -> Option<f64> {
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
    };

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
    let crypto_const = |prop: &str| -> Option<f64> {
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
    };

    // `zlib.constants` — the Z_*/DEFLATE/INFLATE/GZIP/BROTLI_*/ZSTD_*
    // table Node exposes on `require('node:zlib').constants`. Match the
    // JavaScript-visible table rather than blindly mirroring every zlib.h
    // macro: modern Node exposes ZLIB_VERNUM but omits Z_TREES.
    // Required by axios for its stream wiring.
    let zlib_const = |prop: &str| -> Option<f64> {
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
            _ => return None,
        };
        Some(v as f64)
    };

    // `http2.constants` — the subset of Node's `require('node:http2').constants`
    // that real code reads: the `:`-prefixed pseudo-header names, the common
    // header-name string constants, the NGHTTP2 error/session codes, and the
    // HTTP_STATUS_* numbers. `@hono/node-server` reaches for these by name
    // (#1651). Mixed string/number values, so this closure returns the f64
    // directly rather than going through the `i64 as f64` shape used above.
    let http2_const = |prop: &str| -> Option<f64> {
        Some(match prop {
            // Pseudo-headers (HTTP/2 request/response framing).
            "HTTP2_HEADER_STATUS" => str_val(":status"),
            "HTTP2_HEADER_METHOD" => str_val(":method"),
            "HTTP2_HEADER_AUTHORITY" => str_val(":authority"),
            "HTTP2_HEADER_SCHEME" => str_val(":scheme"),
            "HTTP2_HEADER_PATH" => str_val(":path"),
            "HTTP2_HEADER_PROTOCOL" => str_val(":protocol"),
            // Common header-name constants.
            "HTTP2_HEADER_ACCEPT" => str_val("accept"),
            "HTTP2_HEADER_ACCEPT_ENCODING" => str_val("accept-encoding"),
            "HTTP2_HEADER_AUTHORIZATION" => str_val("authorization"),
            "HTTP2_HEADER_CACHE_CONTROL" => str_val("cache-control"),
            "HTTP2_HEADER_CONNECTION" => str_val("connection"),
            "HTTP2_HEADER_CONTENT_ENCODING" => str_val("content-encoding"),
            "HTTP2_HEADER_CONTENT_LENGTH" => str_val("content-length"),
            "HTTP2_HEADER_CONTENT_TYPE" => str_val("content-type"),
            "HTTP2_HEADER_COOKIE" => str_val("cookie"),
            "HTTP2_HEADER_DATE" => str_val("date"),
            "HTTP2_HEADER_ETAG" => str_val("etag"),
            "HTTP2_HEADER_HOST" => str_val("host"),
            "HTTP2_HEADER_LOCATION" => str_val("location"),
            "HTTP2_HEADER_SET_COOKIE" => str_val("set-cookie"),
            "HTTP2_HEADER_USER_AGENT" => str_val("user-agent"),
            // Default HPACK dynamic-table / frame sizes.
            "DEFAULT_SETTINGS_HEADER_TABLE_SIZE" => 4096.0,
            "DEFAULT_SETTINGS_ENABLE_PUSH" => 1.0,
            "DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE" => 65535.0,
            "DEFAULT_SETTINGS_MAX_FRAME_SIZE" => 16384.0,
            // NGHTTP2 error codes (RST_STREAM / GOAWAY).
            "NGHTTP2_NO_ERROR" => 0.0,
            "NGHTTP2_PROTOCOL_ERROR" => 1.0,
            "NGHTTP2_INTERNAL_ERROR" => 2.0,
            "NGHTTP2_FLOW_CONTROL_ERROR" => 3.0,
            "NGHTTP2_SETTINGS_TIMEOUT" => 4.0,
            "NGHTTP2_STREAM_CLOSED" => 5.0,
            "NGHTTP2_FRAME_SIZE_ERROR" => 6.0,
            "NGHTTP2_REFUSED_STREAM" => 7.0,
            "NGHTTP2_CANCEL" => 8.0,
            "NGHTTP2_COMPRESSION_ERROR" => 9.0,
            "NGHTTP2_CONNECT_ERROR" => 10.0,
            "NGHTTP2_ENHANCE_YOUR_CALM" => 11.0,
            "NGHTTP2_INADEQUATE_SECURITY" => 12.0,
            "NGHTTP2_HTTP_1_1_REQUIRED" => 13.0,
            // Session/flag constants.
            "NGHTTP2_SESSION_SERVER" => 0.0,
            "NGHTTP2_SESSION_CLIENT" => 1.0,
            "NGHTTP2_FLAG_NONE" => 0.0,
            "NGHTTP2_FLAG_END_STREAM" => 1.0,
            "NGHTTP2_FLAG_END_HEADERS" => 4.0,
            "NGHTTP2_FLAG_ACK" => 1.0,
            // The HTTP_STATUS_* numbers code commonly branches on.
            "HTTP_STATUS_OK" => 200.0,
            "HTTP_STATUS_CREATED" => 201.0,
            "HTTP_STATUS_ACCEPTED" => 202.0,
            "HTTP_STATUS_NO_CONTENT" => 204.0,
            "HTTP_STATUS_NOT_MODIFIED" => 304.0,
            "HTTP_STATUS_BAD_REQUEST" => 400.0,
            "HTTP_STATUS_UNAUTHORIZED" => 401.0,
            "HTTP_STATUS_FORBIDDEN" => 403.0,
            "HTTP_STATUS_NOT_FOUND" => 404.0,
            "HTTP_STATUS_METHOD_NOT_ALLOWED" => 405.0,
            "HTTP_STATUS_INTERNAL_SERVER_ERROR" => 500.0,
            "HTTP_STATUS_NOT_IMPLEMENTED" => 501.0,
            "HTTP_STATUS_BAD_GATEWAY" => 502.0,
            "HTTP_STATUS_SERVICE_UNAVAILABLE" => 503.0,
            _ => return None,
        })
    };

    match module_name {
        // node:punycode (deprecated, #2513) — the bundled punycode.js version
        // and the `ucs2` code-point helper sub-namespace (#2607).
        "punycode" => match property {
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
        "constants" => fs_const(property)
            .or_else(|| os_signal_const(property))
            .or_else(|| os_errno_const(property))
            .or_else(|| os_priority_const(property))
            .or_else(|| os_dlopen_const(property))
            .or_else(|| crypto_const(property)),
        "path" => match property {
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
            "posix" => Some(create_sub_namespace("path.posix")),
            "win32" => Some(create_sub_namespace("path.win32")),
            _ => None,
        },
        "path.posix" => match property {
            "sep" => Some(str_val("/")),
            "delimiter" => Some(str_val(":")),
            "toNamespacedPath" | "_makeLong" => Some(bound_native_callable_export_value(
                "path.posix",
                "toNamespacedPath",
            )),
            "posix" => Some(native_namespace_or_create("path.posix", namespace_obj)),
            "win32" => Some(create_sub_namespace("path.win32")),
            _ => None,
        },
        "path.win32" => match property {
            "sep" => Some(str_val("\\")),
            "delimiter" => Some(str_val(";")),
            "toNamespacedPath" | "_makeLong" => Some(bound_native_callable_export_value(
                "path.win32",
                "toNamespacedPath",
            )),
            "posix" => Some(create_sub_namespace("path.posix")),
            "win32" => Some(native_namespace_or_create("path.win32", namespace_obj)),
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
            _ => fs_const(property),
        },
        "fs.constants" => fs_const(property),
        "buffer" => match property {
            "Buffer" => Some(buffer_constructor_value()),
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
                &OS_CONSTANTS_CACHE,
            )),
            _ => None,
        },
        "os.constants" => match property {
            "signals" => Some(create_cached_sub_namespace(
                "os.constants.signals",
                &OS_CONSTANTS_SIGNALS_CACHE,
            )),
            "errno" => Some(create_cached_sub_namespace(
                "os.constants.errno",
                &OS_CONSTANTS_ERRNO_CACHE,
            )),
            "priority" => Some(create_cached_sub_namespace(
                "os.constants.priority",
                &OS_CONSTANTS_PRIORITY_CACHE,
            )),
            "dlopen" => Some(create_cached_sub_namespace(
                "os.constants.dlopen",
                &OS_CONSTANTS_DLOPEN_CACHE,
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
            "default" => Some(native_namespace_or_create("util", namespace_obj)),
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
        "crypto" => match property {
            "constants" => Some(create_sub_namespace("crypto.constants")),
            "Certificate" => Some(create_sub_namespace("crypto.Certificate")),
            // #1366: `crypto.subtle` is the WebCrypto SubtleCrypto
            // instance. Resolve to a sub-namespace so `typeof
            // crypto.subtle === "object"` matches Node and call
            // sites that read `subtle` as a value (e.g.
            // `const s = crypto.subtle; s.digest(...)`) get an
            // object. The actual `subtle.<method>(...)` lowering
            // is handled statically by HIR (see
            // `lower/expr_call/nested_namespace.rs`).
            "subtle" => Some(create_sub_namespace("crypto.subtle")),
            _ => None,
        },
        "crypto.constants" => crypto_const(property),
        "events" => match property {
            "defaultMaxListeners" => Some(10.0),
            "captureRejections" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "errorMonitor" => Some(crate::symbol::js_symbol_for(str_val("events.errorMonitor"))),
            "captureRejectionSymbol" => {
                Some(crate::symbol::js_symbol_for(str_val("nodejs.rejection")))
            }
            _ => None,
        },
        // node:worker_threads value-shaped exports. Perry doesn't spawn JS
        // workers, so the main thread is the only thread — `isMainThread`
        // is always true, `threadId` is 0, `resourceLimits` is empty.
        // Pre-fix `const { isMainThread } = require('worker_threads')` read
        // `undefined`, which made the `if (!isMainThread) common.skip(...)`
        // guard Node uses in main-thread-only tests fire under Perry, so
        // ~8 process tests in the node-core radar (#2135) were "skipping"
        // when they should have been running. (#2135)
        "worker_threads" => match property {
            "isMainThread" => Some(f64::from_bits(JSValue::bool(true).bits())),
            "threadId" => Some(0.0),
            "resourceLimits" => {
                let obj = crate::object::js_object_alloc(0, 0);
                Some(crate::value::js_nanbox_pointer(obj as i64))
            }
            _ => None,
        },
        // `zlib.constants` and the top-level Z_*/DEFLATE/INFLATE shortcuts
        // Node also exposes directly on `require('node:zlib')`.
        "zlib" => match property {
            "constants" => Some(create_sub_namespace("zlib.constants")),
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
            _ => None,
        },
        // node:http2 — `constants` is a sub-namespace object (the spec exposes
        // it as a single frozen object, not loose top-level constants), so
        // `import { constants } from 'node:http2'` binds to a real object and
        // `constants.HTTP2_HEADER_PATH` resolves through `http2.constants`
        // below. The `Http2ServerRequest` / `Http2ServerResponse` /
        // `createSecureServer` exports are handled elsewhere (#1651).
        "http2" => match property {
            "constants" => Some(create_sub_namespace("http2.constants")),
            _ => None,
        },
        "http2.constants" => http2_const(property),
        // node:cluster — all property reads are static constants on the
        // primary process. The test fixture only exercises shape, never
        // forks a worker; the `fork` / `disconnect` / `setupPrimary` /
        // `setupMaster` / `Worker` callables are produced separately by
        // `is_native_module_callable_export` (bound-method closure path).
        "cluster" => match property {
            // Identity flags: we always identify as the primary
            // process. A future `cluster.fork` impl would need to flip
            // these in the spawned child.
            "isPrimary" | "isMaster" => Some(f64::from_bits(JSValue::bool(true).bits())),
            "isWorker" => Some(f64::from_bits(JSValue::bool(false).bits())),
            // No active worker on the primary side.
            "worker" => Some(f64::from_bits(JSValue::undefined().bits())),
            // Empty registries — each read allocates a fresh empty
            // object (the test only reads them once, so the allocation
            // churn is irrelevant).
            "workers" | "settings" => {
                let obj = unsafe { js_object_alloc(0, 0) };
                Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
            }
            // SCHED_RR is the cross-platform default (port-based on
            // Linux/macOS, manual scheduling on Windows). `SCHED_NONE`
            // is 1, `SCHED_RR` is 2; `schedulingPolicy` defaults to RR.
            "schedulingPolicy" | "SCHED_RR" => Some(2.0),
            "SCHED_NONE" => Some(1.0),
            // EventEmitter methods on the cluster module aren't named
            // exports — Node's namespace import reads them as
            // `undefined`. We register them in the api-manifest so the
            // #463 gate doesn't reject the typeof read at compile time;
            // here we resolve them to undefined at runtime.
            "on" | "addListener" => Some(f64::from_bits(JSValue::undefined().bits())),
            _ => None,
        },
        // #1336: Histograms returned by perf_hooks.monitorEventLoopDelay /
        // .createHistogram expose numeric stats via property read. Perry's
        // stub doesn't record samples so every accessor reads 0; `exceeds`
        // and `count` matter for code that branches on counts before
        // computing averages.
        "perf_histogram" => match property {
            "mean" | "min" | "max" | "stddev" | "exceeds" | "count" => Some(0.0),
            "percentiles" | "percentilesBigInt" => {
                let obj = unsafe { js_object_alloc(0, 0) };
                Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Create a NativeModuleRef sub-namespace (e.g. "fs.constants", "path.posix").
/// The compiled code treats the result as another NativeModuleRef, so chained
/// property accesses like `fs.constants.O_RDONLY` work through the dispatch table.
fn create_sub_namespace(name: &str) -> f64 {
    js_create_native_module_namespace(name.as_ptr(), name.len())
}

fn native_namespace_or_create(module_name: &str, namespace_obj: f64) -> f64 {
    let value = JSValue::from_bits(namespace_obj.to_bits());
    if value.is_pointer() {
        let obj = value.as_pointer::<ObjectHeader>();
        if !obj.is_null() {
            let is_matching_namespace = unsafe {
                (*obj).class_id == NATIVE_MODULE_CLASS_ID
                    && read_native_module_name(obj).as_deref() == Some(module_name)
            };
            if is_matching_namespace {
                return namespace_obj;
            }
        }
    }
    js_create_native_module_namespace(module_name.as_ptr(), module_name.len())
}

fn create_cached_sub_namespace(name: &str, cache: &std::sync::atomic::AtomicU64) -> f64 {
    let cached = cache.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    let result = create_sub_namespace(name);
    // GC_STORE_AUDIT(ROOT): os constants caches are mutable roots visited by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(cache, result.to_bits(), Ordering::Relaxed);
    result
}

/// Issue #912 (#909 follow-up): cached `http.METHODS` array. Matches
/// Node 22's exposed list (alphabetically sorted, derived from llhttp's
/// HTTP method table). The array is allocated in the longlived arena so
/// it survives every GC sweep — the cached pointer is shared across
/// every `http.METHODS` / `https.METHODS` / `http2.METHODS` read.
unsafe fn http_methods_array() -> f64 {
    let cached = HTTP_METHODS_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    // Node 22 `require('node:http').METHODS` snapshot.
    const METHODS: &[&str] = &[
        "ACL",
        "BIND",
        "CHECKOUT",
        "CONNECT",
        "COPY",
        "DELETE",
        "GET",
        "HEAD",
        "LINK",
        "LOCK",
        "M-SEARCH",
        "MERGE",
        "MKACTIVITY",
        "MKCALENDAR",
        "MKCOL",
        "MOVE",
        "NOTIFY",
        "OPTIONS",
        "PATCH",
        "POST",
        "PROPFIND",
        "PROPPATCH",
        "PURGE",
        "PUT",
        "QUERY",
        "REBIND",
        "REPORT",
        "SEARCH",
        "SOURCE",
        "SUBSCRIBE",
        "TRACE",
        "UNBIND",
        "UNLINK",
        "UNLOCK",
        "UNSUBSCRIBE",
    ];
    let arr = crate::array::js_array_alloc_with_length_longlived(METHODS.len() as u32);
    let elements_ptr = (arr as *mut u8).add(8) as *mut f64;
    for (i, m) in METHODS.iter().enumerate() {
        let bytes = m.as_bytes();
        let str_ptr =
            crate::string::js_string_from_bytes_longlived(bytes.as_ptr(), bytes.len() as u32);
        let nanboxed = f64::from_bits(
            crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
        );
        *elements_ptr.add(i) = nanboxed;
        crate::array::note_array_slot_layout_only(arr, i, nanboxed.to_bits());
    }
    let value = crate::value::js_nanbox_pointer(arr as i64);
    // GC_STORE_AUDIT(ROOT): HTTP_METHODS_CACHE is a mutable root visited by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &HTTP_METHODS_CACHE,
        value.to_bits(),
        Ordering::Relaxed,
    );
    value
}

/// Create (and cache) the fs.constants object with POSIX file system constants.
// #854: fs.constants object builder retained for the native fs module
#[allow(dead_code)]
unsafe fn create_fs_constants_object() -> f64 {
    let cached = FS_CONSTANTS_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    // POSIX file-access/open/copy/mode constants mirrored from Node's
    // fs.constants surface. Keep this in sync with `fs_const` above so
    // both `fs.constants.X` and destructured constant reads agree.
    let field_names: &[&str] = &[
        "F_OK",
        "R_OK",
        "W_OK",
        "X_OK",
        "O_RDONLY",
        "O_WRONLY",
        "O_RDWR",
        "O_NOFOLLOW",
        "O_CREAT",
        "O_TRUNC",
        "O_APPEND",
        "O_EXCL",
        "COPYFILE_EXCL",
        "COPYFILE_FICLONE",
        "COPYFILE_FICLONE_FORCE",
        "S_IRUSR",
        "S_IWUSR",
        "S_IXUSR",
        "S_IRGRP",
        "S_IWGRP",
        "S_IXGRP",
        "S_IROTH",
        "S_IWOTH",
        "S_IXOTH",
    ];
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
    let field_values: &[f64] = &[
        0.0,
        4.0,
        2.0,
        1.0, // F_OK, R_OK, W_OK, X_OK
        0.0,
        1.0,
        2.0,          // O_RDONLY, O_WRONLY, O_RDWR
        o_nofollow,   // O_NOFOLLOW
        0x200 as f64, // O_CREAT
        0x400 as f64, // O_TRUNC
        0x8 as f64,   // O_APPEND
        0x800 as f64, // O_EXCL
        1.0,
        2.0,
        4.0, // COPYFILE_*
        0o400 as f64,
        0o200 as f64,
        0o100 as f64, // S_I*USR
        0o040 as f64,
        0o020 as f64,
        0o010 as f64, // S_I*GRP
        0o004 as f64,
        0o002 as f64,
        0o001 as f64, // S_I*OTH
    ];

    // Build null-separated packed keys: "F_OK\0R_OK\0..."
    let packed = field_names.join("\0");
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF01, // unique shape_id for fs.constants
        field_names.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );

    for (i, &val) in field_values.iter().enumerate() {
        js_object_set_field(obj, i as u32, JSValue::number(val));
    }

    let result = crate::value::js_nanbox_pointer(obj as i64);
    // GC_STORE_AUDIT(ROOT): FS_CONSTANTS_CACHE is a mutable root visited by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &FS_CONSTANTS_CACHE,
        result.to_bits(),
        Ordering::Relaxed,
    );
    result
}
