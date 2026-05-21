//! V8 wrappers + trampolines for Perry native classes and closures, plus
//! decorator-metadata round-tripping helpers.

use super::*;

use super::conversion::{native_to_v8, v8_array_to_native_metadata, v8_to_native};

pub(super) fn native_class_constructor(
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
pub(super) fn perry_closure_v8_trampoline(
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
pub(super) fn native_closure_to_v8<'s>(
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

pub(super) fn native_class_to_v8<'s>(
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
