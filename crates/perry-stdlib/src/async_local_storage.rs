//! AsyncLocalStorage implementation
//!
//! Native implementation of Node.js AsyncLocalStorage from `async_hooks`.
//! Provides run(), getStore(), enterWith(), exit(), and disable().

use perry_runtime::array::{js_array_length, ArrayHeader};
use perry_runtime::closure::JsThis;
use perry_runtime::closure::{is_closure_ptr, js_closure_call_array, ClosureHeader};
use perry_runtime::native_payload::{self, NativePayloadFamily, PayloadMiss, PayloadPrototype};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Keep the active context in the single perry-runtime provider.  These must be
// extern calls rather than Rust-path calls: in app-only dylib deployments the
// stdlib is a separate image, and linking its own perry-runtime dependency
// would create a second ACTIVE_CONTEXT that promise/timer schedulers cannot
// see (#8037).
extern "C" {
    fn js_async_context_als_run_enter(handle: i64, store: f64);
    fn js_async_context_als_exit_enter(handle: i64);
    fn js_async_context_als_scope_leave();
    fn js_async_context_als_get_store(handle: i64) -> f64;
    fn js_async_context_als_enter_with(handle: i64, store: f64);
    fn js_async_context_als_clear(handle: i64);
}

/// #3092 — `AsyncLocalStorage#run`/`#exit` must reject a non-callable callback
/// with a `TypeError`, matching Node (which throws through its function-apply
/// path). Returns the validated `ClosureHeader` pointer for a callable value,
/// or diverges via `js_throw`. The POINTER_TAG check guards `is_closure_ptr`
/// from the short-string/double bit patterns that can otherwise look
/// pointer-ish enough to segfault.
unsafe fn validate_callback(callback: f64) -> *const ClosureHeader {
    let bits = callback.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as usize;
        if is_closure_ptr(ptr) {
            return ptr as *const ClosureHeader;
        }
    }
    let message = "callback is not a function";
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let msg = perry_runtime::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let msg = scope.root_string_ptr(msg);
    let err = msg.with_mut_ptr(|msg| perry_runtime::error::js_typeerror_new(msg));
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

/// #3093 — invoke a validated callback with the forwarded rest arguments.
/// `args_array` is a raw `*const ArrayHeader` (i64) holding the trailing
/// `...args` packed by the codegen `NA_VARARGS` lowering; `0` / empty array
/// means no forwarded args. Mirrors the data/len extraction used by
/// `AsyncResource#runInAsyncScope` in perry-runtime.
unsafe fn call_with_forwarded_args(cb: *const ClosureHeader, args_array: i64) -> f64 {
    let closure_env = cb as i64;
    if args_array == 0 {
        return js_closure_call_array(
            closure_env,
            perry_runtime::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        );
    }
    let arr = args_array as *const ArrayHeader;
    let len = js_array_length(arr) as i64;
    let data = if arr.is_null() || len == 0 {
        std::ptr::null()
    } else {
        perry_runtime::array::array_elements_ptr(arr as *const ArrayHeader) as *const f64
    };
    js_closure_call_array(
        closure_env,
        perry_runtime::closure::plain_call_receiver(),
        data,
        len,
    )
}

/// AsyncLocalStorage handle. Store stacks live in perry-runtime's active
/// async context so schedulers can snapshot and restore them across async
/// boundaries.
static ASYNC_LOCAL_STORAGE_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: perry_runtime::native_class_ids::ASYNC_LOCAL_STORAGE_LEGACY,
    name: "AsyncLocalStorage",
    constructor_export: Some(("async_hooks", "AsyncLocalStorage")),
    constructor_length: 0,
    links_owner: false,
    install_prototype: install_async_local_storage_prototype,
};

macro_rules! als_method {
    ($proto:expr, $name:literal, $body:ident, $n:tt) => {
        $proto.method(
            $name,
            perry_runtime::fn_info!(
                $body, $n;
                with_declared($n),
                with_flags(perry_runtime::closure::FN_BUILTIN)
            ),
            $n,
        )
    };
}

fn install_async_local_storage_prototype(proto: &mut PayloadPrototype) {
    proto.getter(
        "name",
        perry_runtime::fn_info!(als_name_thunk, 0; with_declared(0), with_flags(perry_runtime::closure::FN_BUILTIN)),
    );
    proto.method(
        "run",
        perry_runtime::fn_info!(
            als_run_thunk, 3;
            with_rest(2),
            with_declared(2),
            with_flags(perry_runtime::closure::FN_BUILTIN)
        ),
        2,
    );
    proto.method(
        "withScope",
        perry_runtime::fn_info!(
            als_run_thunk, 3;
            with_rest(2),
            with_declared(2),
            with_flags(perry_runtime::closure::FN_BUILTIN)
        ),
        2,
    );
    als_method!(proto, "getStore", als_get_store_thunk, 0);
    als_method!(proto, "enterWith", als_enter_with_thunk, 1);
    proto.method(
        "exit",
        perry_runtime::fn_info!(
            als_exit_thunk, 2;
            with_rest(1),
            with_declared(1),
            with_flags(perry_runtime::closure::FN_BUILTIN)
        ),
        1,
    );
    als_method!(proto, "disable", als_disable_thunk, 0);
}

