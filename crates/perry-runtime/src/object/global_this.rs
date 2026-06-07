//! `globalThis` singleton plus built-in constructor/namespace population.

use super::*;

#[path = "global_this_webassembly.rs"]
mod global_this_webassembly;

/// Issue #611: lazily allocate shared `globalThis` for computed global access.
#[no_mangle]
pub extern "C" fn js_get_global_this() -> f64 {
    let cached = GLOBAL_THIS_PTR.load(Ordering::Acquire);
    let ptr = if cached != 0 {
        while !GLOBAL_THIS_READY.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        cached
    } else {
        // First access — allocate. Race-tolerant: if two threads race the
        // initial alloc, the loser's allocation leaks (never freed) but
        // both threads see the winner's pointer afterward via CAS.
        let new_ptr = js_object_alloc(0, 0) as i64;
        // GC_STORE_AUDIT(ROOT): GLOBAL_THIS_PTR is a mutable root visited by scan_object_cache_roots_mut.
        match crate::gc::runtime_compare_exchange_root_atomic_raw_i64(
            &GLOBAL_THIS_PTR,
            0,
            new_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // Populate constructor values for `globalThis.Array` /
                // `context.Array` style reads without changing bare `new Array`.
                populate_global_this_builtins(new_ptr as *mut ObjectHeader);
                GLOBAL_THIS_READY.store(true, Ordering::Release);
                new_ptr
            }
            Err(other) => {
                while !GLOBAL_THIS_READY.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                other
            }
        }
    };
    crate::value::js_nanbox_pointer(ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_global_or_console_property_by_name(
    key: *const crate::StringHeader,
) -> f64 {
    if !key.is_null() {
        let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_len = (*key).byte_len as usize;
        let property_name =
            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
        if is_native_module_callable_export("console", property_name) {
            return js_native_module_property_by_name(
                b"console".as_ptr(),
                "console".len(),
                key_ptr,
                key_len,
            );
        }
    }

    let global_box = js_get_global_this();
    let global = crate::value::JSValue::from_bits(global_box.to_bits());
    if global.is_pointer() {
        let obj = global.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        return js_object_get_field_by_name_f64(obj, key);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// Note: `navigator` (#2923) is installed on the singleton directly (see
// `populate_global_this_builtins`) rather than via this generic namespace
// loop because it needs its own field-populated object, not an empty stub.

/// No-op thunk used as the function body for most singleton globalThis
/// built-in constructor values. Lets `globalThis.Array` carry a real
/// ClosureHeader (so `typeof globalThis.Array === "function"`) without
/// implementing actual constructor dispatch through this path — bare
/// `new Array(n)` continues to flow through codegen's `lower_new` arm and
/// the runtime `js_array_alloc` machinery, so callers that follow the
/// usual `new <Ident>(...)` pattern are unaffected. Calling these
/// sentinels directly (e.g. `globalThis.Array(3)`) returns undefined —
/// best-effort no-op rather than throwing — and remains a known gap for
/// non-String call-form constructors after re-binding the global to a local.
pub(crate) extern "C" fn global_this_builtin_noop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) extern "C" fn global_this_date_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    let string = crate::date::js_date_to_string(crate::date::js_date_new());
    crate::value::js_nanbox_string(string as i64)
}

fn global_this_fetch_option(init: f64, name: &[u8]) -> f64 {
    let value = crate::value::JSValue::from_bits(init.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let raw = crate::value::js_nanbox_get_pointer(init);
    if raw < 0x10000 || !is_valid_obj_ptr(raw as *const u8) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name_f64(raw as *const ObjectHeader, key)
}

fn global_this_fetch_option_string_ptr(init: f64, name: &[u8]) -> *const crate::StringHeader {
    let value = global_this_fetch_option(init, name);
    if matches!(
        value.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return std::ptr::null();
    }
    crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader
}

fn global_this_body_string_ptr(value: f64) -> *const crate::StringHeader {
    if matches!(
        value.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return std::ptr::null();
    }
    crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader
}

fn global_this_headers_handle_from_value(value: f64) -> f64 {
    if matches!(
        value.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return 0.0;
    }
    let headers = super::global_fetch::call_global_headers_new();
    if headers.to_bits() == crate::value::TAG_UNDEFINED {
        return 0.0;
    }
    super::global_fetch::call_global_headers_init_from_value(headers, value);
    headers
}

fn global_this_init_headers_handle(init: f64) -> f64 {
    global_this_headers_handle_from_value(global_this_fetch_option(init, b"headers"))
}

pub(crate) extern "C" fn global_this_blob_thunk(
    _closure: *const crate::closure::ClosureHeader,
    parts: f64,
    options: f64,
) -> f64 {
    let type_value = global_this_fetch_option(options, b"type");
    super::global_fetch::call_global_blob_new(parts, type_value)
}

pub(crate) extern "C" fn global_this_file_thunk(
    _closure: *const crate::closure::ClosureHeader,
    parts: f64,
    name: f64,
    options: f64,
) -> f64 {
    let type_value = global_this_fetch_option(options, b"type");
    let last_modified = global_this_fetch_option(options, b"lastModified");
    let last_modified = if last_modified.to_bits() == crate::value::TAG_UNDEFINED {
        f64::NAN
    } else {
        last_modified
    };
    super::global_fetch::call_global_file_new(parts, name, type_value, last_modified)
}

pub(crate) extern "C" fn global_this_headers_thunk(
    _closure: *const crate::closure::ClosureHeader,
    init: f64,
) -> f64 {
    let headers = super::global_fetch::call_global_headers_new();
    if headers.to_bits() == crate::value::TAG_UNDEFINED {
        return headers;
    }
    if init.to_bits() != crate::value::TAG_UNDEFINED {
        super::global_fetch::call_global_headers_init_from_value(headers, init);
    }
    headers
}

pub(crate) extern "C" fn global_this_response_thunk(
    _closure: *const crate::closure::ClosureHeader,
    body: f64,
    init: f64,
) -> f64 {
    let body_ptr = global_this_body_string_ptr(body);
    let status = global_this_fetch_option(init, b"status");
    let status = if status.to_bits() == crate::value::TAG_UNDEFINED {
        0.0
    } else {
        status
    };
    let status_text_ptr = global_this_fetch_option_string_ptr(init, b"statusText");
    let headers_handle = global_this_init_headers_handle(init);
    super::global_fetch::call_global_response_new(body_ptr, status, status_text_ptr, headers_handle)
}

pub(crate) extern "C" fn global_this_request_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    init: f64,
) -> f64 {
    let url_ptr = crate::value::js_get_string_pointer_unified(input) as *const crate::StringHeader;
    let method_ptr = global_this_fetch_option_string_ptr(init, b"method");
    // Body init coercion that DRAINS a `ReadableStream` body. @hono/node-server
    // wraps the incoming request body as `Readable.toWeb(incoming)` / a
    // `new ReadableStream({...})`, so the plain string coercion would stringify
    // the stream HANDLE to its numeric id and `await c.req.text()` would resolve
    // to a bogus number. Route through the registered body-init helper (stdlib
    // `js_response_body_init_ptr`), which drains the stream's buffered chunks;
    // string bodies fall back to the ordinary coercion. Refs Hono `c.req.text()`.
    let body_ptr = {
        let body_val = global_this_fetch_option(init, b"body");
        if matches!(
            body_val.to_bits(),
            crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
        ) {
            std::ptr::null()
        } else {
            super::global_fetch::call_global_body_init_ptr(body_val)
        }
    };
    let headers_handle = global_this_init_headers_handle(init);
    let referrer_ptr = global_this_fetch_option_string_ptr(init, b"referrer");
    let referrer_policy_ptr = global_this_fetch_option_string_ptr(init, b"referrerPolicy");
    let mode_ptr = global_this_fetch_option_string_ptr(init, b"mode");
    let credentials_ptr = global_this_fetch_option_string_ptr(init, b"credentials");
    let cache_ptr = global_this_fetch_option_string_ptr(init, b"cache");
    let redirect_ptr = global_this_fetch_option_string_ptr(init, b"redirect");
    let integrity_ptr = global_this_fetch_option_string_ptr(init, b"integrity");
    let keepalive = {
        let value = global_this_fetch_option(init, b"keepalive");
        if value.to_bits() == crate::value::TAG_UNDEFINED {
            f64::from_bits(crate::value::TAG_FALSE)
        } else {
            value
        }
    };
    let duplex_ptr = global_this_fetch_option_string_ptr(init, b"duplex");
    let signal = global_this_fetch_option(init, b"signal");
    super::global_fetch::call_global_request_new(
        url_ptr,
        method_ptr,
        body_ptr,
        headers_handle,
        referrer_ptr,
        referrer_policy_ptr,
        mode_ptr,
        credentials_ptr,
        cache_ptr,
        redirect_ptr,
        integrity_ptr,
        keepalive,
        duplex_ptr,
        signal,
    )
}

/// Resolve a NaN-boxed `this` value to a heap `ObjectHeader` pointer, or
/// `None` for a non-pointer / small-handle / null receiver.
unsafe fn subclass_this_object_ptr(this_box: f64) -> Option<*mut ObjectHeader> {
    let bits = this_box.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < 0x100000 || !crate::object::is_valid_obj_ptr(raw as *const u8) {
        return None;
    }
    Some(raw as *mut ObjectHeader)
}

/// Stash the id of a freshly-created native Web-Fetch handle (`handle_box` is
/// the NaN-boxed pointer-tagged value the Request/Response thunk returns) on a
/// subclass instance's `this` under `__perry_fetch_handle__`. Stored as a
/// plain numeric f64 — `fetch_subclass_handle_id` reads it back.
unsafe fn attach_fetch_handle_to_this(this_box: f64, handle_box: f64) {
    if let Some(obj) = subclass_this_object_ptr(this_box) {
        let id = crate::value::js_nanbox_get_pointer(handle_box);
        let key = crate::string::js_string_from_bytes(
            FETCH_SUBCLASS_HANDLE_FIELD.as_ptr(),
            FETCH_SUBCLASS_HANDLE_FIELD.len() as u32,
        );
        crate::object::js_object_set_field_by_name(obj, key, id as f64);
    }
}

/// Attach a native fetch handle to a freshly dynamically-constructed
/// Request/Response subclass instance, building it from the `new` arguments.
/// `kind` is 1 (Request) or 2 (Response). Used by the runtime
/// dynamic-construction path (`js_new_function_construct`) for class-expression
/// / ClassRef subclasses whose `super()` couldn't statically route the parent.
pub(crate) unsafe fn attach_fetch_handle_for_construction(
    inst: *mut ObjectHeader,
    kind: u8,
    args_ptr: *const f64,
    args_len: usize,
) {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg0 = if args_len >= 1 && !args_ptr.is_null() {
        *args_ptr
    } else {
        undef
    };
    let arg1 = if args_len >= 2 && !args_ptr.is_null() {
        *args_ptr.add(1)
    } else {
        undef
    };
    let handle = if kind == 1 {
        global_this_request_thunk(std::ptr::null(), arg0, arg1)
    } else {
        global_this_response_thunk(std::ptr::null(), arg0, arg1)
    };
    let this_box = crate::value::js_nanbox_pointer(inst as i64);
    attach_fetch_handle_to_this(this_box, handle);
}

/// `super(input, init)` for `class X extends Request`. Allocates the underlying
/// native Request handle and stashes it on `this`; inherited body methods /
/// property getters are forwarded to the handle at access time. Returns
/// `undefined` (the super-call value).
#[no_mangle]
pub extern "C" fn js_request_subclass_init(this_box: f64, input: f64, init: f64) -> f64 {
    let handle = global_this_request_thunk(std::ptr::null(), input, init);
    unsafe { attach_fetch_handle_to_this(this_box, handle) };
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `super(body, init)` for `class X extends Response`. Mirror of
/// `js_request_subclass_init` for the Response handle.
#[no_mangle]
pub extern "C" fn js_response_subclass_init(this_box: f64, body: f64, init: f64) -> f64 {
    let handle = global_this_response_thunk(std::ptr::null(), body, init);
    unsafe { attach_fetch_handle_to_this(this_box, handle) };
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// The two shims above are reached only from codegen-emitted IR (the
// `Expr::SuperCall` Request/Response arm); pin them so the auto-optimize
// bitcode rebuild's dead-strip can't drop them (see
// project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_REQUEST_SUBCLASS_INIT: extern "C" fn(f64, f64, f64) -> f64 =
    js_request_subclass_init;
#[used]
static KEEP_JS_RESPONSE_SUBCLASS_INIT: extern "C" fn(f64, f64, f64) -> f64 =
    js_response_subclass_init;

/// `super(...)` for `class X extends <runtime-value constructor>` where the
/// parent expression is an alias of the global `Request`/`Response` constructor
/// — e.g. `@hono/node-server`'s `class Request extends GlobalRequest` with
/// `GlobalRequest = global.Request`. The textual parent name is the alias
/// ("GlobalRequest"), not "Request", so codegen can't statically route it;
/// instead every runtime-value `super()` dispatches through here. When
/// `parent_val` resolves to the Request/Response constructor we allocate the
/// native handle and stash it on `this` (so inherited body methods work);
/// otherwise we fall back to the ordinary implicit-`this`-bound
/// `js_native_call_value`, preserving the prior behavior for every other
/// runtime-value parent (Effect's `Data.Class`, etc.).
#[no_mangle]
pub unsafe extern "C" fn js_fetch_or_value_super(
    parent_val: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    // Resolve the parent constructor kind from the value first. When the
    // `extends` expression is an alias of `global.Request`/`global.Response`
    // (`@hono/node-server`'s `class Request extends GlobalRequest`), the alias
    // var can lower to a constructor-scope local that reads `undefined` at
    // super-time, so `identify_global_builtin_constructor(parent_val)` returns
    // `None`. Fall back to the fetch-parent kind registered against the
    // instance's class at module init (via `js_register_class_parent_dynamic`,
    // where the alias resolved correctly) so the native handle still attaches.
    let kind =
        super::class_registry::identify_global_builtin_constructor(parent_val).or_else(|| {
            let obj = subclass_this_object_ptr(this_box)?;
            match super::class_registry::fetch_parent_kind_in_chain(
                crate::object::js_object_get_class_id(obj),
            ) {
                Some(1) => Some("Request"),
                Some(2) => Some("Response"),
                _ => None,
            }
        });
    match kind {
        Some("Request") | Some("Response") => {
            let arg0 = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                undef
            };
            let arg1 = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                undef
            };
            let handle = if kind == Some("Request") {
                global_this_request_thunk(std::ptr::null(), arg0, arg1)
            } else {
                global_this_response_thunk(std::ptr::null(), arg0, arg1)
            };
            attach_fetch_handle_to_this(this_box, handle);
            undef
        }
        _ => {
            let prev = crate::object::js_implicit_this_set(this_box);
            let r = crate::closure::js_native_call_value(parent_val, args_ptr, args_len);
            crate::object::js_implicit_this_set(prev);
            r
        }
    }
}

#[used]
static KEEP_JS_FETCH_OR_VALUE_SUPER: unsafe extern "C" fn(f64, f64, *const f64, usize) -> f64 =
    js_fetch_or_value_super;

extern "C" fn global_this_response_error_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::global_fetch::call_global_response_static_error()
}

extern "C" fn global_this_response_json_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    init: f64,
) -> f64 {
    let init_status = global_this_fetch_option(init, b"status");
    let init_status = if init_status.to_bits() == crate::value::TAG_UNDEFINED {
        0.0
    } else {
        init_status
    };
    let init_status_text_ptr = global_this_fetch_option_string_ptr(init, b"statusText");
    let headers_handle = global_this_init_headers_handle(init);
    super::global_fetch::call_global_response_static_json(
        value,
        init_status,
        init_status_text_ptr,
        headers_handle,
    )
}

extern "C" fn global_this_response_redirect_thunk(
    _closure: *const crate::closure::ClosureHeader,
    url: f64,
    status: f64,
) -> f64 {
    let url_ptr = crate::value::js_jsvalue_to_string(url) as *const crate::StringHeader;
    let status = if status.to_bits() == crate::value::TAG_UNDEFINED {
        302.0
    } else {
        status
    };
    super::global_fetch::call_global_response_static_redirect(url_ptr, status)
}

extern "C" fn global_this_eval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
) -> f64 {
    let source = crate::builtins::js_string_coerce(source);
    let Some(body) = (unsafe { super::has_own_helpers::str_from_string_header(source) }) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    match normalize_eval_this_body(body).as_deref() {
        Some("this" | "globalThis") => js_get_global_this(),
        Some("typeof this") => {
            let s = b"object";
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            crate::value::js_nanbox_string(ptr as i64)
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

fn normalize_eval_this_body(body: &str) -> Option<String> {
    let mut src = body.trim().trim_end_matches(';').trim();
    for directive in ["\"use strict\"", "'use strict'"] {
        if let Some(rest) = src.strip_prefix(directive) {
            let rest = rest.trim_start();
            if let Some(after_semicolon) = rest.strip_prefix(';') {
                src = after_semicolon.trim().trim_end_matches(';').trim();
            }
        }
    }
    if matches!(src, "this" | "globalThis" | "typeof this") {
        Some(src.to_string())
    } else {
        None
    }
}

pub(crate) extern "C" fn typed_array_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor %TypedArray% requires 'new'")
}

// #4569: Map/Set/WeakMap/WeakSet/WeakRef are constructors — calling them
// without `new` is a TypeError (ECMA-262: an undefined newTarget throws). The
// bare-call form previously fell through to `global_this_builtin_noop_thunk`
// and silently returned `undefined`. (`new Map()` uses the separate
// construct-expression path and is unaffected.)
pub(crate) extern "C" fn map_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor Map requires 'new'")
}

pub(crate) extern "C" fn set_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor Set requires 'new'")
}

pub(crate) extern "C" fn weak_map_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor WeakMap requires 'new'")
}

pub(crate) extern "C" fn weak_set_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor WeakSet requires 'new'")
}

pub(crate) extern "C" fn weak_ref_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::object_ops::throw_object_type_error(b"Constructor WeakRef requires 'new'")
}

extern "C" fn global_this_url_pattern_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    crate::url::js_url_pattern_constructor_call(input, base)
}

fn error_constructor_call(kind: u32, message: f64) -> f64 {
    let error = crate::error::js_error_new_kind_from_value(kind, message);
    crate::value::js_nanbox_pointer(error as i64)
}

pub(crate) extern "C" fn error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_ERROR, message)
}

pub(crate) extern "C" fn type_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_TYPE_ERROR, message)
}

pub(crate) extern "C" fn range_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_RANGE_ERROR, message)
}

pub(crate) extern "C" fn reference_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_REFERENCE_ERROR, message)
}

pub(crate) extern "C" fn syntax_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_SYNTAX_ERROR, message)
}

pub(crate) extern "C" fn eval_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_EVAL_ERROR, message)
}

pub(crate) extern "C" fn uri_error_constructor_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    error_constructor_call(crate::error::ERROR_KIND_URI_ERROR, message)
}

pub(crate) fn builtin_prototype_value(name: &str) -> f64 {
    let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_bits = ctor.to_bits();
    if (ctor_bits >> 48) != 0x7FFD {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
}

pub(crate) extern "C" fn webcrypto_illegal_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

#[no_mangle]
pub extern "C" fn js_webcrypto_illegal_constructor() -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

extern "C" fn global_this_crypto_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::native_module::webcrypto_namespace()
}

fn require_webcrypto_this() -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let jv = crate::value::JSValue::from_bits(this_value.to_bits());
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>();
        if !obj.is_null()
            && unsafe { (*obj).class_id } == super::native_module::NATIVE_MODULE_CLASS_ID
            && unsafe { super::native_module::read_native_module_name(obj) }
                .is_some_and(|name| name == "crypto.webcrypto")
        {
            return this_value;
        }
    }
    crate::fs::validate::throw_type_error_with_code(
        "Value of \"this\" must be of type Crypto",
        "ERR_INVALID_THIS",
    )
}

pub(crate) extern "C" fn webcrypto_get_random_values_thunk(
    _closure: *const crate::closure::ClosureHeader,
    array: f64,
) -> f64 {
    let this_value = require_webcrypto_this();
    unsafe {
        js_native_call_method(
            this_value,
            b"getRandomValues".as_ptr() as *const i8,
            "getRandomValues".len(),
            &array,
            1,
        )
    }
}

pub(crate) extern "C" fn webcrypto_random_uuid_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = require_webcrypto_this();
    unsafe {
        js_native_call_method(
            this_value,
            b"randomUUID".as_ptr() as *const i8,
            "randomUUID".len(),
            std::ptr::null(),
            0,
        )
    }
}

extern "C" fn webcrypto_subtle_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_webcrypto_this();
    super::native_module::subtle_crypto_namespace()
}

fn cryptokey_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = crate::value::JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & crate::value::POINTER_MASK) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    crate::buffer::crypto_key_meta(raw).map(|_| raw)
}

fn cryptokey_brand_error() -> ! {
    super::object_ops::throw_object_type_error(
        b"Value of CryptoKey getter must be an instance of CryptoKey",
    )
}

fn cryptokey_property_getter(key: &[u8]) -> f64 {
    let addr = cryptokey_receiver_addr().unwrap_or_else(|| cryptokey_brand_error());
    unsafe {
        super::crypto_key_property_value(addr, key)
            .map(|value| f64::from_bits(value.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
    }
}

extern "C" fn cryptokey_algorithm_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"algorithm")
}

