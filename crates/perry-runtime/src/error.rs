//! Error object implementation for Perry
//!
//! Provides the built-in Error class and its subclasses.

use crate::string::{js_string_from_bytes, StringHeader};

/// Object type tag for runtime type discrimination
pub const OBJECT_TYPE_REGULAR: u32 = 1;
pub const OBJECT_TYPE_ERROR: u32 = 2;
/// #1789: a heap "class object" — the value a class EXPRESSION evaluates to
/// (a regular object stamped with the compile-time template's `class_id`,
/// carrying per-evaluation static fields as own properties). Marks the value
/// as the CLASS itself (vs an instance) so `typeof` is "function", and
/// `new`/`instanceof` read `class_id` from the object. Own-field get/set
/// treat it like OBJECT_TYPE_REGULAR (the get/set paths are gated on
/// `gc_type`/`class_id`, not on this tag).
pub const OBJECT_TYPE_CLASS: u32 = 3;

/// Error subclass discriminator (stored in `error_kind`).
/// Used by `instanceof TypeError` etc. to check kind without name string compare.
pub const ERROR_KIND_ERROR: u32 = 0;
pub const ERROR_KIND_TYPE_ERROR: u32 = 1;
pub const ERROR_KIND_RANGE_ERROR: u32 = 2;
pub const ERROR_KIND_REFERENCE_ERROR: u32 = 3;
pub const ERROR_KIND_SYNTAX_ERROR: u32 = 4;
pub const ERROR_KIND_AGGREGATE_ERROR: u32 = 5;
pub const ERROR_KIND_EVAL_ERROR: u32 = 6;
pub const ERROR_KIND_URI_ERROR: u32 = 7;

const ERROR_FLAG_HAS_MESSAGE: u32 = 1 << 0;
const ERROR_FLAG_HAS_CAUSE: u32 = 1 << 1;
const ERROR_FLAG_HAS_ERRORS: u32 = 1 << 2;

/// Special class IDs for `instanceof` checks (must match perry-codegen/src/expr.rs)
pub const CLASS_ID_ERROR: u32 = 0xFFFF0001;
pub const CLASS_ID_TYPE_ERROR: u32 = 0xFFFF0010;
pub const CLASS_ID_RANGE_ERROR: u32 = 0xFFFF0011;
pub const CLASS_ID_REFERENCE_ERROR: u32 = 0xFFFF0012;
pub const CLASS_ID_SYNTAX_ERROR: u32 = 0xFFFF0013;
pub const CLASS_ID_AGGREGATE_ERROR: u32 = 0xFFFF0014;
pub const CLASS_ID_EVAL_ERROR: u32 = 0xFFFF0015;
pub const CLASS_ID_URI_ERROR: u32 = 0xFFFF0016;
/// AssertionError is a plain ObjectHeader (so it can carry the extra
/// `actual` / `expected` / `operator` / `code` / `generatedMessage`
/// fields Node attaches), but it is registered via
/// `js_register_class_extends_error` at runtime init so
/// `err instanceof Error` returns true on a thrown AssertionError.
pub const CLASS_ID_ASSERTION_ERROR: u32 = 0xFFFF0020;

pub(crate) fn error_kind_constructor_name(kind: u32) -> &'static str {
    match kind {
        ERROR_KIND_TYPE_ERROR => "TypeError",
        ERROR_KIND_RANGE_ERROR => "RangeError",
        ERROR_KIND_REFERENCE_ERROR => "ReferenceError",
        ERROR_KIND_SYNTAX_ERROR => "SyntaxError",
        ERROR_KIND_AGGREGATE_ERROR => "AggregateError",
        ERROR_KIND_EVAL_ERROR => "EvalError",
        ERROR_KIND_URI_ERROR => "URIError",
        _ => "Error",
    }
}

/// Error object header
#[repr(C)]
pub struct ErrorHeader {
    /// Type tag to distinguish from regular objects (must be first field!)
    pub object_type: u32,
    /// Error kind discriminator (TypeError, RangeError, etc.)
    pub error_kind: u32,
    /// Own-property presence bits for spec-visible slots.
    pub flags: u32,
    /// Error message as a string pointer
    pub message: *mut StringHeader,
    /// Error name (e.g., "Error", "TypeError")
    pub name: *mut StringHeader,
    /// Stack trace (simplified - just a string for now)
    pub stack: *mut StringHeader,
    /// Cause (raw f64 NaN-boxed value, undefined if not set)
    pub cause: f64,
    /// Errors array for AggregateError (raw ArrayHeader pointer or null)
    pub errors: *mut crate::array::ArrayHeader,
}

thread_local! {
    /// #5247: the source location (`file`, 1-based `line`) of the call
    /// currently being dispatched, set by codegen-emitted
    /// `js_set_call_location` immediately before a dynamic method/call
    /// dispatch that can throw "X is not a function" / "is not a
    /// constructor". `make_stack` reads it so the thrown TypeError's
    /// `.stack` shows `at <file>:<line>` instead of `at <anonymous>`.
    ///
    /// Only populated when the program was compiled with `--debug-symbols`
    /// (the flag that gates the codegen emission). `None` in the default
    /// build, so release perf and the `<anonymous>` fallback are unchanged.
    /// Raw `(file_ptr, file_len, line)` of the pending call site. The
    /// pointer is a codegen string-pool rodata global (process lifetime;
    /// `js_set_call_location` is a generated-code-only callee), so storing
    /// it raw and rendering lazily keeps the per-dispatch recording
    /// allocation-free (#6386 — this runs before EVERY dynamic dispatch in
    /// a `--debug-symbols` build).
    static CURRENT_CALL_LOCATION: std::cell::Cell<Option<(usize, usize, u32)>> =
        const { std::cell::Cell::new(None) };
}

/// #5247: record the source location of the call about to be dispatched.
/// Codegen emits this call right before a dynamic dispatch under
/// `--debug-symbols`. `file_ptr`/`file_len` are the (UTF-8) source path;
/// `line` is the 1-based line number. A `line` of 0 clears the location.
///
/// # Safety
/// `file_ptr` must point to `file_len` valid bytes, or be null with
/// `file_len == 0`.
#[no_mangle]
pub unsafe extern "C" fn js_set_call_location(file_ptr: *const u8, file_len: usize, line: u32) {
    if line == 0 || file_ptr.is_null() || file_len == 0 {
        CURRENT_CALL_LOCATION.with(|c| c.set(None));
        return;
    }
    CURRENT_CALL_LOCATION.with(|c| c.set(Some((file_ptr as usize, file_len, line))));
}

// Generated-code-only callee: anchor against the auto-optimize LTO dead-strip
// (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_SET_CALL_LOCATION: unsafe extern "C" fn(*const u8, usize, u32) =
    js_set_call_location;

/// #5247: render the current call-location frame, or `<anonymous>` when no
/// location was recorded (default builds, or a synthesized/offset-less site).
fn current_stack_frame() -> String {
    CURRENT_CALL_LOCATION.with(|c| match c.get() {
        Some((file_ptr, file_len, line)) => {
            let bytes = unsafe { std::slice::from_raw_parts(file_ptr as *const u8, file_len) };
            format!("    at {}:{}", String::from_utf8_lossy(bytes), line)
        }
        None => "    at <anonymous>".to_string(),
    })
}