fn canonical_prototype() -> *mut perry_runtime::object::ObjectHeader {
    let value = perry_runtime::object::async_local_storage_prototype_value();
    let proto = perry_runtime::value::js_nanbox_get_pointer(value)
        as *mut perry_runtime::object::ObjectHeader;
    native_payload::adopt_prototype(&ASYNC_LOCAL_STORAGE_FAMILY, proto)
}

fn throw_invalid_receiver() -> ! {
    let message = b"Value of \"this\" must be of type AsyncLocalStorage";
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let msg = perry_runtime::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let msg = scope.root_string_ptr(msg);
    let err = msg.with_mut_ptr(|msg| perry_runtime::error::js_typeerror_new(msg));
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn resolve_async_local_storage_token(receiver: i64) -> Option<i64> {
    let value = perry_runtime::value::js_nanbox_pointer(receiver);
    unsafe {
        native_payload::payload_mut_attached::<perry_runtime::async_context::AsyncLocalStoragePayload>(
            value,
            &ASYNC_LOCAL_STORAGE_FAMILY,
        )
        .ok()
        .map(|payload| payload.token())
    }
}

fn ensure_async_local_storage_token(receiver: i64) -> Option<i64> {
    let value = perry_runtime::value::js_nanbox_pointer(receiver);
    match unsafe {
        native_payload::payload_mut_attached::<perry_runtime::async_context::AsyncLocalStoragePayload>(
            value,
            &ASYNC_LOCAL_STORAGE_FAMILY,
        )
    } {
        Ok(payload) => Some(payload.token()),
        Err(PayloadMiss::Closed) => {
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let receiver = scope.root_nanbox_f64(value);
            native_payload::attach_to_object(
                receiver.get_nanbox_f64(),
                &ASYNC_LOCAL_STORAGE_FAMILY,
                perry_runtime::async_context::AsyncLocalStoragePayload::default(),
                0,
            );
            resolve_async_local_storage_token(perry_runtime::value::js_nanbox_get_pointer(
                receiver.get_nanbox_f64(),
            ))
        }
        Err(PayloadMiss::Foreign) => None,
    }
}

/// Stamp an ordinary source-compiled subclass instance with a native ALS
/// backing. Its inherited methods receive the ordinary object as `this` and
/// resolve through the hidden handle above.
#[no_mangle]
pub extern "C" fn js_async_local_storage_subclass_init(this_value: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this_value);
    let _ = canonical_prototype();
    native_payload::attach_to_object(
        receiver.get_nanbox_f64(),
        &ASYNC_LOCAL_STORAGE_FAMILY,
        perry_runtime::async_context::AsyncLocalStoragePayload::default(),
        0,
    );
    receiver.get_nanbox_f64()
}

/// Create a new AsyncLocalStorage instance
#[no_mangle]
pub extern "C" fn js_async_local_storage_new() -> f64 {
    native_payload::alloc_with_prototype(
        &ASYNC_LOCAL_STORAGE_FAMILY,
        perry_runtime::async_context::AsyncLocalStoragePayload::default(),
        0,
        &[],
        canonical_prototype(),
    )
}

/// AsyncLocalStorage.run(store, callback, ...args)
/// Push store onto stack, call callback with the forwarded rest args, pop
/// store, return result. `args_array` carries the `...args` packed by the
/// codegen `NA_VARARGS` lowering (#3093).
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_run(
    receiver: i64,
    store: f64,
    callback: f64,
    args_array: i64,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(receiver));
    let store = scope.root_nanbox_f64(store);
    let callback = scope.root_nanbox_f64(callback);
    let args_array = scope.root_raw_const_ptr(args_array as *const ArrayHeader);
    // Validate before mutating the async context so an invalid callback throws
    // without leaving a pushed store behind (#3092).
    let _ = validate_callback(callback.get_nanbox_f64());
    let receiver = perry_runtime::value::js_nanbox_get_pointer(receiver.get_nanbox_f64());
    let token =
        ensure_async_local_storage_token(receiver).unwrap_or_else(|| throw_invalid_receiver());

    // A context guard mirrors the pop below: if the callback throws,
    // `js_throw` applies the guard while unwinding so the catch site still
    // observes the pre-`run` store (#788, Node restores via try/finally).
    js_async_context_als_run_enter(token, store.get_nanbox_f64());
    let cb = validate_callback(callback.get_nanbox_f64());
    let result = call_with_forwarded_args(cb, args_array.get_raw_const_ptr::<ArrayHeader>() as i64);
    js_async_context_als_scope_leave();

    result
}

