use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

/// Returns true if `class_id` corresponds to a registered class. Used by
/// `js_value_typeof` (refs #618 / #420 followup) to distinguish a class
/// reference (NaN-boxed INT32 with class_id payload) from a regular int32
/// numeric value — JS spec says `typeof <class>` is "function", but
/// Perry's INT32_TAG storage shape is shared with numeric int32, so the
/// runtime needs an explicit registry check. Consults both
/// REGISTERED_CLASS_IDS (every class) and CLASS_VTABLE_REGISTRY (classes
/// with methods) so even classes registered before the explicit-id call
/// runs still detect via the vtable.
pub fn is_class_id_registered(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = REGISTERED_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            if set.contains(&class_id) {
                return true;
            }
        }
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .map(|m| m.contains_key(&class_id))
        .unwrap_or(false)
}

/// Register a class method in the vtable registry.
/// Called at startup from the init function for every class method/getter.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
    has_synthetic_arguments: i64,
    has_rest: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.methods.insert(
        name,
        VTableMethodEntry {
            func_ptr: func_ptr as usize,
            param_count: param_count as u32,
            has_synthetic_arguments: has_synthetic_arguments != 0,
            has_rest: has_rest != 0,
        },
    );
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Own (non-inherited) instance accessor func_ptrs for `class_id` + `name`:
/// `(getter_ptr, setter_ptr)`, each 0 when that half is absent. Consulted by
/// `Object.getOwnPropertyDescriptor(C.prototype, name)`.
pub(crate) fn class_own_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let guard = CLASS_VTABLE_REGISTRY.read().ok()?;
    let reg = guard.as_ref()?;
    let vt = reg.get(&class_id)?;
    let g = vt.getters.get(name).copied().unwrap_or(0);
    let s = vt.setters.get(name).copied().unwrap_or(0);
    if g == 0 && s == 0 {
        None
    } else {
        Some((g, s))
    }
}

/// Own static accessor func_ptrs for the class *constructor*. Mirrors
/// `class_own_accessor_ptrs` against `CLASS_STATIC_ACCESSORS`.
pub(crate) fn class_own_static_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let guard = CLASS_STATIC_ACCESSORS.read().ok()?;
    let reg = guard.as_ref()?;
    let pair = reg.get(&class_id)?.get(name).copied()?;
    if pair.0 == 0 && pair.1 == 0 {
        None
    } else {
        Some(pair)
    }
}

/// Trampoline giving a raw vtable getter func_ptr (`fn(this) -> f64`) the
/// closure calling convention. The receiver comes from `IMPLICIT_THIS`, set
/// by the method-call dispatch the closure value travels through.
extern "C" fn class_accessor_getter_thunk(closure: *const crate::closure::ClosureHeader) -> f64 {
    let raw = unsafe { crate::closure::js_closure_get_capture_ptr(closure, 0) } as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = crate::object::js_implicit_this_get();
    let f: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(raw) };
    f(this)
}

/// Trampoline for a raw vtable setter func_ptr (`fn(this, value) -> f64`).
extern "C" fn class_accessor_setter_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let raw = unsafe { crate::closure::js_closure_get_capture_ptr(closure, 0) } as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = crate::object::js_implicit_this_get();
    let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(raw) };
    f(this, value)
}

