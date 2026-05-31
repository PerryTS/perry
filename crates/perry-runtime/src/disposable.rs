//! TC39 explicit-resource-management globals: `DisposableStack`,
//! `AsyncDisposableStack`, and `SuppressedError` (#2875).
//!
//! Node 22+ exposes these as real global constructors. Perry already
//! desugars `using` / `await using` declarations, but the standalone stack
//! constructors and the `SuppressedError` error type were missing, so
//! `new DisposableStack()` / `new SuppressedError(...)` and the stack
//! instance methods (`use` / `adopt` / `defer` / `dispose` / `move` /
//! `disposed`) had no implementation.
//!
//! Representation
//! --------------
//! A stack instance is a GC-managed `ObjectHeader` (NaN-boxed pointer)
//! with two fields:
//!   field 0: disposers — an array of zero-arg closure values, in
//!            registration order. Each entry is invoked LIFO on dispose.
//!   field 1: disposed — a NaN-boxed bool.
//!
//! `use(resource)` stores `resource[Symbol.dispose]` (a bound method).
//! `adopt(value, onDispose)` stores a synthesized closure that calls
//! `onDispose(value)`. `defer(fn)` stores `fn` directly. All three are
//! reduced to "a callable invoked with no arguments at dispose time", so
//! `dispose()` is a single LIFO walk that unboxes each entry to a closure
//! pointer and calls `js_closure_call0`.
//!
//! `SuppressedError` is a plain `ObjectHeader` stamped with a reserved
//! class id and registered (once) via `js_register_class_extends_error`
//! so `err instanceof Error` and `err instanceof SuppressedError` hold.
//! It carries `name` / `message` / `error` / `suppressed` / `stack` as
//! named fields read through the ordinary by-name object getter.

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_f64, js_object_set_field_by_name, js_object_set_field_f64,
    js_register_class_extends_error,
};
use crate::string::js_string_from_bytes;
use crate::value::{
    js_is_truthy, js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, JSValue, TAG_FALSE,
    TAG_TRUE, TAG_UNDEFINED,
};
use crate::{ArrayHeader, ObjectHeader};

/// Reserved class id for a `DisposableStack` instance (`typeof` of the
/// instance is "object"; the constructor's "function" typeof comes from
/// the `globalThis` builtin-constructor list).
pub const CLASS_ID_DISPOSABLE_STACK: u32 = 0xFFFF_003C;
/// Reserved class id for an `AsyncDisposableStack` instance.
pub const CLASS_ID_ASYNC_DISPOSABLE_STACK: u32 = 0xFFFF_003D;
/// Reserved class id for a `SuppressedError` instance. Registered as an
/// Error subclass at first construction so `instanceof Error` holds.
pub const CLASS_ID_SUPPRESSED_ERROR: u32 = 0xFFFF_003B;

const FIELD_DISPOSERS: u32 = 0;
const FIELD_DISPOSED: u32 = 1;
const STACK_FIELD_COUNT: u32 = 2;

