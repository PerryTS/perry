//! Value bridge between NaN-boxed JSValue and V8 values
//!
//! This module handles conversion between the Perry runtime's NaN-boxed
//! representation and V8's value system.
//!
//! ## V8 Object Handle Table
//!
//! V8 objects (objects, arrays, functions) returned to native code are stored
//! in a thread-local handle table. The native code receives a handle ID that
//! can be used to retrieve the V8 object for subsequent operations.

use deno_core::v8;
use perry_runtime::JSValue;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::interop::{bump_js_handle_released, bump_js_handle_stored, bump_v8_entry, V8EntryKind};

// NaN-boxing constants (must match perry-runtime/src/value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// Tag for V8 object handles - these are opaque references to V8 objects
/// stored in the handle table, NOT native Perry objects
const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Thread-local storage for V8 object handles
thread_local! {
    /// Maps handle IDs to V8 Global handles
    static JS_OBJECT_HANDLES: RefCell<HashMap<u64, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Stable V8 constructor-like wrappers for Perry class references.
    static NATIVE_CLASS_HANDLES: RefCell<HashMap<u32, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Stable V8 function wrappers for Perry closures. Keyed on the raw
    /// `*const ClosureHeader` pointer so the SAME Perry closure crossing into
    /// V8 twice surfaces as the SAME `v8::Function` instance — load-bearing for
    /// `reflect-metadata`'s WeakMap lookups (decorator records metadata on
    /// `descriptor.value`; NestJS RouterExplorer reads it back via
    /// `prototype['methodName']` — both must hash to the same WeakMap key).
    /// (#1021 NestJS decorator-routing blocker.)
    static NATIVE_CLOSURE_HANDLES: RefCell<HashMap<usize, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// V8 Promise resolvers waiting on native Perry promises returned through callbacks.
    static NATIVE_PROMISE_RESOLVERS: RefCell<HashMap<u64, v8::Global<v8::PromiseResolver>>> = RefCell::new(HashMap::new());
    /// Snapshot of untampered intrinsics used by the conservative JS export
    /// data-object fast path. Captured during `js_runtime_init`, before user
    /// modules can replace `globalThis.Object` or its methods.
    static EXPORT_SNAPSHOT_INTRINSICS: RefCell<Option<ExportSnapshotIntrinsics>> = const { RefCell::new(None) };
    /// Counter for generating unique handle IDs
    static NEXT_HANDLE_ID: Cell<u64> = const { Cell::new(1) };
    static NEXT_NATIVE_PROMISE_RESOLVER_ID: Cell<u64> = const { Cell::new(1) };
}

struct ExportSnapshotIntrinsics {
    object_prototype: v8::Global<v8::Value>,
    object_is_frozen: v8::Global<v8::Function>,
}

pub fn capture_export_snapshot_intrinsics(scope: &mut v8::PinScope<'_, '_>) {
    let Some(intrinsics) = load_export_snapshot_intrinsics(scope) else {
        // If the lookup of `globalThis.Object` / its `prototype` / `isFrozen`
        // ever fails at runtime init, every export-data-object fast-path
        // eligibility check will silently return false (`is_plain_object`
        // requires the intrinsics cell to be set). That would manifest as a
        // perf cliff rather than a correctness bug — surface it loudly so
        // regressions don't hide as "slow but still working".
        eprintln!(
            "perry-jsruntime: failed to capture Object intrinsics at init; \
             JS export-data-object snapshot fast path disabled \
             (every export read will go through V8 fallback)"
        );
        return;
    };
    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        *cell.borrow_mut() = Some(intrinsics);
    });
}

fn load_export_snapshot_intrinsics(
    scope: &mut v8::PinScope<'_, '_>,
) -> Option<ExportSnapshotIntrinsics> {
    let global = scope.get_current_context().global(scope);
    let object_key = v8::String::new(scope, "Object")?;
    let object_value = global.get(scope, object_key.into())?;
    let object_ctor = v8::Local::<v8::Object>::try_from(object_value).ok()?;

    let prototype_key = v8::String::new(scope, "prototype")?;
    let object_prototype = object_ctor.get(scope, prototype_key.into())?;

    let is_frozen_key = v8::String::new(scope, "isFrozen")?;
    let is_frozen_value = object_ctor.get(scope, is_frozen_key.into())?;
    let object_is_frozen = v8::Local::<v8::Function>::try_from(is_frozen_value).ok()?;

    Some(ExportSnapshotIntrinsics {
        object_prototype: v8::Global::new(scope, object_prototype),
        object_is_frozen: v8::Global::new(scope, object_is_frozen),
    })
}

fn native_class_constructor(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // For `new fn()` calls, V8 has already populated `args.this()` with a
    // fresh object whose `[[Prototype]]` is `fn.prototype` — leave it alone
    // so NestJS's `Object.getPrototypeOf(new metatype())[methodName]` can
    // walk the prototype methods we populated in
    // `populate_native_class_v8_prototype`. (#1021.)
    //
    // For bare `fn()` calls (no `new`), `this` is the global object and we
    // must NOT use it. Fall back to a fresh empty object, which is the
    // historical behavior — none of Perry's V8-fallback callers actually
    // hit this path, but keeping it safe avoids leaking the global into
    // unexpected callers.
    if args.is_construct_call() {
        retval.set(args.this().into());
    } else {
        retval.set(v8::Object::new(scope).into());
    }
}

// Issue: Effect.pipe(map) chain — when a Perry closure (raw `*const
// ClosureHeader` pointer that's been NaN-boxed with POINTER_TAG) crosses
// into V8 as an argument, it must surface as a real v8::Function so JS
// code can invoke it. Without this wrapper, V8 saw a string/object proxy
// (from `native_object_to_v8`'s fallback paths) and threw "f is not a
// function" when Effect's internal pipeline tried to call the mapping
// function.
//
// Mirrors `native_callback_trampoline` (interop.rs) but stores the
// closure pointer directly in the v8::Function's `data` slot instead of
// going through the NATIVE_CALLBACKS registry — we already have the
// closure pointer in hand and don't need a stable callback_id for it.
fn perry_closure_v8_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let data = args.data();
    if !data.is_external() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let external = v8::Local::<v8::External>::try_from(data).unwrap();
    let closure_ptr = external.value() as i64;
    if closure_ptr == 0 {
        retval.set(v8::undefined(scope).into());
        return;
    }

    let arg_count = args.length();
    let mut native_args: Vec<f64> = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count {
        let arg = args.get(i);
        native_args.push(v8_to_native(scope, arg));
    }

    let _scope_guard = crate::stash_trampoline_scope(scope);

    type ClosureCallFn = unsafe extern "C" fn(i64, *const f64, i64) -> f64;
    let func: ClosureCallFn = perry_runtime::closure::js_closure_call_array;
    let result = unsafe { func(closure_ptr, native_args.as_ptr(), native_args.len() as i64) };

    let v8_result = native_to_v8(scope, result);
    retval.set(v8_result);
}

/// Wrap a Perry closure (raw pointer to a `ClosureHeader` with
/// `CLOSURE_MAGIC` at offset 12) as a `v8::Function`. Used by
/// `native_object_to_v8` when an argument passed to V8 turns out to be a
/// native closure — typically when a `LocalGet` holding an arrow function
/// is passed to a V8-imported call site like `Effect.map(fn)`.
///
/// The returned `v8::Function` is cached per closure pointer
/// (`NATIVE_CLOSURE_HANDLES`) so that repeated crossings of the SAME closure
/// surface as the SAME function identity on the V8 side. `reflect-metadata`'s
/// `WeakMap` keys depend on this — without identity stability the metadata
/// the `@Get('/ping')` decorator writes on `descriptor.value` cannot be
/// recovered when NestJS reads `prototype['methodName']`. (#1021.)
fn native_closure_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> Option<v8::Local<'s, v8::Value>> {
    if ptr.is_null() {
        return None;
    }
    let key = ptr as usize;
    if let Some(existing) = NATIVE_CLOSURE_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&key)
            .map(|global| v8::Local::new(scope, global))
    }) {
        return Some(existing);
    }
    // Closure pointer is *const ClosureHeader. Stash the raw address in a
    // v8::External so the trampoline can recover it on invocation.
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let function = v8::Function::builder(perry_closure_v8_trampoline)
        .data(external.into())
        .build(scope)?;
    // Also expose the pointer as an own property so
    // `v8_to_native_metadata_target` can recover Perry's POINTER_TAG | ptr
    // identity when the function flows back across the boundary. Without
    // this round-trip, `descriptor.value` and `prototype['ping']` hash to
    // different NaN-box bits on the Perry side and the mirrored entry in
    // `REFLECT_METADATA` is unreachable. (#1021.)
    if let Some(prop_key) = v8::String::new(scope, "__perry_closure_ptr") {
        let ptr_external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
        function.set(scope, prop_key.into(), ptr_external.into());
    }
    let value: v8::Local<v8::Value> = function.into();
    NATIVE_CLOSURE_HANDLES.with(|handles| {
        handles
            .borrow_mut()
            .insert(key, v8::Global::new(scope, value));
    });
    Some(value)
}

