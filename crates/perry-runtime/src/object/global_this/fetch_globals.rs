use super::super::*;
use super::*;

thread_local! {
    /// This thread's `globalThis`. The realm global is allocated in a *per-thread*
    /// arena, but `GLOBAL_THIS_PTR` (the GC-root slot) is a process-global static.
    /// A pointer published there by another, now-finished thread (the unit-test
    /// harness runs each test on its own thread; `perry/thread` workers have their
    /// own arenas) points into freed/reused memory — reading `globalThis.Array`
    /// through it returns `undefined`, or worse derefs an invalid header. Caching
    /// the global per thread means we only ever hand back a global this thread
    /// created, and never dereference another thread's pointer to "validate" it.
    static THREAD_GLOBAL_THIS: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

thread_local! {
    /// Module top-level `this` (Node-CJS `module.exports` stand-in) — a
    /// lazily-allocated plain object distinct from `globalThis`. See
    /// `Expr::ModuleTopThis`.
    static THREAD_MODULE_TOP_THIS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// `this` in module top-level code. Node runs files as CommonJS where
/// top-level `this` is `module.exports`: a fresh ordinary object, NOT the
/// global. One object per thread (Perry links the whole program into one
/// binary; the test corpus is single-module).
#[no_mangle]
pub extern "C" fn js_module_top_this() -> f64 {
    let cached = THREAD_MODULE_TOP_THIS.with(|c| c.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let obj = super::super::alloc::js_object_alloc(0, 0);
    let val = crate::value::js_nanbox_pointer(obj as i64);
    THREAD_MODULE_TOP_THIS.with(|c| c.set(val.to_bits()));
    // Keep it alive across GCs — the cell is a raw bits cache, not a scanned
    // root, so register the slot address as a global root once.
    crate::gc::runtime_write_barrier_root_heap_word(obj as u64);
    let slot = THREAD_MODULE_TOP_THIS.with(|c| c.as_ptr() as usize);
    crate::gc::js_gc_register_global_root(slot as i64);
    val
}

/// Keepalive anchor: `js_module_top_this` is referenced only from
/// codegen-generated `.o` files, so the auto-optimize whole-program LLVM
/// rebuild would dead-strip it without this `#[used]` pin (see
/// project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_MODULE_TOP_THIS: extern "C" fn() -> f64 = js_module_top_this;

/// Issue #611: lazily allocate `globalThis` for computed global access.
#[no_mangle]
pub extern "C" fn js_get_global_this() -> f64 {
    let mine = THREAD_GLOBAL_THIS.with(|c| c.get());
    if mine != 0 {
        return crate::value::js_nanbox_pointer(mine);
    }
    // Register this thread's GC root scanners before the global exists, so the
    // global (and the `Array`/`Object` intrinsics it holds) is born under a live
    // root and survives later collections on this thread. Worker threads and the
    // unit-test harness never run `js_gc_init()`, so without this a collection
    // would reclaim the global mid-use, leaving a dangling intrinsic. No-op in
    // production (already initialized) and inside the GC tests' controlled scopes.
    crate::gc::ensure_gc_initialized();
    // First access on this thread — allocate our own global.
    let new_ptr = js_object_alloc(0, 0) as i64;
    THREAD_GLOBAL_THIS.with(|c| c.set(new_ptr));
    // Publish to the process-global GC-root slot so this thread's collector marks
    // it (the unit-test harness runs tests sequentially, so the slot always holds
    // the running thread's global). `GLOBAL_THIS_READY` is toggled around
    // population so any concurrent reader spins until the field bag is complete.
    GLOBAL_THIS_READY.store(false, Ordering::Release);
    // GC_STORE_AUDIT(ROOT): GLOBAL_THIS_PTR is a mutable root visited by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_raw_i64(&GLOBAL_THIS_PTR, new_ptr, Ordering::Release);
    // Populate constructor values for `globalThis.Array` / `context.Array` style
    // reads without changing bare `new Array`.
    populate_global_this_builtins(new_ptr as *mut ObjectHeader);
    GLOBAL_THIS_READY.store(true, Ordering::Release);
    crate::value::js_nanbox_pointer(new_ptr)
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

fn global_this_headers_handle_from_value(value: f64) -> f64 {
    if matches!(
        value.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return 0.0;
    }
    let headers = super::super::global_fetch::call_global_headers_new();
    if headers.to_bits() == crate::value::TAG_UNDEFINED {
        return 0.0;
    }
    super::super::global_fetch::call_global_headers_init_from_value(headers, value);
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
    super::super::global_fetch::call_global_blob_new(parts, type_value)
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
    super::super::global_fetch::call_global_file_new(parts, name, type_value, last_modified)
}

pub(crate) extern "C" fn global_this_headers_thunk(
    _closure: *const crate::closure::ClosureHeader,
    init: f64,
) -> f64 {
    let headers = super::super::global_fetch::call_global_headers_new();
    if headers.to_bits() == crate::value::TAG_UNDEFINED {
        return headers;
    }
    if init.to_bits() != crate::value::TAG_UNDEFINED {
        super::super::global_fetch::call_global_headers_init_from_value(headers, init);
    }
    headers
}

pub(crate) extern "C" fn global_this_response_thunk(
    _closure: *const crate::closure::ClosureHeader,
    body: f64,
    init: f64,
) -> f64 {
    // Route the body through the registered body-init helper (stdlib
    // `js_response_body_init_ptr`) so a binary body — Buffer / Uint8Array /
    // ArrayBuffer — copies its raw bytes instead of being stringified to a
    // zero-filled payload (#5435). String bodies fall back to the ordinary
    // coercion. Mirrors the Request thunk's body handling above.
    let body_ptr = if matches!(
        body.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        std::ptr::null()
    } else {
        super::super::global_fetch::call_global_body_init_ptr(body)
    };
    let status = global_this_fetch_option(init, b"status");
    let status = if status.to_bits() == crate::value::TAG_UNDEFINED {
        0.0
    } else {
        status
    };
    let status_text_ptr = global_this_fetch_option_string_ptr(init, b"statusText");
    let headers_handle = global_this_init_headers_handle(init);
    super::super::global_fetch::call_global_response_new(
        body_ptr,
        status,
        status_text_ptr,
        headers_handle,
    )
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
            super::super::global_fetch::call_global_body_init_ptr(body_val)
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
    super::super::global_fetch::call_global_request_new(
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
    if !crate::value::addr_class::is_plausible_heap_addr(raw) {
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

/// Stash the NaN-boxed Temporal cell (`cell_box`, what a `Temporal.<Type>`
/// constructor thunk returns) on a `class X extends Temporal.<Type>` subclass
/// instance's `this` under `__perry_temporal_cell__`. Stored as a real
/// pointer-valued field so method/getter/instanceof dispatch can recover the
/// cell (`temporal_subclass_cell`) and GC keeps it alive. (#5587)
#[cfg(feature = "temporal")]
unsafe fn attach_temporal_cell_to_this(this_box: f64, cell_box: f64) {
    if let Some(obj) = subclass_this_object_ptr(this_box) {
        let key = crate::string::js_string_from_bytes(
            crate::object::TEMPORAL_SUBCLASS_CELL_FIELD.as_ptr(),
            crate::object::TEMPORAL_SUBCLASS_CELL_FIELD.len() as u32,
        );
        crate::object::js_object_set_field_by_name(obj, key, cell_box);
    }
}

/// `class X extends Temporal.<Type>` super-call handling, shared by the two
/// `super()` lowerings: the flat-arg runtime-value dispatcher
/// (`js_fetch_or_value_super`, the non-spread `super(a, b)` path) and the
/// args-array `js_super_construct_apply` (the `super(...spread)` path). When
/// `parent_val` is a Temporal constructor, run it (Temporal ctors return a
/// fresh cell and never mutate the implicit `this`) and stash the returned cell
/// on `this_box` so method / getter / instanceof dispatch can recover the
/// Temporal brand. Returns `true` when handled. (#5587)
#[cfg(feature = "temporal")]
pub(crate) unsafe fn temporal_subclass_super(
    parent_val: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> bool {
    if super::temporal_ctor_kind(parent_val).is_none() {
        return false;
    }
    // Several Temporal constructors (`PlainDate`/`PlainTime`/`Instant`/…) are
    // `[[Construct]]`-only and throw "requires 'new'" when `new.target` is
    // undefined. Invoking the parent here IS a construct, so set `new.target`
    // to the parent ctor for the duration of the call (the cell it returns is
    // re-homed onto the subclass `this`; the exact new.target identity is not
    // observable to these native ctors beyond being defined). Restore after.
    let prev_this = crate::object::js_implicit_this_set(this_box);
    let prev_nt = crate::object::js_new_target_set(parent_val);
    let cell = crate::closure::js_native_call_value(parent_val, args_ptr, args_len);
    crate::object::js_new_target_set(prev_nt);
    crate::object::js_implicit_this_set(prev_this);
    if crate::temporal::is_temporal_value(cell) {
        attach_temporal_cell_to_this(this_box, cell);
    }
    true
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

/// #5657: native builtin constructors that REJECT being called as a plain
/// function (`X is not a function` / `Constructor X requires 'new'`). A
/// `class D extends <one of these> {}` must NOT run the parent as a function in
/// `super()` — Perry can't give the instance the builtin's internal slots, so
/// `super()` is a best-effort no-op (the instance is already allocated with the
/// correct dynamic-parent prototype chain, so `instanceof` holds). This is the
/// VALUE-based companion to codegen's name-based guard
/// (`crate::expr::is_other_builtin_constructor_name` in
/// `perry-codegen/src/expr/this_super_call.rs`); the two lists must stay in
/// lockstep. Resolving by value (not textual name) also catches aliased parents
/// — `const AB = ArrayBuffer; class X extends AB {}` — which the name guard
/// can't see. `Request`/`Response` (native fetch-handle attach) and the `Error`
/// family (callable error thunk) are deliberately EXCLUDED: they keep their
/// dispatch.
fn is_uncallable_builtin_super_parent(name: &str) -> bool {
    matches!(
        name,
        "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Array"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "Boolean"
            | "Number"
            | "String"
            | "Date"
            | "RegExp"
            | "Promise"
            | "Function"
            | "BigInt"
            | "Symbol"
            | "Object"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
    )
}

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
    // `class X extends Temporal.<Type>` (non-spread `super(a, b)`): a Temporal
    // constructor returns a fresh NaN-boxed cell and does NOT mutate the
    // implicit `this`, so the ordinary dispatch below would drop that cell and
    // leave the subclass instance an empty object with no Temporal brand. Stash
    // the cell on `this` instead. The native ctor never calls the subclass
    // constructor, so `called`-counter invariants hold. (#5587)
    //
    // `parent_val` can arrive stale/undefined for an aliased heritage
    // (`const D = Temporal.Duration; class X extends D`) when codegen re-evaluates
    // the extends expression in constructor scope — exactly the case the
    // Request/Response branch below recovers via the decl-time stash. Mirror that:
    // when the immediate value isn't a Temporal ctor, fall back to the parent
    // value recorded against this instance's class id at declaration time.
    #[cfg(feature = "temporal")]
    {
        let temporal_parent = if super::temporal_ctor_kind(parent_val).is_some() {
            parent_val
        } else if let Some(obj) = subclass_this_object_ptr(this_box) {
            let cid = crate::object::js_object_get_class_id(obj);
            crate::object::class_registry::js_get_dynamic_parent_value(cid)
        } else {
            parent_val
        };
        if temporal_subclass_super(temporal_parent, this_box, args_ptr, args_len) {
            return undef;
        }
    }
    // `class X extends Intl.<Ctor>` (`super(locales, options)`): an Intl service
    // constructor returns a fresh branded object and throws "requires 'new'"
    // when `new.target` is undefined, so — like Temporal — recognize the parent
    // and construct it with `new.target` set, re-homing the instance's brand +
    // methods onto `this`. `parent_val` can arrive stale for an aliased heritage
    // (`const L = Intl.Locale; class X extends L`); recover the decl-time parent.
    {
        let intl_parent = if crate::intl::is_intl_constructor_value(parent_val) {
            parent_val
        } else if let Some(obj) = subclass_this_object_ptr(this_box) {
            let cid = crate::object::js_object_get_class_id(obj);
            crate::object::class_registry::js_get_dynamic_parent_value(cid)
        } else {
            parent_val
        };
        if crate::intl::intl_subclass_super(intl_parent, this_box, args_ptr, args_len) {
            return undef;
        }
    }
    // Resolve the parent constructor kind from the value first. When the
    // `extends` expression is an alias of `global.Request`/`global.Response`
    // (`@hono/node-server`'s `class Request extends GlobalRequest`), the alias
    // var can lower to a constructor-scope local that reads `undefined` at
    // super-time, so `identify_global_builtin_constructor(parent_val)` returns
    // `None`. Fall back to the fetch-parent kind registered against the
    // instance's class at module init (via `js_register_class_parent_dynamic`,
    // where the alias resolved correctly) so the native handle still attaches.
    let kind = super::super::class_registry::identify_global_builtin_constructor(parent_val)
        .or_else(|| {
            let obj = subclass_this_object_ptr(this_box)?;
            match super::super::class_registry::fetch_parent_kind_in_chain(
                crate::object::js_object_get_class_id(obj),
            ) {
                Some(1) => Some("Request"),
                Some(2) => Some("Response"),
                _ => None,
            }
        });
    // #5657: a native builtin base that can't be called as a function (incl.
    // ALIASED parents — `const AB = ArrayBuffer; class X extends AB {}` — which
    // the codegen name guard can't see). No-op rather than throwing
    // "X is not a function" / "Constructor X requires 'new'".
    if let Some(name) = kind {
        if is_uncallable_builtin_super_parent(name) {
            return undef;
        }
    }
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
            // `class PQ extends t {}` nested inside another function (webpack/
            // ncc inner modules — next/dist/compiled/p-queue extending
            // eventemitter3): HIR lowers the heritage Ident at class-DECL
            // scope, but codegen re-emits that expression inside the
            // constructor, where the captured slot index is unrelated, so
            // `parent_val` arrives stale (undefined). The decl-site
            // `js_register_class_parent_dynamic` call DID see the live value
            // and recorded it in CLASS_PARENT_CLOSURES — prefer that
            // registration whenever `parent_val` isn't actually callable, so
            // the parent function body still runs with `this` bound (sets
            // `this._events` etc.). A valid closure / class-object parent
            // value keeps the existing direct-dispatch path untouched.
            let mut callee = parent_val;
            let bits = parent_val.to_bits();
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
            const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
            const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
            // A dynamic parent that resolved to a ClassRef (INT32-tagged) is a
            // real registered Perry class — `class X extends _mod.default`
            // where the default export is a user class (Next.js
            // `NextNodeServer extends base-server`'s default `Server`). A
            // ClassRef is NaN-tagged, so `js_native_call_value` below would
            // early-return `undefined` (it treats NaN as not callable) and the
            // base constructor would never run — parent `this.<field> = …`
            // writes (e.g. `this.nextConfig = opts`) would be lost. Invoke the
            // class constructor directly on `this` instead.
            if bits & TAG_MASK == INT32_TAG {
                let parent_cid = bits as u32;
                if let Some(obj) = subclass_this_object_ptr(this_box) {
                    super::super::class_constructors::run_class_constructor_on_this_flat(
                        parent_cid, obj as i64, args_ptr, args_len,
                    );
                }
                // A ClassRef is NaN-tagged and is NEVER callable via
                // `js_native_call_value` (it early-returns `undefined`). Return
                // here unconditionally — whether or not a constructor was found
                // and run — instead of falling through to the closure-dispatch
                // path below, which would (a) silently produce `undefined` and
                // (b) skip the `parent_closure_in_chain` recovery that only
                // applies to closure/object parents, not a ClassRef.
                return undef;
            }
            let usable = if bits & TAG_MASK == POINTER_TAG {
                let p = (bits & PTR_MASK) as usize;
                // A real callability test: a closure, or a per-evaluation class
                // OBJECT (constructor). The prior `class_id != 0` accepted any
                // pointer-tagged object with a class id — including non-callable
                // instances — so a stale captured slot holding one of those
                // skipped the `parent_closure_in_chain` recovery below and
                // dispatched `js_native_call_value` on a non-function.
                crate::closure::is_closure_ptr(p)
                    || super::super::class_registry::is_class_object_ptr(p as *const u8)
            } else {
                // INT32-tagged ClassRefs route through the static super paths
                // before reaching here; anything else (undefined / a stale
                // numeric slot) is not a constructor.
                bits & TAG_MASK == 0x7FFE_0000_0000_0000
            };
            if !usable {
                if let Some(obj) = subclass_this_object_ptr(this_box) {
                    let cid = crate::object::js_object_get_class_id(obj);
                    if let Some(addr) = super::super::class_registry::parent_closure_in_chain(cid) {
                        callee = f64::from_bits(POINTER_TAG | addr as u64);
                    }
                }
            }
            let prev = crate::object::js_implicit_this_set(this_box);
            let r = crate::closure::js_native_call_value(callee, args_ptr, args_len);
            crate::object::js_implicit_this_set(prev);
            r
        }
    }
}

#[used]
static KEEP_JS_FETCH_OR_VALUE_SUPER: unsafe extern "C" fn(f64, f64, *const f64, usize) -> f64 =
    js_fetch_or_value_super;

pub(crate) extern "C" fn global_this_response_error_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    super::super::global_fetch::call_global_response_static_error()
}

pub(crate) extern "C" fn global_this_response_json_thunk(
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
    super::super::global_fetch::call_global_response_static_json(
        value,
        init_status,
        init_status_text_ptr,
        headers_handle,
    )
}

pub(crate) extern "C" fn global_this_response_redirect_thunk(
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
    super::super::global_fetch::call_global_response_static_redirect(url_ptr, status)
}

pub(crate) extern "C" fn global_this_eval_thunk(
    _closure: *const crate::closure::ClosureHeader,
    source: f64,
) -> f64 {
    // PerformEval step: "If Type(x) is not String, return x." A non-string
    // argument (number, boolean, null, undefined, or a String/Number/Boolean
    // *wrapper object*) is returned unchanged — eval does not evaluate it. This
    // must run before any ToString coercion. (test262
    // language/eval-code/indirect/non-string-{object,primitive})
    if !crate::value::JSValue::from_bits(source.to_bits()).is_string() {
        return source;
    }
    let source = crate::builtins::js_string_coerce(source);
    let Some(body) = (unsafe { super::super::has_own_helpers::str_from_string_header(source) })
    else {
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