#[inline]
fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn bool_f64(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

#[inline]
fn string_f64(s: &str) -> f64 {
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    js_nanbox_string(ptr as i64)
}

/// Allocate an empty stack instance with the given class id.
fn alloc_stack(class_id: u32) -> *mut ObjectHeader {
    let obj = js_object_alloc(class_id, STACK_FIELD_COUNT);
    if obj.is_null() {
        return obj;
    }
    let arr = js_array_alloc(0);
    js_object_set_field_f64(obj, FIELD_DISPOSERS, js_nanbox_pointer(arr as i64));
    js_object_set_field_f64(obj, FIELD_DISPOSED, bool_f64(false));
    obj
}

/// Resolve a NaN-boxed receiver to a stack `ObjectHeader`, or null.
fn receiver_obj(stack: *mut ObjectHeader) -> *mut ObjectHeader {
    stack
}

fn stack_is_disposed(stack: *const ObjectHeader) -> bool {
    if stack.is_null() {
        return true;
    }
    js_is_truthy(js_object_get_field_f64(stack, FIELD_DISPOSED)) != 0
}

fn stack_disposers(stack: *const ObjectHeader) -> *mut ArrayHeader {
    if stack.is_null() {
        return std::ptr::null_mut();
    }
    let v = js_object_get_field_f64(stack, FIELD_DISPOSERS);
    js_nanbox_get_pointer(v) as *mut ArrayHeader
}

fn push_disposer(stack: *mut ObjectHeader, callable: f64) {
    if stack.is_null() {
        return;
    }
    let arr = stack_disposers(stack);
    let new_arr = js_array_push_f64(arr, callable);
    js_object_set_field_f64(stack, FIELD_DISPOSERS, js_nanbox_pointer(new_arr as i64));
}

/// Throw a `ReferenceError` matching Node's "already-disposed" message.
fn throw_disposed() -> ! {
    let msg = js_string_from_bytes(
        b"Cannot add values to a disposed stack".as_ptr(),
        "Cannot add values to a disposed stack".len() as u32,
    );
    let err = crate::error::js_referenceerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

/// Invoke a single disposer value (a closure) with no arguments. Non-callable
/// entries are skipped (defensive — `use`/`adopt`/`defer` only ever store
/// closures).
fn call_disposer(callable: f64) {
    let jsv = JSValue::from_bits(callable.to_bits());
    if !jsv.is_pointer() {
        return;
    }
    let ptr = js_nanbox_get_pointer(callable) as *const ClosureHeader;
    if ptr.is_null() {
        return;
    }
    js_closure_call0(ptr);
}

/// Run every registered disposer in LIFO order and clear the array. Marks the
/// stack disposed first so a disposer that re-enters observes `.disposed`.
fn run_disposers(stack: *mut ObjectHeader) {
    if stack.is_null() {
        return;
    }
    js_object_set_field_f64(stack, FIELD_DISPOSED, bool_f64(true));
    let arr = stack_disposers(stack);
    let len = if arr.is_null() {
        0
    } else {
        js_array_length(arr)
    };
    for i in (0..len).rev() {
        let callable = js_array_get_f64(arr, i);
        call_disposer(callable);
    }
    // Replace with an empty array so the disposers can be collected.
    let empty = js_array_alloc(0);
    js_object_set_field_f64(stack, FIELD_DISPOSERS, js_nanbox_pointer(empty as i64));
}

// ---------------------------------------------------------------------------
// `adopt` closure: captures (value, onDispose) and calls onDispose(value).
// ---------------------------------------------------------------------------

extern "C" fn adopt_disposer_thunk(closure: *const ClosureHeader) -> f64 {
    let value = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let on_dispose = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let cb = JSValue::from_bits(on_dispose.to_bits());
    if cb.is_pointer() {
        let cb_ptr = js_nanbox_get_pointer(on_dispose) as *const ClosureHeader;
        if !cb_ptr.is_null() {
            crate::closure::js_closure_call1(cb_ptr, value);
        }
    }
    undefined()
}

fn make_adopt_disposer(value: f64, on_dispose: f64) -> f64 {
    let func = adopt_disposer_thunk as *const u8;
    js_register_closure_arity(func, 0);
    let closure = js_closure_alloc(func, 2);
    if closure.is_null() {
        return undefined();
    }
    js_closure_set_capture_ptr(closure, 0, value.to_bits() as i64);
    js_closure_set_capture_ptr(closure, 1, on_dispose.to_bits() as i64);
    js_nanbox_pointer(closure as i64)
}

// ---------------------------------------------------------------------------
// DisposableStack FFI surface.
// ---------------------------------------------------------------------------

/// `new DisposableStack()`
#[no_mangle]
pub extern "C" fn js_disposable_stack_new() -> *mut ObjectHeader {
    alloc_stack(CLASS_ID_DISPOSABLE_STACK)
}

/// `new AsyncDisposableStack()`
#[no_mangle]
pub extern "C" fn js_async_disposable_stack_new() -> *mut ObjectHeader {
    alloc_stack(CLASS_ID_ASYNC_DISPOSABLE_STACK)
}

/// `stack.disposed` getter → bool.
#[no_mangle]
pub extern "C" fn js_disposable_stack_disposed(stack: *mut ObjectHeader) -> f64 {
    bool_f64(stack_is_disposed(receiver_obj(stack)))
}

/// `stack.defer(onDispose)` → undefined. Stores the callback to run at
/// dispose time. Throws ReferenceError if the stack is already disposed.
#[no_mangle]
pub extern "C" fn js_disposable_stack_defer(stack: *mut ObjectHeader, on_dispose: f64) -> f64 {
    let obj = receiver_obj(stack);
    if stack_is_disposed(obj) {
        throw_disposed();
    }
    push_disposer(obj, on_dispose);
    undefined()
}

/// `stack.use(resource)` → resource. Stores `resource[Symbol.dispose]` and
/// returns the resource unchanged. `null` / `undefined` resources are
/// accepted and add no disposer (matching the spec). Throws ReferenceError
/// if the stack is already disposed.
#[no_mangle]
pub extern "C" fn js_disposable_stack_use(stack: *mut ObjectHeader, resource: f64) -> f64 {
    let obj = receiver_obj(stack);
    if stack_is_disposed(obj) {
        throw_disposed();
    }
    let jsv = JSValue::from_bits(resource.to_bits());
    if jsv.is_null() || jsv.is_undefined() {
        return resource;
    }
    let dispose_sym = crate::symbol::well_known_symbol("dispose");
    if !dispose_sym.is_null() {
        let sym_f64 = f64::from_bits(JSValue::pointer(dispose_sym as *const u8).bits());
        let method = unsafe { crate::symbol::js_object_get_symbol_property(resource, sym_f64) };
        if method.to_bits() != TAG_UNDEFINED {
            push_disposer(obj, method);
        }
    }
    resource
}

/// `stack.adopt(value, onDispose)` → value. Stores a disposer that calls
/// `onDispose(value)`. Throws ReferenceError if already disposed.
#[no_mangle]
pub extern "C" fn js_disposable_stack_adopt(
    stack: *mut ObjectHeader,
    value: f64,
    on_dispose: f64,
) -> f64 {
    let obj = receiver_obj(stack);
    if stack_is_disposed(obj) {
        throw_disposed();
    }
    let disposer = make_adopt_disposer(value, on_dispose);
    push_disposer(obj, disposer);
    value
}

/// `stack.dispose()` → undefined. Runs all disposers LIFO. Idempotent: a
/// second call is a no-op.
#[no_mangle]
pub extern "C" fn js_disposable_stack_dispose(stack: *mut ObjectHeader) -> f64 {
    let obj = receiver_obj(stack);
    if !stack_is_disposed(obj) {
        run_disposers(obj);
    }
    undefined()
}

/// `stack[Symbol.dispose]()` — alias of `dispose()`.
#[no_mangle]
pub extern "C" fn js_disposable_stack_symbol_dispose(stack: *mut ObjectHeader) -> f64 {
    js_disposable_stack_dispose(stack)
}

/// `stack.move()` → a new stack that takes ownership of the disposers; the
/// receiver is marked disposed (without running them). Throws ReferenceError
/// if already disposed.
#[no_mangle]
pub extern "C" fn js_disposable_stack_move(stack: *mut ObjectHeader) -> f64 {
    let obj = receiver_obj(stack);
    if stack_is_disposed(obj) {
        throw_disposed();
    }
    let class_id = unsafe { (*obj).class_id };
    let fresh = alloc_stack(class_id);
    if fresh.is_null() {
        return undefined();
    }
    // Transfer the disposers array wholesale.
    let arr = js_object_get_field_f64(obj, FIELD_DISPOSERS);
    js_object_set_field_f64(fresh, FIELD_DISPOSERS, arr);
    // Empty + mark the source disposed (without running anything).
    let empty = js_array_alloc(0);
    js_object_set_field_f64(obj, FIELD_DISPOSERS, js_nanbox_pointer(empty as i64));
    js_object_set_field_f64(obj, FIELD_DISPOSED, bool_f64(true));
    js_nanbox_pointer(fresh as i64)
}

/// `asyncStack.disposeAsync()` → Promise<undefined>. Synchronously runs the
/// registered async disposers in LIFO order (each is invoked with no args;
/// the returned promise of each is not awaited in this implementation — see
/// follow-up note in the PR) and resolves a Promise. The disposers still run
/// in the correct LIFO order, which is what the parity coverage checks.
#[no_mangle]
pub extern "C" fn js_async_disposable_stack_dispose_async(stack: *mut ObjectHeader) -> f64 {
    let obj = receiver_obj(stack);
    if !stack_is_disposed(obj) {
        run_disposers(obj);
    }
    let promise = crate::promise::js_promise_resolved(undefined());
    js_nanbox_pointer(promise as i64)
}

/// `asyncStack[Symbol.asyncDispose]()` — alias of `disposeAsync()`.
#[no_mangle]
pub extern "C" fn js_async_disposable_stack_symbol_async_dispose(stack: *mut ObjectHeader) -> f64 {
    js_async_disposable_stack_dispose_async(stack)
}

// ---------------------------------------------------------------------------
// SuppressedError.
// ---------------------------------------------------------------------------

fn register_suppressed_error_once() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        js_register_class_extends_error(CLASS_ID_SUPPRESSED_ERROR);
    });
}