fn native_class_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    class_id: u32,
) -> v8::Local<'s, v8::Value> {
    if let Some(existing) = NATIVE_CLASS_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&class_id)
            .map(|global| v8::Local::new(scope, global))
    }) {
        return existing;
    }

    let function = v8::Function::builder(native_class_constructor)
        .build(scope)
        .unwrap_or_else(|| v8::Function::new(scope, native_class_constructor).unwrap());
    if let Some(key) = v8::String::new(scope, "__perry_native_class_id") {
        let value = v8::Integer::new_from_unsigned(scope, class_id);
        function.set(scope, key.into(), value.into());
    }
    // Surface Perry's user-visible class name as `fn.name` so V8-side code
    // that reads `metatype.name` (NestJS `ModuleTokenFactory.create()`)
    // gets the real class name instead of the default empty string. `name`
    // is a non-writable accessor by default; use `set_name`, which goes
    // through V8's internal slot. (#1021.)
    let class_name_opt = perry_runtime::object::class_name_for_id(class_id);
    if let Some(class_name) = class_name_opt {
        if let Some(name_value) = v8::String::new(scope, &class_name) {
            function.set_name(name_value);
        }
    }

    // Populate the V8 wrapper's prototype with method bindings so
    // V8-side accessors like NestJS's `Object.getPrototypeOf(instance)[method]`
    // (paths-explorer.js) resolve to the same `v8::Function` that the
    // `@Get('/ping')` decorator received as `descriptor.value`. Without this
    // the V8 wrapper's `.prototype` is an empty object and the route lookup
    // can't reach the method descriptor metadata. (#1021.)
    populate_native_class_v8_prototype(scope, function, class_id);

    let value: v8::Local<v8::Value> = function.into();
    NATIVE_CLASS_HANDLES.with(|handles| {
        handles
            .borrow_mut()
            .insert(class_id, v8::Global::new(scope, value));
    });
    value
}

/// Per-method trampoline data: the class id and the leaked method name slice
/// (`&'static [u8]`) we use to dispatch the call. Lives forever (one alloc
/// per (class_id, method_name) pair populated on the V8 prototype).
struct V8MethodDispatchEntry {
    class_id: u32,
    method_name: &'static [u8],
}

/// V8 callback that re-dispatches a method call on a Perry-backed class to
/// the runtime's vtable. The trampoline data is a `v8::External` wrapping
/// a leaked `V8MethodDispatchEntry` (class_id + method name). We dispatch
/// using the class id directly so the receiver doesn't need to be a real
/// Perry-allocated object — V8 instances of our wrapper class only carry
/// the `__perry_native_class_id` marker, not Perry's full ObjectHeader,
/// so a `js_native_call_method` round-trip through the V8 handle table
/// would loop back into V8. (#1021.)
fn perry_v8_instance_method_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let data = args.data();
    if !data.is_external() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let external = v8::Local::<v8::External>::try_from(data).unwrap();
    let entry_ptr = external.value() as *const V8MethodDispatchEntry;
    if entry_ptr.is_null() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let entry = unsafe { &*entry_ptr };

    // Resolve the vtable entry for this method directly. We don't go
    // through `js_native_call_method` because that walks
    // `jsval.as_pointer()` on the receiver — for a Perry class wrapper
    // exposed to V8, the receiver is a V8 object (not a Perry ObjectHeader),
    // so the pointer-walk reads junk bits and the call returns the wrong
    // value (we observed `instance.ping()` returning `1`, the class_id,
    // instead of the method body's `"pong"`). The vtable entry holds the
    // raw `func_ptr` Perry's codegen emitted for the method body; we can
    // invoke it directly through `call_vtable_method` if we expose that
    // entry point through the runtime — but it's `pub(crate)`. Simpler
    // workaround: re-implement the trampoline call here with `this` set
    // to TAG_UNDEFINED. Methods that don't read `this` (decorator-style
    // controller handlers, the NestJS canary) just work. Methods that do
    // use `this` would need real handle-based dispatch (deferred).
    let method_name_str = std::str::from_utf8(entry.method_name).unwrap_or("");
    let func_info = {
        let registry = match perry_runtime::object::CLASS_VTABLE_REGISTRY.read() {
            Ok(g) => g,
            Err(_) => {
                retval.set(v8::undefined(scope).into());
                return;
            }
        };
        registry.as_ref().and_then(|reg| {
            reg.get(&entry.class_id).and_then(|vtable| {
                vtable
                    .methods
                    .get(method_name_str)
                    .map(|m| (m.func_ptr, m.param_count))
            })
        })
    };
    let Some((func_ptr, param_count)) = func_info else {
        retval.set(v8::undefined(scope).into());
        return;
    };

    let arg_count = args.length();
    let mut native_args: Vec<f64> = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count {
        native_args.push(v8_to_native(scope, args.get(i)));
    }

    let _scope_guard = crate::stash_trampoline_scope(scope);

    // Direct vtable method call. Signature is
    //   extern "C" fn(this: f64, a0: f64, a1: f64, ...) -> f64
    // where the declared positional arity is `param_count`. Pad missing
    // args with TAG_UNDEFINED so the calling convention loads the
    // expected number of doubles. Cap at 8 — none of the targeted NestJS
    // controller shapes pass more than that, and going higher would
    // require enumerating every Rust calling-convention arity here.
    const TAG_UNDEFINED_F64: u64 = 0x7FFC_0000_0000_0001;
    let undef = f64::from_bits(TAG_UNDEFINED_F64);
    let arg = |i: usize| -> f64 { native_args.get(i).copied().unwrap_or(undef) };
    // We use TAG_UNDEFINED as `this` so method bodies that don't read `this`
    // (the controller-handler shape) just work. The receiver is fine here
    // because Perry's method functions take `this` as the first f64 param
    // but only the bodies that read `Expr::This` care about its value.
    let this_val = undef;

    type Fn0 = unsafe extern "C" fn(f64) -> f64;
    type Fn1 = unsafe extern "C" fn(f64, f64) -> f64;
    type Fn2 = unsafe extern "C" fn(f64, f64, f64) -> f64;
    type Fn3 = unsafe extern "C" fn(f64, f64, f64, f64) -> f64;
    type Fn4 = unsafe extern "C" fn(f64, f64, f64, f64, f64) -> f64;
    type Fn5 = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64;
    type Fn6 = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64;
    type Fn7 = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64;
    type Fn8 = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64;

    let result = unsafe {
        match param_count {
            0 => (std::mem::transmute::<usize, Fn0>(func_ptr))(this_val),
            1 => (std::mem::transmute::<usize, Fn1>(func_ptr))(this_val, arg(0)),
            2 => (std::mem::transmute::<usize, Fn2>(func_ptr))(this_val, arg(0), arg(1)),
            3 => (std::mem::transmute::<usize, Fn3>(func_ptr))(this_val, arg(0), arg(1), arg(2)),
            4 => (std::mem::transmute::<usize, Fn4>(func_ptr))(
                this_val,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
            ),
            5 => (std::mem::transmute::<usize, Fn5>(func_ptr))(
                this_val,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4),
            ),
            6 => (std::mem::transmute::<usize, Fn6>(func_ptr))(
                this_val,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4),
                arg(5),
            ),
            7 => (std::mem::transmute::<usize, Fn7>(func_ptr))(
                this_val,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4),
                arg(5),
                arg(6),
            ),
            _ => (std::mem::transmute::<usize, Fn8>(func_ptr))(
                this_val,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4),
                arg(5),
                arg(6),
                arg(7),
            ),
        }
    };

    let v8_result = native_to_v8(scope, result);
    retval.set(v8_result);
}

/// Mirror each method registered in Perry's `CLASS_VTABLE_REGISTRY` onto the
/// V8 class wrapper's `.prototype` object. Each slot is a `v8::Function`
/// whose trampoline re-dispatches through `js_native_call_method` with V8's
/// `this` as the receiver — so `Object.getPrototypeOf(new metatype())[name]`
/// resolves to a real method that runs on the instance, not on the class
/// ref. Also exposes a stable identity so `reflect-metadata` decorators
/// that key on `descriptor.value` can find the same function NestJS reads
/// back through `prototype['methodName']`. (#1021 NestJS routing.)
fn populate_native_class_v8_prototype(
    scope: &mut v8::PinScope<'_, '_>,
    function: v8::Local<v8::Function>,
    class_id: u32,
) {
    let prototype_key = match v8::String::new(scope, "prototype") {
        Some(k) => k,
        None => return,
    };
    let prototype_val = match function.get(scope, prototype_key.into()) {
        Some(v) => v,
        None => return,
    };
    let prototype_obj = match v8::Local::<v8::Object>::try_from(prototype_val) {
        Ok(o) => o,
        Err(_) => return,
    };

    let method_names: Vec<String> = {
        let registry = match perry_runtime::object::CLASS_VTABLE_REGISTRY.read() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(reg) = registry.as_ref() else {
            return;
        };
        let Some(vtable) = reg.get(&class_id) else {
            return;
        };
        vtable.methods.keys().cloned().collect()
    };

    for method_name in method_names {
        // Leak both the dispatch entry and the method-name bytes. One alloc
        // per (class_id, method_name) pair, called only at the first crossing
        // of the class into V8 — the cost is bounded by the static set of
        // exported user classes.
        let leaked_bytes: &'static [u8] = method_name.clone().into_bytes().leak();
        let entry: &'static V8MethodDispatchEntry = Box::leak(Box::new(V8MethodDispatchEntry {
            class_id,
            method_name: leaked_bytes,
        }));
        let external = v8::External::new(
            scope,
            entry as *const V8MethodDispatchEntry as *mut std::ffi::c_void,
        );
        let Some(method_fn) = v8::Function::builder(perry_v8_instance_method_trampoline)
            .data(external.into())
            .build(scope)
        else {
            continue;
        };
        // Decorator metadata identity: also expose the Perry-side bound
        // closure pointer on this method function so `descriptor.value`
        // (passed to `@Get('/ping')`) and `prototype['ping']` both hash to
        // the SAME Perry NaN-boxed value when round-tripped through
        // `v8_to_native_metadata_target`. Without this, the metadata
        // `Reflect.defineMetadata(...)` writes against `descriptor.value`
        // cannot be re-read through the prototype slot. (#1021.)
        let bound =
            perry_runtime::object::class_prototype_method_value_for_name(class_id, &method_name);
        if bound.to_bits() != 0x7FFC_0000_0000_0001 {
            let bits = bound.to_bits();
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut std::ffi::c_void;
            if !ptr.is_null() {
                if let Some(prop_key) = v8::String::new(scope, "__perry_closure_ptr") {
                    let ptr_external = v8::External::new(scope, ptr);
                    method_fn.set(scope, prop_key.into(), ptr_external.into());
                }
                // Also surface this method_fn as the cached v8::Function for
                // the closure ptr, so subsequent `native_closure_to_v8`
                // crossings (e.g. when `descriptor.value` flows into a V8
                // decorator) return the SAME function instance.
                let method_value: v8::Local<v8::Value> = method_fn.into();
                NATIVE_CLOSURE_HANDLES.with(|handles| {
                    handles
                        .borrow_mut()
                        .entry(ptr as usize)
                        .or_insert_with(|| v8::Global::new(scope, method_value));
                });
            }
        }
        if let Some(name_v8) = v8::String::new(scope, &method_name) {
            method_fn.set_name(name_v8);
        }
        if let Some(prop_key) = v8::String::new(scope, &method_name) {
            prototype_obj.set(scope, prop_key.into(), method_fn.into());
        }
    }
}