/// Wrap a raw class accessor func_ptr as a callable function VALUE for
/// descriptor reflection (`Object.getOwnPropertyDescriptor(C.prototype,
/// "x").get`). Built-in-shaped: `.length` 0/1, no `.prototype`, native
/// `toString` form. `prop_name` is the accessor's property key — the spec
/// `.name` of a `get`/`set` accessor is the key prefixed with `"get "`/`"set "`
/// (Function Definitions: SetFunctionName with the "get"/"set" prefix), e.g.
/// `Object.getOwnPropertyDescriptor(C.prototype, "x").get.name === "get x"`.
pub(crate) fn class_accessor_function_value(
    raw_ptr: usize,
    is_setter: bool,
    prop_name: &str,
) -> f64 {
    if raw_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let thunk = if is_setter {
        class_accessor_setter_thunk as *const u8
    } else {
        class_accessor_getter_thunk as *const u8
    };
    let closure = crate::closure::js_closure_alloc(thunk, 1);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe { crate::closure::js_closure_set_capture_ptr(closure, 0, raw_ptr as i64) };
    // Spec `.length`: params before the first default/rest. A getter takes no
    // params (0); a setter takes exactly one formal param — but `set m(x = 42)`
    // has `.length === 0` (defaults don't count). Codegen registers the raw
    // accessor func_ptr's default-aware spec length via
    // `js_register_closure_length`; consult it so a defaulted setter reports 0.
    // Fall back to the fixed 1/0 when no registration exists (e.g. native or
    // cross-module accessors whose length wasn't emitted).
    let spec_length = crate::closure::lookup_closure_length(raw_ptr as *const u8)
        .unwrap_or(if is_setter { 1 } else { 0 });
    super::super::native_module::set_builtin_closure_length(closure as usize, spec_length);
    super::super::native_module::set_builtin_closure_non_constructable(closure as usize);
    // Spec `.name` = "get <key>" / "set <key>" with attributes
    // { writable: false, enumerable: false, configurable: true } (mirrors the
    // `Function.prototype.bind` name path). Without this the reflected accessor
    // value's `.name` defaulted to "" — refs class/.../fn-name-accessor-{get,set}.
    let prefix = if is_setter { "set " } else { "get " };
    let fn_name = format!("{prefix}{prop_name}");
    let name_ptr = crate::string::js_string_from_bytes(fn_name.as_ptr(), fn_name.len() as u32);
    let name_value = f64::from_bits(crate::value::JSValue::string_ptr(name_ptr).bits());
    unsafe {
        crate::closure::closure_set_dynamic_prop(closure as usize, "name", name_value);
    }
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
    crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Register a class getter in the vtable registry.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a class setter in the vtable registry.
///
/// Refs #486 (hono): hono's Context has `set res(_res) { ...; this.#res = _res;
/// this.finalized = true; }`. Without setter dispatch in `js_object_set_field_by_name`,
/// `c.res = response` from inside compose's `await handler(c, next)` chain stored
/// the response into a regular field slot but never ran the setter body — so
/// `this.finalized = true` never executed, `c.finalized` stayed false, and
/// hono-base's `if (!context.finalized) throw …` fired.
///
/// Setter signature: `fn(this_f64, value_f64) -> f64` (returns ignored, but
/// codegen emits a return so the LLVM signature matches a regular method body).
#[no_mangle]
pub unsafe extern "C" fn js_register_class_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.setters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a `static get name()` accessor on the class *constructor*
/// (`CLASS_STATIC_ACCESSORS`), not the instance vtable — a static accessor is
/// an own property of `C`, reachable via `C.name` / `C[name]`, and must NOT
/// appear on `C.prototype` or instances. The read/write dispatch already
/// consults `CLASS_STATIC_ACCESSORS` (`class_static_accessor_getter_value` /
/// `class_static_accessor_setter_apply`); this populates it.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    register_class_static_accessor_half(class_id, name_ptr, name_len, func_ptr, true);
}

/// Register a `static set name(v)` accessor. See `js_register_class_static_getter`.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    register_class_static_accessor_half(class_id, name_ptr, name_len, func_ptr, false);
}

// These two are only ever called from codegen-emitted module-init IR (no Rust
// caller), so the auto-optimize whole-program-LLVM build would dead-strip them
// without an anchor. Pin each via a `#[used]` static (mirrors node_v8.rs).
#[used]
static KEEP_REGISTER_STATIC_GETTER: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_getter;
#[used]
static KEEP_REGISTER_STATIC_SETTER: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_setter;

/// Record the spec `.length` (params before the first default/rest) for a class
/// method or accessor. Codegen emits one call per method at module init.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method_bind_length(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    length: i64,
) {
    if name_ptr.is_null() || name_len < 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = match CLASS_METHOD_BIND_LENGTHS.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert((class_id as u32, name), length as u32);
}

#[used]
static KEEP_REGISTER_METHOD_BIND_LENGTH: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_method_bind_length;

/// Record the spec `.length` for a STATIC method (params before the first
/// default/rest). Codegen emits one call per static method at module init.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_method_bind_length(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    length: i64,
) {
    if name_ptr.is_null() || name_len < 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = match CLASS_STATIC_METHOD_BIND_LENGTHS.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert((class_id as u32, name), length as u32);
}

#[used]
static KEEP_REGISTER_STATIC_METHOD_BIND_LENGTH: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_method_bind_length;

unsafe fn register_class_static_accessor_half(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    is_getter: bool,
) {
    // Empty-string keys (`static get ''()`) are legal — admit `name_len == 0`
    // as long as the pointer is non-null.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut guard = CLASS_STATIC_ACCESSORS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let entry = guard
        .as_mut()
        .unwrap()
        .entry(class_id as u32)
        .or_default()
        .entry(name)
        .or_insert((0, 0));
    if is_getter {
        entry.0 = func_ptr as usize;
    } else {
        entry.1 = func_ptr as usize;
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}