unsafe fn make_stack(name: &str, message: &str) -> *mut StringHeader {
    // Build a simple "<name>: <message>\n    at <file>:<line>" string
    // (or "<anonymous>" when no #5247 source location is recorded). Real
    // multi-frame stack traces are not implemented; the test only checks
    // `.includes(message)`.
    let frame = current_stack_frame();
    let s = if message.is_empty() {
        format!("{}\n{}", name, frame)
    } else {
        format!("{}: {}\n{}", name, message, frame)
    };
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

unsafe fn alloc_error(
    kind: u32,
    name_bytes: &[u8],
    message: *mut StringHeader,
    has_message: bool,
) -> *mut ErrorHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    // #321 frontier issue #69 (sibling to #2230's `dyn_index_get` guard):
    // codegen lowers `new Error(value)` by handing the value straight to
    // `js_error_new_with_message` even when `value` is not a string pointer
    // (e.g. effect's Cause.ts ends up calling `new Error(...)` with a tag
    // value like `1` along some pretty-printer paths). `alloc_error` then
    // dereferences the pointer at `(*message).byte_len` (offset 4) — for a
    // `message` of 1 that reads address `0x5` and SIGSEGVs.
    //
    // Defensive runtime guard: if `message` does not look like a real
    // heap-allocated string pointer (NaN-boxing tag values, small ints, and
    // garbage from non-pointer dataflow all fail this gate), coerce it to
    // an empty string before any reads. The same `is_valid_obj_ptr`
    // predicate guards `dyn_index_get/set` (#2230), `js_object_keys`, the
    // by-name field setters, and several typed-feedback probes — this
    // brings the error-allocation path under the same umbrella.
    let message_ptr = if message.is_null() || !crate::object::is_valid_obj_ptr(message as *const u8)
    {
        js_string_from_bytes(b"".as_ptr(), 0)
    } else {
        message
    };
    let message_handle = scope.root_string_ptr(message_ptr);

    let error_name = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
    let error_name_handle = scope.root_string_ptr(error_name);

    let message_ptr = message_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    let msg_str = {
        let len = (*message_ptr).byte_len as usize;
        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let name_str = std::str::from_utf8(name_bytes).unwrap_or("Error");
    let stack = make_stack(name_str, msg_str);
    let stack_handle = scope.root_string_ptr(stack);

    let raw = crate::arena::arena_alloc_gc(
        std::mem::size_of::<ErrorHeader>(),
        std::mem::align_of::<ErrorHeader>(),
        crate::gc::GC_TYPE_ERROR,
    );
    let ptr = raw as *mut ErrorHeader;

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

    (*ptr).object_type = OBJECT_TYPE_ERROR;
    (*ptr).error_kind = kind;
    (*ptr).flags = if has_message {
        ERROR_FLAG_HAS_MESSAGE
    } else {
        0
    };
    (*ptr).message = message_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).name = error_name_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).stack = stack_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).cause = f64::from_bits(TAG_UNDEFINED);
    (*ptr).errors = std::ptr::null_mut();

    ptr
}

pub(crate) unsafe fn error_set_cause(error: *mut ErrorHeader, cause: f64) {
    crate::gc::runtime_store_gc_jsvalue_slot(
        error as usize,
        &(*error).cause as *const f64 as usize,
        cause.to_bits(),
    );
    (*error).flags |= ERROR_FLAG_HAS_CAUSE;
}

pub(crate) unsafe fn error_set_errors(
    error: *mut ErrorHeader,
    errors: *mut crate::array::ArrayHeader,
) {
    crate::gc::runtime_store_gc_heap_word_slot(
        error as usize,
        &(*error).errors as *const _ as usize,
        errors as u64,
    );
    (*error).flags |= ERROR_FLAG_HAS_ERRORS;
}

/// Create a new Error with no message
#[no_mangle]
pub extern "C" fn js_error_new() -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, b"Error", std::ptr::null_mut(), false) }
}

/// Create a new Error with a message
#[no_mangle]
pub extern "C" fn js_error_new_with_message(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, b"Error", message, true) }
}

/// Create a new Error-like object with a custom `.name` and stack prefix.
pub(crate) fn js_error_new_with_name_message(
    name: &'static [u8],
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, name, message, true) }
}

/// Create a new Error-like object with a dynamically supplied `.name`.
pub(crate) fn js_error_new_with_name_message_bytes(
    name: &[u8],
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, name, message, true) }
}

/// Create a new Error with a message and a cause (raw f64 NaN-boxed)
#[no_mangle]
pub extern "C" fn js_error_new_with_cause(
    message: *mut StringHeader,
    cause: f64,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = alloc_error(ERROR_KIND_ERROR, b"Error", message, true);
        error_set_cause(ptr, cause);
        ptr
    }
}

/// Create a new TypeError with a message
#[no_mangle]
pub extern "C" fn js_typeerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_TYPE_ERROR, b"TypeError", message, true) }
}

/// Create a new RangeError with a message
#[no_mangle]
pub extern "C" fn js_rangeerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_RANGE_ERROR, b"RangeError", message, true) }
}

/// #5067 — throw a catchable `RangeError: Array buffer allocation failed`
/// (V8/Node's allocation-failure message) instead of aborting the process
/// when a user-controlled-size backing buffer cannot be allocated. Shared
/// by the Set/Map/RegExp backing-store allocators.
#[cold]
pub(crate) fn throw_allocation_failed() -> ! {
    let msg = b"Array buffer allocation failed";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = js_rangeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Create a new ReferenceError with a message
#[no_mangle]
pub extern "C" fn js_referenceerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_REFERENCE_ERROR, b"ReferenceError", message, true) }
}

thread_local! {
    /// Interned `&'static str` for each distinct Node `ERR_*` code passed
    /// across the FFI boundary, so it can be stored in the
    /// message→code side table read by the `.code` getter. Bounded: each
    /// distinct code string leaks at most once per thread.
    static INTERNED_ERROR_CODES: std::cell::RefCell<std::collections::HashMap<String, &'static str>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn intern_error_code(code: &str) -> &'static str {
    INTERNED_ERROR_CODES.with(|m| {
        if let Some(v) = m.borrow().get(code) {
            return *v;
        }
        let leaked: &'static str = Box::leak(code.to_string().into_boxed_str());
        m.borrow_mut().insert(code.to_string(), leaked);
        leaked
    })
}

/// Generic "build a JS Error subclass carrying a Node `.code`" FFI entry
/// point for out-of-crate callers that have no direct access to
/// `perry-runtime`'s Rust API. Building + registering in this single extern
/// symbol guarantees the message→code registration and the later `.code` read
/// resolve through the same runtime copy, avoiding the staticlib thread-local
/// divergence that split registration/read paths hit.
///
/// `kind`: 0 = Error, 1 = TypeError, 2 = RangeError. The message and code
/// are UTF-8 byte slices. Returns a NaN-boxed Error value.
///
/// # Safety
/// `msg_ptr` must point to `msg_len` valid bytes; `code_ptr` must point to
/// `code_len` valid bytes or be null with `code_len == 0`.
#[no_mangle]
pub unsafe extern "C" fn js_error_value_with_code(
    msg_ptr: *const u8,
    msg_len: usize,
    code_ptr: *const u8,
    code_len: usize,
    kind: i32,
) -> f64 {
    let msg = js_string_from_bytes(msg_ptr, msg_len as u32);
    if !code_ptr.is_null() && code_len > 0 {
        let code_bytes = std::slice::from_raw_parts(code_ptr, code_len);
        if let Ok(code_str) = std::str::from_utf8(code_bytes) {
            let interned = intern_error_code(code_str);
            crate::node_submodules::register_error_code_pub(msg, interned);
        }
    }
    let err = match kind {
        1 => js_typeerror_new(msg),
        2 => js_rangeerror_new(msg),
        _ => js_error_new_with_message(msg),
    };
    crate::value::js_nanbox_pointer(err as i64)
}

/// Generic "throw a JS Error subclass carrying a Node `.code`" FFI entry
/// point for out-of-crate callers (e.g. `perry-ext-http-server`'s http2
/// settings helpers) that have no direct access to `perry-runtime`'s Rust
/// API. Diverges via `js_throw`.
///
/// # Safety
/// Same pointer validity requirements as [`js_error_value_with_code`].
#[no_mangle]
pub unsafe extern "C" fn js_throw_error_with_code(
    msg_ptr: *const u8,
    msg_len: usize,
    code_ptr: *const u8,
    code_len: usize,
    kind: i32,
) -> ! {
    crate::exception::js_throw(js_error_value_with_code(
        msg_ptr, msg_len, code_ptr, code_len, kind,
    ))
}

/// Build a Node-style *system* `Error` value carrying `.message`, `.code`
/// (a Node `E*` string like `"ECONNREFUSED"`), `.syscall` (the failing call,
/// e.g. `"connect"`) and `.errno` (the libuv-negative number). This is the
/// shape Node hands to `socket`/`request` `'error'` listeners for transport
/// failures, so consumers branching on `err.code === 'ECONNREFUSED'` work.
///
/// Like [`js_error_value_with_code`], building + registering through this
/// single extern symbol guarantees the message→{code,syscall,errno} side-table
/// registrations and the later `.code`/`.syscall`/`.errno` getter reads resolve
/// through the same runtime copy (the getters live in
/// `object::field_get_set`). The `code`/`syscall` strings are interned to
/// `&'static str` so they outlive the call.
///
/// # Safety
/// `msg_ptr`/`code_ptr`/`syscall_ptr` must each point to the corresponding
/// `*_len` valid bytes, or be null with their length `0`.
#[no_mangle]
pub unsafe extern "C" fn js_node_system_error_value(
    msg_ptr: *const u8,
    msg_len: usize,
    code_ptr: *const u8,
    code_len: usize,
    syscall_ptr: *const u8,
    syscall_len: usize,
    errno: f64,
) -> f64 {
    let msg = js_string_from_bytes(msg_ptr, msg_len as u32);
    if !code_ptr.is_null() && code_len > 0 {
        let bytes = std::slice::from_raw_parts(code_ptr, code_len);
        if let Ok(s) = std::str::from_utf8(bytes) {
            crate::node_submodules::register_error_code_pub(msg, intern_error_code(s));
        }
    }
    if !syscall_ptr.is_null() && syscall_len > 0 {
        let bytes = std::slice::from_raw_parts(syscall_ptr, syscall_len);
        if let Ok(s) = std::str::from_utf8(bytes) {
            crate::node_submodules::register_error_syscall(msg, intern_error_code(s));
        }
    }
    crate::node_submodules::register_error_errno(msg, errno as i32);
    let err = js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

// These FFI entries are referenced only from extension archives (linked after
// the runtime's bitcode is optimized), so the auto-optimize LTO pass would
// otherwise dead-strip them (see project_auto_optimize_keepalive_3320). The
// `#[used]` anchors pin them.
#[used]
static KEEP_JS_ERROR_VALUE_WITH_CODE: unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    i32,
) -> f64 = js_error_value_with_code;

#[used]
static KEEP_JS_NODE_SYSTEM_ERROR_VALUE: unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    *const u8,
    usize,
    f64,
) -> f64 = js_node_system_error_value;