fn native_class_id_from_v8(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> Option<u32> {
    if !(value.is_function() || value.is_object()) {
        return None;
    }
    let obj = v8::Local::<v8::Object>::try_from(value).ok()?;
    let key = v8::String::new(scope, "__perry_native_class_id")?;
    let id_value = obj.get(scope, key.into())?;
    if id_value.is_undefined() || id_value.is_null() || !id_value.is_uint32() {
        return None;
    }
    let id = id_value.uint32_value(scope)?;
    if id == 0 {
        return None;
    }
    Some(id)
}

pub fn v8_to_native_metadata_target(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(class_id) = native_class_id_from_v8(scope, value) {
        return f64::from_bits(INT32_TAG | class_id as u64);
    }

    // Perry-closure-wrapped V8 functions stash the underlying
    // `*const ClosureHeader` pointer in a `__perry_closure_ptr` v8::External
    // property (see `native_closure_to_v8`). Recover it so the metadata target
    // hashes to the same NaN-boxed identity Perry uses internally — this is
    // what lets `@Get('/ping')` (write site) and NestJS RouterExplorer
    // (read site) agree on the method descriptor. (#1021.)
    if value.is_function() {
        if let Ok(obj) = v8::Local::<v8::Object>::try_from(value) {
            if let Some(key) = v8::String::new(scope, "__perry_closure_ptr") {
                if let Some(ptr_value) = obj.get(scope, key.into()) {
                    if ptr_value.is_external() {
                        let external = v8::Local::<v8::External>::try_from(ptr_value).unwrap();
                        let ptr_bits = external.value() as u64;
                        if ptr_bits != 0 {
                            return f64::from_bits(POINTER_TAG | (ptr_bits & POINTER_MASK));
                        }
                    }
                }
            }
        }
    }

    if value.is_object() {
        if let Ok(obj) = v8::Local::<v8::Object>::try_from(value) {
            if let Some(key) = v8::String::new(scope, "__native_ptr__") {
                if let Some(ptr_value) = obj.get(scope, key.into()) {
                    if ptr_value.is_external() {
                        let external = v8::Local::<v8::External>::try_from(ptr_value).unwrap();
                        return f64::from_bits(
                            POINTER_TAG | (external.value() as u64 & POINTER_MASK),
                        );
                    }
                }
            }
        }
    }

    v8_to_native(scope, value)
}

pub fn v8_to_native_metadata_value(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(class_id) = native_class_id_from_v8(scope, value) {
        return f64::from_bits(INT32_TAG | class_id as u64);
    }

    if value.is_array() {
        let array = v8::Local::<v8::Array>::try_from(value).unwrap();
        let ptr = v8_array_to_native_metadata(scope, array);
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }

    v8_to_native(scope, value)
}

/// Store a V8 value in the handle table and return a handle ID
pub fn store_js_handle(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> u64 {
    let handle_id = NEXT_HANDLE_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    let global = v8::Global::new(scope, value);
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow_mut().insert(handle_id, global);
    });
    bump_js_handle_stored();
    handle_id
}

/// Retrieve a V8 value from the handle table
pub fn get_js_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle: u64,
) -> Option<v8::Local<'s, v8::Value>> {
    JS_OBJECT_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&handle)
            .map(|g| v8::Local::new(scope, g))
    })
}

/// Release a V8 handle from the table
pub fn release_js_handle(handle: u64) -> bool {
    let released = JS_OBJECT_HANDLES.with(|handles| handles.borrow_mut().remove(&handle).is_some());
    if released {
        bump_js_handle_released();
    }
    released
}

/// Check if a NaN-boxed value is a JS handle
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}

/// Extract handle ID from a NaN-boxed JS handle value
pub fn get_handle_id(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        Some(bits & POINTER_MASK)
    } else {
        None
    }
}

/// Create a NaN-boxed value representing a JS handle
pub fn make_js_handle_value(handle_id: u64) -> f64 {
    f64::from_bits(JS_HANDLE_TAG | (handle_id & POINTER_MASK))
}

fn store_native_promise_resolver(
    scope: &mut v8::PinScope<'_, '_>,
    resolver: v8::Local<v8::PromiseResolver>,
) -> u64 {
    let resolver_id = NEXT_NATIVE_PROMISE_RESOLVER_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .insert(resolver_id, v8::Global::new(scope, resolver));
    });
    resolver_id
}

fn take_native_promise_resolver<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    resolver_id: u64,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .remove(&resolver_id)
            .map(|resolver| v8::Local::new(scope, resolver))
    })
}

/// Fix up a native value for JS interop boundary.
/// Raw pointers (non-NaN-boxed I64 values bitcast to F64) need POINTER_TAG
/// so that native_to_v8 can properly convert them to V8 arrays/objects.
pub fn fixup_native_for_v8(value: f64) -> f64 {
    let bits = value.to_bits();
    // Raw heap pointers on arm64 are typically 0x0000_0001_xxxx_xxxx to 0x0000_000F_xxxx_xxxx
    // These appear as subnormal f64 values (exponent = 0, mantissa != 0)
    // No legitimate JS number would have bits in this range
    if bits > 0x0000_0001_0000_0000 && bits < 0x0001_0000_0000_0000 {
        // Raw pointer - add POINTER_TAG so native_to_v8 can convert it
        f64::from_bits(POINTER_TAG | (bits & POINTER_MASK))
    } else {
        value
    }
}

/// Convert a native NaN-boxed value to a V8 value
pub fn native_to_v8<'s>(scope: &mut v8::PinScope<'s, '_>, value: f64) -> v8::Local<'s, v8::Value> {
    let bits = value.to_bits();

    // Check special values
    if bits == TAG_UNDEFINED {
        return v8::undefined(scope).into();
    }
    if bits == TAG_NULL {
        return v8::null(scope).into();
    }
    if bits == TAG_FALSE {
        return v8::Boolean::new(scope, false).into();
    }
    if bits == TAG_TRUE {
        return v8::Boolean::new(scope, true).into();
    }

    let tag = bits & TAG_MASK;

    // Check for JS handle (V8 object reference)
    if tag == JS_HANDLE_TAG {
        let handle_id = bits & POINTER_MASK;
        if let Some(v8_val) = get_js_handle(scope, handle_id) {
            return v8_val;
        }
        return v8::undefined(scope).into();
    }

    // Check for int32
    if tag == INT32_TAG {
        let int_val = (bits & 0xFFFF_FFFF) as i32;
        // Perry encodes class references as INT32_TAG | class_id (see
        // `Expr::ClassRef` codegen). When such a value crosses into V8 we
        // surface it as a stable constructor-like function so JS code can use
        // it as a metadata target. NOTE: this means raw integers that happen
        // to equal a registered class id (low positive numbers, the common
        // range) cannot round-trip through the bridge — they materialize as
        // the class function on the JS side. Decorator metadata is the only
        // existing caller, where the input is always a real class ref. If a
        // future caller needs int round-trip, switch class refs to a
        // dedicated NaN-box tag (see review on #754).
        if int_val > 0 && perry_runtime::object::is_class_id_registered(int_val as u32) {
            return native_class_to_v8(scope, int_val as u32);
        }
        return v8::Integer::new(scope, int_val).into();
    }

    // Check for string pointer
    if tag == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            let rust_str = unsafe { native_string_to_rust(ptr) };
            if let Some(v8_str) = v8::String::new(scope, &rust_str) {
                return v8_str.into();
            }
        }
        return v8::String::empty(scope).into();
    }

    if tag == SHORT_STRING_TAG {
        let value = JSValue::from_bits(bits);
        let mut buf = [0u8; perry_runtime::value::SHORT_STRING_MAX_LEN];
        let len = value.short_string_to_buf(&mut buf);
        let rust_str = String::from_utf8_lossy(&buf[..len]);
        if let Some(v8_str) = v8::String::new(scope, &rust_str) {
            return v8_str.into();
        }
        return v8::String::empty(scope).into();
    }

    // Check for BigInt pointer
    if tag == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_bigint_to_v8(scope, ptr);
        }
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    // Check for object/array pointer
    if tag == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_object_to_v8(scope, ptr);
        }
        return v8::null(scope).into();
    }

    // Otherwise it's a regular f64 number
    // Check if it's a valid IEEE 754 number (not NaN with our special tags)
    if (bits & 0x7FF0_0000_0000_0000) != 0x7FF0_0000_0000_0000
        || (bits & 0x000F_FFFF_FFFF_FFFF) == 0
    {
        return v8::Number::new(scope, value).into();
    }

    // Fallback to undefined for unrecognized values
    v8::undefined(scope).into()
}