extern "C" fn cryptokey_extractable_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    cryptokey_property_getter(b"extractable")
}

extern "C" fn cryptokey_type_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    cryptokey_property_getter(b"type")
}

extern "C" fn cryptokey_usages_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    cryptokey_property_getter(b"usages")
}

pub(crate) fn webcrypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, arity) = match property_name {
        "getRandomValues" => (webcrypto_get_random_values_thunk as *const u8, 1),
        "randomUUID" => (webcrypto_random_uuid_thunk as *const u8, 0),
        _ => return None,
    };
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    super::native_module::set_bound_native_closure_name(closure, property_name);
    super::native_module::set_builtin_closure_length(closure as usize, arity);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}

fn subtle_crypto_method_spec(property_name: &str) -> Option<(*const u8, u32)> {
    match property_name {
        "encapsulateBits" => Some((subtle_crypto_encapsulate_bits_thunk as *const u8, 2)),
        "decapsulateBits" => Some((subtle_crypto_decapsulate_bits_thunk as *const u8, 3)),
        "encapsulateKey" => Some((subtle_crypto_encapsulate_key_thunk as *const u8, 5)),
        "decapsulateKey" => Some((subtle_crypto_decapsulate_key_thunk as *const u8, 6)),
        _ => None,
    }
}

pub(crate) fn subtle_crypto_method_value(property_name: &str) -> Option<f64> {
    let (func_ptr, length) = subtle_crypto_method_spec(property_name)?;
    crate::closure::js_register_closure_rest(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    super::native_module::set_bound_native_closure_name(closure, property_name);
    super::native_module::set_builtin_closure_length(closure as usize, length);
    Some(crate::value::js_nanbox_pointer(closure as i64))
}

pub(crate) extern "C" fn global_this_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let rest_value = crate::value::JSValue::from_bits(rest.to_bits());
    let args_arr = if rest_value.is_pointer() {
        rest_value.as_pointer::<crate::array::ArrayHeader>()
    } else {
        std::ptr::null()
    };
    let argc = crate::array::js_array_length(args_arr);
    if argc == 1 {
        let first = crate::array::js_array_get_f64(args_arr, 0);
        let arr = crate::array::js_array_constructor_single(first);
        return crate::value::js_nanbox_pointer(arr as i64);
    }
    let arr = crate::array::js_array_alloc(argc);
    unsafe {
        (*arr).length = argc;
        for i in 0..argc {
            let value = crate::array::js_array_get_f64(args_arr, i);
            crate::array::js_array_set_f64(arr, i, value);
        }
    }
    crate::value::js_nanbox_pointer(arr as i64)
}

pub(crate) extern "C" fn global_this_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let string_ptr = crate::builtins::js_string_coerce(value);
    crate::value::js_nanbox_string(string_ptr as i64)
}

pub(crate) extern "C" fn global_this_object_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::object::js_object_coerce(value)
}

extern "C" fn global_this_structured_clone_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _options: f64,
) -> f64 {
    crate::builtins::js_structured_clone(value)
}

extern "C" fn global_this_atob_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let decoded = crate::string::js_atob(value);
    crate::value::js_nanbox_string(decoded as i64)
}

extern "C" fn global_this_btoa_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let encoded = crate::string::js_btoa(value);
    crate::value::js_nanbox_string(encoded as i64)
}

extern "C" fn math_f16round_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::math::js_math_f16round(value)
}

extern "C" fn math_random_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::math::js_math_random()
}

fn math_number_arg(value: f64) -> f64 {
    crate::builtins::js_number_coerce(value)
}

fn math_to_int32(value: f64) -> i32 {
    let n = math_number_arg(value);
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    (n.trunc().rem_euclid(TWO_32) as u32) as i32
}

fn math_to_uint32(value: f64) -> u32 {
    math_to_int32(value) as u32
}

macro_rules! math_unary_thunk {
    ($name:ident, $body:expr) => {
        extern "C" fn $name(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
            let x = math_number_arg(value);
            ($body)(x)
        }
    };
}

math_unary_thunk!(math_abs_thunk, |x: f64| x.abs());
math_unary_thunk!(math_acos_thunk, |x: f64| crate::math::js_math_acos(x));
math_unary_thunk!(math_acosh_thunk, |x: f64| crate::math::js_math_acosh(x));
math_unary_thunk!(math_asin_thunk, |x: f64| crate::math::js_math_asin(x));
math_unary_thunk!(math_asinh_thunk, |x: f64| crate::math::js_math_asinh(x));
math_unary_thunk!(math_atan_thunk, |x: f64| crate::math::js_math_atan(x));
math_unary_thunk!(math_atanh_thunk, |x: f64| crate::math::js_math_atanh(x));
math_unary_thunk!(math_cbrt_thunk, |x: f64| crate::math::js_math_cbrt(x));
math_unary_thunk!(math_ceil_thunk, |x: f64| x.ceil());
math_unary_thunk!(math_cos_thunk, |x: f64| crate::math::js_math_cos(x));
math_unary_thunk!(math_cosh_thunk, |x: f64| crate::math::js_math_cosh(x));
math_unary_thunk!(math_exp_thunk, |x: f64| x.exp());
math_unary_thunk!(math_expm1_thunk, |x: f64| crate::math::js_math_expm1(x));
math_unary_thunk!(math_floor_thunk, |x: f64| x.floor());
math_unary_thunk!(math_fround_thunk, |x: f64| crate::math::js_math_fround(x));
math_unary_thunk!(math_log_thunk, |x: f64| crate::math::js_math_log(x));
math_unary_thunk!(math_log10_thunk, |x: f64| crate::math::js_math_log10(x));
math_unary_thunk!(math_log1p_thunk, |x: f64| crate::math::js_math_log1p(x));
math_unary_thunk!(math_log2_thunk, |x: f64| crate::math::js_math_log2(x));
math_unary_thunk!(math_sin_thunk, |x: f64| crate::math::js_math_sin(x));
math_unary_thunk!(math_sinh_thunk, |x: f64| crate::math::js_math_sinh(x));
math_unary_thunk!(math_sqrt_thunk, |x: f64| x.sqrt());
math_unary_thunk!(math_tan_thunk, |x: f64| crate::math::js_math_tan(x));
math_unary_thunk!(math_tanh_thunk, |x: f64| crate::math::js_math_tanh(x));
math_unary_thunk!(math_trunc_thunk, |x: f64| x.trunc());

extern "C" fn math_round_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    let x = math_number_arg(value);
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let rounded = (x + 0.5).floor();
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

extern "C" fn math_sign_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    let x = math_number_arg(value);
    if x == 0.0 || x.is_nan() {
        x
    } else if x.is_sign_negative() {
        -1.0
    } else {
        1.0
    }
}

extern "C" fn math_clz32_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    math_to_uint32(value).leading_zeros() as f64
}

extern "C" fn math_atan2_thunk(
    _closure: *const crate::closure::ClosureHeader,
    y: f64,
    x: f64,
) -> f64 {
    crate::math::js_math_atan2(math_number_arg(y), math_number_arg(x))
}

extern "C" fn math_imul_thunk(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    math_to_int32(a).wrapping_mul(math_to_int32(b)) as f64
}

extern "C" fn math_pow_thunk(
    _closure: *const crate::closure::ClosureHeader,
    base: f64,
    exp: f64,
) -> f64 {
    crate::math::js_math_pow(math_number_arg(base), math_number_arg(exp))
}

extern "C" fn math_min_thunk(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    let values = global_this_rest_array_values(rest);
    if values.is_empty() {
        return f64::INFINITY;
    }
    let mut result = f64::INFINITY;
    for value in values {
        let n = math_number_arg(value);
        if n.is_nan() {
            return f64::NAN;
        }
        if n < result || (n == 0.0 && result == 0.0 && n.is_sign_negative()) {
            result = n;
        }
    }
    result
}

extern "C" fn math_max_thunk(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    let values = global_this_rest_array_values(rest);
    if values.is_empty() {
        return f64::NEG_INFINITY;
    }
    let mut result = f64::NEG_INFINITY;
    for value in values {
        let n = math_number_arg(value);
        if n.is_nan() {
            return f64::NAN;
        }
        if n > result || (n == 0.0 && result == 0.0 && n.is_sign_positive()) {
            result = n;
        }
    }
    result
}

extern "C" fn math_hypot_thunk(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    let mut result = 0.0;
    for value in global_this_rest_array_values(rest) {
        result = crate::math::js_math_hypot(result, math_number_arg(value).abs());
    }
    result
}

// #2905: thunks for the standard global helper functions. Each coerces its
// arguments the same way the bare-call HIR lowering does and forwards to the
// shared runtime helper so a rebound / property-read reference matches Node.

extern "C" fn global_this_parse_int_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    radix: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_int(s, radix)
}

extern "C" fn global_this_parse_float_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_float(s)
}

extern "C" fn global_this_is_nan_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_nan(value)
}

extern "C" fn global_this_is_finite_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_finite(value)
}

extern "C" fn global_this_encode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri(value))
}

extern "C" fn global_this_decode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri(value))
}

extern "C" fn global_this_encode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri_component(value))
}

extern "C" fn global_this_decode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri_component(value))
}

// #4511: legacy `escape()` / `unescape()` (ES Annex B). Used in the wild by
// `qs` for `%uXXXX` decoding, so any app pulling in `qs` (e.g. via `stripe`)
// needs them as real callable globalThis function values.
extern "C" fn global_this_escape_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_escape(value))
}

extern "C" fn global_this_unescape_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_unescape(value))
}

// #2889: call-form thunks for `Number`/`Boolean` global constructor values.
// `Object`/`String` already have dedicated thunks above; these mirror the
// bare-call HIR lowering (`Expr::NumberCoerce` / `Expr::BooleanCoerce`) so
// `const N = Number; N("42")` and `const B = Boolean; B(0)` match Node.
pub(crate) extern "C" fn global_this_number_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        // `Number()` with no args returns 0; an explicit `undefined` arg → NaN.
        // The closure-call path zero-fills missing args with TAG_UNDEFINED, so
        // we can't distinguish — match the common `Number()` → 0 case.
        return f64::from_bits(crate::value::JSValue::number(0.0).bits());
    }
    crate::builtins::js_number_coerce(value)
}

pub(crate) extern "C" fn global_this_boolean_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let b = crate::value::js_is_truthy(value) != 0;
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

extern "C" fn global_this_error_capture_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    constructor_opt: f64,
) -> f64 {
    crate::error::js_error_capture_stack_trace(target, constructor_opt)
}

/// #2904: `Error.isError(value)` thunk — delegates to the runtime duck-check.
extern "C" fn global_this_error_is_error_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::error::js_error_is_error(value)
}

/// #2904: `Error.prepareStackTrace` default — Node leaves a hook here that
/// formats the stack from structured frames. Perry's stack strings are
/// coarse; the installed default returns the existing `error.stack` string
/// (or empty) so `typeof Error.prepareStackTrace === "function"` holds and
/// callers that invoke it get a usable string rather than a crash.
extern "C" fn global_this_error_prepare_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    error: f64,
    _structured_stack: f64,
) -> f64 {
    let jsval = crate::value::JSValue::from_bits(error.to_bits());
    if jsval.is_pointer() {
        let ptr = crate::value::js_nanbox_get_pointer(error) as *mut crate::error::ErrorHeader;
        if !ptr.is_null() {
            let stack = crate::error::js_error_get_stack(ptr);
            if !stack.is_null() {
                return crate::value::js_nanbox_string(stack as i64);
            }
        }
    }
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    crate::value::js_nanbox_string(empty as i64)
}

pub(super) fn global_this_rest_array_values(rest: f64) -> Vec<f64> {
    let value = crate::value::JSValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return Vec::new();
    }
    let arr = value.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

extern "C" fn function_prototype_call_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    rest: f64,
) -> f64 {
    let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let args = global_this_rest_array_values(rest);
    let (args_ptr, args_len) = if args.is_empty() {
        (std::ptr::null::<f64>(), 0)
    } else {
        (args.as_ptr(), args.len())
    };
    let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
    let result = unsafe { crate::closure::js_native_call_value(target, args_ptr, args_len) };
    IMPLICIT_THIS.with(|c| c.set(prev_this));
    result
}

/// `Function.prototype.bind` as a real callable thunk. Reads the target
/// function from `IMPLICIT_THIS` (set by `.call`/`.apply`/`Reflect.apply`),
/// flattens `(thisArg, ...boundArgs)` into one argument list, and delegates to
/// `js_function_bind` (which builds the BOUND_FUNCTION closure).
///
/// Previously `bind` was installed as a *no-op* proto method, so calling it as
/// a value — `Reflect.apply(Function.prototype.bind, fn, [thisArg])` or
/// `Function.prototype.bind.apply(fn, …)` — returned `undefined` instead of a
/// bound function. The `Function.prototype.call.bind(method)` uncurry idiom in
/// `call-bind-apply-helpers` (used by call-bound → side-channel → qs → Stripe)
/// hit exactly this: `Reflect.apply(bind, call, [fn])` yielded `undefined`.
extern "C" fn function_prototype_bind_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    rest: f64,
) -> f64 {
    let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let mut args: Vec<f64> = Vec::with_capacity(1);
    args.push(this_arg);
    args.extend(global_this_rest_array_values(rest));
    unsafe { crate::closure::js_function_bind(target, args.as_ptr(), args.len()) }
}

extern "C" fn global_this_set_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    delay: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 0) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::js_set_timeout_callback(callback, delay))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_timeout_callback_args(
                callback,
                delay,
                args.as_ptr(),
                args.len() as i32,
            )
        })
    }
}

extern "C" fn global_this_clear_timeout_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_set_interval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    delay: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 1) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::setInterval(callback, delay))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_interval_callback_args(
                callback,
                delay,
                args.as_ptr(),
                args.len() as i32,
            )
        })
    }
}

extern "C" fn global_this_clear_interval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_set_immediate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 2) };
    let args = global_this_rest_array_values(rest);
    if args.is_empty() {
        crate::value::js_nanbox_pointer(crate::timer::js_set_immediate_callback(callback))
    } else {
        crate::value::js_nanbox_pointer(unsafe {
            crate::timer::js_set_immediate_callback_args(callback, args.as_ptr(), args.len() as i32)
        })
    }
}

extern "C" fn global_this_clear_immediate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    crate::timer::js_clear_immediate_value(arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn global_this_queue_microtask_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
) -> f64 {
    let callback = unsafe { crate::timer::js_timer_validate_callback(callback, 3) };
    crate::builtins::js_queue_microtask(callback);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Thunk for `Object.prototype.toString` exposed as a callable closure
/// value. Mirrors `Object.prototype.toString.call(x)` — returns the
/// `"[object Tag]"` string for the receiver in IMPLICIT_THIS.
///
/// Tag detection uses the same coarse NaN-box / GC-type discrimination
/// the rest of the runtime relies on: arrays → `"[object Array]"`,
/// strings → `"[object String]"`, null/undefined → matching tags,
/// numbers/bools/functions → primitive/builtin tags, generic objects →
/// `"[object Object]"`.
///
/// Unblocks ramda's `_isArguments.js` IIFE which evaluates
/// `Object.prototype.toString.call(arguments)` at module-init time
/// — pre-fix the chained `Object.prototype.toString` read returned
/// `undefined`, so the `.call` access threw before the IIFE body ran.
extern "C" fn object_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    // Delegate to the canonical `js_object_to_string` so this callable form
    // (`const f = Object.prototype.toString; f.call(x)`) shares the full brand
    // table (Map/Set/WeakMap/Promise/RegExp/Symbol/BigInt/typed arrays/Date/
    // buffers/…). Previously this thunk duplicated a coarse discrimination that
    // mis-tagged typed arrays as `[object Number]` and everything beyond
    // Array/Error/Date as `[object Object]`.
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    unsafe { crate::object::js_object_to_string(f64::from_bits(this_bits)) }
}

extern "C" fn object_prototype_is_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // RequireObjectCoercible(this): `Object.prototype.isPrototypeOf.call(null)`
    // / `.call(undefined)` must throw a TypeError (spec step 1), matching the
    // sibling Object.prototype methods. Pre-fix this silently returned false.
    let this_jsv = JSValue::from_bits(this_value.to_bits());
    if this_jsv.is_null() || this_jsv.is_undefined() {
        super::object_ops::throw_object_type_error(
            b"Object.prototype.isPrototypeOf called on null or undefined",
        );
    }
    f64::from_bits(
        JSValue::bool(unsafe { super::js_object_is_prototype_of_value(this_value, value) }).bits(),
    )
}

/// #4533: native error subclass constructors whose `[[Prototype]]` is `Error`
/// (their `.prototype.[[Prototype]]` already links to `Error.prototype`).
fn is_native_error_subclass_constructor(name: &str) -> bool {
    matches!(
        name,
        "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
    )
}

extern "C" fn date_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let string = crate::date::js_date_to_string(this_value);
    crate::value::js_nanbox_string(string as i64)
}

extern "C" fn object_prototype_has_own_property_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::object_ops::js_object_has_own(this_value, key)
}

extern "C" fn object_prototype_property_is_enumerable_thunk(
    _closure: *const crate::closure::ClosureHeader,
    key: f64,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    super::js_object_property_is_enumerable(this_value, key)
}

extern "C" fn error_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let this_jsv = crate::value::JSValue::from_bits(this_value.to_bits());
    if !this_jsv.is_pointer() || this_jsv.is_null() || this_jsv.is_undefined() {
        super::object_ops::throw_object_type_error(
            b"Error.prototype.toString called on non-object",
        );
    }
    let raw = crate::value::js_nanbox_get_pointer(this_value) as *const u8;
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        super::object_ops::throw_object_type_error(
            b"Error.prototype.toString called on non-object",
        );
    }

    let name = error_to_string_property(this_value, b"name", "Error");
    let message = error_to_string_property(this_value, b"message", "");
    let result = if name.is_empty() {
        message
    } else if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    };
    let s = crate::string::js_string_from_bytes(result.as_ptr(), result.len() as u32);
    crate::value::js_nanbox_string(s as i64)
}

fn error_to_string_property(this_value: f64, key: &'static [u8], default: &str) -> String {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let obj = crate::value::js_nanbox_get_pointer(this_value) as *const ObjectHeader;
    let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    let value_jsv = crate::value::JSValue::from_bits(value.to_bits());
    if value_jsv.is_undefined() {
        return default.to_string();
    }
    let string = crate::value::js_jsvalue_to_string(value);
    unsafe { string_header_to_owned(string) }
}

unsafe fn string_header_to_owned(ptr: *const crate::StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let len = (*ptr).byte_len as usize;
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

extern "C" fn object_prototype_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::js_object_default_value_of(this_value) }
}

extern "C" fn object_prototype_to_locale_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    unsafe { super::js_object_default_to_locale_string(this_value) }
}

unsafe fn function_apply_args(args_array: f64) -> Vec<f64> {
    let value = JSValue::from_bits(args_array.to_bits());
    if value.is_undefined() || value.is_null() {
        return Vec::new();
    }
    let is_array = JSValue::from_bits(crate::array::js_array_is_array(args_array).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        return Vec::new();
    }
    let arr = if value.is_pointer() {
        value.as_pointer::<crate::array::ArrayHeader>()
    } else if (args_array.to_bits() >> 48) == 0 {
        args_array.to_bits() as *const crate::array::ArrayHeader
    } else {
        std::ptr::null()
    };
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(f64::from_bits(
            crate::array::js_array_get(arr, i as u32).bits(),
        ));
    }
    out
}

extern "C" fn function_prototype_apply_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this_arg: f64,
    args_array: f64,
) -> f64 {
    unsafe {
        let target = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
        let args = function_apply_args(args_array);
        let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
        let result = crate::closure::js_native_call_value(target, args.as_ptr(), args.len());
        IMPLICIT_THIS.with(|c| c.set(prev_this));
        result
    }
}

/// #4101: `Function.prototype.toString` as a real callable thunk. Reads the
/// receiver from `IMPLICIT_THIS` (set by `.call`/`.apply`'s runtime arm), then:
///   • throws a `TypeError` when `this` is not callable (the spec brand check
///     deferred from #4098 — `Function.prototype.toString.call({})`), and
///   • otherwise returns the function's reconstructed source text.
/// A dedicated thunk (rather than the shared no-op) so the brand check is
/// scoped to `Function.prototype.toString` and never fires for the lenient
/// `Object.prototype.toString` (which keeps its own real thunk).
extern "C" fn function_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        0
    };
    if raw == 0 || !crate::closure::is_closure_ptr(raw) {
        super::object_ops::throw_object_type_error(
            b"Function.prototype.toString requires that 'this' be a Function",
        );
    }
    let func_ptr = unsafe { (*(raw as *const crate::closure::ClosureHeader)).func_ptr as usize };
    let s = crate::builtins::function_source_for_func_ptr(func_ptr);
    let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(str_ptr).bits())
}

