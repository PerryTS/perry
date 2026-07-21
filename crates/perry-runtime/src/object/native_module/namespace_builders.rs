use super::*;
use std::sync::atomic::Ordering;
/// Create a NativeModuleRef sub-namespace (e.g. "fs.constants", "path.posix").
/// The compiled code treats the result as another NativeModuleRef, so chained
/// property accesses like `fs.constants.O_RDONLY` work through the dispatch table.
pub(crate) fn create_sub_namespace(name: &str) -> f64 {
    js_create_native_module_namespace(name.as_ptr(), name.len())
}

pub(crate) fn native_namespace_or_create(module_name: &str, namespace_obj: f64) -> f64 {
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

pub(crate) fn create_cached_sub_namespace(name: &str, cache: &std::sync::atomic::AtomicU64) -> f64 {
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
pub(crate) unsafe fn http_methods_array() -> f64 {
    let cached = crate::object::HTTP_METHODS_CACHE.load(Ordering::Relaxed);
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
        &crate::object::HTTP_METHODS_CACHE,
        value.to_bits(),
        Ordering::Relaxed,
    );
    value
}

fn global_agent_string_value(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

unsafe fn global_agent_string_from_header(
    ptr: *const crate::string::StringHeader,
) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(|s| s.to_string())
}

unsafe fn global_agent_value_to_string(value: JSValue) -> String {
    let ptr = crate::value::js_jsvalue_to_string(f64::from_bits(value.bits()));
    global_agent_string_from_header(ptr).unwrap_or_default()
}

unsafe fn global_agent_value_to_json_string(value: JSValue) -> String {
    let ptr = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
    global_agent_string_from_header(ptr).unwrap_or_default()
}

fn global_agent_is_truthy(value: JSValue) -> bool {
    crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0
}

fn global_agent_is_undefined(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
}

unsafe fn global_agent_object_ptr(value: f64) -> Option<*const ObjectHeader> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let ptr = if top16 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
    } else if top16 == 0 && bits >= 0x10000 {
        bits as *const ObjectHeader
    } else {
        return None;
    };
    (!ptr.is_null()).then_some(ptr)
}

unsafe fn global_agent_get_field_raw(value: f64, field: &str) -> Option<JSValue> {
    let ptr = global_agent_object_ptr(value)?;
    let key = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    Some(js_object_get_field_by_name(ptr, key))
}

unsafe fn global_agent_get_string_field(value: f64, field: &str) -> Option<String> {
    let field_value = global_agent_get_field_raw(value, field)?;
    if field_value.is_undefined() || field_value.is_null() {
        return None;
    }
    if field_value.is_any_string() {
        let coerced = crate::builtins::js_string_coerce(f64::from_bits(field_value.bits()));
        return global_agent_string_from_header(coerced);
    }
    if field_value.is_number() {
        return Some(format!("{}", field_value.as_number() as i64));
    }
    None
}

unsafe fn global_agent_get_number_field(value: f64, field: &str) -> Option<f64> {
    let field_value = global_agent_get_field_raw(value, field)?;
    if field_value.is_undefined() || field_value.is_null() {
        return None;
    }
    field_value.is_number().then(|| field_value.as_number())
}

unsafe fn global_agent_has_name_option(value: f64) -> bool {
    for field in ["host", "port", "localAddress", "family", "socketPath"] {
        if let Some(field_value) = global_agent_get_field_raw(value, field) {
            if !field_value.is_undefined() {
                return true;
            }
        }
    }
    false
}

unsafe fn global_agent_select_options(first: f64, second: f64) -> f64 {
    if global_agent_is_undefined(second) {
        return first;
    }
    if global_agent_has_name_option(first) {
        first
    } else {
        second
    }
}

unsafe fn global_agent_build_http_name(options: f64) -> String {
    let bits = options.to_bits();
    if bits == JSValue::undefined().bits() || bits == JSValue::null().bits() {
        return "localhost::".to_string();
    }

    let host =
        global_agent_get_string_field(options, "host").unwrap_or_else(|| "localhost".to_string());
    let port = global_agent_get_string_field(options, "port").unwrap_or_default();
    let local_address = global_agent_get_string_field(options, "localAddress").unwrap_or_default();
    let mut name = format!("{}:{}:{}", host, port, local_address);

    if let Some(family) = global_agent_get_number_field(options, "family") {
        let family = family as i64;
        if family == 4 || family == 6 {
            name.push(':');
            name.push_str(&family.to_string());
        }
    }
    if let Some(socket_path) = global_agent_get_string_field(options, "socketPath") {
        name.push(':');
        name.push_str(&socket_path);
    }

    name
}