/// Convert a V8 value to a native NaN-boxed value
///
/// For simple values (undefined, null, boolean, number, string), this converts
/// them to Perry's native NaN-boxed representation.
///
/// For complex values (objects, arrays, functions), this stores them in the
/// handle table and returns a JS handle. This preserves V8 objects for
/// subsequent method calls.
pub fn v8_to_native(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> f64 {
    if value.is_undefined() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    if value.is_null() {
        return f64::from_bits(TAG_NULL);
    }

    if value.is_boolean() {
        let b = value.is_true();
        return f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    }

    // Check number before int32 as numbers can also be int32
    if value.is_number() && !value.is_int32() {
        let num = value.number_value(scope).unwrap_or(f64::NAN);
        return num;
    }

    if value.is_int32() {
        let int_val = value.int32_value(scope).unwrap_or(0);
        return f64::from_bits(INT32_TAG | (int_val as u32 as u64));
    }

    if value.is_string() {
        let v8_str = value.to_string(scope).unwrap();
        let rust_str = v8_str.to_rust_string_lossy(scope);
        let ptr = rust_string_to_native(&rust_str);
        return f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK));
    }

    // Check for BigInt (used by ethers.js and other blockchain libraries)
    if value.is_big_int() {
        let bigint = v8::Local::<v8::BigInt>::try_from(value).unwrap();
        let ptr = v8_bigint_to_native(scope, bigint);
        return f64::from_bits(BIGINT_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For functions, always store as JS handle to preserve callability
    if value.is_function() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // For arrays and objects, store as JS handle to preserve V8 methods and prototype chain
    // This is critical for objects returned from JS function calls (e.g., express())
    // which may have methods we need to call later (e.g., app.use(), app.get())
    if value.is_array() || value.is_object() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // Fallback to undefined
    f64::from_bits(TAG_UNDEFINED)
}

/// Convert JS module-export values to Perry values.
///
/// Frozen plain data objects exported from JS modules are safe to snapshot into
/// native Perry objects. That keeps follow-on property reads on constants like
/// `MODULE_METADATA.PROVIDERS` native instead of bouncing back into V8 for each
/// field. Mutable objects, accessors, proxies, custom prototypes, functions,
/// promises, arrays, symbols, or nested non-data values stay as V8 handles.
pub fn v8_to_native_export_value(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(snapshot) = v8_plain_data_object_to_native(scope, value, 0) {
        return snapshot;
    }

    v8_to_native(scope, value)
}

/// Convert a V8 value to a native NaN-boxed value, converting arrays to native arrays
///
/// This variant converts arrays to native Perry arrays instead of JS handles.
/// Use this when you know the result should be a native array (e.g., for Array operations).
#[allow(dead_code)]
pub fn v8_to_native_array(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> f64 {
    // For arrays, convert to native Perry array
    if value.is_array() {
        let array = v8::Local::<v8::Array>::try_from(value).unwrap();
        let ptr = v8_array_to_native(scope, array);
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For everything else, use the standard conversion
    v8_to_native(scope, value)
}

fn v8_plain_data_object_to_native(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
    depth: usize,
) -> Option<f64> {
    if depth > 4
        || value.is_function()
        || value.is_array()
        || value.is_promise()
        || v8_value_is_proxy(scope, value)
        || !value.is_object()
    {
        return None;
    }

    let obj = v8::Local::<v8::Object>::try_from(value).ok()?;
    if !is_plain_object(scope, obj) {
        return None;
    }
    if !v8_object_is_frozen(scope, obj)? {
        return None;
    }

    let mut names_args = v8::GetPropertyNamesArgsBuilder::new();
    let names = obj.get_own_property_names(
        scope,
        names_args
            .mode(v8::KeyCollectionMode::OwnOnly)
            .property_filter(v8::PropertyFilter::ALL_PROPERTIES)
            .index_filter(v8::IndexFilter::IncludeIndices)
            .key_conversion(v8::KeyConversionMode::ConvertToString)
            .build(),
    )?;
    if names.length() == 0 {
        return None;
    }
    let mut fields: Vec<(String, f64)> = Vec::with_capacity(names.length() as usize);

    for i in 0..names.length() {
        let key = names.get_index(scope, i)?;
        if key.is_symbol() {
            return None;
        }
        let key_string = key.to_string(scope)?.to_rust_string_lossy(scope);
        let field_value = frozen_data_descriptor_value(scope, obj, key)?;
        let native_value =
            if let Some(snapshot) = v8_plain_data_object_to_native(scope, field_value, depth + 1) {
                snapshot
            } else if is_plain_data_leaf(field_value) {
                v8_to_native(scope, field_value)
            } else {
                return None;
            };
        fields.push((key_string, native_value));
    }

    let native_obj = perry_runtime::js_object_alloc(0, 0);
    for (key, value) in fields {
        let key_ptr = perry_runtime::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        perry_runtime::js_object_set_field_by_name(native_obj, key_ptr, value);
    }

    Some(f64::from_bits(
        POINTER_TAG | (native_obj as u64 & POINTER_MASK),
    ))
}

fn is_plain_data_leaf(value: v8::Local<v8::Value>) -> bool {
    value.is_undefined()
        || value.is_null()
        || value.is_boolean()
        || value.is_number()
        || value.is_string()
        || value.is_big_int()
}

fn v8_value_is_proxy(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> bool {
    if value.is_proxy() {
        return true;
    }

    let global = scope.get_current_context().global(scope);
    let Some(deno_key) = v8::String::new(scope, "Deno") else {
        return false;
    };
    let Some(deno_value) = global.get(scope, deno_key.into()) else {
        return false;
    };
    let Ok(deno) = v8::Local::<v8::Object>::try_from(deno_value) else {
        return false;
    };
    let Some(core_key) = v8::String::new(scope, "core") else {
        return false;
    };
    let Some(core_value) = deno.get(scope, core_key.into()) else {
        return false;
    };
    let Ok(core) = v8::Local::<v8::Object>::try_from(core_value) else {
        return false;
    };

    if call_v8_boolean_method(scope, core, "isProxy", value).unwrap_or(false) {
        return true;
    }

    let Some(ops_key) = v8::String::new(scope, "ops") else {
        return false;
    };
    let Some(ops_value) = core.get(scope, ops_key.into()) else {
        return false;
    };
    let Ok(ops) = v8::Local::<v8::Object>::try_from(ops_value) else {
        return false;
    };
    call_v8_boolean_method(scope, ops, "op_is_proxy", value).unwrap_or(false)
}

fn call_v8_boolean_method(
    scope: &mut v8::PinScope<'_, '_>,
    receiver: v8::Local<v8::Object>,
    method_name: &str,
    arg: v8::Local<v8::Value>,
) -> Option<bool> {
    let key = v8::String::new(scope, method_name)?;
    let method_value = receiver.get(scope, key.into())?;
    let method = v8::Local::<v8::Function>::try_from(method_value).ok()?;
    let result = method.call(scope, receiver.into(), &[arg])?;
    if result.is_boolean() {
        Some(result.boolean_value(scope))
    } else {
        None
    }
}

fn is_plain_object(scope: &mut v8::PinScope<'_, '_>, obj: v8::Local<v8::Object>) -> bool {
    let Some(proto) = obj.get_prototype(scope) else {
        return false;
    };
    if proto.is_null() {
        return true;
    }

    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        let intrinsics = cell.borrow();
        let Some(intrinsics) = intrinsics.as_ref() else {
            return false;
        };
        let object_proto = v8::Local::new(scope, &intrinsics.object_prototype);
        proto.strict_equals(object_proto)
    })
}

fn v8_object_is_frozen(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
) -> Option<bool> {
    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        let intrinsics = cell.borrow();
        let intrinsics = intrinsics.as_ref()?;
        let is_frozen = v8::Local::new(scope, &intrinsics.object_is_frozen);
        let receiver = v8::undefined(scope).into();
        let obj_value: v8::Local<v8::Value> = obj.into();
        let result = is_frozen.call(scope, receiver, &[obj_value])?;
        if result.is_boolean() {
            Some(result.boolean_value(scope))
        } else {
            None
        }
    })
}

fn frozen_data_descriptor_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    obj: v8::Local<v8::Object>,
    key: v8::Local<v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    let name = v8::Local::<v8::Name>::try_from(key).ok()?;
    let descriptor_value = obj.get_own_property_descriptor(scope, name)?;
    if descriptor_value.is_undefined() || !descriptor_value.is_object() {
        return None;
    }
    let descriptor = v8::Local::<v8::Object>::try_from(descriptor_value).ok()?;

    let get_key = v8::String::new(scope, "get")?;
    let getter = descriptor.get(scope, get_key.into())?;
    if !getter.is_undefined() {
        return None;
    }

    let set_key = v8::String::new(scope, "set")?;
    let setter = descriptor.get(scope, set_key.into())?;
    if !setter.is_undefined() {
        return None;
    }

    let writable_key = v8::String::new(scope, "writable")?;
    let writable = descriptor.get(scope, writable_key.into())?;
    if !writable.is_boolean() || writable.boolean_value(scope) {
        return None;
    }

    let configurable_key = v8::String::new(scope, "configurable")?;
    let configurable = descriptor.get(scope, configurable_key.into())?;
    if !configurable.is_boolean() || configurable.boolean_value(scope) {
        return None;
    }

    let value_key = v8::String::new(scope, "value")?;
    if !descriptor.has(scope, value_key.into())? {
        return None;
    }
    let descriptor_value = descriptor.get(scope, value_key.into())?;
    let current_value = obj.get(scope, key)?;
    if !current_value.same_value(descriptor_value) {
        return None;
    }

    Some(descriptor_value)
}

/// Convert a native string pointer to a Rust String
unsafe fn native_string_to_rust(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }

    // StringHeader layout: { utf16_len: u32, byte_len: u32, capacity: u32, refcount: u32, flags: u32, data: [u8] }
    #[repr(C)]
    struct StringHeader {
        _utf16_len: u32,
        byte_len: u32,
        _capacity: u32,
        _refcount: u32,
        _flags: u32,
    }

    let header = ptr as *const StringHeader;
    let length = (*header).byte_len as usize;
    let data_ptr = ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, length);

    String::from_utf8_lossy(bytes).to_string()
}