/// Thunk for `Array.prototype.slice` exposed as a real callable closure
/// value. Reads the array receiver from `IMPLICIT_THIS` (set by
/// `Function.prototype.call`/`.apply`'s runtime arm in
/// `js_native_call_method`) and forwards to the shared slice-value helper.
///
/// Coerces start/end through the shared array slice helper, with
/// `undefined` mapping to `0` for start and end-of-array for end — matching
/// `Array.prototype.slice`'s ECMA-262 defaults.
///
/// Unblocks the `Array.prototype.slice.call(list, …)` pattern that
/// ramda's curry/variadic helpers use heavily (refs `_curry1`,
/// `_curry2`, and every variadic op like `addIndex`/`addIndexRight`/
/// `useWith`/`unapply`/`flip`/`call`). Without this, `Array.prototype.slice`
/// read off the singleton's empty proto object as `undefined` and the
/// chained `.call` access threw
/// `Cannot read properties of undefined (reading 'call')` at module init.
extern "C" fn array_prototype_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    start_val: f64,
    end_val: f64,
) -> f64 {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let arr_ptr = if this_jsv.is_pointer() {
        this_jsv.as_pointer::<crate::array::ArrayHeader>()
    } else {
        // Tolerate raw-i64-encoded array receivers (some module-init
        // call sites stash array pointers in IMPLICIT_THIS without
        // NaN-boxing). The clean_arr_ptr check inside js_array_slice
        // re-validates.
        let raw = this_bits as *const crate::array::ArrayHeader;
        if (raw as usize) > 0x10000 {
            raw
        } else {
            std::ptr::null()
        }
    };
    if arr_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let result = unsafe {
        if let Some(arr) =
            crate::object::arguments_object_to_array(arr_ptr as *const crate::object::ObjectHeader)
        {
            crate::array::js_array_slice_values(arr, start_val, end_val)
        } else {
            crate::array::js_array_slice_values(arr_ptr, start_val, end_val)
        }
    };
    f64::from_bits(crate::value::js_nanbox_pointer(result as i64).to_bits())
}

fn array_buffer_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_array_buffer(raw) {
        Some(raw)
    } else {
        None
    }
}

fn array_buffer_brand_error() -> ! {
    super::object_ops::throw_object_type_error(
        b"Method get ArrayBuffer.prototype.byteLength called on incompatible receiver",
    )
}

extern "C" fn array_buffer_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match array_buffer_receiver_addr() {
        Some(addr) => {
            let buf = addr as *const crate::buffer::BufferHeader;
            f64::from_bits(
                crate::value::JSValue::number(crate::buffer::js_buffer_length(buf) as f64).bits(),
            )
        }
        None => array_buffer_brand_error(),
    }
}

extern "C" fn array_buffer_is_view_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    let addr = if jv.is_pointer() {
        (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if value.to_bits() >> 48 == 0 && value.to_bits() > 0x10000 {
        value.to_bits() as usize
    } else {
        0
    };
    let is_view = (addr != 0
        && !crate::buffer::is_any_array_buffer(addr)
        && (crate::buffer::is_uint8array_buffer(addr) || crate::buffer::is_data_view(addr)))
        || jsvalue_extends_data_view(value)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some();
    f64::from_bits(crate::value::JSValue::bool(is_view).bits())
}

fn jsvalue_extends_data_view(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr) {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let obj = ptr as *const ObjectHeader;
        let class_id = (*obj).class_id;
        class_id != 0 && crate::object::extends_builtin_data_view(class_id)
    }
}

/// Resolve the `IMPLICIT_THIS` receiver to a `(typed-array ptr, kind)` if it
/// is a typed array, else `None`. Backs the `%TypedArray%.prototype` accessor
/// getters installed for reflection (#2060) — these fire when user code does
/// `desc.get.call(int8arr)` after pulling the descriptor out via
/// `Object.getOwnPropertyDescriptor`. Mirrors the receiver-extraction the
/// `Array.prototype.slice` thunk uses (NaN-boxed pointer or raw-i64 form).
fn typed_array_receiver() -> Option<(*const crate::typedarray::TypedArrayHeader, u8)> {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    let kind = crate::typedarray::lookup_typed_array_kind(raw)?;
    Some((raw as *const crate::typedarray::TypedArrayHeader, kind))
}

fn typed_array_brand_error() -> ! {
    super::object_ops::throw_object_type_error(
        b"Method get %TypedArray%.prototype accessor called on incompatible receiver",
    )
}

fn string_value_to_owned(value: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let s = crate::builtins::js_string_coerce(value);
    if s.is_null() {
        return None;
    }
    unsafe {
        let bytes = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let len = (*s).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(bytes, len))
            .ok()
            .map(ToOwned::to_owned)
    }
}

fn typed_array_constructor_this_kind() -> Option<u8> {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let ptr = crate::value::js_nanbox_get_pointer(this_value) as usize;
    if ptr == 0 || !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    let name_value = crate::closure::closure_get_dynamic_prop(ptr, "name");
    let name = string_value_to_owned(f64::from_bits(name_value.to_bits()))?;
    crate::typedarray::kind_for_name(&name)
}

fn typed_array_buffer_value(ta: *const crate::typedarray::TypedArrayHeader) -> f64 {
    let buf = crate::typedarray::typed_array_to_array_buffer(ta);
    if buf.is_null() {
        typed_array_brand_error();
    }
    crate::value::js_nanbox_pointer(buf as i64)
}

/// `%TypedArray%.prototype.length` getter — element count of the receiver.
extern "C" fn typed_array_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, _)) => {
            let len = crate::typedarray::js_typed_array_length(ta);
            f64::from_bits(crate::value::JSValue::number(len as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteLength` getter — `length * BYTES_PER_ELEMENT`.
extern "C" fn typed_array_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, kind)) => {
            let len = crate::typedarray::js_typed_array_length(ta) as usize;
            let elem_size = crate::typedarray::elem_size_for_kind(kind);
            f64::from_bits(crate::value::JSValue::number((len * elem_size) as f64).bits())
        }
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.byteOffset` getter — always 0 (Perry views are not
/// backed by an offset into a shared `ArrayBuffer`).
extern "C" fn typed_array_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some(_) => f64::from_bits(crate::value::JSValue::number(0.0).bits()),
        None => typed_array_brand_error(),
    }
}

/// `%TypedArray%.prototype.buffer` getter. Perry does not yet model a
/// first-class `ArrayBuffer` behind a view, so this returns `undefined` for
/// now (matching the existing `int8arr.buffer` data-path behavior). The
/// accessor still exists so reflection sees a real getter — closing the
/// `getOwnPropertyDescriptor(...).get` cascade in #2060.
extern "C" fn typed_array_buffer_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    match typed_array_receiver() {
        Some((ta, _)) => typed_array_buffer_value(ta),
        None => typed_array_brand_error(),
    }
}

/// Install the four `%TypedArray%.prototype` accessor descriptors
/// (`length`, `byteLength`, `byteOffset`, `buffer`) on a typed-array
/// constructor's prototype object so `Object.getOwnPropertyDescriptor`
/// reflects them as `{ get, set: undefined, enumerable: false,
/// configurable: true }`. #2060.
fn install_typed_array_proto_accessors(proto_obj: *mut ObjectHeader) {
    unsafe {
        // 0-arg getters: `.call(this)` forwards 0 user args.
        let mk = |f: *const u8| -> u64 {
            crate::closure::js_register_closure_arity(f, 0);
            let c = crate::closure::js_closure_alloc(f, 0);
            if c.is_null() {
                0
            } else {
                crate::value::js_nanbox_pointer(c as i64).to_bits()
            }
        };
        install_builtin_getter(
            proto_obj,
            "length",
            mk(typed_array_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteLength",
            mk(typed_array_byte_length_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "byteOffset",
            mk(typed_array_byte_offset_getter_thunk as *const u8),
        );
        install_builtin_getter(
            proto_obj,
            "buffer",
            mk(typed_array_buffer_getter_thunk as *const u8),
        );
    }
}

/// Install `%Function.prototype% [ @@hasInstance ]` (#3662). Pre-fix this was
/// `undefined` — `typeof Function.prototype[Symbol.hasInstance]` reported
/// "undefined", a reflective `.call` threw, and a class with a custom
/// `static [Symbol.hasInstance]` was the only way to reach the protocol. The
/// method is keyed by the real well-known `Symbol.hasInstance` (not an
/// `@@`-string own property, which would leak into `getOwnPropertyNames`).
fn install_function_has_instance_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        let func_ptr = super::instanceof::function_prototype_has_instance_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 1);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, "[Symbol.hasInstance]");
        super::native_module::set_builtin_closure_length(closure as usize, 1);
        let sym = crate::symbol::well_known_symbol("hasInstance");
        if sym.is_null() {
            return;
        }
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        let sym_value = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
        let fn_value = f64::from_bits(crate::value::js_nanbox_pointer(closure as i64).to_bits());
        crate::symbol::js_object_set_symbol_property(proto_value, sym_value, fn_value);
    }
}

fn install_typed_array_iterator_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    install_proto_method(
        proto_obj,
        "values",
        global_this_builtin_noop_thunk as *const u8,
        0,
    );
    unsafe {
        let values_key = crate::string::js_string_from_bytes(b"values".as_ptr(), 6);
        let values = js_object_get_field_by_name(proto_obj, values_key);
        let iter = crate::symbol::well_known_symbol("iterator");
        if !iter.is_null() && values.bits() != crate::value::TAG_UNDEFINED {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            let iter_value =
                f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits());
            crate::symbol::js_object_set_symbol_property(
                proto_value,
                iter_value,
                f64::from_bits(values.bits()),
            );
        }
    }
}

/// Allocate the shared `%TypedArray%` intrinsic constructor (a closure) and
/// its `.prototype` object, cache both in the GC-rooted atomics, and wire the
/// closure's `prototype` dynamic-prop to point at the shared prototype.
///
/// Spec: `%TypedArray%` is the abstract parent constructor for `Int8Array`,
/// `Uint8Array`, … — `Int8Array.__proto__ === %TypedArray%` and
/// `Object.getPrototypeOf(Int8Array.prototype) === %TypedArray%.prototype`.
/// Perry didn't model this before #2145, so test262's TypedArray-prototype
/// walks read `null.prototype` and the constructor's `__proto__` returned the
/// `0.0` no-value placeholder (`typeof Int8Array.__proto__ === "number"`).
///
/// Idempotent: subsequent calls return the cached pointer. Called from
/// `populate_global_this_builtins` (single-threaded under the singleton CAS),
/// so the AtomicI64 stores don't need to race-resolve.
fn ensure_typed_array_intrinsic() -> (*mut crate::closure::ClosureHeader, *mut ObjectHeader) {
    let existing_ctor = crate::object::TYPED_ARRAY_INTRINSIC_PTR.load(Ordering::Acquire);
    let existing_proto = crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if existing_ctor != 0 && existing_proto != 0 {
        return (
            existing_ctor as *mut crate::closure::ClosureHeader,
            existing_proto as *mut ObjectHeader,
        );
    }
    let ctor = crate::closure::js_closure_alloc(typed_array_constructor_call_thunk as *const u8, 0);
    let proto = js_object_alloc(0, 0);
    if ctor.is_null() || proto.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    crate::closure::js_register_closure_arity(typed_array_constructor_call_thunk as *const u8, 0);
    super::native_module::set_bound_native_closure_name(ctor, "TypedArray");
    super::native_module::set_builtin_closure_length(ctor as usize, 0);
    super::set_builtin_property_attrs(
        ctor as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        ctor as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    // Wire `%TypedArray%.prototype` so `getPrototypeOf(Int8Array).prototype`
    // hits a real object instead of undefined.
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, proto_key, proto_value);
    super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::PropertyAttrs::new(false, false, false),
    );
    // #2060: the four reflectable `length`/`byteLength`/`byteOffset`/`buffer`
    // accessor descriptors are own properties of `%TypedArray%.prototype` per
    // spec, NOT of the per-kind proto. Pre-#2145 they were installed on each
    // per-kind proto because `getPrototypeOf(per_kind_proto)` returned the
    // per-kind proto itself (identity), so the same lookup happened to land
    // there. After #2145 wires the per-kind protos to share the intrinsic
    // proto, the descriptors must live on the intrinsic itself for
    // `Object.getOwnPropertyDescriptor(getPrototypeOf(Int8Array.prototype),
    // "length")` to keep working.
    install_typed_array_proto_accessors(proto);
    install_typed_array_iterator_symbol(proto);
    // The per-kind prototypes (`Int8Array.prototype`, …) inherit ALL of their
    // methods from this shared `%TypedArray%.prototype` (their `[[Prototype]]`),
    // so `Int8Array.prototype.hasOwnProperty("map") === false` and
    // `Int8Array.prototype.map === %TypedArray%.prototype.map` (test262's
    // `prototype/*/inherited.js`).
    //
    // NOTE: the generic `Object.prototype` data methods + a `toString` are
    // intentionally NOT installed here. The intrinsic prototype is allocated
    // with zero inline field slots and already carries ~34 own properties
    // (accessors + `@@iterator` + the spec methods below); adding the extra ~6
    // crosses an inline-storage boundary that trips a latent field-count
    // overflow (a heap-layout-dependent SIGSEGV under GC pressure). They are not
    // needed for parity — `toLocaleString` is already a brand-checking method
    // below, and `hasOwnProperty`/`valueOf`/etc. dispatch natively on instances.
    // Install the brand-checking spec methods on the shared `%TypedArray%`
    // intrinsic prototype. test262's `testTypedArray.js` harness reads
    // `TypedArray.prototype.<m>` (where `TypedArray ===
    // Object.getPrototypeOf(Int8Array)`), so the brand check for
    // `%TypedArray%.prototype.<m>.call(badReceiver)` must fire when the method
    // is read off the intrinsic, and the per-kind protos resolve their reads
    // here via the `[[Prototype]]` chain.
    typed_array_proto_thunks::install_typed_array_proto_methods(proto);
    install_constructor_static_with_call_arity(
        ctor,
        "from",
        typed_array_from_thunk as *const u8,
        1,
        3,
        false,
    );
    install_constructor_static(ctor, "of", typed_array_of_thunk as *const u8, 0, true);
    crate::object::TYPED_ARRAY_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
    (ctor, proto)
}

/// Public accessor for the `%TypedArray%.prototype` object. Returns the cached
/// pointer if `populate_global_this_builtins` has run (so the intrinsic is
/// initialised), else null. Used by `js_object_get_prototype_of` to resolve
/// `Object.getPrototypeOf(Int8Array.prototype)` to the shared prototype.
pub(crate) fn typed_array_intrinsic_proto_ptr() -> *mut ObjectHeader {
    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as *mut ObjectHeader
}

// ---------------------------------------------------------------------------
// #3664: generator / async-generator intrinsic prototype towers.
// ---------------------------------------------------------------------------

/// Distinguishes plain vs async generator closures for the intrinsic-tower
/// lookups.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratorKind {
    Sync,
    Async,
}

/// Classify a `GC_TYPE_CLOSURE` pointer as a (plain | async) generator
/// function, or `None` for any other closure. Async generators register in
/// BOTH the generator and async registries (the lowering carries `is_async &&
/// is_generator`), so async-registry membership disambiguates the two.
fn closure_generator_kind(closure_ptr: usize) -> Option<GeneratorKind> {
    let closure = closure_ptr as *const crate::closure::ClosureHeader;
    let func_ptr = crate::closure::get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return None;
    }
    // Async generators are registered in BOTH registries (they share the sync
    // generator's `{next,return,throw}` lowering), so check the async-generator
    // registry first — it's the only signal that disambiguates the two.
    if crate::closure::is_registered_async_generator_function(func_ptr) {
        Some(GeneratorKind::Async)
    } else if crate::closure::is_registered_generator_function(func_ptr) {
        Some(GeneratorKind::Sync)
    } else {
        None
    }
}

fn intrinsic_pointer_value(slot: i64) -> Option<f64> {
    if slot != 0 {
        Some(crate::value::js_nanbox_pointer(slot))
    } else {
        None
    }
}

/// `Object.getPrototypeOf(g)` for a generator-function closure `g` →
/// `%Generator%` / `%AsyncGenerator%` (a.k.a. `<Ctor>.prototype`). Returns
/// `None` for non-generator closures so the caller keeps its existing
/// `closure_static_prototype` / null resolution. (#3664)
pub(crate) fn generator_function_proto_of(closure_ptr: usize) -> Option<f64> {
    let kind = closure_generator_kind(closure_ptr)?;
    // The towers are normally built in `populate_global_this_builtins`, but a
    // program that reflects on a generator without ever touching `globalThis`
    // would otherwise see null. Build lazily (idempotent) on first use.
    ensure_generator_intrinsics();
    let slot = match kind {
        GeneratorKind::Sync => crate::object::GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire),
        GeneratorKind::Async => {
            crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire)
        }
    };
    intrinsic_pointer_value(slot)
}

/// `g.constructor` for a generator-function closure `g` → `%GeneratorFunction%`
/// / `%AsyncGeneratorFunction%`. `None` for non-generator closures. (#3664)
pub(crate) fn generator_function_constructor_of(closure_ptr: usize) -> Option<f64> {
    let proto = generator_function_proto_of(closure_ptr)?;
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader;
    if proto_ptr.is_null() {
        return None;
    }
    let key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    let value = js_object_get_field_by_name(proto_ptr, key);
    Some(f64::from_bits(value.bits()))
}

/// `g.prototype` for a generator-function closure `g`: a lazily-created object
/// whose `[[Prototype]]` is `%Generator.prototype%` / `%AsyncGenerator.prototype%`,
/// cached as the closure's own `prototype` dynamic-prop so the identity is
/// stable across reads (`g.prototype === g.prototype`). Returns `None` for
/// non-generator closures (their `.prototype` keeps its existing behaviour).
/// A live generator instance's `[[Prototype]]` is set to this object (Phase 3b),
/// completing the spec chain `g() → g.prototype → %Generator.prototype%`. (#3664)
pub(crate) fn generator_function_prototype_of(closure_ptr: usize) -> Option<f64> {
    let kind = closure_generator_kind(closure_ptr)?;
    // A previously-created (or user-assigned) `prototype` wins — preserves
    // identity and lets `g.prototype = X` overrides stick.
    let existing = crate::closure::closure_get_dynamic_prop(closure_ptr, "prototype");
    if existing.to_bits() != crate::value::TAG_UNDEFINED {
        return Some(f64::from_bits(existing.to_bits()));
    }
    ensure_generator_intrinsics();
    let gen_proto = generator_prototype_ptr(matches!(kind, GeneratorKind::Async));
    let obj = js_object_alloc(0, 0);
    if obj.is_null() {
        return None;
    }
    if !gen_proto.is_null() {
        let proto_bits = crate::value::js_nanbox_pointer(gen_proto as i64).to_bits();
        super::prototype_chain::object_set_static_prototype(obj as usize, proto_bits);
    }
    let obj_value = crate::value::js_nanbox_pointer(obj as i64);
    crate::closure::closure_set_dynamic_prop(closure_ptr, "prototype", obj_value);
    Some(obj_value)
}

/// `%Generator.prototype%` / `%AsyncGenerator.prototype%` pointer (the object
/// carrying `next`/`return`/`throw`). Used by Phase 2/3 to wire `g.prototype`'s
/// `[[Prototype]]` and the live generator-object chain. Null until
/// `populate_global_this_builtins` has run. (#3664)
pub(crate) fn generator_prototype_ptr(is_async: bool) -> *mut ObjectHeader {
    ensure_generator_intrinsics();
    let slot = if is_async {
        crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire)
    } else {
        crate::object::GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire)
    };
    slot as *mut ObjectHeader
}

/// Set a data property on an intrinsic object and record its descriptor attrs
/// for `Object.getOwnPropertyDescriptor` reflection. (#3664)
fn set_intrinsic_data_prop(
    obj: *mut ObjectHeader,
    name: &str,
    value: f64,
    attrs: super::PropertyAttrs,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
    super::set_builtin_property_attrs(obj as usize, name.to_string(), attrs);
}

/// Set `obj[Symbol.toStringTag] = tag` (the descriptor is the spec default
/// `{ writable:false, enumerable:false, configurable:true }`). (#3664)
fn set_intrinsic_to_string_tag(obj: *mut ObjectHeader, tag: &str) {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    let tag_str = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
            f64::from_bits(crate::js_nanbox_string(tag_str as i64).to_bits()),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        obj as usize,
        sym as usize,
        super::PropertyAttrs::new(false, false, true),
    );
}

/// Build a `TypeError` value for a `%Generator.prototype%` method invoked on a
/// receiver that isn't a generator object (NaN-boxed pointer, not thrown). (#3664)
fn generator_receiver_type_error_value(method: &[u8]) -> f64 {
    let mut msg = b"Generator.prototype.".to_vec();
    msg.extend_from_slice(method);
    msg.extend_from_slice(b" called on incompatible receiver");
    let h = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(h);
    crate::value::js_nanbox_pointer(err as i64)
}