/// AsyncLocalStorage.getStore()
/// Returns the current store (top of stack) or undefined
#[no_mangle]
pub extern "C" fn js_async_local_storage_get_store(receiver: i64) -> f64 {
    let value = perry_runtime::value::js_nanbox_pointer(receiver);
    match unsafe {
        native_payload::payload_mut_attached::<perry_runtime::async_context::AsyncLocalStoragePayload>(
            value,
            &ASYNC_LOCAL_STORAGE_FAMILY,
        )
    } {
        Ok(payload) => unsafe { js_async_context_als_get_store(payload.token()) },
        Err(PayloadMiss::Closed) => {
            f64::from_bits(perry_runtime::value::JSValue::undefined().bits())
        }
        Err(PayloadMiss::Foreign) => throw_invalid_receiver(),
    }
}

/// AsyncLocalStorage.enterWith(store)
/// Push store onto stack (caller is responsible for cleanup)
#[no_mangle]
pub extern "C" fn js_async_local_storage_enter_with(receiver: i64, store: f64) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(receiver));
    let store = scope.root_nanbox_f64(store);
    let receiver = perry_runtime::value::js_nanbox_get_pointer(receiver.get_nanbox_f64());
    if let Some(token) = ensure_async_local_storage_token(receiver) {
        unsafe { js_async_context_als_enter_with(token, store.get_nanbox_f64()) };
    }
}

/// AsyncLocalStorage.exit(callback, ...args)
/// Save current stack, clear it, call callback with the forwarded rest args,
/// restore stack. `args_array` carries the `...args` packed by the codegen
/// `NA_VARARGS` lowering (#3093).
#[no_mangle]
pub unsafe extern "C" fn js_async_local_storage_exit(
    receiver: i64,
    callback: f64,
    args_array: i64,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(receiver));
    let callback = scope.root_nanbox_f64(callback);
    let args_array = scope.root_raw_const_ptr(args_array as *const ArrayHeader);
    // Validate before clearing the context so an invalid callback throws
    // without disturbing the saved store (#3092).
    let _ = validate_callback(callback.get_nanbox_f64());
    let receiver = perry_runtime::value::js_nanbox_get_pointer(receiver.get_nanbox_f64());
    let token =
        ensure_async_local_storage_token(receiver).unwrap_or_else(|| throw_invalid_receiver());
    js_async_context_als_exit_enter(token);

    let cb = validate_callback(callback.get_nanbox_f64());
    let result = call_with_forwarded_args(cb, args_array.get_raw_const_ptr::<ArrayHeader>() as i64);

    js_async_context_als_scope_leave();

    result
}

/// AsyncLocalStorage.disable()
/// Clear the store stack
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable(receiver: i64) {
    let value = perry_runtime::value::js_nanbox_pointer(receiver);
    if let Some(token) = resolve_async_local_storage_token(receiver) {
        unsafe { js_async_context_als_clear(token) };
        native_payload::close_attached::<perry_runtime::async_context::AsyncLocalStoragePayload>(
            value,
            &ASYNC_LOCAL_STORAGE_FAMILY,
        );
    }
}

extern "C" fn als_run_thunk(
    _closure: *const ClosureHeader,
    this: JsThis,
    store: f64,
    callback: f64,
    rest: f64,
) -> f64 {
    unsafe {
        js_async_local_storage_run(
            perry_runtime::value::js_nanbox_get_pointer(this.as_f64()),
            store,
            callback,
            perry_runtime::value::js_nanbox_get_pointer(rest),
        )
    }
}

extern "C" fn als_get_store_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    js_async_local_storage_get_store(perry_runtime::value::js_nanbox_get_pointer(this.as_f64()))
}

extern "C" fn als_name_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    if matches!(
        unsafe {
            native_payload::payload_mut_attached::<
                perry_runtime::async_context::AsyncLocalStoragePayload,
            >(this.as_f64(), &ASYNC_LOCAL_STORAGE_FAMILY)
        },
        Err(PayloadMiss::Foreign)
    ) {
        throw_invalid_receiver();
    }
    perry_runtime::value::js_nanbox_string(perry_runtime::string::js_string_from_bytes(
        b"".as_ptr(),
        0,
    ) as i64)
}

extern "C" fn als_enter_with_thunk(
    _closure: *const ClosureHeader,
    this: JsThis,
    store: f64,
) -> f64 {
    js_async_local_storage_enter_with(
        perry_runtime::value::js_nanbox_get_pointer(this.as_f64()),
        store,
    );
    f64::from_bits(0x7FFC_0000_0000_0001)
}

extern "C" fn als_exit_thunk(
    _closure: *const ClosureHeader,
    this: JsThis,
    callback: f64,
    rest: f64,
) -> f64 {
    unsafe {
        js_async_local_storage_exit(
            perry_runtime::value::js_nanbox_get_pointer(this.as_f64()),
            callback,
            perry_runtime::value::js_nanbox_get_pointer(rest),
        )
    }
}

extern "C" fn als_disable_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    js_async_local_storage_disable(perry_runtime::value::js_nanbox_get_pointer(this.as_f64()));
    f64::from_bits(0x7FFC_0000_0000_0001)
}