/// Convert a Rust string to a native string pointer
fn rust_string_to_native(s: &str) -> *const u8 {
    use perry_runtime::js_string_from_bytes;

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as *const u8
}

extern "C" fn native_promise_v8_resolve(
    closure: *const perry_runtime::closure::ClosureHeader,
    value: f64,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativePromiseResolve);
    let resolver_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as u64;
    crate::with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        if let Some(resolver) = take_native_promise_resolver(scope, resolver_id) {
            let v8_value = native_to_v8(scope, value);
            let _ = resolver.resolve(scope, v8_value);
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn native_promise_v8_reject(
    closure: *const perry_runtime::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativePromiseReject);
    let resolver_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as u64;
    crate::with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        if let Some(resolver) = take_native_promise_resolver(scope, resolver_id) {
            let v8_reason = native_to_v8(scope, reason);
            let _ = resolver.reject(scope, v8_reason);
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    f64::from_bits(TAG_UNDEFINED)
}

fn native_promise_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    promise: *mut perry_runtime::promise::Promise,
) -> v8::Local<'s, v8::Value> {
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        return v8::undefined(scope).into();
    };
    let v8_promise = resolver.get_promise(scope);
    match perry_runtime::promise::js_promise_state(promise) {
        1 => {
            bump_v8_entry(V8EntryKind::NativePromiseResolve);
            let value = perry_runtime::promise::js_promise_value(promise);
            let v8_value = native_to_v8(scope, value);
            let _ = resolver.resolve(scope, v8_value);
        }
        2 => {
            bump_v8_entry(V8EntryKind::NativePromiseReject);
            let reason = perry_runtime::promise::js_promise_reason(promise);
            let v8_reason = native_to_v8(scope, reason);
            let _ = resolver.reject(scope, v8_reason);
        }
        _ => {
            let resolver_id = store_native_promise_resolver(scope, resolver);
            let resolve_closure =
                perry_runtime::closure::js_closure_alloc(native_promise_v8_resolve as *const u8, 1);
            let reject_closure =
                perry_runtime::closure::js_closure_alloc(native_promise_v8_reject as *const u8, 1);
            perry_runtime::closure::js_closure_set_capture_f64(
                resolve_closure,
                0,
                resolver_id as f64,
            );
            perry_runtime::closure::js_closure_set_capture_f64(
                reject_closure,
                0,
                resolver_id as f64,
            );
            let _ =
                perry_runtime::promise::js_promise_then(promise, resolve_closure, reject_closure);
        }
    }
    v8_promise.into()
}

/// Probe whether a small handle id is a sqlite Database registered by
/// either `perry-stdlib::sqlite` or `perry-ext-better-sqlite3`. The
/// `extern "C"` symbol resolves at link time to whichever crate's
/// `js_sqlite_open` registered the handle. When neither crate is in
/// the build, `perry-stdlib::lib::js_sqlite_is_db_handle` provides a
/// 0-returning stub so this always links. Refs #1022.
fn is_sqlite_db_handle(handle_id: usize) -> bool {
    extern "C" {
        fn js_sqlite_is_db_handle(handle: i64) -> i32;
    }
    if handle_id == 0 {
        return false;
    }
    unsafe { js_sqlite_is_db_handle(handle_id as i64) != 0 }
}

/// Counterpart to `is_sqlite_db_handle` for the Statement side of the
/// proxy materialization. Refs #1022.
fn is_sqlite_stmt_handle(handle_id: usize) -> bool {
    extern "C" {
        fn js_sqlite_is_stmt_handle(handle: i64) -> i32;
    }
    if handle_id == 0 {
        return false;
    }
    unsafe { js_sqlite_is_stmt_handle(handle_id as i64) != 0 }
}

/// Look up the perry sqlite handle id stashed on a v8 Object proxy
/// during `materialize_sqlite_db_handle` / `materialize_sqlite_stmt_handle`.
/// Method trampolines call this to recover the receiver. Returns
/// `None` when called from an unbound `Function.prototype.call` site
/// (no `this`) or when `this` is some other object the user passed
/// through; the trampoline then returns `undefined`.
fn read_sqlite_handle_id_from_this(
    scope: &mut v8::PinScope<'_, '_>,
    this: v8::Local<v8::Object>,
) -> Option<i64> {
    let key = v8::String::new(scope, "__perry_sqlite_handle__")?;
    let val = this.get(scope, key.into())?;
    if val.is_external() {
        let ext = v8::Local::<v8::External>::try_from(val).ok()?;
        Some(ext.value() as i64)
    } else if val.is_number() || val.is_int32() {
        let n = val.integer_value(scope)?;
        Some(n)
    } else {
        None
    }
}

/// Convert v8 method args to a freshly-allocated perry ArrayHeader of
/// NaN-boxed values. Used by the sqlite stmt method trampolines to
/// build the `params_arr` ArrayHeader that `js_sqlite_stmt_run` /
/// `js_sqlite_stmt_get` / `js_sqlite_stmt_all` expect. The array is
/// arena-allocated; perry's GC will eventually sweep it.
fn build_native_array_from_v8_args(
    scope: &mut v8::PinScope<'_, '_>,
    args: &v8::FunctionCallbackArguments,
) -> *mut perry_runtime::array::ArrayHeader {
    let arr = perry_runtime::js_array_alloc(0);
    let mut current = arr;
    let count = args.length();
    for i in 0..count {
        let arg = args.get(i);
        let native = v8_to_native(scope, arg);
        current = perry_runtime::js_array_push(
            current,
            perry_runtime::JSValue::from_bits(native.to_bits()),
        );
    }
    current
}

/// Extract a perry StringHeader pointer from a v8 value. Allocates a
/// fresh native string if the input is a JS string; returns null if
/// not a string. The sqlite `prepare` / `exec` / `pragma` FFI shims
/// expect `*const StringHeader`.
fn v8_string_to_native_header(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> *const perry_runtime::StringHeader {
    if !value.is_string() {
        // Try toString — drizzle's sql.raw produces objects whose toString
        // returns the SQL. Defensive cast keeps the common path working
        // when the caller hands us a SqlChunk or similar.
        let s = match value.to_string(scope) {
            Some(s) => s,
            None => return std::ptr::null(),
        };
        let rs = s.to_rust_string_lossy(scope);
        return perry_runtime::js_string_from_bytes(rs.as_ptr(), rs.len() as u32);
    }
    let s = value.to_string(scope).unwrap();
    let rs = s.to_rust_string_lossy(scope);
    perry_runtime::js_string_from_bytes(rs.as_ptr(), rs.len() as u32)
}

// =====================================================================
// SQLite Database / Statement v8 proxies (refs #1022)
// =====================================================================
//
// Drizzle's BetterSQLiteSession is compiled as JS that runs under V8
// fallback. When user code in entry.ts does `const sqlite = new
// Database(":memory:"); const db = drizzle(sqlite);`, the `sqlite`
// handle (a small integer registered by `js_sqlite_open`) crosses the
// native→V8 boundary. Without explicit materialization it goes
// through `native_object_to_v8`'s small-handle branch and lands in
// `materialize_web_fetch_handle`, which doesn't know about sqlite and
// returns `v8::null`. Drizzle then does `this.client.prepare(query.sql)`
// in session.js and crashes with `Cannot read properties of null
// (reading 'prepare')`.
//
// The fix: synthesize a real v8 Object whose `prepare` / `exec` /
// `transaction` / `pragma` / `close` keys are v8 Functions that route
// back to the linked-in `js_sqlite_*` FFI shims. Each trampoline
// recovers the perry handle id from `this.__perry_sqlite_handle__`
// (a v8::External stashed at construction time) and calls the
// matching native function directly.
//
// Statement is mirrored: `prepare` returns a fresh statement-handle
// proxy whose `run` / `all` / `get` / `raw` / `iterate` keys are v8
// Functions over `js_sqlite_stmt_run` / `js_sqlite_stmt_all` /
// `js_sqlite_stmt_get` / `js_sqlite_stmt_raw`.
//
// `transaction` is deferred — better-sqlite3's `transaction(fn)`
// wrapper needs a v8::Function → perry closure adapter that doesn't
// exist yet for the call-into-native direction. drizzle's basic
// insert/select smoke test (entry.ts) doesn't exercise transactions,
// so the deferred coverage is fine for the #1022 close-out. Future
// work: bridge `js_sqlite_transaction` so wrapped JS callbacks BEGIN/
// COMMIT around their body. For now `transaction(fn)` returns a no-op
// callable so drizzle's `if (config.behavior)` chain doesn't crash.

fn sqlite_db_prepare_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::null(scope).into());
        return;
    };
    let sql_v8 = args.get(0);
    let sql_ptr = v8_string_to_native_header(scope, sql_v8);
    if sql_ptr.is_null() {
        retval.set(v8::null(scope).into());
        return;
    }
    extern "C" {
        fn js_sqlite_prepare(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i64;
    }
    let stmt_handle = unsafe { js_sqlite_prepare(handle, sql_ptr) };
    if stmt_handle < 0 {
        retval.set(v8::null(scope).into());
        return;
    }
    let v8_obj = materialize_sqlite_stmt_proxy(scope, stmt_handle);
    retval.set(v8_obj);
}

fn sqlite_db_exec_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::null(scope).into());
        return;
    };
    let sql_v8 = args.get(0);
    let sql_ptr = v8_string_to_native_header(scope, sql_v8);
    if sql_ptr.is_null() {
        retval.set(v8::null(scope).into());
        return;
    }
    extern "C" {
        fn js_sqlite_exec(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i32;
    }
    let _ = unsafe { js_sqlite_exec(handle, sql_ptr) };
    // better-sqlite3 returns the Database for chaining.
    retval.set(this.into());
}