/// Shared body for `%Generator.prototype%`/`%AsyncGenerator.prototype%`'s
/// `next`/`return`/`throw`. These prototype methods exist so test262's
/// brand-check cases (`GeneratorPrototype.next.call(nonGenerator)`) and method-
/// identity reads resolve. The real state machine lives in each generator
/// instance's OWN `next`/`return`/`throw` closures (Perry lowers a generator
/// call to a `{next,return,throw}` object), so for a valid receiver we delegate
/// to the instance's own same-named method. Normal `iter.next()` reads the own
/// property directly and never reaches here, so generator execution is
/// unaffected.
///
/// `is_async` selects the spec's incompatible-receiver behaviour: sync
/// generators throw a `TypeError` synchronously, async generators return a
/// rejected promise (their methods always return promises). (#3664)
fn generator_proto_method(method: &[u8], arg: f64, is_async: bool) -> f64 {
    let bad_receiver = |method: &[u8]| -> f64 {
        let errv = generator_receiver_type_error_value(method);
        if is_async {
            let promise = crate::promise::js_promise_rejected(errv);
            crate::value::js_nanbox_pointer(promise as i64)
        } else {
            crate::exception::js_throw(errv)
        }
    };
    let this = crate::object::js_implicit_this_get();
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return bad_receiver(method);
    }
    let this_obj = jv.as_pointer::<ObjectHeader>();
    // Reject the prototype singletons themselves: they carry these methods as
    // OWN thunks, so delegating below would re-enter this thunk forever. A real
    // generator instance is never the prototype object.
    if this_obj == generator_prototype_ptr(false) || this_obj == generator_prototype_ptr(true) {
        return bad_receiver(method);
    }
    // Brand-check + delegation use OWN properties only. A generator instance
    // (Perry's `{next,return,throw}` object) owns all three state-machine
    // closures; an object that merely INHERITS them (e.g. `g.prototype`, whose
    // [[Prototype]] is `%Generator.prototype%`) is not a generator — and reading
    // the inherited method would resolve back to this very thunk and recurse.
    let own_method = |name: &[u8]| -> Option<*const crate::closure::ClosureHeader> {
        let v = crate::object::js_object_get_own_field_or_undef(this, name.as_ptr(), name.len());
        let vv = JSValue::from_bits(v.to_bits());
        if vv.is_pointer() && crate::closure::is_closure_ptr(vv.as_pointer::<u8>() as usize) {
            Some(vv.as_pointer::<crate::closure::ClosureHeader>())
        } else {
            None
        }
    };
    if own_method(b"next").is_none()
        || own_method(b"return").is_none()
        || own_method(b"throw").is_none()
    {
        return bad_receiver(method);
    }
    // A sync generator instance also owns `next`/`return`/`throw`, so the
    // structural check above can't tell it from an async generator. The
    // `%AsyncGenerator.prototype%` methods must reject a sync-generator `this`
    // (and vice versa): gate on the async request-queue brand.
    if is_async
        != super::async_generator_queue::is_async_generator_instance(this_obj as *mut ObjectHeader)
    {
        return bad_receiver(method);
    }
    match own_method(method) {
        Some(own_closure) => crate::closure::js_closure_call1(own_closure, arg),
        None => bad_receiver(method),
    }
}

extern "C" fn generator_proto_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"next", arg, false)
}
extern "C" fn generator_proto_return_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"return", arg, false)
}
extern "C" fn generator_proto_throw_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"throw", arg, false)
}
extern "C" fn async_generator_proto_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"next", arg, true)
}
extern "C" fn async_generator_proto_return_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"return", arg, true)
}
extern "C" fn async_generator_proto_throw_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"throw", arg, true)
}

/// `%AsyncGenerator.prototype%[Symbol.asyncIterator]()` returns `this` (spec
/// inherits this from `%AsyncIteratorPrototype%`). Without it, `for await` /
/// `GetIterator(obj, async)` over a generator instance can't obtain the async
/// iterator and either throws or silently produces nothing.
extern "C" fn async_generator_proto_async_iterator_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::object::js_implicit_this_get()
}

/// Install a well-known-symbol-keyed method (returning `this`) on a
/// generator/async-generator prototype, with the spec descriptor shape
/// (`name`/`length` own props, non-enumerable value).
fn install_proto_symbol_self_method(
    proto: *mut ObjectHeader,
    symbol_name: &str,
    display_name: &str,
    func_ptr: *const u8,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    super::native_module::set_bound_native_closure_name(closure, display_name);
    super::native_module::set_builtin_closure_length(closure as usize, 0);
    let configurable = super::PropertyAttrs::new(false, false, true);
    super::set_builtin_property_attrs(closure as usize, "name".to_string(), configurable);
    super::set_builtin_property_attrs(closure as usize, "length".to_string(), configurable);
    let sym = crate::symbol::well_known_symbol(symbol_name);
    if sym.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            crate::value::js_nanbox_pointer(closure as i64),
        );
    }
}

/// #4141: link a freshly-built generator/async-generator instance object into
/// the spec `[[Prototype]]` chain. Perry lowers `gen()` to a `{next,return,
/// throw}` object literal; this interposes a fresh intermediate object (the
/// per-instance stand-in for `g.prototype`) as the instance's `[[Prototype]]`,
/// whose own `[[Prototype]]` is `%Generator.prototype%` /
/// `%AsyncGenerator.prototype%`. The result is the two-hop chain Node exposes:
/// `Object.getPrototypeOf(gen())` → intermediate →
/// `Object.getPrototypeOf(...)` → the brand-checked prototype carrying
/// `next`/`return`/`throw`.
///
/// Returns `obj` unchanged so codegen can use it inline in return position.
/// GC: both links go through `object_set_static_prototype`, whose side-table is
/// traced + pointer-rewritten by the collector (see `prototype_chain.rs`), so
/// the intermediate stays live as long as the instance does and dies with it.
#[no_mangle]
pub extern "C" fn js_generator_attach_prototype(obj: f64, is_async: i32) -> f64 {
    let jv = JSValue::from_bits(obj.to_bits());
    if !jv.is_pointer() {
        return obj;
    }
    let obj_ptr = jv.as_pointer::<u8>() as usize;
    if obj_ptr == 0 {
        return obj;
    }
    if is_async != 0 {
        super::async_generator_queue::wrap_async_generator_instance(obj_ptr as *mut ObjectHeader);
    }
    let gen_proto = generator_prototype_ptr(is_async != 0);
    if gen_proto.is_null() {
        return obj;
    }
    // Intermediate object stands in for `g.prototype`: own `[[Prototype]]` is
    // `%Generator.prototype%`, carries no own methods (the instance inherits
    // `next`/`return`/`throw` from the brand-checked prototype two hops up).
    let intermediate = js_object_alloc(0, 0);
    if intermediate.is_null() {
        return obj;
    }
    let gen_proto_bits = crate::value::js_nanbox_pointer(gen_proto as i64).to_bits();
    super::prototype_chain::object_set_static_prototype(intermediate as usize, gen_proto_bits);
    let intermediate_bits = crate::value::js_nanbox_pointer(intermediate as i64).to_bits();
    super::prototype_chain::object_set_static_prototype(obj_ptr, intermediate_bits);
    obj
}

/// Link a generator/async-generator instance to the concrete generator
/// function closure's cached `.prototype` object. This is the identity path
/// Node exposes for `Object.getPrototypeOf(g()) === g.prototype`; the
/// fallback `js_generator_attach_prototype` above is used when codegen cannot
/// see the owning closure.
#[no_mangle]
pub extern "C" fn js_generator_attach_closure_prototype(
    obj: f64,
    closure_ptr: *const crate::closure::ClosureHeader,
) -> f64 {
    let jv = JSValue::from_bits(obj.to_bits());
    if !jv.is_pointer() {
        return obj;
    }
    let obj_ptr = jv.as_pointer::<u8>() as usize;
    if obj_ptr == 0 {
        return obj;
    }

    let closure = crate::closure::clean_closure_ptr(closure_ptr);
    if closure.is_null() || crate::closure::get_valid_func_ptr(closure).is_null() {
        return obj;
    }

    // Async-generator instances need the request-queue wrapper installed on
    // their `next`/`return`/`throw` so same-stack follow-up calls queue (spec
    // AsyncGeneratorEnqueue) and `.return(v)` awaits `v`. The non-closure
    // fallback (`js_generator_attach_prototype`) does this when codegen knows
    // the function is async; on the closure-identity path we read the async
    // brand from the function's registration (the `async function*` wrapper
    // symbol is recorded via `js_register_closure_async_generator_function`).
    if crate::closure::is_registered_async_generator_function(crate::closure::get_valid_func_ptr(
        closure,
    )) {
        super::async_generator_queue::wrap_async_generator_instance(obj_ptr as *mut ObjectHeader);
    }

    let Some(proto) = generator_function_prototype_of(closure as usize) else {
        return obj;
    };
    let proto_jv = JSValue::from_bits(proto.to_bits());
    if !proto_jv.is_pointer() {
        return obj;
    }

    super::prototype_chain::object_set_static_prototype(obj_ptr, proto.to_bits());
    obj
}

/// Build one generator-intrinsic tower (sync or async) and store its three
/// objects in the GC-rooted atomics declared in `object/mod.rs`.
///
/// Spec chain (sync names; async mirrors with the `Async` prefix):
/// ```text
/// %GeneratorFunction%             ctor closure, name "GeneratorFunction", length 1
///   .prototype = %Generator%      (non-writable, non-enumerable, non-configurable)
/// %Generator%  (= %GeneratorFunction.prototype%)
///   .constructor = %GeneratorFunction%      (non-writable, non-enum, configurable)
///   .prototype   = %Generator.prototype%    (non-writable, non-enum, configurable)
///   [Symbol.toStringTag] = "GeneratorFunction"
/// %Generator.prototype%  (= %GeneratorFunction.prototype.prototype%)
///   .constructor = %Generator%              (non-writable, non-enum, configurable)
///   .next / .return / .throw                (Phase 1: noop-backed for descriptor tests)
///   [Symbol.toStringTag] = "Generator"
/// ```
fn build_generator_tower(
    is_async: bool,
    ctor_slot: &std::sync::atomic::AtomicI64,
    proto_slot: &std::sync::atomic::AtomicI64,
    gen_proto_slot: &std::sync::atomic::AtomicI64,
) {
    let (ctor_name, ctor_tag, inst_tag) = if is_async {
        (
            "AsyncGeneratorFunction",
            "AsyncGeneratorFunction",
            "AsyncGenerator",
        )
    } else {
        ("GeneratorFunction", "GeneratorFunction", "Generator")
    };
    let noop = global_this_builtin_noop_thunk as *const u8;
    let ctor = crate::closure::js_closure_alloc(noop, 0);
    let proto = js_object_alloc(0, 0); // %Generator% / %AsyncGenerator%
    let gen_proto = js_object_alloc(0, 0); // %Generator.prototype%
    if ctor.is_null() || proto.is_null() || gen_proto.is_null() {
        return;
    }
    let non_writable = super::PropertyAttrs::new(false, false, false);
    let configurable = super::PropertyAttrs::new(false, false, true);

    // --- %GeneratorFunction% constructor ---
    crate::closure::js_register_closure_arity(noop, 1);
    super::native_module::set_bound_native_closure_name(ctor, ctor_name);
    super::native_module::set_builtin_closure_length(ctor as usize, 1);
    super::set_builtin_property_attrs(ctor as usize, "name".to_string(), configurable);
    super::set_builtin_property_attrs(ctor as usize, "length".to_string(), configurable);
    set_intrinsic_data_prop(
        ctor as *mut ObjectHeader,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
        non_writable,
    );

    // --- %Generator% (= %GeneratorFunction.prototype%) ---
    set_intrinsic_data_prop(
        proto,
        "constructor",
        crate::value::js_nanbox_pointer(ctor as i64),
        configurable,
    );
    set_intrinsic_data_prop(
        proto,
        "prototype",
        crate::value::js_nanbox_pointer(gen_proto as i64),
        configurable,
    );
    set_intrinsic_to_string_tag(proto, ctor_tag);

    // --- %Generator.prototype% ---
    set_intrinsic_data_prop(
        gen_proto,
        "constructor",
        crate::value::js_nanbox_pointer(proto as i64),
        configurable,
    );
    let (next_thunk, return_thunk, throw_thunk) = if is_async {
        (
            async_generator_proto_next_thunk as *const u8,
            async_generator_proto_return_thunk as *const u8,
            async_generator_proto_throw_thunk as *const u8,
        )
    } else {
        (
            generator_proto_next_thunk as *const u8,
            generator_proto_return_thunk as *const u8,
            generator_proto_throw_thunk as *const u8,
        )
    };
    install_proto_method(gen_proto, "next", next_thunk, 1);
    install_proto_method(gen_proto, "return", return_thunk, 1);
    install_proto_method(gen_proto, "throw", throw_thunk, 1);
    // Spec: `%AsyncGenerator.prototype%` inherits `[Symbol.asyncIterator]` from
    // `%AsyncIteratorPrototype%` (returning `this`). Without it, `for await (x of
    // gen())` over an async-generator *method instance* can't resolve the async
    // iterator and hangs/yields nothing (the instance carries no own iterator
    // symbol). The async-iterator-acquisition path (`js_get_async_iterator`)
    // sets the implicit-this before invoking this thunk, so it returns the
    // generator instance.
    //
    // Note: the SYNC `%Generator.prototype%` deliberately gets NO own
    // `[Symbol.iterator]` here — the sync `for-of` iterator-acquisition path
    // (`js_get_iterator`) does NOT bind implicit-this before invoking a
    // `[Symbol.iterator]` method, so a `this`-returning thunk would resolve to
    // `undefined` and break `for (x of gen())`. Sync generators already iterate
    // through their own `next` via the builtin-iterator recognizers.
    if is_async {
        install_proto_symbol_self_method(
            gen_proto,
            "asyncIterator",
            "[Symbol.asyncIterator]",
            async_generator_proto_async_iterator_thunk as *const u8,
        );
    }
    set_intrinsic_to_string_tag(gen_proto, inst_tag);

    ctor_slot.store(ctor as i64, Ordering::Release);
    proto_slot.store(proto as i64, Ordering::Release);
    gen_proto_slot.store(gen_proto as i64, Ordering::Release);
}

/// Build both generator intrinsic towers. Idempotent; called once from
/// `populate_global_this_builtins` under the globalThis singleton CAS. (#3664)
fn ensure_generator_intrinsics() {
    if crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) == 0 {
        build_generator_tower(
            false,
            &crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::GENERATOR_INTRINSIC_PROTO_PTR,
            &crate::object::GENERATOR_PROTOTYPE_PTR,
        );
    }
    if crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) == 0 {
        build_generator_tower(
            true,
            &crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
            &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
        );
    }
}

