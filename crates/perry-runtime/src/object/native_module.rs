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
    static NATIVE_MODULE_ACCESSOR_EXPORTS: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    static HANDLE_PROPERTY_BIND_REENTRY: Cell<bool> = const { Cell::new(false) };
    static BUFFER_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    static SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    static SQLITE_SESSION_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    static UTIL_INSPECT_DEFAULT_OPTIONS: Cell<u64> = const { Cell::new(0) };
    static UTIL_INSPECT_STYLES: Cell<u64> = const { Cell::new(0) };
    static UTIL_INSPECT_COLORS: Cell<u64> = const { Cell::new(0) };
    static TIMERS_PROMISES_PARENT_NAMESPACE: Cell<u64> = const { Cell::new(0) };
    static ZLIB_CODES_OBJECT: Cell<u64> = const { Cell::new(0) };
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
    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|cache| {
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
    SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    SQLITE_SESSION_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_STYLES.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_COLORS.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    TIMERS_PROMISES_PARENT_NAMESPACE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    ZLIB_CODES_OBJECT.with(|slot| {
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
const WORKER_THREADS_LOCK_MANAGER_CLASS_ID: u32 = 0xFFFF_00B1;

static BUFFER_POOL_SIZE_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(8192f64.to_bits());

pub(crate) fn buffer_pool_size() -> f64 {
    f64::from_bits(BUFFER_POOL_SIZE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn set_buffer_pool_size(value: f64) {
    BUFFER_POOL_SIZE_BITS.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn worker_threads_locks_value() -> f64 {
    let name = "LockManager";
    unsafe {
        js_register_class_id(WORKER_THREADS_LOCK_MANAGER_CLASS_ID);
        js_register_class_name(
            WORKER_THREADS_LOCK_MANAGER_CLASS_ID,
            name.as_ptr(),
            name.len() as u32,
        );
    }
    let obj = js_object_alloc(WORKER_THREADS_LOCK_MANAGER_CLASS_ID, 0);
    crate::value::js_nanbox_pointer(obj as i64)
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
    let module_name = module_name.strip_prefix("node:").unwrap_or(module_name);
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

// #3677: `Object.keys(zlib.constants)` enumeration. Node exposes the full
// Z_*/BROTLI_*/ZSTD_* table as enumerable own keys (170 keys). Every key here
// is backed by a value in `zlib_const` (the value-read dispatch), so
// enumeration and direct reads agree. Order matches Node's insertion order.
const ZLIB_CONSTANTS_KEYS: &[&[u8]] = &[
    b"Z_NO_FLUSH",
    b"Z_PARTIAL_FLUSH",
    b"Z_SYNC_FLUSH",
    b"Z_FULL_FLUSH",
    b"Z_FINISH",
    b"Z_BLOCK",
    b"Z_OK",
    b"Z_STREAM_END",
    b"Z_NEED_DICT",
    b"Z_ERRNO",
    b"Z_STREAM_ERROR",
    b"Z_DATA_ERROR",
    b"Z_MEM_ERROR",
    b"Z_BUF_ERROR",
    b"Z_VERSION_ERROR",
    b"Z_NO_COMPRESSION",
    b"Z_BEST_SPEED",
    b"Z_BEST_COMPRESSION",
    b"Z_DEFAULT_COMPRESSION",
    b"Z_FILTERED",
    b"Z_HUFFMAN_ONLY",
    b"Z_RLE",
    b"Z_FIXED",
    b"Z_DEFAULT_STRATEGY",
    b"ZLIB_VERNUM",
    b"DEFLATE",
    b"INFLATE",
    b"GZIP",
    b"GUNZIP",
    b"DEFLATERAW",
    b"INFLATERAW",
    b"UNZIP",
    b"BROTLI_DECODE",
    b"BROTLI_ENCODE",
    b"ZSTD_DECOMPRESS",
    b"ZSTD_COMPRESS",
    b"Z_MIN_WINDOWBITS",
    b"Z_MAX_WINDOWBITS",
    b"Z_DEFAULT_WINDOWBITS",
    b"Z_MIN_CHUNK",
    b"Z_MAX_CHUNK",
    b"Z_DEFAULT_CHUNK",
    b"Z_MIN_MEMLEVEL",
    b"Z_MAX_MEMLEVEL",
    b"Z_DEFAULT_MEMLEVEL",
    b"Z_MIN_LEVEL",
    b"Z_MAX_LEVEL",
    b"Z_DEFAULT_LEVEL",
    b"BROTLI_OPERATION_PROCESS",
    b"BROTLI_OPERATION_FLUSH",
    b"BROTLI_OPERATION_FINISH",
    b"BROTLI_OPERATION_EMIT_METADATA",
    b"BROTLI_PARAM_MODE",
    b"BROTLI_MODE_GENERIC",
    b"BROTLI_MODE_TEXT",
    b"BROTLI_MODE_FONT",
    b"BROTLI_DEFAULT_MODE",
    b"BROTLI_PARAM_QUALITY",
    b"BROTLI_MIN_QUALITY",
    b"BROTLI_MAX_QUALITY",
    b"BROTLI_DEFAULT_QUALITY",
    b"BROTLI_PARAM_LGWIN",
    b"BROTLI_MIN_WINDOW_BITS",
    b"BROTLI_MAX_WINDOW_BITS",
    b"BROTLI_LARGE_MAX_WINDOW_BITS",
    b"BROTLI_DEFAULT_WINDOW",
    b"BROTLI_PARAM_LGBLOCK",
    b"BROTLI_MIN_INPUT_BLOCK_BITS",
    b"BROTLI_MAX_INPUT_BLOCK_BITS",
    b"BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING",
    b"BROTLI_PARAM_SIZE_HINT",
    b"BROTLI_PARAM_LARGE_WINDOW",
    b"BROTLI_PARAM_NPOSTFIX",
    b"BROTLI_PARAM_NDIRECT",
    b"BROTLI_DECODER_RESULT_ERROR",
    b"BROTLI_DECODER_RESULT_SUCCESS",
    b"BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT",
    b"BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT",
    b"BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION",
    b"BROTLI_DECODER_PARAM_LARGE_WINDOW",
    b"BROTLI_DECODER_NO_ERROR",
    b"BROTLI_DECODER_SUCCESS",
    b"BROTLI_DECODER_NEEDS_MORE_INPUT",
    b"BROTLI_DECODER_NEEDS_MORE_OUTPUT",
    b"BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE",
    b"BROTLI_DECODER_ERROR_FORMAT_RESERVED",
    b"BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE",
    b"BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET",
    b"BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME",
    b"BROTLI_DECODER_ERROR_FORMAT_CL_SPACE",
    b"BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE",
    b"BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT",
    b"BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1",
    b"BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2",
    b"BROTLI_DECODER_ERROR_FORMAT_TRANSFORM",
    b"BROTLI_DECODER_ERROR_FORMAT_DICTIONARY",
    b"BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS",
    b"BROTLI_DECODER_ERROR_FORMAT_PADDING_1",
    b"BROTLI_DECODER_ERROR_FORMAT_PADDING_2",
    b"BROTLI_DECODER_ERROR_FORMAT_DISTANCE",
    b"BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET",
    b"BROTLI_DECODER_ERROR_INVALID_ARGUMENTS",
    b"BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES",
    b"BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS",
    b"BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP",
    b"BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1",
    b"BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2",
    b"BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES",
    b"BROTLI_DECODER_ERROR_UNREACHABLE",
    b"ZSTD_e_continue",
    b"ZSTD_e_flush",
    b"ZSTD_e_end",
    b"ZSTD_fast",
    b"ZSTD_dfast",
    b"ZSTD_greedy",
    b"ZSTD_lazy",
    b"ZSTD_lazy2",
    b"ZSTD_btlazy2",
    b"ZSTD_btopt",
    b"ZSTD_btultra",
    b"ZSTD_btultra2",
    b"ZSTD_c_compressionLevel",
    b"ZSTD_c_windowLog",
    b"ZSTD_c_hashLog",
    b"ZSTD_c_chainLog",
    b"ZSTD_c_searchLog",
    b"ZSTD_c_minMatch",
    b"ZSTD_c_targetLength",
    b"ZSTD_c_strategy",
    b"ZSTD_c_enableLongDistanceMatching",
    b"ZSTD_c_ldmHashLog",
    b"ZSTD_c_ldmMinMatch",
    b"ZSTD_c_ldmBucketSizeLog",
    b"ZSTD_c_ldmHashRateLog",
    b"ZSTD_c_contentSizeFlag",
    b"ZSTD_c_checksumFlag",
    b"ZSTD_c_dictIDFlag",
    b"ZSTD_c_nbWorkers",
    b"ZSTD_c_jobSize",
    b"ZSTD_c_overlapLog",
    b"ZSTD_d_windowLogMax",
    b"ZSTD_CLEVEL_DEFAULT",
    b"ZSTD_error_no_error",
    b"ZSTD_error_GENERIC",
    b"ZSTD_error_prefix_unknown",
    b"ZSTD_error_version_unsupported",
    b"ZSTD_error_frameParameter_unsupported",
    b"ZSTD_error_frameParameter_windowTooLarge",
    b"ZSTD_error_corruption_detected",
    b"ZSTD_error_checksum_wrong",
    b"ZSTD_error_literals_headerWrong",
    b"ZSTD_error_dictionary_corrupted",
    b"ZSTD_error_dictionary_wrong",
    b"ZSTD_error_dictionaryCreation_failed",
    b"ZSTD_error_parameter_unsupported",
    b"ZSTD_error_parameter_combination_unsupported",
    b"ZSTD_error_parameter_outOfBound",
    b"ZSTD_error_tableLog_tooLarge",
    b"ZSTD_error_maxSymbolValue_tooLarge",
    b"ZSTD_error_maxSymbolValue_tooSmall",
    b"ZSTD_error_stabilityCondition_notRespected",
    b"ZSTD_error_stage_wrong",
    b"ZSTD_error_init_missing",
    b"ZSTD_error_memory_allocation",
    b"ZSTD_error_workSpace_tooSmall",
    b"ZSTD_error_dstSize_tooSmall",
    b"ZSTD_error_srcSize_wrong",
    b"ZSTD_error_dstBuffer_null",
    b"ZSTD_error_noForwardProgress_destFull",
    b"ZSTD_error_noForwardProgress_inputEmpty",
];

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
    // #3683: POSIX file-flag, libuv, and default-cipher-metadata tail.
    b"UV_DIRENT_UNKNOWN",
    b"UV_DIRENT_FILE",
    b"UV_DIRENT_DIR",
    b"UV_DIRENT_LINK",
    b"UV_DIRENT_FIFO",
    b"UV_DIRENT_SOCKET",
    b"UV_DIRENT_CHAR",
    b"UV_DIRENT_BLOCK",
    b"UV_FS_SYMLINK_DIR",
    b"UV_FS_SYMLINK_JUNCTION",
    b"UV_FS_O_FILEMAP",
    b"UV_FS_COPYFILE_EXCL",
    b"UV_FS_COPYFILE_FICLONE",
    b"UV_FS_COPYFILE_FICLONE_FORCE",
    b"S_IFMT",
    b"S_IFREG",
    b"S_IFDIR",
    b"S_IFCHR",
    b"S_IFBLK",
    b"S_IFIFO",
    b"S_IFLNK",
    b"S_IFSOCK",
    b"S_IRWXU",
    b"S_IRWXG",
    b"S_IRWXO",
    b"O_DIRECTORY",
    b"O_NOCTTY",
    b"O_NONBLOCK",
    b"O_SYNC",
    b"O_DSYNC",
    b"defaultCoreCipherList",
];

const ASYNC_HOOKS_DEFAULT_KEYS: &[&[u8]] = &[
    b"AsyncLocalStorage",
    b"createHook",
    b"executionAsyncId",
    b"triggerAsyncId",
    b"executionAsyncResource",
    b"asyncWrapProviders",
    b"AsyncResource",
];

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

const BUFFER_NAMESPACE_KEYS: &[&[u8]] = &[
    b"Buffer",
    b"transcode",
    b"isUtf8",
    b"isAscii",
    b"kMaxLength",
    b"kStringMaxLength",
    b"btoa",
    b"atob",
    b"constants",
    b"INSPECT_MAX_BYTES",
    b"Blob",
    b"resolveObjectURL",
    b"File",
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

const OS_DEFAULT_KEYS: &[&[u8]] = &[
    b"arch",
    b"availableParallelism",
    b"cpus",
    b"endianness",
    b"freemem",
    b"getPriority",
    b"homedir",
    b"hostname",
    b"loadavg",
    b"networkInterfaces",
    b"platform",
    b"release",
    b"setPriority",
    b"tmpdir",
    b"totalmem",
    b"type",
    b"userInfo",
    b"uptime",
    b"version",
    b"machine",
    b"constants",
    b"EOL",
    b"devNull",
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

const PATH_DEFAULT_KEYS: &[&[u8]] = &[
    b"resolve",
    b"normalize",
    b"isAbsolute",
    b"join",
    b"relative",
    b"toNamespacedPath",
    b"dirname",
    b"basename",
    b"extname",
    b"format",
    b"parse",
    b"matchesGlob",
    b"sep",
    b"delimiter",
    b"win32",
    b"posix",
    b"_makeLong",
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

const QUERYSTRING_DEFAULT_KEYS: &[&[u8]] = &[
    b"unescapeBuffer",
    b"unescape",
    b"escape",
    b"stringify",
    b"encode",
    b"parse",
    b"decode",
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

const FS_NAMESPACE_KEYS: &[&[u8]] = &[
    b"_toUnixTimestamp",
    b"access",
    b"accessSync",
    b"appendFile",
    b"appendFileSync",
    b"chmod",
    b"chmodSync",
    b"chown",
    b"chownSync",
    b"close",
    b"closeSync",
    b"constants",
    b"copyFile",
    b"copyFileSync",
    b"cp",
    b"cpSync",
    b"createReadStream",
    b"createWriteStream",
    b"exists",
    b"existsSync",
    b"fchmod",
    b"fchmodSync",
    b"fchown",
    b"fchownSync",
    b"fdatasync",
    b"fdatasyncSync",
    b"fstat",
    b"fstatSync",
    b"fsync",
    b"fsyncSync",
    b"ftruncate",
    b"ftruncateSync",
    b"futimes",
    b"futimesSync",
    b"glob",
    b"globSync",
    b"lchmod",
    b"lchmodSync",
    b"lchown",
    b"lchownSync",
    b"link",
    b"linkSync",
    b"lstat",
    b"lstatSync",
    b"lutimes",
    b"lutimesSync",
    b"mkdir",
    b"mkdirSync",
    b"mkdtemp",
    b"mkdtempSync",
    b"open",
    b"openSync",
    b"opendir",
    b"opendirSync",
    b"promises",
    b"read",
    b"readFile",
    b"readFileSync",
    b"readSync",
    b"readdir",
    b"readdirSync",
    b"readlink",
    b"readlinkSync",
    b"readv",
    b"readvSync",
    b"realpath",
    b"realpathSync",
    b"rename",
    b"renameSync",
    b"rm",
    b"rmSync",
    b"rmdir",
    b"rmdirSync",
    b"stat",
    b"statSync",
    b"statfs",
    b"statfsSync",
    b"symlink",
    b"symlinkSync",
    b"truncate",
    b"truncateSync",
    b"unlink",
    b"unlinkSync",
    b"unwatchFile",
    b"utimes",
    b"utimesSync",
    b"watch",
    b"watchFile",
    b"write",
    b"writeFile",
    b"writeFileSync",
    b"writeSync",
    b"writev",
    b"writevSync",
];

const URL_DEFAULT_KEYS: &[&[u8]] = &[
    b"Url",
    b"parse",
    b"resolve",
    b"resolveObject",
    b"format",
    b"URL",
    b"URLSearchParams",
    b"domainToASCII",
    b"domainToUnicode",
    b"pathToFileURL",
    b"fileURLToPath",
    b"fileURLToPathBuffer",
    b"urlToHttpOptions",
];

const URL_NAMESPACE_KEYS: &[&[u8]] = &[
    b"URL",
    b"URLSearchParams",
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

const UTIL_DEFAULT_KEYS: &[&[u8]] = &[
    b"aborted",
    b"callbackify",
    b"convertProcessSignalToExitCode",
    b"debug",
    b"debuglog",
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
    b"TextDecoder",
    b"TextEncoder",
    b"transferableAbortController",
    b"transferableAbortSignal",
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

// Linux-only open() flags: Node only enumerates these on platforms whose libc
// defines them (e.g. `O_DIRECT`/`O_NOATIME` are absent on macOS), so gate the
// enumerable-key tail by target so `Object.keys(constants)` matches Node here.
#[cfg(target_os = "linux")]
fn deprecated_constants_keys() -> &'static [&'static [u8]] {
    use std::sync::OnceLock;
    static MERGED: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    MERGED
        .get_or_init(|| {
            let mut v: Vec<&'static [u8]> = Vec::with_capacity(DEPRECATED_CONSTANTS_KEYS.len() + 6);
            for &k in DEPRECATED_CONSTANTS_KEYS {
                if k == b"SIGCHLD" {
                    v.push(k);
                    v.push(b"SIGSTKFLT");
                    continue;
                }
                if k == b"SIGIO" {
                    v.push(k);
                    v.push(b"SIGPOLL");
                    v.push(b"SIGPWR");
                    continue;
                }
                if k == b"RTLD_LOCAL" {
                    v.push(k);
                    #[cfg(target_env = "gnu")]
                    v.push(b"RTLD_DEEPBIND");
                    continue;
                }
                if k == b"defaultCoreCipherList" {
                    v.push(b"O_DIRECT");
                    v.push(b"O_NOATIME");
                }
                v.push(k);
            }
            v
        })
        .as_slice()
}

#[cfg(target_os = "macos")]
fn deprecated_constants_keys() -> &'static [&'static [u8]] {
    use std::sync::OnceLock;
    static MERGED: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    MERGED
        .get_or_init(|| {
            let mut v: Vec<&'static [u8]> = Vec::with_capacity(DEPRECATED_CONSTANTS_KEYS.len() + 2);
            for &k in DEPRECATED_CONSTANTS_KEYS {
                if k == b"SIGSYS" {
                    v.push(k);
                    v.push(b"SIGINFO");
                    continue;
                }
                if k == b"defaultCoreCipherList" {
                    v.push(b"O_SYMLINK");
                }
                v.push(k);
            }
            v
        })
        .as_slice()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn deprecated_constants_keys() -> &'static [&'static [u8]] {
    DEPRECATED_CONSTANTS_KEYS
}

#[cfg(test)]
mod tests {
    use super::deprecated_constants_keys;

    #[test]
    fn rtld_deepbind_key_is_platform_gated() {
        let has_rtld_deepbind = deprecated_constants_keys()
            .iter()
            .any(|key| *key == b"RTLD_DEEPBIND");
        assert_eq!(
            has_rtld_deepbind,
            cfg!(all(target_os = "linux", target_env = "gnu"))
        );
    }
}

const FS_NAMESPACE_EXPORT_KEYS: &[&[u8]] = &[
    b"appendFile",
    b"appendFileSync",
    b"access",
    b"accessSync",
    b"chown",
    b"chownSync",
    b"chmod",
    b"chmodSync",
    b"close",
    b"closeSync",
    b"copyFile",
    b"copyFileSync",
    b"cp",
    b"cpSync",
    b"createReadStream",
    b"createWriteStream",
    b"exists",
    b"existsSync",
    b"fchown",
    b"fchownSync",
    b"fchmod",
    b"fchmodSync",
    b"fdatasync",
    b"fdatasyncSync",
    b"fstat",
    b"fstatSync",
    b"fsync",
    b"fsyncSync",
    b"ftruncate",
    b"ftruncateSync",
    b"futimes",
    b"futimesSync",
    b"glob",
    b"globSync",
    b"lchown",
    b"lchownSync",
    b"lchmod",
    b"lchmodSync",
    b"link",
    b"linkSync",
    b"lstat",
    b"lstatSync",
    b"lutimes",
    b"lutimesSync",
    b"mkdir",
    b"mkdirSync",
    b"mkdtemp",
    b"mkdtempDisposableSync",
    b"mkdtempSync",
    b"open",
    b"openAsBlob",
    b"openSync",
    b"readdir",
    b"readdirSync",
    b"read",
    b"readSync",
    b"readv",
    b"readvSync",
    b"readFile",
    b"readFileSync",
    b"readlink",
    b"readlinkSync",
    b"realpath",
    b"realpathSync",
    b"rename",
    b"renameSync",
    b"rm",
    b"rmSync",
    b"rmdir",
    b"rmdirSync",
    b"stat",
    b"statfs",
    b"statSync",
    b"statfsSync",
    b"symlink",
    b"symlinkSync",
    b"truncate",
    b"truncateSync",
    b"unwatchFile",
    b"unlink",
    b"unlinkSync",
    b"utimes",
    b"utimesSync",
    b"watch",
    b"watchFile",
    b"writeFile",
    b"writeFileSync",
    b"write",
    b"writeSync",
    b"writev",
    b"writevSync",
    b"Dirent",
    b"Stats",
    b"ReadStream",
    b"WriteStream",
    b"FileReadStream",
    b"FileWriteStream",
    b"Utf8Stream",
    b"_toUnixTimestamp",
    b"Dir",
    b"opendir",
    b"opendirSync",
    b"constants",
    b"promises",
];

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
        "fs" => Some(FS_NAMESPACE_EXPORT_KEYS),
        "async_hooks" => Some(ASYNC_HOOKS_NAMESPACE_KEYS),
        "async_hooks.default" => Some(ASYNC_HOOKS_DEFAULT_KEYS),
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
        "sqlite" => Some(&[
            b"DatabaseSync",
            b"Session",
            b"StatementSync",
            b"backup",
            b"constants",
            b"default",
        ]),
        "sqlite.constants" => Some(SQLITE_CONSTANTS_KEYS),
        "domain" => Some(&[b"_stack", b"Domain", b"createDomain", b"create", b"active"]),
        // #3677: zlib.constants enumerates the full Z_*/BROTLI_*/ZSTD_* table.
        "zlib.constants" => Some(ZLIB_CONSTANTS_KEYS),
        // Deprecated path alias enumerable on the top-level and style
        // sub-namespaces, matching Node's `Object.keys(...).includes`.
        "path" => Some(PATH_NAMESPACE_KEYS),
        "path.default" => Some(PATH_DEFAULT_KEYS),
        "path.posix" | "path.win32" => Some(&[b"_makeLong"]),
        "fs" => Some(FS_NAMESPACE_KEYS),
        "constants" => Some(deprecated_constants_keys()),
        "buffer" => Some(BUFFER_NAMESPACE_KEYS),
        "querystring" => Some(QUERYSTRING_NAMESPACE_KEYS),
        "querystring.default" => Some(QUERYSTRING_DEFAULT_KEYS),
        "timers" => Some(TIMERS_NAMESPACE_KEYS),
        "os" => Some(OS_NAMESPACE_KEYS),
        "os.default" => Some(OS_DEFAULT_KEYS),
        "url" => Some(URL_NAMESPACE_KEYS),
        "url.default" => Some(URL_DEFAULT_KEYS),
        "util" => Some(UTIL_NAMESPACE_KEYS),
        "util.default" => Some(UTIL_DEFAULT_KEYS),
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
            b"Stream",
            b"getDefaultAutoSelectFamily",
            b"setDefaultAutoSelectFamily",
            b"getDefaultAutoSelectFamilyAttemptTimeout",
            b"setDefaultAutoSelectFamilyAttemptTimeout",
        ]),
        "https" => Some(&[
            b"Agent",
            b"Server",
            b"createServer",
            b"get",
            b"request",
            b"globalAgent",
        ]),
        "events" => Some(EVENTS_NAMESPACE_KEYS),
        "timers" => Some(&[
            b"setTimeout",
            b"clearTimeout",
            b"setImmediate",
            b"clearImmediate",
            b"setInterval",
            b"clearInterval",
            b"promises",
        ]),
        "timers/promises" => Some(&[b"setTimeout", b"setImmediate", b"setInterval", b"scheduler"]),
        "zlib" => Some(&[b"codes"]),
        _ => None,
    }
}

pub(crate) fn native_module_has_enumerable_key(module_name: &str, key: &str) -> bool {
    native_module_enumerable_keys(module_name)
        .is_some_and(|keys| keys.iter().any(|candidate| *candidate == key.as_bytes()))
}

fn cjs_default_base_module(module_name: &str) -> Option<&'static str> {
    match module_name {
        "async_hooks.default" => Some("async_hooks"),
        "os.default" => Some("os"),
        "path.default" => Some("path"),
        "querystring.default" => Some("querystring"),
        "url.default" => Some("url"),
        "util.default" => Some("util"),
        _ => None,
    }
}

fn cjs_default_namespace_name(module_name: &str) -> Option<&'static str> {
    match module_name {
        "async_hooks" => Some("async_hooks.default"),
        "os" => Some("os.default"),
        "path" => Some("path.default"),
        "querystring" => Some("querystring.default"),
        "url" => Some("url.default"),
        "util" => Some("util.default"),
        _ => None,
    }
}

fn create_cjs_default_namespace(module_name: &str) -> Option<f64> {
    let name = cjs_default_namespace_name(module_name)?;
    Some(js_create_native_module_namespace(name.as_ptr(), name.len()))
}

fn cjs_default_export_value(module_name: &str) -> Option<f64> {
    match module_name {
        "events" => Some(bound_native_callable_export_value("events", "EventEmitter")),
        "async_hooks" | "os" | "path" | "querystring" | "url" | "util" => {
            create_cjs_default_namespace(module_name)
        }
        _ => None,
    }
}

fn canonical_native_callable_property<'a>(module_name: &str, property_name: &'a str) -> &'a str {
    match (module_name, property_name) {
        ("fs", "FileReadStream") => "ReadStream",
        ("fs", "FileWriteStream") => "WriteStream",
        ("path" | "path.posix" | "path.win32", "_makeLong") => "toNamespacedPath",
        ("querystring", "decode") => "parse",
        ("querystring", "encode") => "stringify",
        _ => property_name,
    }
}

fn should_cache_native_module_namespace(module_name: &str) -> bool {
    matches!(
        module_name,
        "assert/strict"
            | "async_hooks"
            | "async_hooks.default"
            | "constants"
            | "events"
            | "fs.constants"
            | "os"
            | "os.default"
            | "path"
            | "path.default"
            | "querystring"
            | "querystring.default"
            | "process"
            | "url"
            | "url.default"
            | "util"
            | "util.default"
            | "util.types"
            | "path.posix"
            | "path.win32"
            | "timers/promises"
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
    if module_name == "dns" && property_name == "promises" {
        crate::dns::dns_promises_init_servers_from_callback_if_unset();
        let submodule = "dns/promises";
        return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
    }

    if module_name == "util" && property_name == "debug" {
        return bound_native_callable_export_value("util", "debuglog");
    }

    // #3679: node:v8 lifecycle namespaces. `v8.startupSnapshot` /
    // `v8.promiseHooks` are object-valued exports; resolve them to
    // dedicated native-module namespace objects so `typeof === "object"` and
    // their methods dispatch through `dispatch_native_module_method`.
    if module_name == "v8" && matches!(property_name, "startupSnapshot" | "promiseHooks") {
        let submodule = if property_name == "startupSnapshot" {
            "v8.startupSnapshot"
        } else {
            "v8.promiseHooks"
        };
        return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
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
    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn bound_native_callable_export_value(module_name: &str, property_name: &str) -> f64 {
    let module_name = cjs_default_base_module(module_name).unwrap_or(module_name);
    let property_name = canonical_native_callable_property(module_name, property_name);
    let callable_module_name = if module_name == "util.types" {
        "util/types"
    } else {
        module_name
    };
    let key = format!("{callable_module_name}\0{property_name}");
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let method_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let ns = js_create_native_module_namespace(
        callable_module_name.as_ptr(),
        callable_module_name.len(),
    );
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, ns);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_bytes.len() as i64);
    let exposed_name = if module_name == "fs" {
        native_callable_export_display_name(module_name, property_name)
    } else if module_name == "url" && property_name == "resolveObject" {
        "urlResolveObject"
    } else if module_name == "fs" && property_name == "_toUnixTimestamp" {
        "toUnixTimestamp"
    } else {
        property_name
    };
    set_bound_native_closure_name(closure, exposed_name);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    let closure_addr = closure as usize;

    if module_name == "tty" && matches!(property_name, "ReadStream" | "WriteStream") {
        attach_tty_stream_prototype(value, property_name);
    }
    if module_name == "tls" && property_name == "SecureContext" {
        attach_tls_secure_context_prototype(value);
    }
    if module_name == "wasi" && property_name == "WASI" {
        crate::wasi::attach_wasi_constructor_prototype(value);
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
    if module_name == "sqlite" && property_name == "DatabaseSync" {
        attach_sqlite_database_sync_prototype(value);
    }
    if module_name == "sqlite" && property_name == "Session" {
        attach_sqlite_session_prototype(value);
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
        for method in [
            "addAbortListener",
            "once",
            "on",
            "getEventListeners",
            "getMaxListeners",
            "listenerCount",
            "setMaxListeners",
        ] {
            let method_value = bound_native_callable_export_value("events", method);
            crate::closure::closure_set_dynamic_prop(closure_addr, method, method_value);
        }
        crate::closure::closure_set_dynamic_prop(closure_addr, "EventEmitter", value);
        crate::closure::closure_set_dynamic_prop(closure_addr, "defaultMaxListeners", 10.0);
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "usingDomains",
            f64::from_bits(JSValue::bool(false).bits()),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "captureRejections",
            f64::from_bits(JSValue::bool(false).bits()),
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "captureRejectionSymbol", {
            let name = "nodejs.rejection";
            let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            unsafe { crate::symbol::js_symbol_for(f64::from_bits(JSValue::string_ptr(ptr).bits())) }
        });
        crate::closure::closure_set_dynamic_prop(closure_addr, "errorMonitor", {
            let name = "events.errorMonitor";
            let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            unsafe { crate::symbol::js_symbol_for(f64::from_bits(JSValue::string_ptr(ptr).bits())) }
        });
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "init",
            bound_native_callable_export_value("events", "init"),
        );
    }

    if module_name == "util" && property_name == "promisify" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            crate::util_promisify::promisify_custom_symbol(),
        );
    }
    if module_name == "util" && property_name == "inspect" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            util_inspect_custom_symbol(),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "defaultOptions",
            util_inspect_default_options_value(),
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "styles", util_inspect_styles());
        crate::closure::closure_set_dynamic_prop(closure_addr, "colors", util_inspect_colors());
    }

    NATIVE_CALLABLE_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