fn sqlite_db_pragma_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let pragma_v8 = args.get(0);
    let pragma_ptr = v8_string_to_native_header(scope, pragma_v8);
    let value_v8 = args.get(1);
    let value_ptr = if value_v8.is_undefined() || value_v8.is_null() {
        std::ptr::null()
    } else {
        v8_string_to_native_header(scope, value_v8)
    };
    extern "C" {
        fn js_sqlite_pragma(
            db_handle: i64,
            pragma_ptr: *const perry_runtime::StringHeader,
            value_ptr: *const perry_runtime::StringHeader,
        ) -> *mut perry_runtime::StringHeader;
    }
    let result_ptr = unsafe { js_sqlite_pragma(handle, pragma_ptr, value_ptr) };
    if result_ptr.is_null() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let native_str_bits = STRING_TAG | (result_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_str_bits));
    retval.set(v);
}

fn sqlite_db_close_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    if let Some(handle) = read_sqlite_handle_id_from_this(scope, this) {
        extern "C" {
            fn js_sqlite_close(db_handle: i64) -> i32;
        }
        let _ = unsafe { js_sqlite_close(handle) };
    }
    retval.set(v8::undefined(scope).into());
}

fn sqlite_db_transaction_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub for #1022 close-out — drizzle's smoke test (entry.ts) doesn't
    // call into the transaction path. Returns a callable whose
    // `deferred` / `immediate` / `exclusive` properties return the
    // wrapped function unchanged, so drizzle's
    // `nativeTx[config.behavior ?? "deferred"](tx)` chain doesn't
    // crash when called. Real BEGIN/COMMIT lifecycle is deferred until
    // a v8→perry closure adapter ships (#TBD).
    let fn_arg = args.get(0);
    if !fn_arg.is_function() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let wrapper = v8::Object::new(scope);
    for behavior in ["deferred", "immediate", "exclusive"] {
        if let Some(k) = v8::String::new(scope, behavior) {
            wrapper.set(scope, k.into(), fn_arg);
        }
    }
    retval.set(wrapper.into());
}

fn sqlite_stmt_run_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_run(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::object::ObjectHeader;
    }
    let obj_ptr = unsafe { js_sqlite_stmt_run(handle, params_arr) };
    if obj_ptr.is_null() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let native_bits = POINTER_TAG | (obj_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

fn sqlite_stmt_all_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::array::ArrayHeader;
    }
    let arr_ptr = unsafe { js_sqlite_stmt_all(handle, params_arr) };
    if arr_ptr.is_null() {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    }
    let native_bits = POINTER_TAG | (arr_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

fn sqlite_stmt_get_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_get(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> f64;
    }
    let result_f64 = unsafe { js_sqlite_stmt_get(handle, params_arr) };
    let v = native_to_v8(scope, result_f64);
    retval.set(v);
}

fn sqlite_stmt_raw_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    // `stmt.raw()` returns `this` for chaining (`stmt.raw().all(...)`).
    // Flip the perry-side raw_mode flag so subsequent .all/.get return
    // arrays-of-arrays rather than arrays-of-objects.
    if let Some(handle) = read_sqlite_handle_id_from_this(scope, this) {
        extern "C" {
            fn js_sqlite_stmt_raw(stmt_handle: i64) -> i64;
        }
        let _ = unsafe { js_sqlite_stmt_raw(handle) };
    }
    retval.set(this.into());
}

fn sqlite_stmt_pluck_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub for drizzle — pluck() returns this for chaining. Drizzle
    // doesn't exercise pluck on the prepared-query path, but keeping
    // the method present prevents `stmt.pluck is not a function` if
    // a future codepath enables it. The actual pluck behavior
    // (return first column only) isn't bridged today.
    let _ = scope;
    let _ = args;
    let this = args.this();
    retval.set(this.into());
}

fn sqlite_stmt_columns_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub — returns an empty array. drizzle's PreparedQuery doesn't
    // call columns() on the smoke-test path; full bridging would need
    // a `js_sqlite_stmt_columns` FFI that returns an array of
    // `{name, column, table, database, type}` descriptors.
    let _ = args;
    let arr = v8::Array::new(scope, 0);
    retval.set(arr.into());
}

fn sqlite_stmt_iterate_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Backed by `stmt.all(...)` and wrapped in a JS array iterator. Not
    // a true streaming iterator (which would need a perry-side cursor
    // handle), but drizzle's only iterate consumer is for-await, which
    // works against an Array's `[Symbol.iterator]`.
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::array::ArrayHeader;
    }
    let arr_ptr = unsafe { js_sqlite_stmt_all(handle, params_arr) };
    if arr_ptr.is_null() {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    }
    let native_bits = POINTER_TAG | (arr_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

/// Attach the perry handle id to a v8 Object proxy. Stashed as a
/// v8::External under `__perry_sqlite_handle__` so the method
/// trampolines can recover it via `read_sqlite_handle_id_from_this`.
fn attach_sqlite_handle_id(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
    handle_id: i64,
) {
    let external = v8::External::new(scope, handle_id as *mut std::ffi::c_void);
    if let Some(k) = v8::String::new(scope, "__perry_sqlite_handle__") {
        obj.set(scope, k.into(), external.into());
    }
}

/// Attach a v8::Function (built from a callback) under `obj[name]`.
fn attach_method(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
    name: &str,
    cb: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    if let Some(func) = v8::Function::new(scope, cb) {
        if let Some(k) = v8::String::new(scope, name) {
            obj.set(scope, k.into(), func.into());
        }
    }
}

/// Materialize a v8::Object proxy for a perry sqlite Database handle.
/// `prepare` / `exec` / `transaction` / `pragma` / `close` are v8
/// Functions backed by the linked `js_sqlite_*` FFI shims. Refs #1022.
fn materialize_sqlite_db_proxy<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: i64,
) -> v8::Local<'s, v8::Value> {
    let obj = v8::Object::new(scope);
    attach_sqlite_handle_id(scope, obj, handle_id);
    attach_method(scope, obj, "prepare", sqlite_db_prepare_trampoline);
    attach_method(scope, obj, "exec", sqlite_db_exec_trampoline);
    attach_method(scope, obj, "pragma", sqlite_db_pragma_trampoline);
    attach_method(scope, obj, "close", sqlite_db_close_trampoline);
    attach_method(scope, obj, "transaction", sqlite_db_transaction_trampoline);
    obj.into()
}

/// Materialize a v8::Object proxy for a perry sqlite Statement handle.
/// `run` / `get` / `all` / `raw` / `iterate` / `pluck` / `columns`
/// are v8 Functions backed by the linked `js_sqlite_stmt_*` FFI
/// shims. Refs #1022.
fn materialize_sqlite_stmt_proxy<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: i64,
) -> v8::Local<'s, v8::Value> {
    let obj = v8::Object::new(scope);
    attach_sqlite_handle_id(scope, obj, handle_id);
    attach_method(scope, obj, "run", sqlite_stmt_run_trampoline);
    attach_method(scope, obj, "all", sqlite_stmt_all_trampoline);
    attach_method(scope, obj, "get", sqlite_stmt_get_trampoline);
    attach_method(scope, obj, "raw", sqlite_stmt_raw_trampoline);
    attach_method(scope, obj, "iterate", sqlite_stmt_iterate_trampoline);
    attach_method(scope, obj, "pluck", sqlite_stmt_pluck_trampoline);
    attach_method(scope, obj, "columns", sqlite_stmt_columns_trampoline);
    obj.into()
}

/// Materialize a snapshot v8 Object for a perry-stdlib Web Fetch handle
/// (Request / Response). Properties are extracted via the public dispatch
/// helpers in `perry_stdlib::fetch`. Headers/Blob ids return `v8::null`
/// for now — they expose methods, not scalar properties, and adding method
/// bridging requires a Proxy + HANDLE_METHOD_DISPATCH callback (future work).
fn materialize_web_fetch_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: usize,
) -> v8::Local<'s, v8::Value> {
    if handle_id == 0 {
        return v8::null(scope).into();
    }

    // sqlite Database / Statement proxies (refs #1022). Drizzle's
    // BetterSQLiteSession reads `this.client.prepare(query.sql)` /
    // `stmt.run(...)` from session.js running in V8 fallback. Without
    // these, the small handle id flows through to the unknown-id
    // fallback and surfaces as `v8::null`, then drizzle throws
    // `Cannot read properties of null (reading 'prepare')`. Detect
    // sqlite handles up front and synthesize a method-bearing proxy
    // before any other materializer runs.
    if is_sqlite_db_handle(handle_id) {
        return materialize_sqlite_db_proxy(scope, handle_id as i64);
    }
    if is_sqlite_stmt_handle(handle_id) {
        return materialize_sqlite_stmt_proxy(scope, handle_id as i64);
    }

    // Try Request first — read a probe property to confirm membership.
    if let Some(url_f64) = perry_stdlib::dispatch_request_property(handle_id, "url") {
        let obj = v8::Object::new(scope);
        let url_v8 = native_to_v8(scope, url_f64);
        if let Some(k) = v8::String::new(scope, "url") {
            obj.set(scope, k.into(), url_v8);
        }
        if let Some(method_f64) = perry_stdlib::dispatch_request_property(handle_id, "method") {
            let m = native_to_v8(scope, method_f64);
            if let Some(k) = v8::String::new(scope, "method") {
                obj.set(scope, k.into(), m);
            }
        }
        if let Some(body_f64) = perry_stdlib::dispatch_request_property(handle_id, "body") {
            let b = native_to_v8(scope, body_f64);
            if let Some(k) = v8::String::new(scope, "body") {
                obj.set(scope, k.into(), b);
            }
        }
        return obj.into();
    }

    // Then Response.
    if let Some(status_f64) = perry_stdlib::dispatch_response_property(handle_id, "status") {
        let obj = v8::Object::new(scope);
        let status_v8 = native_to_v8(scope, status_f64);
        if let Some(k) = v8::String::new(scope, "status") {
            obj.set(scope, k.into(), status_v8);
        }
        if let Some(st_f64) = perry_stdlib::dispatch_response_property(handle_id, "statusText") {
            let v = native_to_v8(scope, st_f64);
            if let Some(k) = v8::String::new(scope, "statusText") {
                obj.set(scope, k.into(), v);
            }
        }
        if let Some(ok_f64) = perry_stdlib::dispatch_response_property(handle_id, "ok") {
            let v = native_to_v8(scope, ok_f64);
            if let Some(k) = v8::String::new(scope, "ok") {
                obj.set(scope, k.into(), v);
            }
        }
        return obj.into();
    }

    // Unknown handle id — return null (safe fallback, no segfault).
    v8::null(scope).into()
}