fn install_math_namespace(ns_obj: *mut ObjectHeader) {
    if ns_obj.is_null() {
        return;
    }
    for (name, func_ptr, arity) in [
        ("abs", math_abs_thunk as *const u8, 1),
        ("acos", math_acos_thunk as *const u8, 1),
        ("acosh", math_acosh_thunk as *const u8, 1),
        ("asin", math_asin_thunk as *const u8, 1),
        ("asinh", math_asinh_thunk as *const u8, 1),
        ("atan", math_atan_thunk as *const u8, 1),
        ("atanh", math_atanh_thunk as *const u8, 1),
        ("atan2", math_atan2_thunk as *const u8, 2),
        ("ceil", math_ceil_thunk as *const u8, 1),
        ("cbrt", math_cbrt_thunk as *const u8, 1),
        ("expm1", math_expm1_thunk as *const u8, 1),
        ("clz32", math_clz32_thunk as *const u8, 1),
        ("cos", math_cos_thunk as *const u8, 1),
        ("cosh", math_cosh_thunk as *const u8, 1),
        ("exp", math_exp_thunk as *const u8, 1),
        ("floor", math_floor_thunk as *const u8, 1),
        ("fround", math_fround_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
    install_proto_method_rest_with_length(ns_obj, "hypot", math_hypot_thunk as *const u8, 2, 0);
    for (name, func_ptr, arity) in [
        ("imul", math_imul_thunk as *const u8, 2),
        ("log", math_log_thunk as *const u8, 1),
        ("log1p", math_log1p_thunk as *const u8, 1),
        ("log2", math_log2_thunk as *const u8, 1),
        ("log10", math_log10_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
    install_proto_method_rest_with_length(ns_obj, "max", math_max_thunk as *const u8, 2, 0);
    install_proto_method_rest_with_length(ns_obj, "min", math_min_thunk as *const u8, 2, 0);
    for (name, func_ptr, arity) in [
        ("pow", math_pow_thunk as *const u8, 2),
        ("random", math_random_thunk as *const u8, 0),
        ("round", math_round_thunk as *const u8, 1),
        ("sign", math_sign_thunk as *const u8, 1),
        ("sin", math_sin_thunk as *const u8, 1),
        ("sinh", math_sinh_thunk as *const u8, 1),
        ("sqrt", math_sqrt_thunk as *const u8, 1),
        ("tan", math_tan_thunk as *const u8, 1),
        ("tanh", math_tanh_thunk as *const u8, 1),
        ("trunc", math_trunc_thunk as *const u8, 1),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }

    let constant_attrs = super::PropertyAttrs::new(false, false, false);
    for (name, value) in [
        ("E", std::f64::consts::E),
        ("LN10", std::f64::consts::LN_10),
        ("LN2", std::f64::consts::LN_2),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("PI", std::f64::consts::PI),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ] {
        set_intrinsic_data_prop(ns_obj, name, value, constant_attrs);
    }

    install_proto_method(ns_obj, "f16round", math_f16round_thunk as *const u8, 1);
}

// ---- TC39 Temporal namespace (#4686) -------------------------------------
//
// Each `Temporal.<Type>` constructor is a constructable native closure hung off
// the `Temporal` namespace object. `new Temporal.Duration(...)` resolves the
// closure via a normal property read, then `js_new_function_construct` invokes
// it; the thunk allocates a Temporal cell and returns it, which overrides the
// empty default `this` (see `constructor_return_overrides_this`). Statics
// (`from`, `compare`) are installed on the constructor closure with call-arity
// 0 so every argument lands in the rest array the thunk reads.

extern "C" fn temporal_duration_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_duration_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_duration_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::duration::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_instant_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_instant_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_instant_from_epoch_ms_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_epoch_milliseconds_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_instant_from_epoch_ns_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::from_epoch_nanoseconds_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_instant_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::instant::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_time::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_date_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_date_time::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_year_month_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_year_month_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_year_month_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_year_month::compare_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_month_day_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_month_day::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_plain_month_day_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::plain_month_day::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_zoned_date_time_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::construct(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_zoned_date_time_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::from_static(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_zoned_date_time_compare_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::zoned_date_time::compare_static(&global_this_rest_array_values(rest))
}

// Temporal.Now is a namespace (not a constructor) — method thunks on a plain
// object, installed like Math. Each reads the host clock fresh.
extern "C" fn temporal_now_instant_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::instant(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_now_timezone_id_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::time_zone_id(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_now_plain_date_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_date_time_iso(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_now_plain_date_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_date_iso(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_now_plain_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::plain_time_iso(&global_this_rest_array_values(rest))
}

extern "C" fn temporal_now_zoned_date_time_iso_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    crate::temporal::now::zoned_date_time_iso(&global_this_rest_array_values(rest))
}

/// Build the `Temporal.Now` namespace object (a plain object of method thunks).
fn build_temporal_now_namespace() -> f64 {
    let now_obj = js_object_alloc(0, 0);
    if now_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    for (name, thunk, len) in [
        ("instant", temporal_now_instant_thunk as *const u8, 0u32),
        ("timeZoneId", temporal_now_timezone_id_thunk as *const u8, 0),
        (
            "plainDateTimeISO",
            temporal_now_plain_date_time_iso_thunk as *const u8,
            0,
        ),
        (
            "plainDateISO",
            temporal_now_plain_date_iso_thunk as *const u8,
            0,
        ),
        (
            "plainTimeISO",
            temporal_now_plain_time_iso_thunk as *const u8,
            0,
        ),
        (
            "zonedDateTimeISO",
            temporal_now_zoned_date_time_iso_thunk as *const u8,
            0,
        ),
    ] {
        install_proto_method_rest_with_length(now_obj, name, thunk, len, 0);
    }
    set_intrinsic_to_string_tag(now_obj, "Temporal.Now");
    crate::value::js_nanbox_pointer(now_obj as i64)
}

/// Install a constructable `Temporal.<name>` constructor closure on the
/// `Temporal` namespace object and return it so statics can be hung off it.
/// Variadic (all args in the rest array, call-arity 0). Unlike
/// `install_constructor_static`, it does NOT mark the closure non-constructable
/// — `new Temporal.<name>(...)` must dispatch through the generic construct
/// path and use the returned cell.
fn install_temporal_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    spec_length: u32,
) -> *mut crate::closure::ClosureHeader {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return std::ptr::null_mut();
    }
    crate::closure::js_register_closure_rest(func_ptr, 0);
    super::native_module::set_bound_native_closure_name(closure, name);
    super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::set_builtin_property_attrs(
        ns_obj as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    closure
}

fn install_temporal_namespace(ns_obj: *mut ObjectHeader) {
    if ns_obj.is_null() {
        return;
    }
    // Temporal.Duration (#4688)
    let duration = install_temporal_constructor(
        ns_obj,
        "Duration",
        temporal_duration_ctor_thunk as *const u8,
        0,
    );
    if !duration.is_null() {
        install_constructor_static_with_call_arity(
            duration,
            "from",
            temporal_duration_from_thunk as *const u8,
            1,
            0,
            true,
        );
        install_constructor_static_with_call_arity(
            duration,
            "compare",
            temporal_duration_compare_thunk as *const u8,
            2,
            0,
            true,
        );
    }

    // Temporal.Instant (#4690)
    let instant = install_temporal_constructor(
        ns_obj,
        "Instant",
        temporal_instant_ctor_thunk as *const u8,
        1,
    );
    if !instant.is_null() {
        install_temporal_from_compare(
            instant,
            temporal_instant_from_thunk as *const u8,
            temporal_instant_compare_thunk as *const u8,
        );
        install_constructor_static_with_call_arity(
            instant,
            "fromEpochMilliseconds",
            temporal_instant_from_epoch_ms_thunk as *const u8,
            1,
            0,
            true,
        );
        install_constructor_static_with_call_arity(
            instant,
            "fromEpochNanoseconds",
            temporal_instant_from_epoch_ns_thunk as *const u8,
            1,
            0,
            true,
        );
    }

    // Temporal.PlainDate (#4691)
    let plain_date = install_temporal_constructor(
        ns_obj,
        "PlainDate",
        temporal_plain_date_ctor_thunk as *const u8,
        3,
    );
    if !plain_date.is_null() {
        install_temporal_from_compare(
            plain_date,
            temporal_plain_date_from_thunk as *const u8,
            temporal_plain_date_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainTime (#4692)
    let plain_time = install_temporal_constructor(
        ns_obj,
        "PlainTime",
        temporal_plain_time_ctor_thunk as *const u8,
        0,
    );
    if !plain_time.is_null() {
        install_temporal_from_compare(
            plain_time,
            temporal_plain_time_from_thunk as *const u8,
            temporal_plain_time_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainDateTime (#4693)
    let plain_date_time = install_temporal_constructor(
        ns_obj,
        "PlainDateTime",
        temporal_plain_date_time_ctor_thunk as *const u8,
        3,
    );
    if !plain_date_time.is_null() {
        install_temporal_from_compare(
            plain_date_time,
            temporal_plain_date_time_from_thunk as *const u8,
            temporal_plain_date_time_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainYearMonth (#4694)
    let plain_year_month = install_temporal_constructor(
        ns_obj,
        "PlainYearMonth",
        temporal_plain_year_month_ctor_thunk as *const u8,
        2,
    );
    if !plain_year_month.is_null() {
        install_temporal_from_compare(
            plain_year_month,
            temporal_plain_year_month_from_thunk as *const u8,
            temporal_plain_year_month_compare_thunk as *const u8,
        );
    }

    // Temporal.PlainMonthDay (#4694) — `from` only, no `compare` per spec.
    let plain_month_day = install_temporal_constructor(
        ns_obj,
        "PlainMonthDay",
        temporal_plain_month_day_ctor_thunk as *const u8,
        2,
    );
    if !plain_month_day.is_null() {
        install_constructor_static_with_call_arity(
            plain_month_day,
            "from",
            temporal_plain_month_day_from_thunk as *const u8,
            1,
            0,
            true,
        );
    }

    // Temporal.ZonedDateTime (#4695)
    let zoned = install_temporal_constructor(
        ns_obj,
        "ZonedDateTime",
        temporal_zoned_date_time_ctor_thunk as *const u8,
        2,
    );
    if !zoned.is_null() {
        install_temporal_from_compare(
            zoned,
            temporal_zoned_date_time_from_thunk as *const u8,
            temporal_zoned_date_time_compare_thunk as *const u8,
        );
    }

    // Temporal.Now namespace (#4689)
    let now_value = build_temporal_now_namespace();
    let now_key = crate::string::js_string_from_bytes(b"Now".as_ptr(), 3);
    js_object_set_field_by_name(ns_obj, now_key, now_value);
    super::set_builtin_property_attrs(
        ns_obj as usize,
        "Now".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

/// Install the standard `from` (spec length 1) and `compare` (spec length 2)
/// statics — both variadic with call-arity 0 — on a Temporal constructor.
fn install_temporal_from_compare(
    ctor: *mut crate::closure::ClosureHeader,
    from_thunk: *const u8,
    compare_thunk: *const u8,
) {
    install_constructor_static_with_call_arity(ctor, "from", from_thunk, 1, 0, true);
    install_constructor_static_with_call_arity(ctor, "compare", compare_thunk, 2, 0, true);
}

/// Populate the freshly-allocated globalThis singleton with built-in
/// constructor / namespace properties. Called exactly once from the CAS
/// winner in `js_get_global_this`. Constructors get a ClosureHeader-
/// backed value so `typeof globalThis.Array === "function"`; namespaces
/// (`Math`, `JSON`, `Reflect`) get a plain ObjectHeader (`typeof ===
/// "object"`). Both shapes carry a `prototype` dynamic property pointing
/// at an empty object so `<Builtin>.prototype` reads return a real
/// pointer instead of undefined, which is what unblocks lodash's
/// `var arrayProto = Array.prototype` chained read inside
/// `runInContext`.
pub(crate) fn populate_global_this_builtins(singleton: *mut ObjectHeader) {
    if singleton.is_null() {
        return;
    }
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    {
        let name = b"globalThis";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton as i64);
        js_object_set_field_by_name(singleton, key, value);
    }
    {
        // #4511: Node exposes the global object as `global` too
        // (`global === globalThis`). Install the same self-reference so bare
        // `global` / `(global as any).x` reads resolve to the real singleton
        // instead of the unknown-identifier sentinel.
        let name = b"global";
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = crate::value::js_nanbox_pointer(singleton as i64);
        js_object_set_field_by_name(singleton, key, value);
        super::set_builtin_property_attrs(
            singleton as usize,
            "global".to_string(),
            super::PropertyAttrs::new(true, true, true),
        );
    }
    // #2145: pre-allocate the shared `%TypedArray%` intrinsic so per-kind
    // typed-array constructors can link their `__proto__` to it as they're
    // built below, and the per-kind `.prototype` objects can be flagged with
    // `OBJ_FLAG_TYPED_ARRAY_PROTO` for `Object.getPrototypeOf` resolution.
    let (typed_array_intrinsic_ctor, _) = ensure_typed_array_intrinsic();
    // #3664: build the generator / async-generator intrinsic prototype towers
    // so `Object.getPrototypeOf(function*(){})`, `g.constructor`, and the
    // `%Generator(.prototype)%` chains resolve to real objects.
    ensure_generator_intrinsics();
    // Constructors: ClosureHeader-backed so typeof is "function".
    // #4533: native error subclasses must link to `Error` / `Error.prototype`.
    // `Error` is listed before its subclasses in GLOBAL_THIS_BUILTIN_CONSTRUCTORS,
    // so these are populated before the subclass iterations consume them.
    let mut error_ctor_bits: Option<u64> = None;
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        if name == "Buffer" {
            let name_bytes = name.as_bytes();
            let name_key =
                crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            let ctor_value = super::native_module::buffer_constructor_value();
            js_object_set_field_by_name(singleton, name_key, ctor_value);
            super::set_builtin_property_attrs(
                singleton as usize,
                name.to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
            continue;
        }
        let func_ptr = match name {
            "Array" => global_this_array_thunk as *const u8,
            "Object" => global_this_object_thunk as *const u8,
            "String" => global_this_string_thunk as *const u8,
            // #2889: call-form `Number(x)` / `Boolean(x)` through a rebound
            // global value coerce like the bare-call lowering does.
            "Number" => global_this_number_thunk as *const u8,
            "Boolean" => global_this_boolean_thunk as *const u8,
            "Error" => error_constructor_call_thunk as *const u8,
            "TypeError" => type_error_constructor_call_thunk as *const u8,
            "RangeError" => range_error_constructor_call_thunk as *const u8,
            "ReferenceError" => reference_error_constructor_call_thunk as *const u8,
            "SyntaxError" => syntax_error_constructor_call_thunk as *const u8,
            "EvalError" => eval_error_constructor_call_thunk as *const u8,
            "URIError" => uri_error_constructor_call_thunk as *const u8,
            "MessageChannel" => {
                crate::messaging::js_message_channel_constructor_call_error as *const u8
            }
            "MessagePort" => crate::messaging::js_message_port_constructor_call_error as *const u8,
            "BroadcastChannel" => {
                crate::messaging::js_broadcast_channel_constructor_call_error as *const u8
            }
            "Date" => global_this_date_thunk as *const u8,
            "Blob" => global_this_blob_thunk as *const u8,
            "File" => global_this_file_thunk as *const u8,
            "Headers" => global_this_headers_thunk as *const u8,
            "Request" => global_this_request_thunk as *const u8,
            "Response" => global_this_response_thunk as *const u8,
            "URLPattern" => global_this_url_pattern_call_thunk as *const u8,
            "Storage" => crate::web_storage::storage_constructor_illegal as *const u8,
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                webcrypto_illegal_constructor_thunk as *const u8
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => typed_array_constructor_call_thunk as *const u8,
            // #4569: collection constructors throw when called without `new`.
            "Map" => map_constructor_call_thunk as *const u8,
            "Set" => set_constructor_call_thunk as *const u8,
            "WeakMap" => weak_map_constructor_call_thunk as *const u8,
            "WeakSet" => weak_set_constructor_call_thunk as *const u8,
            "WeakRef" => weak_ref_constructor_call_thunk as *const u8,
            _ => global_this_builtin_noop_thunk as *const u8,
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        match name {
            "Array" => {
                crate::closure::js_register_closure_rest(func_ptr, 0);
            }
            "Date" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Object" | "String" | "Number" | "Boolean" | "BroadcastChannel" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Headers" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "Blob" | "Request" | "Response" => {
                crate::closure::js_register_closure_arity(func_ptr, 2);
            }
            "File" => {
                crate::closure::js_register_closure_arity(func_ptr, 3);
            }
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "EvalError" | "URIError" => {
                crate::closure::js_register_closure_arity(func_ptr, 1);
            }
            "MessageChannel" | "MessagePort" | "Storage" => {
                crate::closure::js_register_closure_arity(func_ptr, 0);
            }
            "URLPattern" => {
                crate::closure::js_register_closure_arity(func_ptr, 2);
            }
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => {
                crate::closure::js_register_closure_arity(func_ptr, 0);
            }
            _ => {}
        }
        // #2889: install static methods (`Object.keys`, `Array.isArray`, ...)
        // on the constructor closure so rebound usage like
        // `const O = Object; O.keys(x)` dispatches through the real helpers.
        install_builtin_constructor_statics(name, closure_ptr);
        if name == "Number" {
            install_number_static_data_properties(closure_ptr);
        }
        // #3655: every constructor carries spec-correct own `name`/`length`
        // data properties (`{ writable:false, enumerable:false,
        // configurable:true }`). The shared no-op thunk can't carry a name via
        // the func-ptr registry (every constructor would read the same one),
        // so record both per-closure. Without this, a rebound constructor read
        // `Date.name === ""` / `Date.length === 0` and test262's
        // `verifyProperty(Ctor, 'name'|'length', …)` failed "should be an own
        // property".
        super::native_module::set_bound_native_closure_name(closure_ptr, name);
        if let Some(len) = builtin_constructor_spec_length(name) {
            super::native_module::set_builtin_closure_length(closure_ptr as usize, len);
        }
        super::set_builtin_property_attrs(
            closure_ptr as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure_ptr as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        if name == "Error" {
            install_error_static_methods(closure_ptr);
        }
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        // #4533: `Object.getPrototypeOf(TypeError) === Error`. The constructor's
        // `[[Prototype]]` is `Error` itself (not `Function.prototype`).
        if name == "Error" {
            error_ctor_bits = Some(ctor_value.to_bits());
        } else if is_native_error_subclass_constructor(name) {
            if let Some(proto_bits) = error_ctor_bits {
                crate::closure::closure_set_static_prototype(closure_ptr as usize, proto_bits);
            }
        }
        // Stash `prototype` on the closure's dynamic-prop side table.
        // `js_object_set_field_by_name` detects the CLOSURE_MAGIC tag
        // at offset 12 and dispatches into `closure_set_dynamic_prop`
        // for us; both reads and writes share that side table.
        let proto_obj = if name == "Array" {
            crate::array::js_array_alloc(0) as *mut ObjectHeader
        } else {
            js_object_alloc(0, 0)
        };
        if !proto_obj.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
            super::set_builtin_property_attrs(
                closure_ptr as usize,
                "prototype".to_string(),
                super::PropertyAttrs::new(false, false, false),
            );
            let ctor_key = crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                "constructor".len() as u32,
            );
            js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
            super::set_builtin_property_attrs(
                proto_obj as usize,
                "constructor".to_string(),
                super::PropertyAttrs::new(true, false, true),
            );
            if is_web_fetch_constructor(name) {
                js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::PropertyAttrs::new(true, false, true),
                );
            }
            if name == "Array" {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "constructor".to_string(),
                    super::PropertyAttrs::new(true, false, true),
                );
            }
            if matches!(
                name,
                "Navigator"
                    | "TextEncoderStream"
                    | "TextDecoderStream"
                    | "CompressionStream"
                    | "DecompressionStream"
            ) {
                let constructor_key =
                    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
            }
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that read their receiver from
            // `IMPLICIT_THIS` and dispatch to the corresponding native
            // entry point — works in tandem with `.call`/`.apply` since
            // those arms (#970) rebind IMPLICIT_THIS before forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
            install_error_prototype_data_properties(name, proto_obj);
            if matches!(name, "MessageChannel" | "MessagePort" | "BroadcastChannel") {
                crate::messaging::populate_messaging_prototype(name, proto_obj, ctor_value);
            }
            if name == "Storage" {
                crate::web_storage::install_storage_globals(
                    singleton,
                    closure_ptr,
                    proto_obj,
                    ctor_value,
                );
            }
            if matches!(name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
                super::native_module::install_webcrypto_constructor_proto(proto_obj, ctor_value);
            }
            if name == "WebSocket" {
                websocket_global::install_constructor_shape(closure_ptr, proto_obj);
            }
            // #2145: link per-kind typed-array constructors into the
            // `%TypedArray%` chain. `Int8Array.__proto__ === %TypedArray%`
            // and `Object.getPrototypeOf(Int8Array.prototype) ===
            // %TypedArray%.prototype`. Both reads are resolved off this
            // wiring (closure static-prototype side-table for the ctor;
            // `OBJ_FLAG_TYPED_ARRAY_PROTO` + the cached
            // `TYPED_ARRAY_INTRINSIC_PROTO_PTR` for the per-kind proto).
            if !typed_array_intrinsic_ctor.is_null()
                && matches!(
                    name,
                    "Int8Array"
                        | "Uint8Array"
                        | "Uint8ClampedArray"
                        | "Int16Array"
                        | "Uint16Array"
                        | "Int32Array"
                        | "Uint32Array"
                        | "Float16Array"
                        | "Float32Array"
                        | "Float64Array"
                        | "BigInt64Array"
                        | "BigUint64Array"
                )
            {
                let intrinsic_bits =
                    crate::value::js_nanbox_pointer(typed_array_intrinsic_ctor as i64).to_bits();
                crate::closure::closure_set_static_prototype(closure_ptr as usize, intrinsic_bits);
                unsafe {
                    let gc = (proto_obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *mut crate::gc::GcHeader;
                    (*gc)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                }
                // Record the per-kind proto's `[[Prototype]]` as the shared
                // `%TypedArray%.prototype` so the ordinary property-get chain
                // walk (`resolve_inherited_field`) finds the inherited methods
                // (`map`, `filter`, `toString`, …) that no longer live on the
                // per-kind proto as own properties. `Object.getPrototypeOf`
                // already resolves via the flag above; this link drives value
                // reads like `Int8Array.prototype.map`.
                let intrinsic_proto =
                    crate::object::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
                if intrinsic_proto != 0 {
                    let proto_bits = crate::value::js_nanbox_pointer(intrinsic_proto).to_bits();
                    super::prototype_chain::object_set_static_prototype(
                        proto_obj as usize,
                        proto_bits,
                    );
                }
            }
            // #4140: per-kind `BYTES_PER_ELEMENT` own data property on BOTH the
            // constructor and its prototype, matching Node's descriptor
            // `{ value, writable:false, enumerable:false, configurable:false }`.
            // The bare `Uint8Array.BYTES_PER_ELEMENT` read folds at compile time
            // (#2902), but the reflective forms — `getOwnPropertyDescriptor`,
            // `hasOwnProperty`, and the chained `Float64Array.prototype
            // .BYTES_PER_ELEMENT` — resolve off these installed own properties.
            let ta_bytes_per_element = match name {
                "Int8Array" | "Uint8Array" | "Uint8ClampedArray" => Some(1.0),
                "Int16Array" | "Uint16Array" | "Float16Array" => Some(2.0),
                "Int32Array" | "Uint32Array" | "Float32Array" => Some(4.0),
                "Float64Array" | "BigInt64Array" | "BigUint64Array" => Some(8.0),
                _ => None,
            };
            if let Some(bytes) = ta_bytes_per_element {
                let bpe_attrs = super::PropertyAttrs::new(false, false, false);
                for target in [closure_ptr as *mut ObjectHeader, proto_obj] {
                    let bpe_key = crate::string::js_string_from_bytes(
                        b"BYTES_PER_ELEMENT".as_ptr(),
                        b"BYTES_PER_ELEMENT".len() as u32,
                    );
                    js_object_set_field_by_name(target, bpe_key, bytes);
                    super::set_builtin_property_attrs(
                        target as usize,
                        "BYTES_PER_ELEMENT".to_string(),
                        bpe_attrs,
                    );
                }
            }
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
        super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    for name in GLOBAL_THIS_BUILTIN_FUNCTIONS.iter().copied() {
        let (func_ptr, arity, has_rest, enumerable) = match name {
            "eval" => (global_this_eval_thunk as *const u8, 1, false, false),
            "fetch" => (
                super::global_fetch::global_this_fetch_thunk as *const u8,
                1,
                true,
                true,
            ),
            "structuredClone" => (
                global_this_structured_clone_thunk as *const u8,
                2,
                false,
                true,
            ),
            "atob" => (global_this_atob_thunk as *const u8, 1, false, true),
            "btoa" => (global_this_btoa_thunk as *const u8, 1, false, true),
            "setTimeout" => (global_this_set_timeout_thunk as *const u8, 2, true, true),
            "clearTimeout" => (global_this_clear_timeout_thunk as *const u8, 1, false, true),
            "setInterval" => (global_this_set_interval_thunk as *const u8, 2, true, true),
            "clearInterval" => (
                global_this_clear_interval_thunk as *const u8,
                1,
                false,
                true,
            ),
            "setImmediate" => (global_this_set_immediate_thunk as *const u8, 1, true, true),
            "clearImmediate" => (
                global_this_clear_immediate_thunk as *const u8,
                1,
                false,
                true,
            ),
            "queueMicrotask" => (
                global_this_queue_microtask_thunk as *const u8,
                1,
                false,
                true,
            ),
            // #2905: standard global helper functions.
            "parseInt" => (global_this_parse_int_thunk as *const u8, 2, false, false),
            "parseFloat" => (global_this_parse_float_thunk as *const u8, 1, false, false),
            "isNaN" => (global_this_is_nan_thunk as *const u8, 1, false, false),
            "isFinite" => (global_this_is_finite_thunk as *const u8, 1, false, false),
            "encodeURI" => (global_this_encode_uri_thunk as *const u8, 1, false, false),
            "decodeURI" => (global_this_decode_uri_thunk as *const u8, 1, false, false),
            "encodeURIComponent" => (
                global_this_encode_uri_component_thunk as *const u8,
                1,
                false,
                false,
            ),
            "decodeURIComponent" => (
                global_this_decode_uri_component_thunk as *const u8,
                1,
                false,
                false,
            ),
            // #4511: legacy escape/unescape (ES Annex B).
            // #4511: legacy escape/unescape (ES Annex B).
            "escape" => (global_this_escape_thunk as *const u8, 1, false, false),
            "unescape" => (global_this_unescape_thunk as *const u8, 1, false, false),
            _ => continue,
        };
        let closure_ptr = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure_ptr.is_null() {
            continue;
        }
        if has_rest {
            crate::closure::js_register_closure_rest(func_ptr, arity);
        } else {
            crate::closure::js_register_closure_arity(func_ptr, arity);
        }
        unsafe {
            crate::builtins::js_register_function_name(func_ptr, name.as_ptr(), name.len() as u32);
        }
        super::native_module::set_builtin_closure_length(closure_ptr as usize, arity);
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let fn_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(singleton, name_key, fn_value);
        super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, enumerable, true),
        );
    }
    // ECMA-262 21.1.2.12 / 21.1.2.13: `Number.parseFloat` and `Number.parseInt`
    // are the SAME function objects as the global `parseFloat` / `parseInt`
    // (`Number.parseFloat === parseFloat`). The Number constructor statics were
    // installed above with fresh thunks — before the global helpers existed —
    // so re-point them now at the global closures we just created on the
    // singleton. A value-read of `Number.parseFloat` resolves to the Number
    // constructor's own `parseFloat` field (see expr_member.rs reroute-undo),
    // which now holds the identical closure the bare `parseFloat` resolves to.
    alias_number_static_to_global_function(singleton, "parseFloat");
    alias_number_static_to_global_function(singleton, "parseInt");
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ns_value = if matches!(name, "console" | "process") {
            js_create_native_module_namespace(name_bytes.as_ptr(), name_bytes.len())
        } else if name == "WebAssembly" {
            global_this_webassembly::create_webassembly_namespace()
        } else {
            let ns_obj = js_object_alloc(0, 0);
            if ns_obj.is_null() {
                continue;
            }
            // #4139 + #4149: reify each namespace's own members as real
            // properties so the reflection APIs (`getOwnPropertyDescriptor`,
            // `getOwnPropertyNames`) observe them. Call sites (`Math.max(...)`,
            // `JSON.stringify(...)`, `Reflect.get(...)`) are codegen intrinsics
            // gated on the AST shape and never read these fields. Math uses the
            // richer install that also exposes per-method name/length descriptors.
            match name {
                "Math" => {
                    install_math_namespace(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Math");
                }
                "JSON" => {
                    install_json_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "JSON");
                }
                "Reflect" => {
                    install_reflect_namespace_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Reflect");
                }
                "Atomics" => install_atomics_namespace_members(ns_obj),
                "Intl" => crate::intl::install_intl_namespace(ns_obj),
                "Temporal" => {
                    install_temporal_namespace(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Temporal");
                }
                _ => {}
            }
            crate::value::js_nanbox_pointer(ns_obj as i64)
        };
        js_object_set_field_by_name(singleton, name_key, ns_value);
        super::set_builtin_property_attrs(
            singleton as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }
    // node:perf_hooks `performance` global — bind it to the same singleton the
    // named import resolves to, so `globalThis.performance ===
    // require("perf_hooks").performance` (#1327). typeof stays "object".
    {
        let pname = b"performance";
        let pkey = crate::string::js_string_from_bytes(pname.as_ptr(), pname.len() as u32);
        let pval = crate::perf_hooks::performance_namespace();
        js_object_set_field_by_name(singleton, pkey, pval);
    }
    // Perf_hooks constructors are globals identical to the module exports.
    for name in [
        "Performance",
        "PerformanceEntry",
        "PerformanceMark",
        "PerformanceMeasure",
        "PerformanceObserver",
        "PerformanceObserverEntryList",
        "PerformanceResourceTiming",
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = super::native_module::bound_native_callable_export_value("perf_hooks", name);
        js_object_set_field_by_name(singleton, key, value);
    }
    super::native_module::install_global_webcrypto(singleton);
    let func_ptr = global_this_crypto_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let getter = crate::closure::js_closure_alloc(func_ptr, 0);
    let getter_bits = if getter.is_null() {
        0
    } else {
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    };
    super::set_builtin_accessor_descriptor(
        singleton as usize,
        "crypto".to_string(),
        super::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        super::PropertyAttrs::new(true, true, true),
    );
    // #2923: `globalThis.navigator` — Node's browser-compatible runtime
    // metadata object. typeof is "object". Built once per process.
    {
        let nname = b"navigator";
        let nkey = crate::string::js_string_from_bytes(nname.as_ptr(), nname.len() as u32);
        // Read the `Navigator` constructor we installed on the singleton above
        // and hand it to the navigator builder directly. We must NOT call
        // `js_navigator_object()` here: it re-fetches the constructor via
        // `js_get_global_this_builtin_value` → `js_get_global_this`, which would
        // re-enter this very lazy-init (GLOBAL_THIS_READY is still false until we
        // return) and recurse/spin forever.
        let nav_ctor_key = crate::string::js_string_from_bytes(b"Navigator".as_ptr(), 9);
        let nav_ctor = js_object_get_field_by_name(singleton, nav_ctor_key);
        let nval =
            crate::navigator::navigator_object_with_constructor(f64::from_bits(nav_ctor.bits()));
        js_object_set_field_by_name(singleton, nkey, nval);
    }
}

/// Re-point a `Number.<name>` static at the global function of the same name so
/// the two are the identical object (`Number.parseFloat === parseFloat`). Both
/// the global helper and the `Number` constructor are already installed on the
/// `singleton` by the time this runs. No-op if either lookup fails.
fn alias_number_static_to_global_function(singleton: *mut ObjectHeader, name: &str) {
    let global_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let global_fn = js_object_get_field_by_name(singleton, global_key);
    if (global_fn.bits() >> 48) != 0x7FFD {
        return;
    }
    let number_key = crate::string::js_string_from_bytes(b"Number".as_ptr(), 6);
    let number_ctor = js_object_get_field_by_name(singleton, number_key);
    if (number_ctor.bits() >> 48) != 0x7FFD {
        return;
    }
    let ctor_ptr = (number_ctor.bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if ctor_ptr.is_null() {
        return;
    }
    let static_key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(ctor_ptr, static_key, f64::from_bits(global_fn.bits()));
    super::set_builtin_property_attrs(
        ctor_ptr as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_error_static_methods(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let func_ptr = global_this_error_capture_stack_trace_thunk as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 2);
    super::native_module::set_bound_native_closure_name(closure, "captureStackTrace");

    let key = crate::string::js_string_from_bytes(b"captureStackTrace".as_ptr(), 17);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        "captureStackTrace".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    // #2904: `Error.isError` — V8/Node Error duck-check.
    install_error_static_fn(
        ctor,
        "isError",
        global_this_error_is_error_thunk as *const u8,
        1,
    );

    // #2904: `Error.prepareStackTrace` — default stack-formatting hook.
    install_error_static_fn(
        ctor,
        "prepareStackTrace",
        global_this_error_prepare_stack_trace_thunk as *const u8,
        2,
    );

    // #2904: `Error.stackTraceLimit` — writable number controlling captured
    // frame count. Node's default is 10; Perry's stacks are coarse but the
    // property must read as a number and be writable.
    let limit_key = crate::string::js_string_from_bytes(b"stackTraceLimit".as_ptr(), 15);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, limit_key, 10.0);
    super::set_builtin_property_attrs(
        ctor as usize,
        "stackTraceLimit".to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
}

/// #2904: install a callable static method on the `Error` constructor closure
/// as a non-enumerable, writable, configurable data property (matching Node's
/// property descriptors for the V8 static helpers).
fn install_error_static_fn(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::native_module::set_bound_native_closure_name(closure, name);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

// =====================================================================
// #2889: static methods on rebound global built-in constructor values.
//
// `const O = Object; O.keys(x)` reads `keys` off the `Object` constructor
// closure's dynamic-prop side table, then calls it. Pre-fix nothing was
// installed there, so the read returned `undefined`. These thunks delegate
// to the same runtime helpers the direct `Object.keys(x)` lowering uses.
// =====================================================================

fn nanbox_array_or_undef(arr: *mut crate::array::ArrayHeader) -> f64 {
    if arr.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(arr as i64)
    }
}

extern "C" fn object_keys_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    nanbox_array_or_undef(super::js_object_keys_value(value))
}

extern "C" fn object_values_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::js_object_values_value(value))
}

extern "C" fn object_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    nanbox_array_or_undef(super::js_object_entries_value(value))
}

extern "C" fn object_freeze_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_freeze(value)
}

extern "C" fn object_create_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_create(value)
}

extern "C" fn object_get_prototype_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_get_prototype_of(value)
}

extern "C" fn object_get_own_property_names_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_get_own_property_names(value)
}

extern "C" fn object_get_own_property_descriptor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
) -> f64 {
    super::js_object_get_own_property_descriptor(obj, key)
}

extern "C" fn object_define_property_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
    descriptor: f64,
) -> f64 {
    super::js_object_define_property(obj, key, descriptor)
}

extern "C" fn object_from_entries_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    super::js_object_from_entries(value)
}

extern "C" fn object_assign_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    rest: f64,
) -> f64 {
    let validated = unsafe { super::js_object_assign_validate_target(target) };
    for source in global_this_rest_array_values(rest) {
        unsafe { super::js_object_assign_one(validated, source) };
    }
    validated
}

/// `Object.hasOwn(obj, key)` (ES2022) reified as a callable value so the
/// feature-detect idiom `typeof Object.hasOwn === "undefined" ? … :
/// Object.hasOwn` (iconv-lite's merge-exports, #3527) binds a real callable
/// instead of a non-callable handle. Backed by the same runtime helper as
/// `Object.prototype.hasOwnProperty.call(obj, key)`.
extern "C" fn object_hasown_thunk(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    key: f64,
) -> f64 {
    super::object_ops::js_object_has_own(obj, key)
}

extern "C" fn array_is_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::array::js_array_is_array(value)
}

extern "C" fn array_from_thunk(_closure: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    // Reflective `Array.from.call(C, items)` / `Array.from.apply(C, [items])`
    // binds `C` as the implicit `this`. Read it FIRST (before any nested call
    // can overwrite it) and run the spec algorithm — when `C IsConstructor`,
    // the result is built via `Construct(C)`. A plain reflective call (no
    // explicit receiver) leaves `this` as undefined / a non-constructor, so
    // the default `%Array%` path is taken.
    let c = crate::object::js_implicit_this_get();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    crate::array::array_from_full(c, value, undefined, undefined)
}

extern "C" fn array_of_thunk(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    // Reflective `Array.of.call(C, ...items)` binds `C` as the implicit `this`.
    // Read it FIRST (before any nested call can overwrite it); when `C
    // IsConstructor` the result is built via `Construct(C, «len»)`, otherwise the
    // default `%Array%` path is taken. See `array_of_full` (ECMA-262 §23.1.2.3).
    let c = crate::object::js_implicit_this_get();
    let vals = global_this_rest_array_values(rest);
    crate::array::array_of_full(c, &vals)
}

extern "C" fn number_is_nan_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_nan(value)
}

extern "C" fn number_is_finite_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_finite(value)
}

extern "C" fn number_is_integer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_integer(value)
}

/// Shared impl for `BigInt.asIntN`/`asUintN` (both the ctor-static thunks and
/// the `("bigint", ...)` native-module dispatch). Coerces `bits` via ToIndex
/// (RangeError on negative/non-integer), brand-checks `value` is a BigInt
/// (TypeError otherwise), and returns the NaN-boxed result. `signed` selects
/// asIntN vs asUintN. Diverges (`!`) on bad input, matching Node.
/// `ToBigInt(value)` for `BigInt.asIntN`/`asUintN`'s second argument. BigInt
/// passes through; Boolean → 0n/1n; String → StringToBigInt; an object is first
/// reduced through ToPrimitive("number") (running its `valueOf`/`toString`) and
/// re-coerced; a Number/undefined/null/Symbol throws a TypeError. The
/// primitive cases reuse the same `to_bigint_for_store` helper that backs
/// `BigInt64Array` element writes.
fn bigint_to_bigint_arg(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() && !jv.is_bigint() {
        // Array → ToPrimitive finds no `valueOf` override and falls to
        // `Array.prototype.toString` = `join(",")`, then ToBigInt on that string
        // (`[] => "" => 0n`, `[10n] => "10" => 10n`, `[1,2] => "1,2" => throws`).
        // `js_to_primitive` doesn't apply array join, so handle it first —
        // mirrors the array arm in `js_number_coerce`. #2378.
        const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
        if crate::array::js_array_is_array(value).to_bits() == TAG_TRUE_BITS {
            let arr_ptr = jv.as_pointer::<crate::array::ArrayHeader>();
            let comma = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
            let joined = unsafe { crate::array::js_array_join(arr_ptr, comma) };
            return bigint_to_bigint_arg(crate::value::js_nanbox_string(joined as i64));
        }
        // Object: ToPrimitive("number") then re-coerce. Try a custom
        // [Symbol.toPrimitive] first, then OrdinaryToPrimitive
        // (valueOf-before-toString). A primitive result recurses; anything
        // unconvertible falls through to the TypeError in `to_bigint_for_store`.
        let prim = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if prim.to_bits() != value.to_bits() {
            return bigint_to_bigint_arg(prim);
        }
        if let crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) =
            unsafe { crate::value::ordinary_to_primitive_number_for_add(value) }
        {
            if p.to_bits() != value.to_bits() {
                return bigint_to_bigint_arg(p);
            }
        }
    }
    crate::typedarray::bigint::to_bigint_for_store(value)
}

pub(crate) fn bigint_as_n_dispatch(bits_arg: f64, value_arg: f64, signed: bool) -> f64 {
    // Step 1: `bits = ? ToIndex(bits)`. ToIndex = ToIntegerOrInfinity(ToNumber)
    // with a `0 <= n <= 2^53-1` range check. `js_number_coerce` is the full
    // ToNumber (strings, booleans, null/undefined, and objects via
    // ToPrimitive("number") — so a `bits` object's `valueOf`/`toString` runs
    // here, BEFORE `value` is touched, preserving the spec coercion order).
    let bits_num = crate::builtins::js_number_coerce(bits_arg);
    let bits_int = if bits_num.is_nan() {
        0.0
    } else {
        bits_num.trunc()
    };
    if bits_int < 0.0 || bits_int > 9_007_199_254_740_991.0 {
        crate::fs::validate::throw_range_error_with_code(
            "The number of bits is invalid (must be a non-negative integer)",
        );
    }
    // Step 2: `bigint = ? ToBigInt(bigint)`. ToBigInt coerces BigInt / Boolean /
    // String (and objects via ToPrimitive); a Number/undefined/null/Symbol
    // throws a TypeError. Runs strictly after ToIndex(bits) above.
    let value_bigint = bigint_to_bigint_arg(value_arg);
    let jv = JSValue::from_bits(value_bigint.to_bits());
    let bits = bits_int as u32;
    let ptr = jv.as_bigint_ptr() as *const crate::bigint::BigIntHeader;
    let r = if signed {
        crate::bigint::js_bigint_as_int_n(bits, ptr)
    } else {
        crate::bigint::js_bigint_as_uint_n(bits, ptr)
    };
    f64::from_bits(crate::value::js_nanbox_bigint(r as i64).to_bits())
}

/// FFI entry for the codegen-lowered `BigInt.asIntN(bits, x)` direct call.
#[no_mangle]
pub extern "C" fn js_bigint_as_int_n_call(bits: f64, value: f64) -> f64 {
    bigint_as_n_dispatch(bits, value, true)
}

/// FFI entry for the codegen-lowered `BigInt.asUintN(bits, x)` direct call.
#[no_mangle]
pub extern "C" fn js_bigint_as_uint_n_call(bits: f64, value: f64) -> f64 {
    bigint_as_n_dispatch(bits, value, false)
}

extern "C" fn bigint_as_int_n_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bits: f64,
    value: f64,
) -> f64 {
    bigint_as_n_dispatch(bits, value, true)
}