#[used]
static KEEP_JS_THROW_ERROR_WITH_CODE: unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    i32,
) -> ! = js_throw_error_with_code;

/// Throw `ERR_PERRY_UNIMPLEMENTED` for a registered-but-stub API. Used
/// by the stub-elimination epic's strict mode (#4918/#4919): a runtime
/// stub calls [`crate::stub_diag::perry_runtime_stub`] to warn, then —
/// if `PERRY_STRICT_STUBS=1` — diverges here instead of returning a
/// fake value. `api` is the JS-facing name; `issue` an optional tag.
pub fn throw_unimplemented_stub(api: &str, issue: Option<&str>) -> ! {
    let msg = match issue {
        Some(tag) => format!(
            "{} is not implemented in Perry (stub); set PERRY_STRICT_STUBS=0 to allow the fake result — tracking {}",
            api, tag
        ),
        None => format!(
            "{} is not implemented in Perry (stub); set PERRY_STRICT_STUBS=0 to allow the fake result",
            api
        ),
    };
    let code = b"ERR_PERRY_UNIMPLEMENTED";
    // SAFETY: both slices are valid UTF-8 byte ranges living for the
    // duration of the call; kind 0 = generic Error.
    unsafe {
        js_throw_error_with_code(msg.as_ptr(), msg.len(), code.as_ptr(), code.len(), 0);
    }
}

/// Convenience for runtime stub sites: warn (first-call) and, under
/// `PERRY_STRICT_STUBS`, throw `ERR_PERRY_UNIMPLEMENTED`. Returns
/// normally in non-strict mode so the caller proceeds with its
/// deterministic/fake fallback. (#4918/#4919)
pub fn stub_warn_or_throw(api: &'static str, reason: &'static str, issue: Option<&'static str>) {
    crate::stub_diag::perry_runtime_stub(api, reason, issue);
    if crate::stub_diag::strict_stubs_enabled() {
        throw_unimplemented_stub(api, issue);
    }
}

/// Create a new SyntaxError with a message
#[no_mangle]
pub extern "C" fn js_syntaxerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_SYNTAX_ERROR, b"SyntaxError", message, true) }
}

/// Create a new EvalError with a message
#[no_mangle]
pub extern "C" fn js_evalerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_EVAL_ERROR, b"EvalError", message, true) }
}

/// Create a new URIError with a message
#[no_mangle]
pub extern "C" fn js_urierror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_URI_ERROR, b"URIError", message, true) }
}

/// Create a new AggregateError with an errors array and a message
#[no_mangle]
pub extern "C" fn js_aggregateerror_new(
    errors: *mut crate::array::ArrayHeader,
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = alloc_error(
            ERROR_KIND_AGGREGATE_ERROR,
            b"AggregateError",
            message,
            !message.is_null(),
        );
        error_set_errors(ptr, errors);
        ptr
    }
}

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;

/// #2836: extract the ES2022 `cause` option from a runtime options *value*
/// and apply it to an already-allocated error. Node sets `.cause` whenever
/// the options argument is an object that has a `cause` property (own or
/// inherited); the value can be anything, including `undefined`. Perry reads
/// the property via the generic dynamic index getter so it works for plain
/// object literals AND options held in a variable / produced dynamically.
///
/// Non-object option values (`undefined`, a number, a string, …) leave the
/// cause untouched — matching Node, which simply ignores them.
unsafe fn apply_cause_from_options(error: *mut ErrorHeader, options: f64) {
    let opts = crate::value::JSValue::from_bits(options.to_bits());
    if !opts.is_pointer() {
        return;
    }
    // Only honor a real `cause` slot. `js_dyn_index_get` returns
    // TAG_UNDEFINED both for "no such key" and for `{ cause: undefined }`;
    // either way Node would store `undefined`, but storing undefined is the
    // already-initialized default, so a missing key is a harmless no-op.
    let key = js_string_from_bytes(b"cause".as_ptr(), 5);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    let cause = crate::value::js_dyn_index_get(options, key_f64);
    if cause.to_bits() != TAG_UNDEFINED_BITS {
        error_set_cause(error, cause);
    }
}

/// #5127: apply the ES2022 `cause` option to a user `Error` *subclass*
/// instance when its constructor forwards `super(message, options)`. Such an
/// instance is a generic heap object (not an `ErrorHeader`) — the super-call
/// codegen sets `message`/`name` as object properties — so the cause must be
/// installed as a (non-enumerable) own `cause` property on the object too,
/// matching Node's `InstallErrorCause`. Mirrors `apply_cause_from_options`:
/// reads `options.cause` via the generic getter (works for object literals
/// and runtime-held options alike) and is a no-op for non-object options.
#[no_mangle]
pub extern "C" fn js_error_apply_cause_to_object(
    obj: *mut crate::object::ObjectHeader,
    options: f64,
) {
    let opts = crate::value::JSValue::from_bits(options.to_bits());
    if !opts.is_pointer() {
        return;
    }
    let key = js_string_from_bytes(b"cause".as_ptr(), 5);
    let key_f64 = crate::value::js_nanbox_string(key as i64);
    let cause = crate::value::js_dyn_index_get(options, key_f64);
    if cause.to_bits() != TAG_UNDEFINED_BITS {
        crate::object::js_object_set_field_by_name_nonenum(obj, key as *const StringHeader, cause);
    }
}

/// #2836: allocate an Error (or native subclass) carrying a `{ cause }`
/// option read from an arbitrary runtime options value. `kind` selects the
/// ERROR_KIND_* discriminant so `instanceof TypeError`/etc. keep working.
#[no_mangle]
pub extern "C" fn js_error_new_kind_with_options(
    kind: u32,
    message: *mut StringHeader,
    options: f64,
) -> *mut ErrorHeader {
    let name: &[u8] = match kind {
        ERROR_KIND_TYPE_ERROR => b"TypeError",
        ERROR_KIND_RANGE_ERROR => b"RangeError",
        ERROR_KIND_REFERENCE_ERROR => b"ReferenceError",
        ERROR_KIND_SYNTAX_ERROR => b"SyntaxError",
        ERROR_KIND_EVAL_ERROR => b"EvalError",
        ERROR_KIND_URI_ERROR => b"URIError",
        _ => b"Error",
    };
    unsafe {
        let resolved_kind = if name == b"Error" {
            ERROR_KIND_ERROR
        } else {
            kind
        };
        let ptr = alloc_error(resolved_kind, name, message, !message.is_null());
        apply_cause_from_options(ptr, options);
        ptr
    }
}