extern "C" fn fs_namespace_descriptor_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    unsafe {
        let property_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8;
        let property_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        js_native_module_property_by_name(b"fs".as_ptr(), 2, property_ptr, property_len)
    }
}

extern "C" fn fs_namespace_descriptor_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn fs_namespace_descriptor_getter_value(property_name: &str) -> f64 {
    let key = format!("fs\0get\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let property_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let func_ptr = fs_namespace_descriptor_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, property_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, property_bytes.len() as i64);
    let name = if property_name == "promises" {
        "get".to_string()
    } else {
        format!("get {property_name}")
    };
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

pub(crate) fn fs_namespace_descriptor_setter_value(property_name: &str) -> f64 {
    let key = format!("fs\0set\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let func_ptr = fs_namespace_descriptor_setter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let name = format!("set {property_name}");
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

fn native_callable_export_arity(module: &str, prop: &str) -> Option<u32> {
    match (module, prop) {
        ("events", "EventEmitter") => Some(1),
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
        ("url", "URL") => Some(1),
        ("tls", "getCiphers") => Some(0),
        ("tls", "getCACertificates" | "setDefaultCACertificates" | "createSecureContext") => {
            Some(1)
        }
        ("tls", "checkServerIdentity") => Some(2),
        ("tls", "SecureContext") => Some(1),
        ("url", "Url") => Some(0),
        ("url", "resolveObject") => Some(2),
        ("process", "setSourceMapsEnabled") => Some(1),
        (
            "process",
            "setUncaughtExceptionCaptureCallback" | "addUncaughtExceptionCaptureCallback",
        ) => Some(1),
        ("process", "hasUncaughtExceptionCaptureCallback") => Some(0),
        ("fs", "_toUnixTimestamp") => Some(1),
        ("util", "debug" | "debuglog") => Some(2),
        ("util", "MIMEParams") => Some(0),
        ("util", "MIMEType") => Some(1),
        ("net", "createServer" | "Server") => Some(2),
        ("net", "Socket") => Some(1),
        ("net", "_normalizeArgs") => Some(1),
        ("net", "_createServerHandle") => Some(5),
        ("domain", "Domain" | "createDomain" | "create") => Some(0),
        ("util", "diff") => Some(2),
        ("dns" | "dns/promises", "Resolver") => Some(0),
        ("fs", "ReadStream" | "WriteStream") => Some(2),
        ("fs", "Utf8Stream") => Some(0),
        ("fs", "Dir" | "Dirent") => Some(3),
        ("fs", "Stats") => Some(14),
        ("fs", "mkdtempDisposableSync") => Some(2),
        ("fs", "openAsBlob") => Some(1),
        ("fs", "_toUnixTimestamp") => Some(1),
        ("events", "init") => Some(1),
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
        ("module", "enableCompileCache") => Some(1),
        ("module", "flushCompileCache") => Some(0),
        ("module", "getCompileCacheDir") => Some(0),
        ("module", "getSourceMapsSupport") => Some(0),
        ("module", "setSourceMapsSupport") => Some(1),
        ("module", "stripTypeScriptTypes") => Some(1),
        ("module", "syncBuiltinESMExports") => Some(0),
        ("module", "runMain") => Some(0),
        _ => None,
    }
}

extern "C" fn sqlite_statement_sync_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_error_with_code("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

extern "C" fn sqlite_session_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_error_with_code("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

fn sqlite_statement_sync_constructor_value() -> f64 {
    SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = sqlite_statement_sync_constructor_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        set_bound_native_closure_name(closure, "StatementSync");
        let value = crate::value::js_nanbox_pointer(closure as i64);
        slot.set(value.to_bits());
        value
    })
}

fn sqlite_session_constructor_value() -> f64 {
    SQLITE_SESSION_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = sqlite_session_constructor_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        set_bound_native_closure_name(closure, "Session");
        let value = crate::value::js_nanbox_pointer(closure as i64);
        attach_sqlite_session_prototype(value);
        slot.set(value.to_bits());
        value
    })
}

fn native_callable_export_display_name<'a>(module: &str, prop: &'a str) -> &'a str {
    if module == "fs" {
        match prop {
            "_toUnixTimestamp" => "toUnixTimestamp",
            "Stats" => "deprecated",
            _ => prop,
        }
    } else {
        prop
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

const SQLITE_DATABASE_SYNC_PROTOTYPE_METHODS: &[&str] = &[
    "open",
    "close",
    "exec",
    "prepare",
    "function",
    "aggregate",
    "enableDefensive",
    "setAuthorizer",
    "createTagStore",
    "createSession",
    "applyChangeset",
    "enableLoadExtension",
    "loadExtension",
    "location",
];

const SQLITE_SESSION_PROTOTYPE_METHODS: &[&str] = &["changeset", "patchset", "close"];

extern "C" fn sqlite_database_sync_prototype_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> f64 {
    unsafe {
        let method_name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let method_name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let receiver = crate::object::js_implicit_this_get();
        let args = [arg0, arg1, arg2];
        crate::object::js_native_call_method(
            receiver,
            method_name_ptr,
            method_name_len,
            args.as_ptr(),
            args.len(),
        )
    }
}

fn attach_sqlite_database_sync_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let func_ptr = sqlite_database_sync_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 3);
    for method in SQLITE_DATABASE_SYNC_PROTOTYPE_METHODS {
        let leaked: &'static [u8] = method.as_bytes().to_vec().leak();
        let method_closure = crate::closure::js_closure_alloc(func_ptr, 2);
        if method_closure.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method_closure, 0, leaked.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method_closure, 1, leaked.len() as i64);
        set_bound_native_closure_name(method_closure, method);
        set_builtin_closure_length(method_closure as usize, 0);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
        js_object_set_field_by_name(proto, key, method_value);
        super::set_builtin_property_attrs(
            proto as usize,
            (*method).to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

fn attach_sqlite_session_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let func_ptr = sqlite_database_sync_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 3);
    for method in SQLITE_SESSION_PROTOTYPE_METHODS {
        let leaked: &'static [u8] = method.as_bytes().to_vec().leak();
        let method_closure = crate::closure::js_closure_alloc(func_ptr, 2);
        if method_closure.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method_closure, 0, leaked.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method_closure, 1, leaked.len() as i64);
        set_bound_native_closure_name(method_closure, method);
        set_builtin_closure_length(method_closure as usize, 0);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
        js_object_set_field_by_name(proto, key, method_value);
        super::set_builtin_property_attrs(
            proto as usize,
            (*method).to_string(),
            super::PropertyAttrs::new(true, true, true),
        );
    }

    let dispose_method = "@@__perry_wk_dispose";
    let dispose_leaked: &'static [u8] = dispose_method.as_bytes().to_vec().leak();
    let dispose_closure = crate::closure::js_closure_alloc(func_ptr, 2);
    if !dispose_closure.is_null() {
        crate::closure::js_closure_set_capture_ptr(
            dispose_closure,
            0,
            dispose_leaked.as_ptr() as i64,
        );
        crate::closure::js_closure_set_capture_ptr(dispose_closure, 1, dispose_leaked.len() as i64);
        set_bound_native_closure_name(dispose_closure, "[Symbol.dispose]");
        set_builtin_closure_length(dispose_closure as usize, 0);
        let dispose_value = crate::value::js_nanbox_pointer(dispose_closure as i64);
        let dispose_sym = crate::symbol::well_known_symbol("dispose");
        if !dispose_sym.is_null() {
            let dispose_sym_value = crate::value::js_nanbox_pointer(dispose_sym as i64);
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    crate::value::js_nanbox_pointer(proto as i64),
                    dispose_sym_value,
                    dispose_value,
                );
            }
        }
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

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