/// Convert a native object pointer to a V8 object
fn native_object_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Value> {
    if ptr.is_null() {
        return v8::null(scope).into();
    }

    // perry-stdlib's Web Fetch handles (Request / Response / Headers / Blob)
    // arrive here NaN-boxed as POINTER_TAG values whose lower 48 bits hold a
    // small registry id (1, 2, 3, ...) instead of a real heap pointer (see
    // `perry_stdlib::fetch::handle_to_f64`). Mirror the perry-runtime side
    // small-handle threshold (`object.rs:4665`, `< 0x100000`): below that,
    // the value is a handle id, not a dereferenceable pointer. Without this
    // guard the `gc_header_ptr = ptr - 8` arithmetic below wraps to a huge
    // unsigned value, passes the `> 0x1000` bounds check, and segfaults when
    // we deref `gc_header` (the hono `app.fetch(req)` crash where `req` came
    // back from `new Request(...)` as `0x7FFD_0000_0000_0001`).
    //
    // For Request and Response we materialize a real v8 Object so V8-side code
    // (hono, sveltekit, etc.) can read `request.url` / `response.status` etc.
    // The synthesized object is a snapshot — methods like `req.text()` and
    // streaming semantics aren't bridged here yet (would require a Proxy that
    // calls back through HANDLE_METHOD_DISPATCH). For unknown small ids fall
    // through to `v8::null` rather than crashing.
    let ptr_usize = ptr as usize;
    if ptr_usize < 0x10_0000 {
        return materialize_web_fetch_handle(scope, ptr_usize);
    }

    // Issue (jose JWT blocker): Uint8Array / TypedArray pointers crossing
    // into V8 used to fall through to the generic `v8::Array` branch,
    // which turned a perry Uint8Array into a v8 Array. Libraries running
    // in the V8 fallback (jose, jsonwebtoken) check `instanceof Uint8Array`
    // on signing inputs/outputs and fail with "Received an instance of
    // Array". Detect typed-array pointers via the runtime's registry and
    // materialize a real v8 `Uint8Array` (or matching TypedArray) with a
    // copy of the underlying bytes so V8 owns the backing store.
    //
    // Two perry representations cross the boundary here:
    //   - `TypedArrayHeader` — `new Uint8Array([..])` and TypedArray ops.
    //   - `BufferHeader` marked via `mark_as_uint8array` — what
    //     `TextEncoder().encode(...)` and `Buffer.from(...)` return.
    //     Layout is identical (`length: u32, capacity: u32`) but the
    //     "kind" is implicit (always uint8) and tracked in a separate
    //     registry. Handle both before the generic-object branch.
    {
        let buf_addr = ptr as usize;
        // BufferHeader path: registered Uint8Array buffer with the
        // packed-u8 layout. Must materialize as v8 Uint8Array so jose's
        // `instanceof Uint8Array` checks pass.
        let is_buf = perry_runtime::buffer::is_registered_buffer(buf_addr);
        let is_marked_u8 = perry_runtime::buffer::is_uint8array_buffer(buf_addr);
        if is_buf || is_marked_u8 {
            let buf = ptr as *const perry_runtime::buffer::BufferHeader;
            let length = unsafe { (*buf).length } as usize;
            let data_ptr = unsafe {
                (ptr as *const u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>())
            };
            let ab = v8::ArrayBuffer::new(scope, length);
            if length > 0 {
                let bs = ab.get_backing_store();
                let dst = bs.data().map(|nn| nn.as_ptr() as *mut u8);
                if let Some(dst) = dst {
                    unsafe { std::ptr::copy_nonoverlapping(data_ptr, dst, length) };
                }
            }
            if let Some(ta) = v8::Uint8Array::new(scope, ab, 0, length) {
                return ta.into();
            }
        }
        if let Some(kind) = perry_runtime::typedarray::lookup_typed_array_kind(buf_addr) {
            let ta = ptr as *const perry_runtime::typedarray::TypedArrayHeader;
            let length = unsafe { (*ta).length } as usize;
            let elem_size = perry_runtime::typedarray::elem_size_for_kind(kind);
            let byte_len = length.saturating_mul(elem_size);
            let data_ptr = unsafe {
                (ptr as *const u8).add(std::mem::size_of::<
                    perry_runtime::typedarray::TypedArrayHeader,
                >())
            };
            // Build an ArrayBuffer owned by V8 and copy the perry bytes into it.
            // Using a copy (not a backing-store wrapper) keeps lifetimes simple:
            // perry's GC can reclaim the source without confusing V8.
            let ab = v8::ArrayBuffer::new(scope, byte_len);
            if byte_len > 0 {
                let bs = ab.get_backing_store();
                let dst = bs.data().map(|nn| nn.as_ptr() as *mut u8);
                if let Some(dst) = dst {
                    unsafe { std::ptr::copy_nonoverlapping(data_ptr, dst, byte_len) };
                }
            }
            // Element kind → V8 TypedArray constructor.
            use perry_runtime::typedarray as ta_mod;
            let ta_value: v8::Local<v8::Value> = match kind {
                ta_mod::KIND_INT8 => v8::Int8Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT8 | ta_mod::KIND_UINT8_CLAMPED => {
                    // V8 has Uint8ClampedArray as a separate type, but jose
                    // / jsonwebtoken only branch on `Uint8Array`. Use the
                    // plain Uint8Array unless we explicitly need clamped.
                    v8::Uint8Array::new(scope, ab, 0, length)
                        .map(|v| v.into())
                        .unwrap_or_else(|| v8::Array::new(scope, 0).into())
                }
                ta_mod::KIND_INT16 => v8::Int16Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT16 => v8::Uint16Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_INT32 => v8::Int32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT32 => v8::Uint32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_FLOAT32 => v8::Float32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_FLOAT64 => v8::Float64Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                _ => v8::Array::new(scope, 0).into(),
            };
            return ta_value;
        }
    }

    // Use GcHeader (8 bytes before user pointer) to reliably determine type.
    // All Perry arrays and objects are arena-allocated with GcHeader via arena_alloc_gc.
    let gc_header_ptr = (ptr as usize).wrapping_sub(perry_runtime::gc::GC_HEADER_SIZE);
    if gc_header_ptr > 0x1000 {
        let gc_header = unsafe { &*(gc_header_ptr as *const perry_runtime::gc::GcHeader) };
        let is_arena = (gc_header.gc_flags & perry_runtime::gc::GC_FLAG_ARENA) != 0;

        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_PROMISE {
            return native_promise_to_v8(scope, ptr as *mut perry_runtime::promise::Promise);
        }

        // Issue: Effect.pipe(map) chain — a Perry closure passed to V8 as
        // an arg (e.g. `Effect.map(fn)` where `fn` is a local arrow) lands
        // here with POINTER_TAG. Confirm the `CLOSURE_MAGIC` tag before
        // wrapping so we don't misidentify a generic native object as a
        // closure. The HIR-level `JsCreateCallback` rewrite handles inline
        // `Closure` literals; this is the LocalGet / FuncRef fallback
        // path.
        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_CLOSURE {
            const CLOSURE_TYPE_TAG_OFFSET: usize = 12;
            let type_tag = unsafe { *(ptr.add(CLOSURE_TYPE_TAG_OFFSET) as *const u32) };
            if type_tag == perry_runtime::closure::CLOSURE_MAGIC {
                if let Some(func_value) = native_closure_to_v8(scope, ptr) {
                    return func_value;
                }
            }
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_ARRAY {
            // GC-tracked array: ArrayHeader { length: u32, capacity: u32 } + f64 elements
            let header = ptr as *const perry_runtime::array::ArrayHeader;
            let length = unsafe { (*header).length };
            let elements_ptr = unsafe {
                ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
            };
            let v8_array = v8::Array::new(scope, length as i32);
            for i in 0..length {
                let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
                let v8_elem = native_to_v8(scope, elem_f64);
                v8_array.set_index(scope, i, v8_elem);
            }
            return v8_array.into();
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_OBJECT {
            // GC-tracked object: ObjectHeader (24 bytes) + field values
            let obj_header = ptr as *const perry_runtime::object::ObjectHeader;
            let field_count = unsafe { (*obj_header).field_count };
            let keys_array = unsafe { (*obj_header).keys_array };

            let v8_obj = v8::Object::new(scope);

            if !keys_array.is_null() && field_count > 0 {
                // Object has named keys - iterate and set each field
                let keys_length = unsafe { (*keys_array).length };
                let keys_elements_ptr = unsafe {
                    (keys_array as *const u8)
                        .add(std::mem::size_of::<perry_runtime::array::ArrayHeader>())
                        as *const f64
                };
                // Fields are stored as f64 (NaN-boxed JSValues) right after ObjectHeader
                let fields_ptr = unsafe {
                    ptr.add(std::mem::size_of::<perry_runtime::object::ObjectHeader>())
                        as *const f64
                };

                let count = std::cmp::min(field_count, keys_length);
                for i in 0..count {
                    // Get key string from keys_array. Keys may be heap strings or
                    // inline short strings, so route through the general V8 bridge.
                    let key_f64 = unsafe { *keys_elements_ptr.add(i as usize) };
                    let key_val = native_to_v8(scope, key_f64);
                    let v8_key = match key_val.to_string(scope) {
                        Some(k) => k,
                        None => continue,
                    };

                    // Get field value (NaN-boxed f64)
                    let field_f64 = unsafe { *fields_ptr.add(i as usize) };
                    let v8_val = native_to_v8(scope, field_f64);

                    v8_obj.set(scope, v8_key.into(), v8_val);
                }
            }

            return v8_obj.into();
        }
    }

    // Safety check: If the pointer looks like a StringHeader (length + capacity match,
    // and data after header is valid UTF-8), convert it as a string instead of an array.
    // This handles the case where a string pointer accidentally gets POINTER_TAG instead of STRING_TAG.
    {
        let str_header = ptr as *const perry_runtime::string::StringHeader;
        let str_len = unsafe { (*str_header).byte_len } as usize;
        let str_cap = unsafe { (*str_header).capacity } as usize;
        if str_len > 0 && str_len <= 100_000 && str_cap >= str_len && str_cap <= str_len + 64 {
            // Capacity is close to length — looks like a string, not an array
            // (Arrays typically have capacity much larger than needed due to growth)
            let data =
                unsafe { ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>()) };
            let bytes = unsafe { std::slice::from_raw_parts(data, str_len) };
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(v8_str) = v8::String::new(scope, s) {
                    return v8_str.into();
                }
            }
        }
    }

    // Fallback: heuristic array detection for non-arena allocations (Maps, etc.)
    let header = ptr as *const perry_runtime::array::ArrayHeader;
    let length = unsafe { (*header).length };
    let capacity = unsafe { (*header).capacity };
    if length <= 100_000 && capacity >= length && capacity <= 200_000 {
        let elements_ptr = unsafe {
            ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
        };
        let v8_array = v8::Array::new(scope, length as i32);
        for i in 0..length {
            let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
            let v8_elem = native_to_v8(scope, elem_f64);
            v8_array.set_index(scope, i, v8_elem);
        }
        return v8_array.into();
    }

    // Unknown type - wrap native pointer for opaque access
    let obj = v8::Object::new(scope);
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    obj.set(scope, key.into(), external.into());

    obj.into()
}