extern "C" fn bigint_as_uint_n_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bits: f64,
    value: f64,
) -> f64 {
    bigint_as_n_dispatch(bits, value, false)
}

extern "C" fn json_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    text: f64,
    reviver: f64,
) -> f64 {
    let text_ptr = crate::value::js_get_string_pointer_unified(text) as *const crate::StringHeader;
    let reviver_value = JSValue::from_bits(reviver.to_bits());
    let parsed = unsafe {
        if reviver_value.is_pointer()
            && crate::closure::is_closure_ptr(reviver_value.as_pointer::<u8>() as usize)
        {
            crate::json::js_json_parse_with_reviver(
                text_ptr,
                reviver_value.as_pointer::<crate::closure::ClosureHeader>() as i64,
            )
        } else {
            crate::json::js_json_parse(text_ptr)
        }
    };
    f64::from_bits(parsed.bits())
}

extern "C" fn json_stringify_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    replacer: f64,
    space: f64,
) -> f64 {
    f64::from_bits(unsafe { crate::json::js_json_stringify_full(value, replacer, space) as u64 })
}

extern "C" fn json_raw_json_thunk(
    _closure: *const crate::closure::ClosureHeader,
    text: f64,
) -> f64 {
    unsafe { crate::json::js_json_raw_json(text) }
}

extern "C" fn json_is_raw_json_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    unsafe { crate::json::js_json_is_raw_json(value) }
}

extern "C" fn reflect_apply_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    this_arg: f64,
    args: f64,
) -> f64 {
    crate::proxy::js_reflect_apply(target, this_arg, args)
}

extern "C" fn symbol_for_thunk(_closure: *const crate::closure::ClosureHeader, key: f64) -> f64 {
    unsafe { crate::symbol::js_symbol_for(key) }
}

extern "C" fn symbol_key_for_thunk(
    _closure: *const crate::closure::ClosureHeader,
    symbol: f64,
) -> f64 {
    unsafe { crate::symbol::js_symbol_key_for(symbol) }
}

extern "C" fn number_is_safe_integer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_number_is_safe_integer(value)
}

// #4627: reified `String.fromCharCode(...units)` / `fromCodePoint(...points)`.
// Both collect all arguments into `rest` (call-arity 0), so `rest` is already
// the array-like the array-form runtime helpers expect.
extern "C" fn string_from_char_code_static(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_from_char_code_array(rest);
    crate::value::js_nanbox_string(s as i64)
}

extern "C" fn string_from_code_point_static(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_from_code_point_array(rest);
    crate::value::js_nanbox_string(s as i64)
}

// #4521: reified `Promise` statics so `Promise.all` / `Promise.resolve` / etc.
// are first-class function values (correct `.name` / `.length`, usable via
// reference, `.call`, `.apply`, spread). Direct calls (`Promise.all([...])`)
// still take the codegen fast path in `lower_call/console_promise.rs`; these
// thunks back value reads and rebound/`.call` usage by delegating to the same
// runtime entry points the direct-call path emits. Spec-internal observable
// semantics (per-iteration `this.resolve`, real resolve-element closures with
// `[[AlreadyCalled]]`, `NewPromiseCapability(this)`) are a follow-up — these
// thunks intentionally use the native Promise machinery regardless of `this`.
extern "C" fn promise_resolve_static(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_resolve_spec(this_ctor, value)
}

extern "C" fn promise_reject_static(
    _closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_reject_spec(this_ctor, reason)
}

extern "C" fn promise_all_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_all_spec(this_ctor, iterable)
}

extern "C" fn promise_race_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_race_spec(this_ctor, iterable)
}

extern "C" fn promise_all_settled_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_all_settled_spec(this_ctor, iterable)
}

extern "C" fn promise_any_static(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    let this_ctor = crate::object::js_implicit_this_get();
    crate::promise::js_promise_any_spec(this_ctor, iterable)
}

extern "C" fn promise_with_resolvers_static(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let obj = crate::promise::js_promise_with_resolvers();
    crate::value::js_nanbox_pointer(obj as i64)
}

// `Promise.try(fn, ...args)`: call-arity 1 (callback) + rest (forwarded args).
extern "C" fn promise_try_static(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    let rest_ptr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    let p = crate::promise::js_promise_try(callback, rest_ptr);
    crate::value::js_nanbox_pointer(p as i64)
}

// #4627: reified `String.raw(callSite, ...substitutions)` tag function. One
// fixed param (the template/cooked object) then a rest of substitutions, which
// `js_string_raw` reads by numeric index — so `rest` (the collected array) is
// passed straight through as the substitutions array-like.
extern "C" fn string_raw_static(
    _closure: *const crate::closure::ClosureHeader,
    call_site: f64,
    rest: f64,
) -> f64 {
    let s = crate::string::js_string_raw(call_site, rest);
    crate::value::js_nanbox_string(s as i64)
}

extern "C" fn number_parse_float_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    global_this_parse_float_thunk(closure, value)
}

extern "C" fn number_parse_int_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
    radix: f64,
) -> f64 {
    global_this_parse_int_thunk(closure, value, radix)
}

extern "C" fn typed_array_from_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
    map_fn: f64,
    this_arg: f64,
) -> f64 {
    // §%TypedArray%.from step 1-2: `C` is the `this` value; if `IsConstructor(C)`
    // is false, throw a TypeError — BEFORE the source is read. Invoked as a plain
    // function (`var from = TA.from; from([])`) the sloppy `this` is `globalThis`
    // (not a constructor), so this must fire even though a source is supplied
    // (test262 `from/invoked-as-func`). A concrete TA `this` (kind known) is a
    // constructor by definition.
    let kind_opt = typed_array_constructor_this_kind();
    if kind_opt.is_none() {
        require_typed_array_from_of_constructor();
    }
    // Past the constructor check, the source is read — its `@@iterator` invoked,
    // or its `length` getter + indexed elements evaluated — and any throwing
    // user iterator/getter propagates. The map callback is validated up front by
    // `js_array_from_mapped`.
    let mapped = map_fn.to_bits() != crate::value::TAG_UNDEFINED;
    let arr = if mapped {
        crate::array::js_array_from_mapped(source, map_fn, this_arg)
    } else {
        crate::array::js_array_from_value(source)
    };
    typed_array_create_from_values(kind_opt, arr)
}

/// `%TypedArray%.from`/`.of` step "If IsConstructor(`this`) is false, throw a
/// TypeError". Only called when the `this` value is not a concrete typed-array
/// constructor (kind unknown); a user constructor passes, anything else throws.
fn require_typed_array_from_of_constructor() {
    let this_ctor = crate::object::js_implicit_this_get();
    if !value_is_constructor(this_ctor) {
        super::object_ops::throw_object_type_error(
            b"TypedArray.from/of called with a `this` that is not a constructor",
        );
    }
}

/// `IsConstructor(value)` for the typed-array `from`/`of` `this` check: a class
/// ref, a proxy, or a non-arrow user closure that is not a flagged
/// non-constructable builtin.
fn value_is_constructor(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        return true; // class-ref constructor
    }
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return true;
    }
    if (bits >> 48) == 0x7FFD {
        let raw = (bits & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(raw) {
            if crate::closure::closure_is_arrow(raw as *const crate::closure::ClosureHeader) {
                return false;
            }
            return !super::native_module::builtin_closure_is_non_constructable_value(value);
        }
    }
    false
}