fn native_string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn native_bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn native_object_value(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn native_set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn native_color_tuple(open: i32, close: i32) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(2);
    crate::array::js_array_set_f64(arr, 0, open as f64);
    crate::array::js_array_set_f64(arr, 1, close as f64);
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn util_inspect_custom_symbol() -> f64 {
    unsafe { crate::symbol::js_symbol_for(native_string_value("nodejs.util.inspect.custom")) }
}

pub(crate) fn util_inspect_default_options_value() -> f64 {
    UTIL_INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "showHidden", native_bool_value(false));
        native_set_field(obj, "depth", 2.0);
        native_set_field(obj, "colors", native_bool_value(false));
        native_set_field(obj, "customInspect", native_bool_value(true));
        native_set_field(obj, "showProxy", native_bool_value(false));
        native_set_field(obj, "maxArrayLength", 100.0);
        native_set_field(obj, "maxStringLength", 10000.0);
        native_set_field(obj, "breakLength", 80.0);
        native_set_field(obj, "compact", 3.0);
        native_set_field(obj, "sorted", native_bool_value(false));
        native_set_field(obj, "getters", native_bool_value(false));
        native_set_field(obj, "numericSeparator", native_bool_value(false));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_styles() -> f64 {
    UTIL_INSPECT_STYLES.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "special", native_string_value("cyan"));
        native_set_field(obj, "number", native_string_value("yellow"));
        native_set_field(obj, "bigint", native_string_value("yellow"));
        native_set_field(obj, "boolean", native_string_value("yellow"));
        native_set_field(obj, "undefined", native_string_value("grey"));
        native_set_field(obj, "null", native_string_value("bold"));
        native_set_field(obj, "string", native_string_value("green"));
        native_set_field(obj, "symbol", native_string_value("green"));
        native_set_field(obj, "date", native_string_value("magenta"));
        native_set_field(obj, "regexp", native_string_value("red"));
        native_set_field(obj, "module", native_string_value("underline"));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_colors() -> f64 {
    UTIL_INSPECT_COLORS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for style in crate::util_style_text::INSPECT_COLOR_STYLES {
            native_set_field(obj, style.name, native_color_tuple(style.open, style.close));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn zlib_codes_object() -> f64 {
    const ZLIB_RETURN_CODES: &[(&str, i32)] = &[
        ("Z_OK", 0),
        ("Z_STREAM_END", 1),
        ("Z_NEED_DICT", 2),
        ("Z_ERRNO", -1),
        ("Z_STREAM_ERROR", -2),
        ("Z_DATA_ERROR", -3),
        ("Z_MEM_ERROR", -4),
        ("Z_BUF_ERROR", -5),
        ("Z_VERSION_ERROR", -6),
    ];

    ZLIB_CODES_OBJECT.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for (name, value) in ZLIB_RETURN_CODES.iter().take(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }
        for (name, value) in ZLIB_RETURN_CODES {
            native_set_field(obj, name, *value as f64);
        }
        for (name, value) in ZLIB_RETURN_CODES.iter().skip(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

pub(crate) fn timers_promises_parent_namespace() -> f64 {
    TIMERS_PROMISES_PARENT_NAMESPACE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let module_name = "timers/promises";
        let value = js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
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

fn attach_tty_stream_prototype(constructor_value: f64, name: &str) {
    crate::tty::attach_tty_constructor_prototype(constructor_value, name);
}

fn attach_tls_secure_context_prototype(constructor_value: f64) {
    crate::tls::attach_secure_context_constructor_prototype(constructor_value);
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
        ("module", "isBuiltin") => Some(1),
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

thread_local! {
    /// Per-closure spec `.length` for built-in *prototype methods*. Those
    /// methods all share one no-op closure thunk
    /// (`global_this_builtin_noop_thunk`), so the func-ptr-keyed
    /// `CLOSURE_ARITY_REGISTRY` can't give `Array.prototype.map.length === 1`
    /// while `Array.prototype.slice.length === 2` — the last install would
    /// win for every method. Recording the length per *closure instance* here
    /// (keyed by the closure pointer, like the user-facing dynamic-prop table
    /// but isolated from it so a user `fn.length = x` write can't perturb it)
    /// lets the `.length` value-read and `getOwnPropertyDescriptor` agree with
    /// the spec count. #3143.
    static BUILTIN_CLOSURE_LENGTH: std::cell::RefCell<std::collections::HashMap<usize, u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Record the spec `.length` for a built-in prototype-method closure. See
/// [`BUILTIN_CLOSURE_LENGTH`].
pub(crate) fn set_builtin_closure_length(closure: usize, length: u32) {
    BUILTIN_CLOSURE_LENGTH.with(|m| {
        m.borrow_mut().insert(closure, length);
    });
}

/// Look up the recorded spec `.length` for a built-in prototype-method
/// closure, or `None` if this closure isn't one. See [`BUILTIN_CLOSURE_LENGTH`].
pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&closure).copied())
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
    let module = cjs_default_base_module(module).unwrap_or(module);
    let prop = canonical_native_callable_property(module, prop);
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
            // #3698: node:tls.connect import-surface fix — the manifest-claimed
            // named/namespace import must be function-valued (typeof ===
            // "function"). Deeper TLS behavior is tracked separately
            // (#3196-#3200); only `connect` is in the api-manifest today, so
            // it's the only tls symbol exposed here.
            | ("tls", "connect")
            | ("module", "createRequire")
            | ("module", "findPackageJSON")
            | ("module", "findSourceMap")
            | ("module", "flushCompileCache")
            | ("module", "getCompileCacheDir")
            | ("module", "getSourceMapsSupport")
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
    )
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    _namespace_obj: f64,
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
    let module_name = unsafe { get_module_name_from_namespace(_namespace_obj) };

    // Check for known constant properties first
    if let Some(val) =
        unsafe { get_native_module_constant(module_name, property_name, _namespace_obj) }
    {
        return val;
    }

    // Not a constant. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if is_native_module_callable_export(module_name, property_name) {
        return bound_native_callable_export_value(module_name, property_name);
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

    bound_native_callable_export_value(module_name, property_name)
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
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            if matches!(
                name,
                "append"
                    | "delete"
                    | "entries"
                    | "forEach"
                    | "get"
                    | "getSetCookie"
                    | "has"
                    | "keys"
                    | "set"
                    | "Symbol.iterator"
                    | "@@iterator"
                    | "values"
            ) {
                let bits = instance.to_bits();
                if (bits >> 48) == 0x7FFD {
                    let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
                    if id > 0 && id < 0x100000 {
                        if let Some(dispatch) = handle_property_dispatch() {
                            let value = HANDLE_PROPERTY_BIND_REENTRY.with(|guard| {
                                if guard.get() {
                                    None
                                } else {
                                    guard.set(true);
                                    let value =
                                        unsafe { dispatch(id, method_name_ptr, method_name_len) };
                                    guard.set(false);
                                    Some(value)
                                }
                            });
                            if let Some(value) = value {
                                if value.to_bits() != crate::value::TAG_UNDEFINED {
                                    return value;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

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
    if let Some(bits) = CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        if let Some(bits) = cache.get(&(class_id, method_name.to_string())).copied() {
            return Some(bits);
        }
        None
    }) {
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
    class_prototype_method_value_cache_root_store(
        class_id,
        method_name.to_string(),
        value.to_bits(),
    );
    value
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
    let fs_const_tail = |prop: &str| -> Option<f64> {
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
    };

    // #3683: `constants.defaultCoreCipherList` — OpenSSL's built-in default
    // TLS cipher list string Node exposes (informational metadata, not a
    // behavioral toggle). Matches Node's compiled-in default.
    const DEFAULT_CORE_CIPHER_LIST: &str = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA";

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

    let dns_const = |prop: &str| -> Option<f64> {
        Some(match prop {
            "ADDRCONFIG" => 1024.0,
            "V4MAPPED" => 2048.0,
            "ALL" => 256.0,
            "NODATA" => str_val("ENODATA"),
            "FORMERR" => str_val("EFORMERR"),
            "SERVFAIL" => str_val("ESERVFAIL"),
            "NOTFOUND" => str_val("ENOTFOUND"),
            "NOTIMP" => str_val("ENOTIMP"),
            "REFUSED" => str_val("EREFUSED"),
            "BADQUERY" => str_val("EBADQUERY"),
            "BADNAME" => str_val("EBADNAME"),
            "BADFAMILY" => str_val("EBADFAMILY"),
            "BADRESP" => str_val("EBADRESP"),
            "CONNREFUSED" => str_val("ECONNREFUSED"),
            "TIMEOUT" => str_val("ETIMEOUT"),
            "EOF" => str_val("EOF"),
            "FILE" => str_val("EFILE"),
            "NOMEM" => str_val("ENOMEM"),
            "DESTRUCTION" => str_val("EDESTRUCTION"),
            "BADSTR" => str_val("EBADSTR"),
            "BADFLAGS" => str_val("EBADFLAGS"),
            "NONAME" => str_val("ENONAME"),
            "BADHINTS" => str_val("EBADHINTS"),
            "NOTINITIALIZED" => str_val("ENOTINITIALIZED"),
            "LOADIPHLPAPI" => str_val("ELOADIPHLPAPI"),
            "ADDRGETNETWORKPARAMS" => str_val("EADDRGETNETWORKPARAMS"),
            "CANCELLED" => str_val("ECANCELLED"),
            _ => return None,
        })
    };

    let sqlite_const = |prop: &str| -> Option<f64> {
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
        "module" => match property {
            "builtinModules" => Some(crate::process::js_module_builtin_modules()),
            "constants" => Some(crate::process::js_module_constants()),
            _ => None,
        },
        "process" => match property {
            "sourceMapsEnabled" => Some(crate::process::js_process_source_maps_enabled()),
            _ => None,
        },
        "dns" => match property {
            "promises" => {
                crate::dns::dns_promises_init_servers_from_callback_if_unset();
                Some(create_sub_namespace("dns/promises"))
            }
            _ => dns_lookup_flag_constant(property)
                .or_else(|| dns_error_alias(property).map(|alias| str_val(alias))),
        },
        "dns/promises" => dns_error_alias(property).map(|alias| str_val(alias)),
        "async_hooks" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("async_hooks"),
            _ => None,
        },
        "querystring" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("querystring"),
            _ => None,
        },
        "constants" => fs_const(property)
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
        "tls" => match property {
            "DEFAULT_ECDH_CURVE" => Some(str_val("auto")),
            "DEFAULT_MAX_VERSION" => Some(str_val("TLSv1.3")),
            "DEFAULT_MIN_VERSION" => Some(str_val("TLSv1.2")),
            "DEFAULT_CIPHERS" => Some(str_val(crate::tls::DEFAULT_CIPHERS)),
            "CLIENT_RENEG_LIMIT" => Some(3.0),
            "CLIENT_RENEG_WINDOW" => Some(600.0),
            "rootCertificates" => Some(crate::tls::js_tls_root_certificates()),
            _ => None,
        },
        "wasi" => match property {
            "default" => Some(native_namespace_or_create("wasi", namespace_obj)),
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
        "url" => match property {
            "default" if !is_cjs_default_object => cjs_default_export_value("url"),
            _ => None,
        },
        "net" => match property {
            "Stream" => Some(bound_native_callable_export_value("net", "Socket")),
            _ => None,
        },
        "timers" => match property {
            "promises" => Some(timers_promises_parent_namespace()),
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
            "default" if !is_cjs_default_object => cjs_default_export_value("events"),
            "defaultMaxListeners" => Some(10.0),
            "usingDomains" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "captureRejections" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "errorMonitor" => Some(crate::symbol::js_symbol_for(str_val("events.errorMonitor"))),
            "captureRejectionSymbol" => {
                Some(crate::symbol::js_symbol_for(str_val("nodejs.rejection")))
            }
            "init" => Some(bound_native_callable_export_value("events", "init")),
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
            "isMainThread" => Some(f64::from_bits(JSValue::bool(true).bits())),
            "isInternalThread" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "parentPort" | "workerData" => Some(f64::from_bits(crate::value::TAG_NULL)),
            "threadId" => Some(0.0),
            "threadName" => Some(str_val("")),
            "resourceLimits" => {
                let obj = crate::object::js_object_alloc(0, 0);
                Some(crate::value::js_nanbox_pointer(obj as i64))
            }
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
            _ => None,
        },
        "https" => match property {
            "globalAgent" => Some(unsafe { https_global_agent_object() }),
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
        "dns" => dns_const(property),
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

unsafe fn https_global_agent_object() -> f64 {
    if let Some(bits) =
        NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get("https.globalAgent").copied())
    {
        return f64::from_bits(bits);
    }

    let field_names = [
        "defaultPort",
        "protocol",
        "keepAlive",
        "maxSockets",
        "maxFreeSockets",
    ];
    let packed = field_names.join("\0");
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF12,
        field_names.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    js_object_set_field(obj, 0, JSValue::number(443.0));
    let protocol = crate::string::js_string_from_bytes(b"https:".as_ptr(), 6);
    js_object_set_field(obj, 1, JSValue::string_ptr(protocol));
    js_object_set_field(obj, 2, JSValue::bool(true));
    js_object_set_field(obj, 3, JSValue::number(f64::INFINITY));
    js_object_set_field(obj, 4, JSValue::number(256.0));

    let result = crate::value::js_nanbox_pointer(obj as i64);
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        cache
            .borrow_mut()
            .insert("https.globalAgent".to_string(), result.to_bits());
    });
    result
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