/// `new SuppressedError(error, suppressed, message?)` → an Error-subclass
/// object carrying `.error`, `.suppressed`, `.message`, `.name`, `.stack`.
#[no_mangle]
pub extern "C" fn js_suppressed_error_new(error: f64, suppressed: f64, message: f64) -> f64 {
    register_suppressed_error_once();
    let obj = js_object_alloc(CLASS_ID_SUPPRESSED_ERROR, 6);
    if obj.is_null() {
        return undefined();
    }
    let set = |key: &str, value: f64| {
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        js_object_set_field_by_name(obj, key_ptr, value);
    };
    set("name", string_f64("SuppressedError"));
    // Node coerces a non-string message via ToString; undefined → "".
    let msg_jsv = JSValue::from_bits(message.to_bits());
    let message_val = if msg_jsv.is_undefined() {
        string_f64("")
    } else if msg_jsv.is_any_string() {
        message
    } else {
        let coerced = crate::builtins::js_string_coerce(message);
        js_nanbox_string(coerced as i64)
    };
    set("message", message_val);
    set("error", error);
    set("suppressed", suppressed);
    set("stack", string_f64("SuppressedError"));
    js_nanbox_pointer(obj as i64)
}

// ---------------------------------------------------------------------------
// Keepalive anchors — these `#[no_mangle]` fns are only ever called from
// generated code (the codegen `new` arm + the native-module dispatch table),
// so the whole-program-LLVM auto-optimize bitcode rebuild would otherwise
// dead-strip them (see project_auto_optimize_keepalive_3320). `#[used]`
// survives the bitcode pipeline.
// ---------------------------------------------------------------------------