/// Build the result of `%TypedArray%.from` / `%TypedArray%.of` from a
/// materialized values array, honoring a custom `this` constructor.
///
/// When `this` is a concrete typed-array constructor (`Int8Array`, …) the
/// fast path builds the view directly. Otherwise (`%TypedArray%.from.call(
/// userCtor, …)`) the spec's `TypedArrayCreate(C, «len»)` is realized by
/// `Construct(C, [len])` and the values are written into the result via the
/// element [[Set]] path — so a user constructor that throws propagates, and one
/// that returns an arbitrary (sufficiently long) typed array is used verbatim
/// (test262 `from/of` `custom-ctor*`).
fn typed_array_create_from_values(
    kind_opt: Option<u8>,
    arr: *mut crate::array::ArrayHeader,
) -> f64 {
    if let Some(kind) = kind_opt {
        let ta = crate::typedarray::js_typed_array_new_from_array(kind as i32, arr);
        return crate::value::js_nanbox_pointer(ta as i64);
    }
    let ctor = crate::object::js_implicit_this_get();
    let len = crate::array::js_array_length(arr) as usize;
    let len_arg = [f64::from_bits(
        crate::value::JSValue::number(len as f64).bits(),
    )];
    let target = unsafe { super::js_new_function_construct(ctor, len_arg.as_ptr(), 1) };
    // `TypedArrayCreate` requires the constructed object to be a typed array
    // with at least `len` elements.
    let addr = crate::typedarray_props::typed_array_addr_from_value(target).unwrap_or_else(|| {
        super::object_ops::throw_object_type_error(
            b"TypedArray.from/of constructor did not return a TypedArray",
        )
    });
    let ta_ptr = addr as *mut crate::typedarray::TypedArrayHeader;
    let target_len = unsafe { crate::typedarray::js_typed_array_length(ta_ptr) } as usize;
    if target_len < len {
        // `TypedArrayCreate(C, «len»)` throws a *TypeError* (not RangeError)
        // when the constructed typed array is shorter than the requested length
        // (test262 `from/of` `custom-ctor-returns-smaller-instance-throws`).
        super::object_ops::throw_object_type_error(
            b"Derived TypedArray constructor created an array which was too small",
        );
    }
    for k in 0..len {
        let v = crate::array::js_array_get(arr, k as u32);
        crate::typedarray::js_typed_array_set(ta_ptr, k as i32, f64::from_bits(v.bits()));
    }
    target
}

extern "C" fn typed_array_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let kind_opt = typed_array_constructor_this_kind();
    if kind_opt.is_none() {
        require_typed_array_from_of_constructor();
    }
    let vals = global_this_rest_array_values(rest);
    let len = vals.len() as u32;
    let arr = crate::array::js_array_alloc(len);
    unsafe {
        (*arr).length = len;
        for (i, &v) in vals.iter().enumerate() {
            crate::array::js_array_set_f64(arr, i as u32, v);
        }
    }
    typed_array_create_from_values(kind_opt, arr)
}

fn require_promise_constructor_this() {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let promise_ctor = js_get_global_this_builtin_value(b"Promise".as_ptr(), 7);
    if this_value.to_bits() != promise_ctor.to_bits() {
        super::object_ops::throw_object_type_error(
            b"Promise static method requires a Promise constructor receiver",
        );
    }
}

extern "C" fn promise_resolve_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::js_promise_resolved(value);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_reject_thunk(
    _closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::js_promise_rejected(reason);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_all_thunk(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::combinators::js_promise_all_iterable(iterable);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_race_thunk(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::combinators::js_promise_race_iterable(iterable);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_all_settled_thunk(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::combinators::js_promise_all_settled_iterable(iterable);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_any_thunk(
    _closure: *const crate::closure::ClosureHeader,
    iterable: f64,
) -> f64 {
    require_promise_constructor_this();
    let promise = crate::promise::combinators::js_promise_any_iterable(iterable);
    crate::value::js_nanbox_pointer(promise as i64)
}

extern "C" fn promise_with_resolvers_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    require_promise_constructor_this();
    let result = crate::promise::js_promise_with_resolvers();
    crate::value::js_nanbox_pointer(result as i64)
}

extern "C" fn promise_try_thunk(
    _closure: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    require_promise_constructor_this();
    let rest_value = JSValue::from_bits(rest.to_bits());
    let rest_ptr = if rest_value.is_pointer() {
        rest_value.as_pointer::<crate::array::ArrayHeader>()
    } else {
        std::ptr::null()
    };
    let promise = crate::promise::js_promise_try(callback, rest_ptr);
    crate::value::js_nanbox_pointer(promise as i64)
}

fn promise_static_function_spec(name: &str) -> Option<(*const u8, u32, u32, bool)> {
    match name {
        "resolve" => Some((promise_resolve_thunk as *const u8, 1, 1, false)),
        "reject" => Some((promise_reject_thunk as *const u8, 1, 1, false)),
        "all" => Some((promise_all_thunk as *const u8, 1, 1, false)),
        "race" => Some((promise_race_thunk as *const u8, 1, 1, false)),
        "allSettled" => Some((promise_all_settled_thunk as *const u8, 1, 1, false)),
        "any" => Some((promise_any_thunk as *const u8, 1, 1, false)),
        "withResolvers" => Some((promise_with_resolvers_thunk as *const u8, 0, 0, false)),
        "try" => Some((promise_try_thunk as *const u8, 1, 1, true)),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn js_promise_static_function_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let Ok(name) = std::str::from_utf8(name_bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let Some((func_ptr, spec_length, call_arity, has_rest)) = promise_static_function_spec(name)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };

    let ctor_value = js_get_global_this_builtin_value(b"Promise".as_ptr(), 7);
    let ctor_ptr =
        crate::value::js_nanbox_get_pointer(ctor_value) as *mut crate::closure::ClosureHeader;
    if !ctor_ptr.is_null() {
        let existing = crate::closure::closure_get_dynamic_prop(ctor_ptr as usize, name);
        if existing.to_bits() != crate::value::TAG_UNDEFINED {
            return existing;
        }
    }

    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    super::native_module::set_bound_native_closure_name(closure, name);
    super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);

    let value = crate::value::js_nanbox_pointer(closure as i64);
    if !ctor_ptr.is_null() {
        crate::closure::closure_set_dynamic_prop(ctor_ptr as usize, name, value);
        super::set_builtin_property_attrs(
            ctor_ptr as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }
    value
}

extern "C" fn url_can_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url::js_url_coerce_string(input);
    let ok = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url::js_url_can_parse(input_ptr)
    } else {
        let base_ptr = crate::url::js_url_coerce_string(base);
        crate::url::js_url_can_parse_with_base(input_ptr, base_ptr)
    };
    f64::from_bits(crate::value::JSValue::bool(ok != 0).bits())
}

extern "C" fn url_parse_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    base: f64,
) -> f64 {
    let input_ptr = crate::url::js_url_coerce_string(input);
    let url = if base.to_bits() == crate::value::TAG_UNDEFINED {
        crate::url::js_url_parse(input_ptr)
    } else {
        let base_ptr = crate::url::js_url_coerce_string(base);
        crate::url::js_url_parse_with_base(input_ptr, base_ptr)
    };
    if url.is_null() {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        crate::value::js_nanbox_pointer(url as i64)
    }
}

extern "C" fn subtle_crypto_supports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let args = global_this_rest_array_values(rest);
    if args.len() < 2 {
        let message = format!(
            "Failed to execute 'supports' on 'SubtleCrypto': 2 arguments required, but only {} present.",
            args.len()
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_MISSING_ARGS");
    }

    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let op = args[0];
    let algorithm = args[1];
    let length = args.get(2).copied().unwrap_or(undefined);
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        unsafe { std::mem::transmute(ptr) };
    let dispatch_args = [op, algorithm, length];
    unsafe {
        dispatch(
            b"supports".as_ptr(),
            "supports".len(),
            dispatch_args.as_ptr(),
            dispatch_args.len(),
        )
    }
}

fn is_subtle_crypto_this(value: f64) -> bool {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let obj = js_value.as_pointer::<ObjectHeader>();
    !obj.is_null()
        && unsafe { (*obj).class_id } == super::native_module::NATIVE_MODULE_CLASS_ID
        && unsafe { super::native_module::read_native_module_name(obj) }
            .is_some_and(|name| name == "crypto.subtle")
}

fn rejected_type_error_with_code_promise(message: &str, code: &'static str) -> f64 {
    let reason = crate::fs::validate::build_type_error_with_code_value(message, code);
    let promise = crate::promise::js_promise_rejected(reason);
    crate::value::js_nanbox_pointer(promise as i64)
}

fn subtle_crypto_dispatch_rest(method_name: &str, rest: f64) -> f64 {
    let this_value = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if !is_subtle_crypto_this(this_value) {
        return rejected_type_error_with_code_promise(
            "Value of \"this\" must be of type SubtleCrypto",
            "ERR_INVALID_THIS",
        );
    }

    let args = global_this_rest_array_values(rest);
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(Ordering::SeqCst);
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        unsafe { std::mem::transmute(ptr) };
    unsafe {
        dispatch(
            method_name.as_ptr(),
            method_name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

extern "C" fn subtle_crypto_encapsulate_bits_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("encapsulateBits", rest)
}

extern "C" fn subtle_crypto_decapsulate_bits_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("decapsulateBits", rest)
}

extern "C" fn subtle_crypto_encapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("encapsulateKey", rest)
}

extern "C" fn subtle_crypto_decapsulate_key_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    subtle_crypto_dispatch_rest("decapsulateKey", rest)
}

/// Install a single callable static method on a constructor closure as a
/// `{ writable: true, enumerable: false, configurable: true }` data property
/// (matching Node's descriptors for built-in statics). `has_rest` registers
/// the func pointer as a rest-arg closure so trailing args arrive as an array.
pub(super) fn install_constructor_static(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
    has_rest: bool,
) {
    install_constructor_static_with_call_arity(ctor, name, func_ptr, arity, arity, has_rest);
}