unsafe fn global_agent_append_https_name_fields(name: &mut String, options: f64) {
    let bits = options.to_bits();
    if bits == JSValue::undefined().bits() || bits == JSValue::null().bits() {
        for _ in 0..20 {
            name.push(':');
        }
        return;
    }

    let host_value = global_agent_get_field_raw(options, "host");

    let push_truthy_string = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(value) = global_agent_get_field_raw(options, field) {
            if global_agent_is_truthy(value) {
                name.push_str(&global_agent_value_to_string(value));
            }
        }
    };
    let push_defined = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(value) = global_agent_get_field_raw(options, field) {
            if !value.is_undefined() {
                name.push_str(&global_agent_value_to_string(value));
            }
        }
    };

    push_truthy_string(name, "ca");
    push_truthy_string(name, "cert");
    push_truthy_string(name, "clientCertEngine");
    push_truthy_string(name, "ciphers");
    push_truthy_string(name, "key");
    push_truthy_string(name, "pfx");
    push_defined(name, "rejectUnauthorized");

    name.push(':');
    if let Some(servername) = global_agent_get_field_raw(options, "servername") {
        if global_agent_is_truthy(servername) {
            let same_as_host = match host_value {
                Some(host) if global_agent_is_truthy(host) => {
                    global_agent_value_to_string(host) == global_agent_value_to_string(servername)
                }
                _ => false,
            };
            if !same_as_host {
                name.push_str(&global_agent_value_to_string(servername));
            }
        }
    }

    push_truthy_string(name, "minVersion");
    push_truthy_string(name, "maxVersion");
    push_truthy_string(name, "secureProtocol");
    push_truthy_string(name, "crl");
    push_defined(name, "honorCipherOrder");
    push_truthy_string(name, "ecdhCurve");
    push_truthy_string(name, "dhparam");
    push_defined(name, "secureOptions");
    push_truthy_string(name, "sessionIdContext");

    name.push(':');
    if let Some(value) = global_agent_get_field_raw(options, "sigalgs") {
        if global_agent_is_truthy(value) {
            name.push_str(&global_agent_value_to_json_string(value));
        }
    }

    push_truthy_string(name, "privateKeyIdentifier");
    push_truthy_string(name, "privateKeyEngine");
}

unsafe fn global_agent_build_name(options: f64, is_https: bool) -> String {
    let mut name = global_agent_build_http_name(options);
    if is_https {
        global_agent_append_https_name_fields(&mut name, options);
    }
    name
}

extern "C" fn global_agent_get_name_thunk(
    closure: *const crate::closure::ClosureHeader,
    first: f64,
    second: f64,
) -> f64 {
    unsafe {
        let is_https = crate::closure::js_closure_get_capture_ptr(closure, 0) != 0;
        let options = global_agent_select_options(first, second);
        global_agent_string_value(&global_agent_build_name(options, is_https))
    }
}

extern "C" fn global_agent_keep_socket_alive_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _socket: f64,
) -> f64 {
    f64::from_bits(JSValue::bool(true).bits())
}

extern "C" fn global_agent_reuse_socket_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _socket: f64,
    _request: f64,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

extern "C" fn global_agent_destroy_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn global_agent_method_value(
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    exposed_length: u32,
    is_https: Option<bool>,
) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, call_arity);
    let captures = if is_https.is_some() { 1 } else { 0 };
    let closure = crate::closure::js_closure_alloc(func_ptr, captures);
    if let Some(is_https) = is_https {
        crate::closure::js_closure_set_capture_ptr(closure, 0, i64::from(is_https));
    }
    set_bound_native_closure_name(closure, name);
    set_builtin_closure_length(closure as usize, exposed_length);
    set_builtin_closure_non_constructable(closure as usize);
    crate::value::js_nanbox_pointer(closure as i64)
}

unsafe fn global_agent_prototype(is_https: bool) -> f64 {
    let proto = js_object_alloc(0, 0);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    let attrs = super::PropertyAttrs::new(true, false, true);
    for (name, value) in [
        (
            "keepSocketAlive",
            global_agent_method_value(
                "keepSocketAlive",
                global_agent_keep_socket_alive_thunk as *const u8,
                1,
                1,
                None,
            ),
        ),
        (
            "reuseSocket",
            global_agent_method_value(
                "reuseSocket",
                global_agent_reuse_socket_thunk as *const u8,
                2,
                2,
                None,
            ),
        ),
        (
            "getName",
            global_agent_method_value(
                "getName",
                global_agent_get_name_thunk as *const u8,
                2,
                0,
                Some(is_https),
            ),
        ),
        (
            "destroy",
            global_agent_method_value(
                "destroy",
                global_agent_destroy_thunk as *const u8,
                0,
                0,
                None,
            ),
        ),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(proto, key, value);
        set_property_attrs(proto as usize, name.to_string(), attrs);
    }
    proto_value
}

pub(crate) unsafe fn https_global_agent_object() -> f64 {
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
    crate::object::js_object_set_prototype_of(result, global_agent_prototype(true));
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        cache
            .borrow_mut()
            .insert("https.globalAgent".to_string(), result.to_bits());
    });
    result
}