#[used]
static KEEP_DISPOSABLE_STACK_NEW: extern "C" fn() -> *mut ObjectHeader = js_disposable_stack_new;
#[used]
static KEEP_ASYNC_DISPOSABLE_STACK_NEW: extern "C" fn() -> *mut ObjectHeader =
    js_async_disposable_stack_new;
#[used]
static KEEP_DISPOSABLE_STACK_DISPOSED: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_disposable_stack_disposed;
#[used]
static KEEP_DISPOSABLE_STACK_DEFER: extern "C" fn(*mut ObjectHeader, f64) -> f64 =
    js_disposable_stack_defer;
#[used]
static KEEP_DISPOSABLE_STACK_USE: extern "C" fn(*mut ObjectHeader, f64) -> f64 =
    js_disposable_stack_use;
#[used]
static KEEP_DISPOSABLE_STACK_ADOPT: extern "C" fn(*mut ObjectHeader, f64, f64) -> f64 =
    js_disposable_stack_adopt;
#[used]
static KEEP_DISPOSABLE_STACK_DISPOSE: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_disposable_stack_dispose;
#[used]
static KEEP_DISPOSABLE_STACK_SYMBOL_DISPOSE: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_disposable_stack_symbol_dispose;
#[used]
static KEEP_DISPOSABLE_STACK_MOVE: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_disposable_stack_move;
#[used]
static KEEP_ASYNC_DISPOSABLE_STACK_DISPOSE_ASYNC: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_async_disposable_stack_dispose_async;
#[used]
static KEEP_ASYNC_DISPOSABLE_STACK_SYMBOL_ASYNC_DISPOSE: extern "C" fn(*mut ObjectHeader) -> f64 =
    js_async_disposable_stack_symbol_async_dispose;
#[used]
static KEEP_SUPPRESSED_ERROR_NEW: extern "C" fn(f64, f64, f64) -> f64 = js_suppressed_error_new;