fn throw_not_iterable_type_error() -> ! {
    let message = b"is not iterable";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn error_kind_name(kind: u32) -> (&'static [u8], u32) {
    match kind {
        ERROR_KIND_TYPE_ERROR => (b"TypeError", ERROR_KIND_TYPE_ERROR),
        ERROR_KIND_RANGE_ERROR => (b"RangeError", ERROR_KIND_RANGE_ERROR),
        ERROR_KIND_REFERENCE_ERROR => (b"ReferenceError", ERROR_KIND_REFERENCE_ERROR),
        ERROR_KIND_SYNTAX_ERROR => (b"SyntaxError", ERROR_KIND_SYNTAX_ERROR),
        ERROR_KIND_AGGREGATE_ERROR => (b"AggregateError", ERROR_KIND_AGGREGATE_ERROR),
        ERROR_KIND_EVAL_ERROR => (b"EvalError", ERROR_KIND_EVAL_ERROR),
        ERROR_KIND_URI_ERROR => (b"URIError", ERROR_KIND_URI_ERROR),
        _ => (b"Error", ERROR_KIND_ERROR),
    }
}

fn coerce_error_message_value(value: f64) -> Option<*mut StringHeader> {
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        return None;
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let msg = js_string_from_bytes(b"Cannot convert a Symbol value to a string".as_ptr(), 41);
        let err = js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    Some(crate::builtins::js_string_coerce(value))
}

#[no_mangle]
pub extern "C" fn js_error_new_from_value(value: f64) -> *mut ErrorHeader {
    js_error_new_kind_from_value(ERROR_KIND_ERROR, value)
}

#[no_mangle]
pub extern "C" fn js_error_new_kind_from_value(kind: u32, value: f64) -> *mut ErrorHeader {
    let (name, resolved_kind) = error_kind_name(kind);
    unsafe {
        match coerce_error_message_value(value) {
            Some(message) => alloc_error(resolved_kind, name, message, true),
            None => alloc_error(resolved_kind, name, std::ptr::null_mut(), false),
        }
    }
}

#[no_mangle]
pub extern "C" fn js_error_new_with_cause_from_value(value: f64, cause: f64) -> *mut ErrorHeader {
    unsafe {
        let ptr = js_error_new_from_value(value);
        error_set_cause(ptr, cause);
        ptr
    }
}

#[no_mangle]
pub extern "C" fn js_error_new_kind_with_options_from_value(
    kind: u32,
    value: f64,
    options: f64,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = js_error_new_kind_from_value(kind, value);
        apply_cause_from_options(ptr, options);
        ptr
    }
}

/// #2838/#2836: full `new AggregateError(errors, message?, options?)`
/// constructor. Consumes the `errors` iterable synchronously (throwing
/// `TypeError` when it is omitted or non-iterable), stores `.message`, and
/// applies the `{ cause }` option from `options` if present.
///
/// `errors` and `options` arrive as raw NaN-boxed values (the iterable must
/// not be pre-coerced to an array pointer — Sets / strings / generators must
/// reach `materialize_iterable` intact).
#[no_mangle]
pub extern "C" fn js_aggregateerror_new_full(
    errors: f64,
    message: *mut StringHeader,
    options: f64,
) -> *mut ErrorHeader {
    // #2838: reuse the spec-shaped iterable→array converter that backs the
    // Promise combinators (`Promise.any`/`all`/…). It accepts arrays, strings,
    // Set/Map, buffers, generators, and any object exposing `[Symbol.iterator]`
    // or a bare `.next` field, and returns `Err(_)` for non-iterables
    // (`undefined`, numbers, plain objects) — exactly the AggregateError
    // contract.
    let arr = match crate::promise::combinators::combinator_iterable_to_array(errors) {
        Ok(arr) => arr,
        Err(_) => throw_not_iterable_type_error(),
    };
    unsafe {
        let ptr = alloc_error(
            ERROR_KIND_AGGREGATE_ERROR,
            b"AggregateError",
            message,
            !message.is_null(),
        );
        error_set_errors(ptr, arr);
        apply_cause_from_options(ptr, options);
        ptr
    }
}

/// #2904: `Error.isError(value)` — V8/Node duck-check that returns `true`
/// only for genuine Error instances (any kind: base Error, TypeError, …,
/// AggregateError, and AssertionError-style objects registered as extending
/// Error). Plain objects, primitives, and null/undefined return `false`.
#[no_mangle]
pub extern "C" fn js_error_is_error(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    unsafe {
        let ptr = crate::value::js_nanbox_get_pointer(value) as *const u8;
        if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr) {
            return f64::from_bits(crate::value::TAG_FALSE);
        }
        let object_type = std::ptr::read(ptr as *const u32);
        if object_type == OBJECT_TYPE_ERROR {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
    }
    f64::from_bits(crate::value::TAG_FALSE)
}

/// Get the message property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_message(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"".as_ptr(), 0);
        }
        (*error).message
    }
}

/// Get the name property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_name(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"Error".as_ptr(), 5);
        }
        (*error).name
    }
}

pub(crate) unsafe fn js_error_has_own_property(error: *mut ErrorHeader, key: &str) -> bool {
    if error.is_null() {
        return false;
    }
    if crate::node_submodules::error_user_prop(error as usize, key).is_some() {
        return true;
    }
    match key {
        "message" => ((*error).flags & ERROR_FLAG_HAS_MESSAGE) != 0,
        "cause" => ((*error).flags & ERROR_FLAG_HAS_CAUSE) != 0,
        "errors" => ((*error).flags & ERROR_FLAG_HAS_ERRORS) != 0,
        "stack" => true,
        _ => false,
    }
}

pub(crate) unsafe fn js_error_builtin_own_property_is_enumerable(
    error: *mut ErrorHeader,
    key: &str,
) -> Option<bool> {
    if error.is_null() {
        return Some(false);
    }
    if crate::node_submodules::error_user_prop(error as usize, key).is_some() {
        return Some(true);
    }
    match key {
        "message" if ((*error).flags & ERROR_FLAG_HAS_MESSAGE) != 0 => Some(false),
        "cause" if ((*error).flags & ERROR_FLAG_HAS_CAUSE) != 0 => Some(false),
        "errors" if ((*error).flags & ERROR_FLAG_HAS_ERRORS) != 0 => Some(false),
        "stack" => Some(false),
        _ => None,
    }
}

/// Get the stack property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_stack(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"".as_ptr(), 0);
        }
        (*error).stack
    }
}

fn throw_builtin_not_constructor(name: &'static str) -> ! {
    let message = format!("{name} is not a constructor");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub extern "C" fn js_throw_symbol_constructor_type_error() -> f64 {
    throw_builtin_not_constructor("Symbol")
}

#[no_mangle]
pub extern "C" fn js_throw_bigint_constructor_type_error() -> f64 {
    throw_builtin_not_constructor("BigInt")
}

#[no_mangle]
pub extern "C" fn js_throw_strict_eval_arguments_syntax_error() -> f64 {
    let message = b"Unexpected eval or arguments in strict mode";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// PerformEval early errors (super outside its context / undeclared private
/// name in eval code) — a SyntaxError thrown when the eval call evaluates.
#[no_mangle]
pub extern "C" fn js_throw_eval_syntax_error(message: f64) -> f64 {
    let message = value_to_lossy_string(message);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_THROW_EVAL_SYNTAX_ERROR: extern "C" fn(f64) -> f64 = js_throw_eval_syntax_error;

#[no_mangle]
pub extern "C" fn js_throw_restricted_function_property_assignment() -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Restricted function property assignment",
        "ERR_INVALID_ARG_TYPE",
    )
}

// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_THROW_RESTRICTED_FN_PROP_ASSIGN: extern "C" fn() -> f64 =
    js_throw_restricted_function_property_assignment;

#[no_mangle]
pub extern "C" fn js_throw_math_constructor_type_error() -> f64 {
    throw_builtin_not_constructor("Math")
}

#[no_mangle]
pub extern "C" fn js_throw_json_constructor_type_error() -> f64 {
    throw_builtin_not_constructor("JSON")
}

#[no_mangle]
pub extern "C" fn js_throw_illegal_constructor_type_error() -> f64 {
    let message = b"Illegal constructor";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn value_to_lossy_string(value: f64) -> String {
    let string = crate::builtins::js_string_coerce(value);
    if string.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*string).byte_len as usize;
        let data = (string as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

#[no_mangle]
pub extern "C" fn js_throw_type_error_const_assignment(name: f64) -> f64 {
    let name = value_to_lossy_string(name);
    let msg = if name.is_empty() {
        "Assignment to constant variable.".to_string()
    } else {
        format!("Assignment to constant variable '{}'.", name)
    };
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64))
}

#[no_mangle]
pub extern "C" fn js_throw_reference_error_unresolvable_assignment(name: f64) -> f64 {
    let name = value_to_lossy_string(name);
    let msg = if name.is_empty() {
        "Assignment to undeclared variable.".to_string()
    } else {
        format!("{} is not defined", name)
    };
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_referenceerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64))
}