/// #3712: `http.globalAgent` shape. Mirrors `https_global_agent_object` but
/// with the http defaults (protocol "http:", defaultPort 80). Node 19+ ships
/// the global agent with keep-alive enabled, so basic field reads match Node.
pub(crate) unsafe fn http_global_agent_object() -> f64 {
    if let Some(bits) =
        NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get("http.globalAgent").copied())
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
    js_object_set_field(obj, 0, JSValue::number(80.0));
    let protocol = crate::string::js_string_from_bytes(b"http:".as_ptr(), 5);
    js_object_set_field(obj, 1, JSValue::string_ptr(protocol));
    // Node 19+ enables HTTP keep-alive on the global agent by default.
    js_object_set_field(obj, 2, JSValue::bool(true));
    js_object_set_field(obj, 3, JSValue::number(f64::INFINITY));
    js_object_set_field(obj, 4, JSValue::number(256.0));

    let result = crate::value::js_nanbox_pointer(obj as i64);
    crate::object::js_object_set_prototype_of(result, global_agent_prototype(false));
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        cache
            .borrow_mut()
            .insert("http.globalAgent".to_string(), result.to_bits());
    });
    result
}

/// #2519: `http.STATUS_CODES` — the standard HTTP status-code → reason-phrase
/// map. Keys are the numeric codes as strings (so `STATUS_CODES[200]` resolves
/// via the usual number→string index coercion). Cached as a scanned root in
/// `NATIVE_MODULE_NAMESPACES` (mirrors `http_global_agent_object`).
pub(crate) unsafe fn http_status_codes_object() -> f64 {
    if let Some(bits) =
        NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get("http.STATUS_CODES").copied())
    {
        return f64::from_bits(bits);
    }

    // Node 22 `require('node:http').STATUS_CODES` snapshot (63 entries).
    const STATUS_CODES: &[(u32, &str)] = &[
        (100, "Continue"),
        (101, "Switching Protocols"),
        (102, "Processing"),
        (103, "Early Hints"),
        (200, "OK"),
        (201, "Created"),
        (202, "Accepted"),
        (203, "Non-Authoritative Information"),
        (204, "No Content"),
        (205, "Reset Content"),
        (206, "Partial Content"),
        (207, "Multi-Status"),
        (208, "Already Reported"),
        (226, "IM Used"),
        (300, "Multiple Choices"),
        (301, "Moved Permanently"),
        (302, "Found"),
        (303, "See Other"),
        (304, "Not Modified"),
        (305, "Use Proxy"),
        (307, "Temporary Redirect"),
        (308, "Permanent Redirect"),
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (402, "Payment Required"),
        (403, "Forbidden"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (406, "Not Acceptable"),
        (407, "Proxy Authentication Required"),
        (408, "Request Timeout"),
        (409, "Conflict"),
        (410, "Gone"),
        (411, "Length Required"),
        (412, "Precondition Failed"),
        (413, "Payload Too Large"),
        (414, "URI Too Long"),
        (415, "Unsupported Media Type"),
        (416, "Range Not Satisfiable"),
        (417, "Expectation Failed"),
        (418, "I'm a Teapot"),
        (421, "Misdirected Request"),
        (422, "Unprocessable Entity"),
        (423, "Locked"),
        (424, "Failed Dependency"),
        (425, "Too Early"),
        (426, "Upgrade Required"),
        (428, "Precondition Required"),
        (429, "Too Many Requests"),
        (431, "Request Header Fields Too Large"),
        (451, "Unavailable For Legal Reasons"),
        (500, "Internal Server Error"),
        (501, "Not Implemented"),
        (502, "Bad Gateway"),
        (503, "Service Unavailable"),
        (504, "Gateway Timeout"),
        (505, "HTTP Version Not Supported"),
        (506, "Variant Also Negotiates"),
        (507, "Insufficient Storage"),
        (508, "Loop Detected"),
        (509, "Bandwidth Limit Exceeded"),
        (510, "Not Extended"),
        (511, "Network Authentication Required"),
    ];

    let keys: Vec<String> = STATUS_CODES.iter().map(|(c, _)| c.to_string()).collect();
    let packed = keys.join("\0");
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF13,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    for (i, (_, msg)) in STATUS_CODES.iter().enumerate() {
        let str_ptr = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        js_object_set_field(obj, i as u32, JSValue::string_ptr(str_ptr));
    }

    let result = crate::value::js_nanbox_pointer(obj as i64);
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        cache
            .borrow_mut()
            .insert("http.STATUS_CODES".to_string(), result.to_bits());
    });
    result
}

/// Create (and cache) the fs.constants object with POSIX file system constants.
// #854: fs.constants object builder retained for the native fs module
#[allow(dead_code)]
pub(crate) unsafe fn create_fs_constants_object() -> f64 {
    let cached = crate::object::FS_CONSTANTS_CACHE.load(Ordering::Relaxed);
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
        &crate::object::FS_CONSTANTS_CACHE,
        result.to_bits(),
        Ordering::Relaxed,
    );
    result
}