/// Convert a native BigInt pointer to a V8 BigInt
fn native_bigint_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Value> {
    use perry_runtime::bigint::BigIntHeader;

    if ptr.is_null() {
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    let header = ptr as *const BigIntHeader;
    let limbs = unsafe { (*header).limbs };

    // Check if the value fits in i64 (most common case)
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        // Fits in a single limb - check sign
        let val = limbs[0];
        if val <= i64::MAX as u64 {
            return v8::BigInt::new_from_i64(scope, val as i64).into();
        }
        // Value is positive but too large for i64, use u64
        return v8::BigInt::new_from_u64(scope, val).into();
    }

    // Check if it's a negative number (two's complement: high bit set in top limb)
    let is_negative = (limbs[3] >> 63) == 1;

    if is_negative {
        // Convert from two's complement to magnitude
        let mut magnitude = limbs;
        // Subtract 1 and invert
        let mut borrow = 1u64;
        for limb in magnitude.iter_mut() {
            let (result, underflow) = limb.overflowing_sub(borrow);
            *limb = !result;
            borrow = if underflow { 1 } else { 0 };
        }
        // Find the actual word count (trim trailing zeros)
        let word_count = magnitude
            .iter()
            .rposition(|&x| x != 0)
            .map(|i| i + 1)
            .unwrap_or(1);
        v8::BigInt::new_from_words(scope, true, &magnitude[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    } else {
        // Positive number with multiple limbs
        // Find the actual word count (trim trailing zeros)
        let word_count = limbs
            .iter()
            .rposition(|&x| x != 0)
            .map(|i| i + 1)
            .unwrap_or(1);
        v8::BigInt::new_from_words(scope, false, &limbs[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    }
}

/// Convert a V8 object to a native object pointer
fn v8_object_to_native(scope: &mut v8::PinScope<'_, '_>, obj: v8::Local<v8::Object>) -> *mut u8 {
    use perry_runtime::{js_object_alloc, js_object_set_field};

    // Check if this object has a native pointer already
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    if let Some(val) = obj.get(scope, key.into()) {
        if val.is_external() {
            let external = v8::Local::<v8::External>::try_from(val).unwrap();
            return external.value() as *mut u8;
        }
    }

    // Get all own property names
    let names = obj
        .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
        .unwrap_or_else(|| v8::Array::new(scope, 0));

    let field_count = names.length();

    // Allocate native object
    let native_obj = js_object_alloc(0, field_count);

    // Set fields (keys handling is simplified for now)
    for i in 0..field_count {
        let key_val = names.get_index(scope, i).unwrap();

        // Get and convert the value
        if let Some(val) = obj.get(scope, key_val) {
            let native_val = v8_to_native(scope, val);
            // Convert f64 bits to JSValue
            let jsval = JSValue::from_bits(native_val.to_bits());
            js_object_set_field(native_obj, i, jsval);
        }
    }

    native_obj as *mut u8
}

/// Convert a V8 array to a native array pointer
fn v8_array_to_native(scope: &mut v8::PinScope<'_, '_>, array: v8::Local<v8::Array>) -> *mut u8 {
    use perry_runtime::js_array_alloc;

    let length = array.length();

    // Allocate native array
    let native_array = js_array_alloc(length);
    unsafe {
        (*native_array).length = length;
    }

    // Convert each element
    // We use js_array_set_f64 which takes the raw f64 bits
    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = v8_to_native(scope, val);
            unsafe {
                // Set the value directly using pointer arithmetic
                // ArrayHeader is { length: u32, capacity: u32 } = 8 bytes
                // Followed by array of f64 values
                let data_ptr = (native_array as *mut u8).add(8) as *mut f64;
                *data_ptr.add(i as usize) = native_val;
            }
        }
    }

    native_array as *mut u8
}

fn v8_array_to_native_metadata(
    scope: &mut v8::PinScope<'_, '_>,
    array: v8::Local<v8::Array>,
) -> *mut u8 {
    use perry_runtime::js_array_alloc;

    let length = array.length();
    let native_array = js_array_alloc(length);
    unsafe {
        (*native_array).length = length;
    }

    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = v8_to_native_metadata_value(scope, val);
            unsafe {
                let data_ptr = (native_array as *mut u8).add(8) as *mut f64;
                *data_ptr.add(i as usize) = native_val;
            }
        }
    }

    native_array as *mut u8
}

/// Convert a V8 BigInt to a native BigInt pointer
fn v8_bigint_to_native(
    _scope: &mut v8::PinScope<'_, '_>,
    bigint: v8::Local<v8::BigInt>,
) -> *mut u8 {
    use perry_runtime::bigint::BigIntHeader;
    use std::alloc::{alloc, Layout};

    // Get the word count to determine the size needed
    let word_count = bigint.word_count();

    // Allocate a BigIntHeader (4 x u64 = 256 bits)
    let layout = Layout::new::<BigIntHeader>();
    let ptr = unsafe { alloc(layout) as *mut BigIntHeader };
    if ptr.is_null() {
        panic!("Failed to allocate BigInt");
    }

    use perry_runtime::bigint::BIGINT_LIMBS;

    if word_count == 0 {
        // Zero value
        unsafe {
            (*ptr).limbs = [0; BIGINT_LIMBS];
        }
        return ptr as *mut u8;
    }

    // Get the words from V8 BigInt
    let mut words = vec![0u64; word_count];
    let (sign_bit, _) = bigint.to_words_array(&mut words);

    // Copy words to our BigIntHeader (up to BIGINT_LIMBS limbs)
    unsafe {
        let mut limbs = [0u64; BIGINT_LIMBS];
        for (i, &word) in words.iter().enumerate().take(BIGINT_LIMBS) {
            limbs[i] = word;
        }

        // Handle negative numbers (two's complement)
        if sign_bit {
            // Negate: invert all bits and add 1
            for limb in limbs.iter_mut() {
                *limb = !*limb;
            }
            // Add 1
            let mut carry = 1u64;
            for limb in limbs.iter_mut() {
                let (result, overflow) = limb.overflowing_add(carry);
                *limb = result;
                carry = if overflow { 1 } else { 0 };
            }
        }

        (*ptr).limbs = limbs;
    }

    ptr as *mut u8
}

/// Convert a native array pointer to a V8 array
pub fn native_array_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Array> {
    if ptr.is_null() {
        return v8::Array::new(scope, 0);
    }

    // ArrayHeader layout: { length: u32, capacity: u32 }
    #[repr(C)]
    struct ArrayHeader {
        length: u32,
        _capacity: u32,
    }

    let header = ptr as *const ArrayHeader;
    let length = unsafe { (*header).length };

    let array = v8::Array::new(scope, length as i32);

    for i in 0..length {
        // Read the f64 value directly from the array data
        let native_val = unsafe {
            let data_ptr = (ptr as *const u8).add(8) as *const f64;
            *data_ptr.add(i as usize)
        };
        let v8_val = native_to_v8(scope, native_val);
        array.set_index(scope, i, v8_val);
    }

    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_constants() {
        // Verify our tag constants match expected values
        assert_eq!(TAG_UNDEFINED, 0x7FFC_0000_0000_0001);
        assert_eq!(TAG_NULL, 0x7FFC_0000_0000_0002);
        assert_eq!(TAG_FALSE, 0x7FFC_0000_0000_0003);
        assert_eq!(TAG_TRUE, 0x7FFC_0000_0000_0004);
    }
}