fn throw_reference_error_message(message: &'static [u8]) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_referenceerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Temporal Dead Zone ReferenceError. Thrown when a lexical `let`/`const`/
/// `class` binding is read, `typeof`-d, or compound-assigned before its
/// declaration has been evaluated — i.e. while its box still holds the
/// `TAG_TDZ` sentinel. `name` is the NaN-boxed binding name (or `undefined`
/// when codegen could not thread a name through, e.g. a captured box read).
/// Message matches V8/Node byte-for-byte: `Cannot access x before
/// initialization`.
#[no_mangle]
pub extern "C" fn js_throw_reference_error_tdz(name: f64) -> f64 {
    let name = value_to_lossy_string(name);
    let msg = if name.is_empty() {
        "Cannot access uninitialized variable before initialization".to_string()
    } else {
        format!("Cannot access {} before initialization", name)
    };
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_referenceerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64))
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code-
/// and runtime-only callee).
#[used]
static KEEP_JS_THROW_REFERENCE_ERROR_TDZ: extern "C" fn(f64) -> f64 = js_throw_reference_error_tdz;

#[no_mangle]
pub extern "C" fn js_throw_reference_error_unresolved_get() -> f64 {
    throw_reference_error_message(b"identifier is not defined")
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_GLOBAL_GET_OR_THROW_UNRESOLVED: extern "C" fn(f64) -> f64 =
    js_global_get_or_throw_unresolved;

/// Read a compile-time-unresolved identifier off `globalThis` (a global the
/// program created dynamically — `Function("this.y = 2")()` — exists only at
/// runtime), throwing the spec ReferenceError when no such global property
/// exists.
#[no_mangle]
pub extern "C" fn js_global_get_or_throw_unresolved(name_value: f64) -> f64 {
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    if gj.is_pointer() {
        let gptr = (gj.bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        let key = crate::builtins::js_string_coerce(name_value);
        if !gptr.is_null() && !key.is_null() {
            let v = unsafe { crate::object::js_object_get_field_by_name(gptr, key) };
            if !v.is_undefined() {
                return f64::from_bits(v.bits());
            }
            // A global binding initialized to `undefined` (a sloppy global var
            // created by `f = undefined`, or B.3.3.3 CreateGlobalVarBinding from
            // eval'd function hoisting) is *resolvable* — reading it yields
            // `undefined`, not a ReferenceError. `js_object_get_field_by_name`
            // can't tell "absent" from "present, value undefined", so confirm
            // the property actually exists (as an OWN property — a global var
            // binding always is) before falling through to the throw.
            let has = crate::object::js_object_has_own(g, name_value);
            if crate::value::js_is_truthy(has) != 0 {
                return f64::from_bits(crate::value::JSValue::undefined().bits());
            }
        }
    }
    let name = value_to_lossy_string(name_value);
    let msg = format!("{} is not defined", name);
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_referenceerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64))
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_GLOBAL_GET_OPTIONAL: extern "C" fn(f64) -> f64 = js_global_get_optional;

#[used]
static KEEP_JS_GLOBAL_UPDATE: extern "C" fn(f64, f64, f64) -> f64 = js_global_update;

/// `++x` / `x++` / `--x` / `x--` where `x` resolves to no lexical binding —
/// i.e. a (sloppy) global property reference. Read globalThis[name] (throwing
/// the spec ReferenceError when the property is absent — a genuinely
/// unresolvable reference, e.g. `++neverDeclared`), ToNumeric, step by 1 of
/// its own type, write the result back to globalThis, and return the
/// post-step value for a prefix op or the pre-step ToNumeric value for a
/// postfix op (#3575: `for (i = 0; i < n; i++)` with an undeclared `i`).
/// The boolean flags arrive NaN-boxed (codegen passes HIR `Bool` literals).
#[no_mangle]
pub extern "C" fn js_global_update(name_value: f64, is_increment: f64, is_prefix: f64) -> f64 {
    let is_increment = crate::value::js_is_truthy(is_increment);
    let is_prefix = crate::value::js_is_truthy(is_prefix) != 0;
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    let key = crate::builtins::js_string_coerce(name_value);
    let mut present = false;
    let old = if gj.is_pointer() && !key.is_null() {
        let gptr = (gj.bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() {
            let v = unsafe { crate::object::js_object_get_field_by_name(gptr, key) };
            if !v.is_undefined()
                || unsafe {
                    crate::object::js_object_has_own(g, name_value).to_bits()
                        == crate::value::TAG_TRUE
                }
            {
                present = true;
            }
            f64::from_bits(v.bits())
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    if !present {
        let name = value_to_lossy_string(name_value);
        let msg = format!("{} is not defined", name);
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = js_referenceerror_new(msg_str);
        return crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
    }
    let numeric = unsafe { crate::value::js_to_numeric(old) };
    let stepped = unsafe { crate::value::js_numeric_step(numeric, is_increment) };
    let gptr = (gj.bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    unsafe { crate::object::js_object_set_field_by_name(gptr, key, stepped) };
    if is_prefix {
        stepped
    } else {
        numeric
    }
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_GLOBAL_ASSIGN_EXISTING_OR_THROW: extern "C" fn(f64, f64) -> f64 =
    js_global_assign_existing_or_throw;

/// Strict-mode assignment to an identifier with no lexical binding
/// (#5989). Per spec (PutValue on an unresolvable-in-strict reference),
/// the name must first resolve against the global object: an EXISTING
/// global property is a normal property write — Next.js 16's
/// `cacheComponents` node-environment extensions do exactly this
/// (`Date = createDate(Date)` in strict CJS to install the dynamic-IO
/// clock interceptor, likewise `crypto`/`Math.random` wrappers) — and
/// only a genuinely absent binding throws the ReferenceError. The old
/// lowering threw unconditionally, so the extension install failed at
/// boot ("Failed to install `Date` class extension") and the dynamic
/// prerender-abort chain never armed. Presence probing + write-back
/// mirror `js_global_update` (the `++x`-on-global sibling). Sloppy mode
/// never reaches this helper — it lowers to a globalThis property set
/// that may CREATE the binding.
#[no_mangle]
pub extern "C" fn js_global_assign_existing_or_throw(name_value: f64, value: f64) -> f64 {
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    let key = crate::builtins::js_string_coerce(name_value);
    let mut present = false;
    if gj.is_pointer() && !key.is_null() {
        let gptr = (gj.bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() {
            let v = unsafe { crate::object::js_object_get_field_by_name(gptr, key) };
            if !v.is_undefined()
                || unsafe {
                    crate::object::js_object_has_own(g, name_value).to_bits()
                        == crate::value::TAG_TRUE
                }
            {
                present = true;
            }
        }
    }
    if !present {
        let name = value_to_lossy_string(name_value);
        let msg = format!("{} is not defined", name);
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = js_referenceerror_new(msg_str);
        return crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
    }
    let gptr = (gj.bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(gptr, key, value);
    // An assignment expression evaluates to its RHS.
    value
}

/// Non-throwing variant of [`js_global_get_or_throw_unresolved`] for
/// `typeof <unresolved ident>`: the spec's GetValue-skips-on-typeof rule means
/// a missing global yields `undefined` rather than a ReferenceError, but a
/// global created at RUNTIME (sloppy `foo = 1` lowers to a globalThis
/// property set — #3575) must still be observed.
#[no_mangle]
pub extern "C" fn js_global_get_optional(name_value: f64) -> f64 {
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    if gj.is_pointer() {
        let gptr = (gj.bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        let key = crate::builtins::js_string_coerce(name_value);
        if !gptr.is_null() && !key.is_null() {
            let v = unsafe { crate::object::js_object_get_field_by_name(gptr, key) };
            return f64::from_bits(v.bits());
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_throw_reference_error_unresolved_assignment() -> f64 {
    throw_reference_error_message(b"assignment to undeclared variable")
}

/// `delete super.prop` / `delete super[expr]` is always a ReferenceError — a
/// super reference can never be the target of `delete` (spec: the
/// UnaryExpression `delete` evaluates the SuperProperty reference, and
/// `Reference.[[Base]]` carries `thisValue` with `IsSuperReference` true, so
/// the `delete` algorithm throws before attempting the delete). The args are
/// evaluated for their side effects first by codegen.
#[no_mangle]
pub extern "C" fn js_throw_reference_error_super_delete() -> f64 {
    throw_reference_error_message(b"Unsupported reference to 'super'")
}

/// A derived constructor whose body never calls `super()` leaves `this`
/// uninitialized; the implicit `return this` (or any `this` access) throws a
/// ReferenceError per ECMAScript. Refs class/subclass/builtin-objects/*/
/// super-must-be-called.
#[no_mangle]
pub extern "C" fn js_throw_reference_error_this_before_super() -> f64 {
    throw_reference_error_message(
        b"Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    )
}

fn throw_capture_stack_trace_target_type_error() -> ! {
    let message = b"The \"targetObject\" argument must be an object";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

// ───────────────────────── V8 CallSite shim ─────────────────────────
// Packages like `depd` and `source-map-support` set `Error.prepareStackTrace`
// and then call CallSite METHODS (`getFileName`, `getLineNumber`, …) on the
// "structured stack trace" array V8 passes as the 2nd argument. Perry doesn't
// capture real JS frames yet, so these return spec-compatible placeholders
// (`getFileName()` may legitimately be `null`; line/column 0). The point is the
// API SHAPE — without callable CallSite methods, `obj.stack.slice(1)[i]
// .getFileName()` throws `(string).getFileName is not a function` and modules
// like `next/dist/compiled/send` (bundled depd) crash at eager init.

extern "C" fn callsite_undefined(_c: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
extern "C" fn callsite_null(_c: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}
extern "C" fn callsite_zero(_c: *const crate::closure::ClosureHeader) -> f64 {
    0.0
}
extern "C" fn callsite_false(_c: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_FALSE)
}
extern "C" fn callsite_true(_c: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_TRUE)
}
extern "C" fn callsite_to_string(_c: *const crate::closure::ClosureHeader) -> f64 {
    let s = js_string_from_bytes(b"<anonymous>".as_ptr(), 11);
    crate::value::js_nanbox_string(s as i64)
}

fn attach_call_site_method(obj: *mut crate::object::ObjectHeader, name: &str, fp: *const u8) {
    crate::closure::js_register_closure_arity(fp, 0);
    // Singleton (func_ptr-keyed, permanent) closure — never collected/moved, so
    // the CallSite's method field stays valid across GC cycles.
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    if closure.is_null() {
        return;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

fn make_call_site() -> f64 {
    let obj = crate::object::js_object_alloc(0, 20);
    let undef = callsite_undefined as *const u8;
    let null = callsite_null as *const u8;
    let zero = callsite_zero as *const u8;
    let f = callsite_false as *const u8;
    let t = callsite_true as *const u8;
    attach_call_site_method(obj, "getFileName", undef);
    attach_call_site_method(obj, "getScriptNameOrSourceURL", undef);
    attach_call_site_method(obj, "getLineNumber", zero);
    attach_call_site_method(obj, "getColumnNumber", zero);
    attach_call_site_method(obj, "getEnclosingLineNumber", zero);
    attach_call_site_method(obj, "getEnclosingColumnNumber", zero);
    attach_call_site_method(obj, "getFunctionName", null);
    attach_call_site_method(obj, "getMethodName", null);
    attach_call_site_method(obj, "getTypeName", null);
    attach_call_site_method(obj, "getFunction", undef);
    attach_call_site_method(obj, "getThis", undef);
    attach_call_site_method(obj, "getEvalOrigin", undef);
    attach_call_site_method(obj, "getPosition", zero);
    attach_call_site_method(obj, "isNative", f);
    attach_call_site_method(obj, "isEval", f);
    attach_call_site_method(obj, "isToplevel", t);
    attach_call_site_method(obj, "isConstructor", f);
    attach_call_site_method(obj, "isAsync", f);
    attach_call_site_method(obj, "isPromiseAll", f);
    attach_call_site_method(obj, "toString", callsite_to_string as *const u8);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Build a structured-stack array of `n` (clamped) CallSite objects.
fn build_structured_stack(n: usize) -> f64 {
    let n = n.clamp(1, 64);
    // All CallSites carry identical placeholder data, so share ONE object across
    // every slot. Returned to the caller (GC-rooted like any native-call result).
    let cs = make_call_site();
    let cs_jsv = crate::value::JSValue::from_bits(cs.to_bits());
    let mut arr = crate::array::js_array_alloc(n as u32);
    for _ in 0..n {
        arr = crate::array::js_array_push(arr, cs_jsv);
    }
    f64::from_bits(crate::value::JSValue::array_ptr(arr).bits())
}

/// If the global `Error.prepareStackTrace` has been overridden with a user
/// function (i.e. it is no longer Perry's default thunk), return its NaN-boxed
/// closure value; otherwise `None`.
fn error_prepare_stack_trace_override() -> Option<f64> {
    let ctor = crate::object::ERROR_CONSTRUCTOR_PTR.with(|c| c.get());
    if ctor == 0 {
        return None;
    }
    let key = js_string_from_bytes(b"prepareStackTrace".as_ptr(), 17);
    let val =
        crate::object::js_object_get_field_by_name(ctor as *const crate::object::ObjectHeader, key);
    if !val.is_pointer() {
        return None;
    }
    let val_f64 = f64::from_bits(val.bits());
    let ptr = crate::value::js_nanbox_get_pointer(val_f64) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    let fp = unsafe { (*(ptr as *const crate::closure::ClosureHeader)).func_ptr } as usize;
    if fp == crate::object::default_prepare_stack_trace_func_ptr() {
        return None;
    }
    Some(val_f64)
}

/// Compute a `.stack` value honoring the current `Error.prepareStackTrace`.
/// With a user override, returns `prepareStackTrace(receiver, [CallSite, …])`;
/// otherwise the coarse string. Returning it (vs storing it) keeps the result
/// GC-rooted by the reading caller — a Rust-side store of the nursery CallSite
/// array gets reclaimed the moment this returns (V8 itself evaluates `.stack`
/// lazily for exactly this reason).
unsafe fn compute_stack_value(receiver: f64) -> f64 {
    if let Some(prep) = error_prepare_stack_trace_override() {
        let structured = build_structured_stack(10);
        let prep_ptr =
            crate::value::js_nanbox_get_pointer(prep) as *const crate::closure::ClosureHeader;
        return crate::closure::js_closure_call2(prep_ptr, receiver, structured);
    }
    let s = make_stack("Error", "");
    crate::value::js_nanbox_string(s as i64)
}

/// Lazy `stack` accessor installed by `Error.captureStackTrace`. Fires on read
/// with `this` bound to the target object (V8 semantics: `prepareStackTrace` is
/// consulted at access time, not capture time).
extern "C" fn error_stack_lazy_getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    unsafe { compute_stack_value(receiver) }
}

/// Install the lazy `stack` getter accessor on `target`.
unsafe fn install_lazy_stack_accessor(target_ptr: *mut crate::object::ObjectHeader) {
    let fp = error_stack_lazy_getter as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc(fp, 0);
    if closure.is_null() {
        return;
    }
    let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
    let key = js_string_from_bytes(b"stack".as_ptr(), 5);
    crate::object::ensure_key_in_keys_array(target_ptr, key);
    crate::object::install_builtin_getter(target_ptr, "stack", getter_bits);
    crate::object::set_accessor_descriptor(
        target_ptr as usize,
        "stack".to_string(),
        crate::object::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        target_ptr as usize,
        "stack".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// `Error.captureStackTrace(target[, constructorOpt])`.
///
/// Perry's stack strings are intentionally coarse today; this helper installs
/// the same non-enumerable `stack` data property shape Node exposes and
/// preserves the invalid-target TypeError contract.
#[no_mangle]
pub extern "C" fn js_error_capture_stack_trace(target: f64, _constructor_opt: f64) -> f64 {
    let target_value = crate::value::JSValue::from_bits(target.to_bits());
    if !target_value.is_pointer() {
        throw_capture_stack_trace_target_type_error();
    }

    unsafe {
        let target_ptr = target_value.as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        if target_ptr.is_null() || !crate::object::is_valid_obj_ptr(target_ptr as *const u8) {
            throw_capture_stack_trace_target_type_error();
        }

        // Install a lazy `stack` getter (V8 semantics): on read it consults the
        // current `Error.prepareStackTrace` and returns `prepareStackTrace(this,
        // [CallSite, …])` — the contract `depd` / `source-map-support` rely on —
        // or the coarse string when there is no override. A getter RETURNS the
        // value to its caller (GC-rooted), unlike a stored nursery array which a
        // minor GC would reclaim the instant `captureStackTrace` returns.
        install_lazy_stack_accessor(target_ptr);
    }

    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Read a `StringHeader`'s UTF-8 bytes into an owned `String` (lossy on
/// invalid UTF-8). Empty on null.
unsafe fn read_string_header_owned(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    String::from_utf8_lossy(bytes).into_owned()
}

/// `Error.prototype.toString()` — ECMAScript §20.5.3.4. Returns `name` when
/// `message` is empty, `message` when `name` is empty, otherwise
/// `"{name}: {message}"`. Previously errors fell back to `Object.prototype`'s
/// `"[object Object]"` for both explicit `e.toString()` and string coercion
/// (`String(e)`, template literals), diverging from Node byte-for-byte (#2135).
#[no_mangle]
pub extern "C" fn js_error_to_string(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        let name = error_to_string_part(error, "name", js_error_get_name(error), "Error");
        let message = error_to_string_part(error, "message", js_error_get_message(error), "");
        let result = if name.is_empty() {
            message
        } else if message.is_empty() {
            name
        } else {
            format!("{name}: {message}")
        };
        js_string_from_bytes(result.as_ptr(), result.len() as u32)
    }
}

unsafe fn error_to_string_part(
    error: *mut ErrorHeader,
    key: &str,
    fallback: *const StringHeader,
    undefined_default: &str,
) -> String {
    if let Some(value) = crate::node_submodules::error_user_prop(error as usize, key) {
        let value_jsv = crate::value::JSValue::from_bits(value.to_bits());
        if value_jsv.is_undefined() {
            return undefined_default.to_string();
        }
        return read_string_header_owned(crate::value::js_jsvalue_to_string(value));
    }
    read_string_header_owned(fallback)
}

/// Get the cause property of an Error (raw f64 NaN-boxed value)
#[no_mangle]
pub extern "C" fn js_error_get_cause(error: *mut ErrorHeader) -> f64 {
    unsafe {
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        if error.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        (*error).cause
    }
}

/// Get the errors array of an AggregateError (raw ArrayHeader pointer).
///
/// Codegen lowers EVERY `obj.errors` property read (regardless of `obj`'s
/// static type — frequently `any`) to this accessor and then unconditionally
/// re-applies `POINTER_TAG` to the result. That is only sound when `obj` is a
/// genuine native error: the `errors` field lives at a fixed byte offset in
/// `ErrorHeader`, so applying it to a *regular* user object reads an unrelated
/// property slot. Observed in the wild: a plain object (`object_type ==
/// OBJECT_TYPE_REGULAR`) whose `+48` slot held NaN-boxed `undefined`
/// (`0x7FFC_0000_0000_0001`); codegen OR-ed `POINTER_TAG` onto it to produce
/// `0x7FFD_0000_0000_0001` — a handle-band id (`raw = 1`), not a heap array —
/// which `for…of` then mis-iterated ("Iterator result is not an object").
///
/// So: only read the fixed `ErrorHeader.errors` slot for an actual native
/// error. For any other receiver, resolve the `errors` own-property
/// *generically* and hand back its clean pointer (or null) so the caller's
/// `POINTER_TAG` re-tag reconstitutes the real array — or a null receiver that
/// `for…of` rejects as not-iterable, exactly as the dynamic property read
/// would have.
#[no_mangle]
pub extern "C" fn js_error_get_errors(error: *mut ErrorHeader) -> *mut crate::array::ArrayHeader {
    unsafe {
        if error.is_null() {
            return std::ptr::null_mut();
        }
        // Reject the small-handle band and implausible addresses before any
        // dereference (these are not heap objects).
        let addr = error as usize;
        if !crate::value::addr_class::is_plausible_heap_addr(addr) {
            return std::ptr::null_mut();
        }
        // Native error objects carry `object_type == OBJECT_TYPE_ERROR` in
        // their first u32; only those have the `errors` field at a fixed
        // offset. (Matches the validation in `js_error_is_error`.)
        let object_type = std::ptr::read(error as *const u32);
        if object_type == OBJECT_TYPE_ERROR {
            return (*error).errors;
        }
        // Not a native error — resolve `.errors` as an ordinary own property
        // and return its clean (untagged) pointer payload. A non-pointer value
        // (undefined / number / …) yields null, which the caller's re-tag turns
        // into a null receiver that `for…of` rejects as not iterable.
        let key = js_string_from_bytes(b"errors".as_ptr(), 6);
        let value = crate::object::js_object_get_field_by_name(
            error as *const crate::object::ObjectHeader,
            key,
        );
        if value.is_pointer() {
            crate::value::js_nanbox_get_pointer(f64::from_bits(value.bits()))
                as *mut crate::array::ArrayHeader
        } else {
            std::ptr::null_mut()
        }
    }
}

/// Get the error kind discriminator (TypeError, RangeError, etc.)
#[no_mangle]
pub extern "C" fn js_error_get_kind(error: *mut ErrorHeader) -> u32 {
    unsafe {
        if error.is_null() {
            return ERROR_KIND_ERROR;
        }
        (*error).error_kind
    }
}

/// Issue #462: property access against `undefined` / `null` must throw
/// `TypeError` per spec. Codegen emits a tag check before the IC fast
/// path; on a TAG_UNDEFINED or TAG_NULL receiver, it calls this helper.
///
/// Issue #596: route through Perry's setjmp/longjmp exception machinery
/// so a user `try { obj.prop } catch (e) { ... }` (or the same shape
/// post-await inside an async fn body) catches the throw instead of
/// the program exiting with the diagnostic. Constructs a real
/// `TypeError` with the V8-shaped message and calls `js_throw` —
/// which longjmps to the nearest enclosing setjmp catch frame, OR
/// prints the uncaught diagnostic + exits 1 if `TRY_DEPTH == 0`
/// (preserving the prior user-visible behavior for unhandled cases).
///
/// `receiver_is_null` distinguishes "Cannot read properties of null"
/// from "Cannot read properties of undefined" (matches V8's wording).
/// `prop_name_ptr` / `prop_name_len` carry the static property name.
#[no_mangle]
pub extern "C" fn js_throw_type_error_property_access(
    receiver_is_null: u32,
    prop_name_ptr: *const u8,
    prop_name_len: usize,
) -> ! {
    let receiver = if receiver_is_null != 0 {
        "null"
    } else {
        "undefined"
    };
    let prop = if prop_name_ptr.is_null() || prop_name_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(prop_name_ptr, prop_name_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let msg = format!(
        "Cannot read properties of {} (reading '{}')",
        receiver, prop
    );
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Issue #510: calling a method on a primitive whose name doesn't
/// resolve on the auto-boxed prototype must throw `TypeError: <expr>
/// is not a function` per spec. Two call sites:
///
///   1. `crates/perry-codegen/src/lower_string_method.rs`'s
///      unknown-method catch-all — when the receiver's static type
///      narrows to `String` (any-typed local refined from a string
///      initializer) and the method name isn't in the hand-handled
///      set. Pre-fix the catch-all silently returned the receiver,
///      so `s.lengt()` evaluated to `s` and execution continued.
///
///   2. `crates/perry-runtime/src/object.rs::js_native_call_method`
///      catch-all — when the receiver is a non-string primitive
///      (number / int32 / bool / bigint) and dispatch is exhausted.
///      Pre-fix the catch-all returned `NULL_OBJECT_BYTES` and
///      execution continued.
///
/// `receiver_kind_*` carries a short label (`"string"` / `"number"` /
/// `"boolean"` / `"bigint"`) used for the diagnostic; pass null/0
/// to omit it. `prop_name_*` carries the called method name.
#[no_mangle]
pub extern "C" fn js_throw_type_error_not_a_function(
    receiver_kind_ptr: *const u8,
    receiver_kind_len: usize,
    prop_name_ptr: *const u8,
    prop_name_len: usize,
) -> ! {
    let kind = if receiver_kind_ptr.is_null() || receiver_kind_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(receiver_kind_ptr, receiver_kind_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let prop = if prop_name_ptr.is_null() || prop_name_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(prop_name_ptr, prop_name_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    // #596: route through Perry's exception machinery so the user's
    // `try { primVal.bogus() } catch (e) { ... }` catches the throw
    // rather than the program exiting. Falls back to print-and-exit
    // via `js_throw`'s `TRY_DEPTH == 0` path when there's no handler.
    let msg = if kind.is_empty() {
        format!("{} is not a function", prop)
    } else {
        format!("({}).{} is not a function", kind, prop)
    };
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// `new <primitive>(…)` where the callee is a primitive value (number, string,
/// boolean, null, undefined, bigint) → `TypeError: <x> is not a constructor`.
///
/// Codegen calls this for `new`-callee shapes whose value is a NaN-boxed
/// primitive the runtime construct path can't always tag-distinguish — most
/// importantly plain `f64` numbers, whose bit pattern overlaps the raw pointer
/// encoding (`new 1`, `new 1.5`). The thrown TypeError is catchable via Perry's
/// exception machinery, matching `try { new 1 } catch (e) { … }`.
#[no_mangle]
pub extern "C" fn js_throw_not_a_constructor() -> f64 {
    let msg = b"is not a constructor";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Issue #615: writes to read-only / frozen / sealed / non-extensible
/// objects must throw a TypeError under strict mode (per spec). TS files
/// are strict-by-default, so Perry treats every property write as if
/// "use strict" were in effect. The runtime call sites in
/// `js_object_set_field_by_name` (and the index-set helpers) used to
/// silently `return;` when the freeze/seal/writable check tripped —
/// matching non-strict-mode JS behavior — but that left
/// `try { obj.x = 1 } catch { ... }` never firing, breaking
/// `Object.freeze` / `Object.seal` / `Object.preventExtensions` /
/// `Object.defineProperty(..., writable:false)` round-trips.
///
/// `kind`:
///   0 → "Cannot assign to read only property '<key>' of object" (writable:false / frozen overwrite)
///   1 → "Cannot add property '<key>', object is not extensible"  (sealed / preventExtensions / frozen new key)
///   2 → "Cannot delete property '<key>' of #<Object>"             (sealed / frozen delete)
#[no_mangle]
pub extern "C" fn js_throw_type_error_immutable_write(
    kind: u32,
    key_ptr: *const u8,
    key_len: usize,
) -> ! {
    let key = if key_ptr.is_null() || key_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(key_ptr, key_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let msg = match kind {
        0 => format!(
            "Cannot assign to read only property '{}' of object '#<Object>'",
            key
        ),
        1 => format!("Cannot add property {}, object is not extensible", key),
        2 => format!("Cannot delete property '{}' of #<Object>", key),
        _ => format!("Cannot modify object: '{}'", key),
    };
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Convenience wrapper for the runtime — accepts the key as a
/// Rust `&str` and re-routes through the FFI throw helper above. Not
/// `#[no_mangle]` because callers are runtime modules, not codegen.
pub(crate) fn throw_immutable_write(kind: u32, key: &str) -> ! {
    js_throw_type_error_immutable_write(kind, key.as_ptr(), key.len())
}

// #2836/#2838/#2904: keep the codegen-emitted error FFIs alive through the
// auto-optimize whole-program-bitcode link. These `#[no_mangle]` fns are
// reachable only from generated `.o`; without `#[used]` anchors the
// internalize+dead-strip pass drops them and the default `perry file.ts -o`
// link fails (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_ERROR_NEW_KIND_WITH_OPTIONS: extern "C" fn(
    u32,
    *mut StringHeader,
    f64,
) -> *mut ErrorHeader = js_error_new_kind_with_options;
#[used]
static KEEP_AGGREGATEERROR_NEW_FULL: extern "C" fn(
    f64,
    *mut StringHeader,
    f64,
) -> *mut ErrorHeader = js_aggregateerror_new_full;
#[used]
static KEEP_ERROR_IS_ERROR: extern "C" fn(f64) -> f64 = js_error_is_error;

#[cfg(test)]
mod tostring_tests {
    use super::*;

    fn s(bytes: &[u8]) -> *mut StringHeader {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    #[test]
    fn error_to_string_name_and_message() {
        let e = js_error_new_with_message(s(b"boom"));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "Error: boom");
    }

    #[test]
    fn error_to_string_no_message_is_just_name() {
        let e = js_error_new_with_message(s(b""));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "Error");
    }

    #[test]
    fn typed_error_to_string_uses_subclass_name() {
        let e = js_error_new_with_name_message(b"TypeError", s(b"bad"));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "TypeError: bad");
    }

    #[test]
    fn get_errors_on_regular_object_reads_real_property_not_fixed_slot() {
        // Codegen lowers EVERY `obj.errors` read to `js_error_get_errors` and
        // then OR-s POINTER_TAG onto the result. For a *regular* object (not a
        // native error), the `ErrorHeader.errors` byte offset (+48) is an
        // unrelated slot — historically this returned NaN-boxed garbage that
        // the caller's re-tag turned into a handle-band id (e.g.
        // `0x7FFD_0000_0000_0001`), crashing `for…of`. The fix resolves the
        // `errors` property generically for non-errors.
        let arr = crate::array::js_array_alloc(2);
        crate::array::js_array_push_f64(arr, 11.0);
        crate::array::js_array_push_f64(arr, 22.0);
        let arr_boxed = crate::value::js_nanbox_pointer(arr as i64);

        // Plain object with an own `errors` property pointing at `arr`.
        let obj = crate::object::js_object_alloc(0, 2);
        let key = s(b"errors");
        crate::object::js_object_set_field_by_name(obj, key, arr_boxed);

        // The accessor receives the *cleaned* (untagged) pointer, as codegen
        // strips the tag before the call.
        let got = js_error_get_errors(obj as *mut ErrorHeader);
        assert_eq!(
            got as usize, arr as usize,
            "regular object's .errors must resolve to its real array property, \
             not the +48 ErrorHeader slot"
        );

        // An object with no `errors` property yields null (→ caller re-tag is a
        // null receiver that `for…of` rejects as not iterable, matching the
        // generic property read).
        let empty = crate::object::js_object_alloc(0, 1);
        assert!(js_error_get_errors(empty as *mut ErrorHeader).is_null());

        // A small-handle-band "pointer" must never be dereferenced.
        assert!(js_error_get_errors(1usize as *mut ErrorHeader).is_null());
    }

    #[test]
    fn get_errors_on_native_aggregate_error_uses_fixed_slot() {
        let arr = crate::array::js_array_alloc(1);
        crate::array::js_array_push_f64(arr, 7.0);
        let agg = js_aggregateerror_new(arr, s(b"agg"));
        let got = js_error_get_errors(agg);
        assert_eq!(
            got as usize, arr as usize,
            "native AggregateError must read its fixed errors slot"
        );
    }

    #[test]
    fn eval_and_uri_errors_have_distinct_kinds_and_names() {
        let eval = js_evalerror_new(s(b"eval"));
        assert_eq!(js_error_get_kind(eval), ERROR_KIND_EVAL_ERROR);
        assert_eq!(
            unsafe { read_string_header_owned(js_error_get_name(eval)) },
            "EvalError"
        );

        let uri = js_urierror_new(s(b"uri"));
        assert_eq!(js_error_get_kind(uri), ERROR_KIND_URI_ERROR);
        assert_eq!(
            unsafe { read_string_header_owned(js_error_get_name(uri)) },
            "URIError"
        );
    }
}