pub(super) fn install_constructor_static_with_call_arity(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    spec_length: u32,
    call_arity: u32,
    has_rest: bool,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    super::native_module::set_bound_native_closure_name(closure, name);
    super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
    super::set_builtin_property_attrs(
        ctor as usize,
        name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_number_static_data_properties(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    let props = [
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("MAX_VALUE", f64::MAX),
        // ECMAScript Number.MIN_VALUE is the smallest *denormal* (5e-324 =
        // 2^-1074 = bit pattern 1), NOT f64::MIN_POSITIVE (smallest *normal*).
        ("MIN_VALUE", f64::from_bits(1)),
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
    ];
    for (name, value) in props {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(ctor as *mut ObjectHeader, key, value);
        super::set_builtin_property_attrs(
            ctor as usize,
            name.to_string(),
            super::PropertyAttrs::new(false, false, false),
        );
    }
}

/// #2889: install the common static methods on the `Object` / `Array`
/// constructor closures so rebound usage (`const O = Object; O.keys(x)`)
/// dispatches through the real runtime helpers. Only the high-traffic
/// statics with simple f64-in / f64-out shapes are reified here; the long
/// tail (`Object.defineProperty`, `Object.getOwnPropertyDescriptor`, …)
/// stays unreified on the rebound value and is a known scope gap.
fn install_builtin_constructor_statics(name: &str, ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    match name {
        "Object" => {
            install_constructor_static(ctor, "keys", object_keys_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "values", object_values_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "entries",
                object_entries_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "freeze", object_freeze_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "create", object_create_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "getPrototypeOf",
                object_get_prototype_of_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyNames",
                object_get_own_property_names_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "getOwnPropertyDescriptor",
                object_get_own_property_descriptor_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "defineProperty",
                object_define_property_thunk as *const u8,
                3,
                false,
            );
            install_constructor_static(
                ctor,
                "fromEntries",
                object_from_entries_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "assign",
                object_assign_thunk as *const u8,
                2,
                1,
                true,
            );
            install_constructor_static(ctor, "hasOwn", object_hasown_thunk as *const u8, 2, false);
        }
        "Array" => {
            install_constructor_static(
                ctor,
                "isArray",
                array_is_array_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "from", array_from_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "of", array_of_thunk as *const u8, 0, true);
        }
        "Promise" => {
            for static_name in [
                "resolve",
                "reject",
                "all",
                "race",
                "allSettled",
                "any",
                "withResolvers",
                "try",
            ] {
                if let Some((func_ptr, spec_length, call_arity, has_rest)) =
                    promise_static_function_spec(static_name)
                {
                    install_constructor_static_with_call_arity(
                        ctor,
                        static_name,
                        func_ptr,
                        spec_length,
                        call_arity,
                        has_rest,
                    );
                }
            }
        }
        "Date" => {
            // `Date.now` / `Date.parse` / `Date.UTC` as real own data props
            // (thunks live in `date_proto_thunks`). The functional calls are
            // codegen intrinsics, so this only affects value reads + reflection.
            date_proto_thunks::install_date_constructor_statics(ctor);
        }
        "Number" => {
            install_constructor_static(ctor, "isNaN", number_is_nan_thunk as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "isFinite",
                number_is_finite_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isInteger",
                number_is_integer_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "isSafeInteger",
                number_is_safe_integer_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "parseFloat",
                number_parse_float_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "parseInt",
                number_parse_int_thunk as *const u8,
                2,
                false,
            );
        }
        "BigInt" => {
            // BigInt.asIntN(bits, bigint) / asUintN(bits, bigint) — spec length 2.
            install_constructor_static(
                ctor,
                "asIntN",
                bigint_as_int_n_thunk as *const u8,
                2,
                false,
            );
            install_constructor_static(
                ctor,
                "asUintN",
                bigint_as_uint_n_thunk as *const u8,
                2,
                false,
            );
        }
        "Symbol" => {
            install_constructor_static(ctor, "for", symbol_for_thunk as *const u8, 1, false);
            install_constructor_static(ctor, "keyFor", symbol_key_for_thunk as *const u8, 1, false);
        }
        "String" => {
            // #4627: reify the variadic `String.fromCharCode` / `fromCodePoint`
            // statics so they are real function values (correct `.name` /
            // `.length`, usable via reference / spread). Call-arity 0 (all args
            // collected into `rest`) with spec `.length` 1. `String.raw` (a tag
            // function) is left on its intrinsic path for now.
            install_constructor_static_with_call_arity(
                ctor,
                "fromCharCode",
                string_from_char_code_static as *const u8,
                1,
                0,
                true,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "fromCodePoint",
                string_from_code_point_static as *const u8,
                1,
                0,
                true,
            );
            // #4627: `String.raw` (tag function) — 1 fixed param (template
            // object) + rest substitutions; spec `.length` 1.
            install_constructor_static_with_call_arity(
                ctor,
                "raw",
                string_raw_static as *const u8,
                1,
                1,
                true,
            );
        }
        "Promise" => {
            // #4521: reify the `Promise` statics as first-class function values
            // (correct `.name` / `.length`, usable via reference / `.call` /
            // `.apply` / spread). Direct calls (`Promise.all([...])`) keep the
            // codegen fast path; these back value reads and rebound usage.
            install_constructor_static(
                ctor,
                "resolve",
                promise_resolve_static as *const u8,
                1,
                false,
            );
            install_constructor_static(
                ctor,
                "reject",
                promise_reject_static as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "all", promise_all_static as *const u8, 1, false);
            install_constructor_static(ctor, "race", promise_race_static as *const u8, 1, false);
            install_constructor_static(
                ctor,
                "allSettled",
                promise_all_settled_static as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "any", promise_any_static as *const u8, 1, false);
            // `withResolvers` takes no arguments → spec `.length` 0.
            install_constructor_static(
                ctor,
                "withResolvers",
                promise_with_resolvers_static as *const u8,
                0,
                false,
            );
            // `try(fn, ...args)`: 1 fixed param (callback) + rest; spec `.length` 1.
            install_constructor_static_with_call_arity(
                ctor,
                "try",
                promise_try_static as *const u8,
                1,
                1,
                true,
            );
        }
        "ArrayBuffer" => {
            install_constructor_static(
                ctor,
                "isView",
                array_buffer_is_view_thunk as *const u8,
                1,
                false,
            );
        }
        "Response" => {
            install_constructor_static(
                ctor,
                "error",
                global_this_response_error_thunk as *const u8,
                0,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "json",
                global_this_response_json_thunk as *const u8,
                1,
                2,
                false,
            );
            install_constructor_static_with_call_arity(
                ctor,
                "redirect",
                global_this_response_redirect_thunk as *const u8,
                1,
                2,
                false,
            );
        }
        "URL" => {
            install_constructor_static(
                ctor,
                "canParse",
                url_can_parse_thunk as *const u8,
                1,
                false,
            );
            install_constructor_static(ctor, "parse", url_parse_thunk as *const u8, 1, false);
        }
        "SubtleCrypto" => {
            install_constructor_static_with_call_arity(
                ctor,
                "supports",
                subtle_crypto_supports_thunk as *const u8,
                2,
                0,
                true,
            );
            super::set_builtin_property_attrs(
                ctor as usize,
                "supports".to_string(),
                super::PropertyAttrs::new(true, true, true),
            );
        }
        _ => {}
    }
}

/// Install a method on a prototype object as a callable closure value with
/// the proper `name` property and registered arity. Used to reify built-in
/// prototype methods so `Array.prototype.map`, `Date.prototype.toISOString`,
/// etc. read back as `typeof === "function"` (issue #2142) — the actual
/// method *call* path is already covered by codegen's NativeMethodCall and
/// the `try_builtin_prototype_method_apply_call` HIR rewrite, so the no-op
/// thunk backing here is only invoked when user code calls the method
/// through indirection (`const m = Array.prototype.map; m.call(arr, fn)`),
/// a rare pattern. The reification is the value-read parity win.
///
/// `func_ptr` defaults to `global_this_builtin_noop_thunk` (returns
/// undefined) for methods we don't have a dedicated thunk for; callers
/// that want spec-accurate call behavior pass a custom thunk instead
/// (`array_prototype_slice_thunk`, `object_prototype_to_string_thunk`).
pub(super) fn install_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::native_module::set_bound_native_closure_name(closure, method_name);
    // #3143: record this method's spec `.length` per closure instance — all
    // noop-backed methods share one func_ptr, so the func-ptr arity registry
    // can't distinguish `map` (1) from `slice` (2). Read back by the `.length`
    // value-accessor and `getOwnPropertyDescriptor`.
    super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    // Built-in prototype methods are `{ writable: true, enumerable: false,
    // configurable: true }` per spec. Record that descriptor (reflection-only,
    // no hot-path gate flip) so `Object.getOwnPropertyDescriptor`, `Object.keys`
    // and `for-in` all observe them as non-enumerable — Test262's `verifyProperty`
    // checks every built-in method this way. See `set_builtin_property_attrs`.
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    // #3143: the method's own `.name` / `.length` data properties are
    // `{ writable: false, enumerable: false, configurable: true }` per spec.
    // Register those on the closure itself so `getOwnPropertyDescriptor(
    // Array.prototype.map, "name")` reports `writable: false` (it previously
    // read the dynamic-prop slot and defaulted to writable). Reflection-only —
    // no hot-path gate flip.
    super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    value
}

pub(super) fn install_proto_method_rest(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    fixed_arity: u32,
) {
    install_proto_method_rest_with_length(
        proto_obj,
        method_name,
        func_ptr,
        fixed_arity,
        fixed_arity,
    );
}

pub(super) fn install_proto_method_rest_with_length(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    spec_length: u32,
    call_fixed_arity: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_register_closure_rest(func_ptr, call_fixed_arity);
    super::native_module::set_bound_native_closure_name(closure, method_name);
    super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(proto_obj, key, value);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    value
}

/// #4139/#4437: reify the `JSON` namespace's own methods for reflection parity
/// and detached value calls. Direct call sites are still codegen intrinsics.
fn install_json_namespace_members(ns_obj: *mut ObjectHeader) {
    const METHODS: &[(&str, *const u8, u32)] = &[
        ("parse", json_parse_thunk as *const u8, 2),
        ("stringify", json_stringify_thunk as *const u8, 3),
        ("rawJSON", json_raw_json_thunk as *const u8, 1),
        ("isRawJSON", json_is_raw_json_thunk as *const u8, 1),
    ];
    for (name, func_ptr, arity) in METHODS.iter().copied() {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

/// #4139: reify the `Reflect` namespace's own methods for reflection parity.
/// See `install_math_namespace` for the rationale.
fn install_reflect_namespace_members(ns_obj: *mut ObjectHeader) {
    let noop = global_this_builtin_noop_thunk as *const u8;
    let methods = [
        ("defineProperty", noop, 3),
        ("deleteProperty", noop, 2),
        ("apply", reflect_apply_thunk as *const u8, 3),
        ("construct", noop, 2),
        ("get", noop, 2),
        ("getOwnPropertyDescriptor", noop, 2),
        ("getPrototypeOf", noop, 1),
        ("has", noop, 2),
        ("isExtensible", noop, 1),
        ("ownKeys", noop, 1),
        ("preventExtensions", noop, 1),
        ("set", noop, 3),
        ("setPrototypeOf", noop, 2),
    ];
    for (name, func_ptr, arity) in methods {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

fn install_atomics_namespace_members(ns_obj: *mut ObjectHeader) {
    for (name, func_ptr, arity) in [
        ("load", crate::atomics::js_atomics_load as *const u8, 2),
        (
            "isLockFree",
            crate::atomics::js_atomics_is_lock_free as *const u8,
            1,
        ),
        ("store", crate::atomics::js_atomics_store as *const u8, 3),
        ("add", crate::atomics::js_atomics_add as *const u8, 3),
        ("sub", crate::atomics::js_atomics_sub as *const u8, 3),
        ("and", crate::atomics::js_atomics_and as *const u8, 3),
        ("or", crate::atomics::js_atomics_or as *const u8, 3),
        ("xor", crate::atomics::js_atomics_xor as *const u8, 3),
        (
            "exchange",
            crate::atomics::js_atomics_exchange as *const u8,
            3,
        ),
        (
            "compareExchange",
            crate::atomics::js_atomics_compare_exchange as *const u8,
            4,
        ),
        ("notify", crate::atomics::js_atomics_notify as *const u8, 3),
        ("wait", crate::atomics::js_atomics_wait as *const u8, 4),
        (
            "waitAsync",
            crate::atomics::js_atomics_wait_async as *const u8,
            4,
        ),
    ] {
        install_proto_method(ns_obj, name, func_ptr, arity);
    }
}

/// Install a list of `(method_name, arity)` pairs on a prototype object.
/// Most entries are reflection-only methods backed by
/// `global_this_builtin_noop_thunk`, but inherited Object methods with
/// observable receiver-sensitive behavior use their real thunk.
fn install_noop_proto_methods(proto_obj: *mut ObjectHeader, methods: &[(&str, u32)]) {
    for (name, arity) in methods.iter().copied() {
        let func_ptr = match name {
            "isPrototypeOf" => object_prototype_is_prototype_of_thunk as *const u8,
            _ => global_this_builtin_noop_thunk as *const u8,
        };
        install_proto_method(proto_obj, name, func_ptr, arity);
    }
}

extern "C" fn url_pattern_test_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = crate::object::js_implicit_this_get();
    let pattern = crate::value::js_nanbox_get_pointer(this_value) as *mut ObjectHeader;
    crate::url::js_url_pattern_test(pattern, input, base)
}

extern "C" fn url_pattern_exec_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    rest: f64,
) -> f64 {
    let base = rest_first_arg(rest);
    let this_value = crate::object::js_implicit_this_get();
    let pattern = crate::value::js_nanbox_get_pointer(this_value) as *mut ObjectHeader;
    crate::url::js_url_pattern_exec(pattern, input, base)
}

fn rest_first_arg(rest: f64) -> f64 {
    let value = crate::value::JSValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let arr = value.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() || crate::array::js_array_length(arr) == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::array::js_array_get_f64(arr, 0)
}

/// Universal `Object.prototype` methods inherited by every receiver in
/// JS. Installed on every built-in constructor's prototype since Perry's
/// prototype chain on these built-ins doesn't walk back up to a shared
/// `Object.prototype` — so `Number.prototype.hasOwnProperty` would
/// otherwise be missing.
const OBJECT_PROTO_METHODS: &[(&str, u32)] = &[
    ("hasOwnProperty", 1),
    ("isPrototypeOf", 1),
    ("propertyIsEnumerable", 1),
    ("toLocaleString", 0),
    ("valueOf", 0),
    // `toString` is installed separately on Object/typed arrays etc. with
    // dedicated thunks; do not include it here to avoid clobbering those.
];

/// Populate well-known method properties on a built-in constructor's
/// prototype object. Each registered method is a closure carrying a
/// proper `name` property so feature-detection idioms like
/// `typeof Array.prototype.map === "function"` and `.name === "map"`
/// agree with Node when the value is read through indirection.
///
/// Two of these methods retain dedicated thunks for spec-accurate call
/// behavior — `Array.prototype.slice` (ramda's curry/variadic helpers
/// reach through `Array.prototype.slice.call(args, …)` and depend on it
/// returning a real sliced array, even via indirection) and
/// `Object.prototype.toString` (ramda's `_isArguments.js` IIFE calls
/// `Object.prototype.toString.call(arguments)` at module-init time).
/// All other methods are noop-backed: typeof + `.name` introspection
/// works, but a stored-and-called-indirect reference returns undefined.
/// The common forms — `arr.map(fn)` (codegen's NativeMethodCall) and
/// `Array.prototype.map.call(arr, fn)` (HIR rewrite, see
/// `try_builtin_prototype_method_apply_call`) — are unaffected.
fn populate_builtin_prototype_methods(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // #3662: Map/Set/WeakMap/WeakSet prototypes get brand-checking thunks
    // (own module, to keep this file under the 2000-line gate).
    if collection_proto_thunks::install_collection_proto_methods(builtin_name, proto_obj) {
        install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        return;
    }
    // #4100: primitive wrapper prototypes need real thunks for their own
    // methods so reflective calls brand-check `this` instead of hitting the
    // generic Object no-op/valueOf fallbacks.
    if primitive_proto_thunks::install_primitive_proto_methods(builtin_name, proto_obj) {
        install_noop_proto_methods(
            proto_obj,
            &[
                ("hasOwnProperty", 1),
                ("isPrototypeOf", 1),
                ("propertyIsEnumerable", 1),
            ],
        );
        if !matches!(builtin_name, "Number") {
            install_noop_proto_methods(proto_obj, &[("toLocaleString", 0)]);
        }
        return;
    }
    match builtin_name {
        "Array" => {
            install_proto_method(
                proto_obj,
                "slice",
                array_prototype_slice_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("at", 1),
                    ("concat", 1),
                    ("copyWithin", 2),
                    ("entries", 0),
                    ("every", 1),
                    ("fill", 1),
                    ("filter", 1),
                    ("find", 1),
                    ("findIndex", 1),
                    ("findLast", 1),
                    ("findLastIndex", 1),
                    ("flat", 0),
                    ("flatMap", 1),
                    ("forEach", 1),
                    ("includes", 1),
                    ("indexOf", 1),
                    ("join", 1),
                    ("keys", 0),
                    ("lastIndexOf", 1),
                    ("map", 1),
                    ("pop", 0),
                    ("push", 1),
                    ("reduce", 1),
                    ("reduceRight", 1),
                    ("reverse", 0),
                    ("shift", 0),
                    ("some", 1),
                    ("sort", 1),
                    ("splice", 2),
                    ("toLocaleString", 0),
                    ("toReversed", 0),
                    ("toSorted", 1),
                    ("toSpliced", 2),
                    ("toString", 0),
                    ("unshift", 1),
                    ("values", 0),
                    ("with", 2),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "ArrayBuffer" => {
            install_noop_proto_methods(proto_obj, &[("slice", 2)]);
            unsafe {
                crate::closure::js_register_closure_arity(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                let getter = crate::closure::js_closure_alloc(
                    array_buffer_byte_length_getter_thunk as *const u8,
                    0,
                );
                if !getter.is_null() {
                    let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
                    install_builtin_getter(proto_obj, "byteLength", getter_bits);
                    set_accessor_descriptor(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        AccessorDescriptor {
                            get: getter_bits,
                            set: 0,
                        },
                    );
                    set_property_attrs(
                        proto_obj as usize,
                        "byteLength".to_string(),
                        PropertyAttrs::new(true, false, true),
                    );
                }
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "DataView" => {
            // Install the reflectable `byteLength`/`byteOffset`/`buffer`
            // accessors and the `get*`/`set*` numeric methods on
            // `DataView.prototype` (own module). Instances already work via
            // codegen / `buffer_dispatch`; these only close the reflection +
            // `DataView.prototype.getInt32.call(dv, …)` cascade.
            super::dataview_proto_thunks::install_dataview_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Object" => {
            install_proto_method(
                proto_obj,
                "toString",
                object_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "propertyIsEnumerable",
                object_prototype_property_is_enumerable_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toLocaleString",
                object_prototype_to_locale_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                object_prototype_value_of_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "propertyIsEnumerable",
                object_prototype_property_is_enumerable_thunk as *const u8,
                1,
            );
        }
        "Function" => {
            // `Function.prototype` has own `length` (0) and `name` ("") data
            // properties, each `{ writable: false, enumerable: false,
            // configurable: true }` (ECMA-262 20.2.3). Install them first so
            // `length` precedes `name` in `getOwnPropertyNames` order, matching
            // the built-in-function property order Test262 checks.
            {
                let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                js_object_set_field_by_name(
                    proto_obj,
                    len_key,
                    f64::from_bits(JSValue::number(0.0).bits()),
                );
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "length".to_string(),
                    super::PropertyAttrs::new(false, false, true),
                );
                let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
                let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
                js_object_set_field_by_name(
                    proto_obj,
                    name_key,
                    f64::from_bits(JSValue::string_ptr(empty).bits()),
                );
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    "name".to_string(),
                    super::PropertyAttrs::new(false, false, true),
                );
            }
            install_proto_method(
                proto_obj,
                "apply",
                function_prototype_apply_thunk as *const u8,
                2,
            );
            install_proto_method_rest(
                proto_obj,
                "bind",
                function_prototype_bind_thunk as *const u8,
                1,
            );
            // #4101: dedicated toString thunk (source reconstruction + brand
            // check) instead of the shared no-op.
            install_proto_method(
                proto_obj,
                "toString",
                function_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method_rest(
                proto_obj,
                "call",
                function_prototype_call_thunk as *const u8,
                1,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_function_has_instance_symbol(proto_obj);
        }
        "String" => {
            // #4713: generic-`this` char-access methods + `Symbol.iterator`, and
            // (this change) every other coercing method (slice/indexOf/split/
            // replace/…) get real reflective thunks (RequireObjectCoercible +
            // ToString) installed by `install_string_proto_methods` so
            // `String.prototype.slice.call(receiver, …)` works on a boxed/object
            // receiver. Only `toString` (and `valueOf`, via OBJECT_PROTO_METHODS)
            // stay no-op-backed: they are brand-checked (must throw on a
            // non-String `this`), not ToString-coercing, so a generic coercing
            // thunk would be wrong.
            string_proto_thunks::install_string_proto_methods("String", proto_obj);
            install_noop_proto_methods(proto_obj, &[("toString", 0)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Number" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("toExponential", 1),
                    ("toFixed", 1),
                    ("toPrecision", 1),
                    ("toString", 1),
                ],
            );
            // OBJECT_PROTO_METHODS installs noop `valueOf`/`toLocaleString`, so
            // it must run BEFORE the brand thunks below — otherwise it clobbers
            // them back to no-ops.
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: `valueOf`/`toLocaleString` brand-check `this` and throw a
            // `TypeError` on an incompatible reflective receiver instead of
            // falling back to `Object.prototype` (`"[object Object]"`).
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::number_proto_value_of_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "toLocaleString",
                primitive_proto_thunks::number_proto_to_locale_string_thunk as *const u8,
                0,
            );
        }
        "Boolean" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: brand-checking `toString`/`valueOf` (mirror `Number`).
            // Installed after OBJECT_PROTO_METHODS so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::boolean_proto_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::boolean_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Symbol" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: Symbol.prototype previously had no own methods, so
            // reflective `Symbol.prototype.toString.call(sym)` resolved to
            // `Object.prototype.toString` (`"[object Symbol]"`) and an
            // incompatible receiver returned `"[object Object]"` instead of
            // throwing. Install brand-checking thunks that re-dispatch to the
            // canonical symbol logic (`"Symbol(x)"`). After OBJECT_PROTO_METHODS
            // so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::symbol_proto_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::symbol_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "BigInt" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // #4100: mirror `Symbol` — brand-checking `toString`(radix)/`valueOf`
            // re-dispatched to the canonical BigInt logic (`(5n).toString(2)`
            // → `"101"`). After OBJECT_PROTO_METHODS so the brand `valueOf` wins.
            install_proto_method(
                proto_obj,
                "toString",
                primitive_proto_thunks::bigint_proto_to_string_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "valueOf",
                primitive_proto_thunks::bigint_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Date" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("getDate", 0),
                    ("getDay", 0),
                    ("getFullYear", 0),
                    ("getHours", 0),
                    ("getMilliseconds", 0),
                    ("getMinutes", 0),
                    ("getMonth", 0),
                    ("getSeconds", 0),
                    ("getTime", 0),
                    ("getTimezoneOffset", 0),
                    ("getUTCDate", 0),
                    ("getUTCDay", 0),
                    ("getUTCFullYear", 0),
                    ("getUTCHours", 0),
                    ("getUTCMilliseconds", 0),
                    ("getUTCMinutes", 0),
                    ("getUTCMonth", 0),
                    ("getUTCSeconds", 0),
                    ("getYear", 0),
                    ("setDate", 1),
                    ("setFullYear", 3),
                    ("setHours", 4),
                    ("setMilliseconds", 1),
                    ("setMinutes", 3),
                    ("setMonth", 2),
                    ("setSeconds", 2),
                    ("setTime", 1),
                    ("setUTCDate", 1),
                    ("setUTCFullYear", 3),
                    ("setUTCHours", 4),
                    ("setUTCMilliseconds", 1),
                    ("setUTCMinutes", 3),
                    ("setUTCMonth", 2),
                    ("setUTCSeconds", 2),
                    ("setYear", 1),
                    ("toDateString", 0),
                    ("toISOString", 0),
                    ("toJSON", 1),
                    ("toLocaleDateString", 0),
                    ("toLocaleString", 0),
                    ("toLocaleTimeString", 0),
                    ("toTimeString", 0),
                    ("toUTCString", 0),
                    ("valueOf", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            // Overwrite the no-op getter entries with brand-checking thunks so
            // `Date.prototype.getX.call(this)` performs `thisTimeValue(this)`
            // (TypeError on a non-Date receiver) and dispatches correctly.
            // MUST run after the OBJECT_PROTO_METHODS block, which would
            // otherwise re-clobber `valueOf` with the generic Object no-op.
            date_proto_thunks::install_date_proto_getters(proto_obj);
            // Same treatment for the mutating setters: `Date.prototype.setX`
            // brand-checks `this`, reads `[[DateValue]]` before coercing args,
            // then mutates the cell. Also after the OBJECT_PROTO_METHODS block.
            date_proto_thunks::install_date_proto_setters(proto_obj);
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "toString",
                date_prototype_to_string_thunk as *const u8,
                0,
            );
        }
        "RegExp" => {
            install_noop_proto_methods(
                proto_obj,
                &[("exec", 1), ("test", 1), ("toString", 0), ("compile", 2)],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "URLPattern" => {
            install_proto_method_rest(proto_obj, "exec", url_pattern_exec_thunk as *const u8, 1);
            install_proto_method_rest(proto_obj, "test", url_pattern_test_thunk as *const u8, 1);
            for name in [
                "hasRegExpGroups",
                "hash",
                "hostname",
                "password",
                "pathname",
                "port",
                "protocol",
                "search",
                "username",
            ] {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                js_object_set_field_by_name(
                    proto_obj,
                    key,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                super::set_builtin_property_attrs(
                    proto_obj as usize,
                    name.to_string(),
                    super::PropertyAttrs::new(false, false, true),
                );
            }
        }
        "Promise" => {
            install_proto_method(
                proto_obj,
                "catch",
                crate::promise::promise_prototype_catch_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "finally",
                crate::promise::promise_prototype_finally_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "then",
                crate::promise::promise_prototype_then_thunk as *const u8,
                2,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "TextEncoder" => {
            install_noop_proto_methods(proto_obj, &[("encode", 1), ("encodeInto", 2)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "TextDecoder" => {
            install_noop_proto_methods(proto_obj, &[("decode", 1)]);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Headers" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getSetCookie", 0),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Request" | "Response" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("blob", 0),
                    ("bytes", 0),
                    ("clone", 0),
                    ("formData", 0),
                    ("json", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Blob" | "File" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("arrayBuffer", 0),
                    ("bytes", 0),
                    ("slice", 0),
                    ("stream", 0),
                    ("text", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "FormData" => {
            install_noop_proto_methods(
                proto_obj,
                &[
                    ("append", 2),
                    ("delete", 1),
                    ("entries", 0),
                    ("forEach", 1),
                    ("get", 1),
                    ("getAll", 1),
                    ("has", 1),
                    ("keys", 0),
                    ("set", 2),
                    ("values", 0),
                ],
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "WebSocket" => {
            websocket_global::install_proto_methods(proto_obj);
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Crypto" => {
            install_webcrypto_proto_getter(
                proto_obj,
                "subtle",
                webcrypto_subtle_getter_thunk as *const u8,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "getRandomValues",
                webcrypto_get_random_values_thunk as *const u8,
                1,
            );
            install_webcrypto_proto_method(
                proto_obj,
                "randomUUID",
                webcrypto_random_uuid_thunk as *const u8,
                0,
            );
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "CryptoKey" => {
            for (name, func_ptr) in [
                ("algorithm", cryptokey_algorithm_getter_thunk as *const u8),
                (
                    "extractable",
                    cryptokey_extractable_getter_thunk as *const u8,
                ),
                ("type", cryptokey_type_getter_thunk as *const u8),
                ("usages", cryptokey_usages_getter_thunk as *const u8),
            ] {
                install_webcrypto_proto_getter(proto_obj, name, func_ptr);
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "SubtleCrypto" => {
            for (name, func_ptr, length) in [
                (
                    "encapsulateBits",
                    subtle_crypto_encapsulate_bits_thunk as *const u8,
                    2,
                ),
                (
                    "decapsulateBits",
                    subtle_crypto_decapsulate_bits_thunk as *const u8,
                    3,
                ),
                (
                    "encapsulateKey",
                    subtle_crypto_encapsulate_key_thunk as *const u8,
                    5,
                ),
                (
                    "decapsulateKey",
                    subtle_crypto_decapsulate_key_thunk as *const u8,
                    6,
                ),
            ] {
                install_webcrypto_proto_method_rest_with_length(proto_obj, name, func_ptr, length);
            }
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
        }
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" => {
            install_noop_proto_methods(proto_obj, OBJECT_PROTO_METHODS);
            install_proto_method(
                proto_obj,
                "toString",
                error_prototype_to_string_thunk as *const u8,
                0,
            );
            install_proto_method(
                proto_obj,
                "isPrototypeOf",
                object_prototype_is_prototype_of_thunk as *const u8,
                1,
            );
            install_proto_method(
                proto_obj,
                "hasOwnProperty",
                object_prototype_has_own_property_thunk as *const u8,
                1,
            );
        }
        // Typed-array constructors: keep the reified per-kind prototype
        // method set (#2142) on each per-kind `.prototype` so direct
        // reads like `Int8Array.prototype.at` continue to return a
        // function. The accessor descriptors
        // (`length`/`byteLength`/`byteOffset`/`buffer`) are installed
        // *only* on the shared `%TypedArray%.prototype` (#2145, in
        // `ensure_typed_array_intrinsic`) — reached via
        // `Object.getPrototypeOf(Int8Array.prototype) ===
        // %TypedArray%.prototype`. Pre-#2145 they were also stamped on
        // each per-kind proto because `getPrototypeOf(per_kind)`
        // returned identity; now that it walks to the intrinsic, they
        // belong on the parent (matches Node's
        // `getOwnPropertyDescriptor(Int8Array.prototype, "length")` =
        // `undefined`).
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" => {
            // Per spec the per-kind prototype is nearly empty: every method,
            // accessor, `Symbol.iterator`, `Symbol.toStringTag`, `toString`,
            // and `toLocaleString` lives on the shared `%TypedArray%.prototype`
            // (this proto's `[[Prototype]]`) and is *inherited*, not own — so
            // `Int8Array.prototype.hasOwnProperty("map") === false` and
            // `Int8Array.prototype.map === %TypedArray%.prototype.map`
            // (test262 `prototype/*/inherited.js`). The only own properties are
            // `constructor` (set in the constructor-setup path) and
            // `BYTES_PER_ELEMENT`. The static-prototype link to the intrinsic
            // is wired alongside the `OBJ_FLAG_TYPED_ARRAY_PROTO` flag so the
            // generic property-get chain walk resolves the inherited methods.
        }
        _ => {}
    }
}

fn install_error_prototype_data_properties(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    let name = match builtin_name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError"
        | "AggregateError" | "EvalError" | "URIError" => builtin_name,
        _ => return,
    };
    if proto_obj.is_null() {
        return;
    }

    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_value =
        crate::string::js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        proto_obj,
        name_key,
        crate::value::js_nanbox_string(name_value as i64),
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "name".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let message_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message_value = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    js_object_set_field_by_name(
        proto_obj,
        message_key,
        crate::value::js_nanbox_string(message_value as i64),
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "message".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_webcrypto_proto_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    install_proto_method(proto_obj, method_name, func_ptr, arity);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
}

fn install_webcrypto_proto_method_rest_with_length(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    func_ptr: *const u8,
    length: u32,
) {
    install_proto_method_rest_with_length(proto_obj, method_name, func_ptr, length, 0);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        method_name.to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
}

fn install_webcrypto_proto_getter(proto_obj: *mut ObjectHeader, name: &str, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let value = if closure.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        crate::value::js_nanbox_pointer(closure as i64)
    };
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto_obj, key, f64::from_bits(crate::value::TAG_UNDEFINED));
    super::set_builtin_accessor_descriptor(
        proto_obj as usize,
        name.to_string(),
        super::AccessorDescriptor {
            get: value.to_bits(),
            set: 0,
        },
        super::PropertyAttrs::new(true, true, true),
    );
}
