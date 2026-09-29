use super::*;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

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

pub(crate) fn record_class_string_member_order(
    class_id: u32,
    name: String,
    is_static: bool,
    definition_order: u32,
) {
    if class_id == 0 {
        return;
    }
    let mut guard = match CLASS_STRING_MEMBER_ORDERS.write() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry((class_id, is_static, name))
        // A later getter/setter or duplicate method redefines the existing
        // property without moving it in [[OwnPropertyKeys]].
        .and_modify(|order| *order = (*order).min(definition_order))
        .or_insert(definition_order);
}

/// A configurable class element that is deleted and later recreated is a new
/// property and must move to the end of its string-key partition. Keep the
/// dispatch registry entry for fast calls, but retire its declaration-order
/// position until a future class evaluation explicitly registers it again.
pub(crate) fn invalidate_class_string_member_order(class_id: u32, name: &str, is_static: bool) {
    if class_id == 0 {
        return;
    }
    let mut guard = match CLASS_STRING_MEMBER_ORDERS.write() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert((class_id, is_static, name.to_string()), u32::MAX);
}

pub(crate) unsafe fn record_class_symbol_member_order(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
    definition_order: u32,
) {
    let symbol_id = (*(sym_key as *const crate::symbol::SymbolHeader)).id;
    CLASS_SYMBOL_MEMBER_ORDERS.with(|orders| {
        let mut guard = orders.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard
            .as_mut()
            .unwrap()
            .entry((class_id, symbol_id, is_static))
            .and_modify(|order| *order = (*order).min(definition_order))
            .or_insert(definition_order);
    });
}

/// Record the ClassBody position of a non-computed string-keyed method or
/// accessor. Codegen emits this beside the existing dispatch registration;
/// computed keys record the same metadata after runtime ToPropertyKey.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_string_member_order(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    is_static: i64,
    definition_order: i64,
) {
    if class_id <= 0 || name_ptr.is_null() || name_len < 0 {
        return;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize))
    else {
        return;
    };
    record_class_string_member_order(
        class_id as u32,
        name.to_string(),
        is_static != 0,
        definition_order as u32,
    );
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_CLASS_STRING_MEMBER_ORDER: unsafe extern "C" fn(
    i64,
    *const u8,
    i64,
    i64,
    i64,
) = js_register_class_string_member_order;

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
        *registry = Some(crate::fast_hash::new_ptr_hash_map());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_default();
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

/// The ClassBody's own public instance accessor declaration for `class_id` +
/// `name`: `(getter_ptr, setter_ptr)`, each 0 when that half is absent.
///
/// Class metadata: read by the decl-prototype installer (`decl_accessors.rs`)
/// and by "does this class declare an accessor `name`" filters. A property
/// read, write or reflection must not answer from it — the prototype's real
/// accessor property is the truth, and `defineProperty` / `delete` may have
/// changed it.
pub(crate) fn class_own_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let guard = CLASS_VTABLE_REGISTRY.read().ok()?;
    let decl = guard.as_ref()?.get(&class_id)?.accessor_decl(name)?;
    (decl.get != 0 || decl.set != 0).then_some((decl.get, decl.set))
}

/// The compiled entries of the ClassBody static accessor `name` of `class_id`
/// as REGISTERED (`CLASS_STATIC_ACCESSORS`): the input the class function
/// object's accessor property is built from, not the property itself — a
/// deleted or redefined accessor is still registered. Private (`#x`) static
/// accessors live only here.
pub(crate) fn class_registered_static_accessor_ptrs(
    class_id: u32,
    name: &str,
) -> Option<(usize, usize)> {
    let guard = CLASS_STATIC_ACCESSORS.read().ok()?;
    let reg = guard.as_ref()?;
    let decl = reg.get(&class_id)?.get(name).copied()?;
    (decl.get != 0 || decl.set != 0).then_some((decl.get, decl.set))
}

/// The recorded spec `.length` of `class_id`'s own setter `name` (static when
/// `is_static`), for [`class_accessor_function_value`].
pub(crate) fn class_own_setter_length(class_id: u32, name: &str, is_static: bool) -> Option<u32> {
    if is_static {
        let guard = CLASS_STATIC_ACCESSORS.read().ok()?;
        guard.as_ref()?.get(&class_id)?.get(name)?.set_length
    } else {
        let guard = CLASS_VTABLE_REGISTRY.read().ok()?;
        guard
            .as_ref()?
            .get(&class_id)?
            .accessor_decl(name)?
            .set_length
    }
}

/// The accessor trampolines' infos: every reflected accessor value shares one
/// per body, so [`class_accessor_source_func_ptr`]'s code compare holds.
static CLASS_ACCESSOR_GETTER_THUNK_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        class_accessor_getter_thunk as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
    );
static CLASS_ACCESSOR_SETTER_THUNK_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        class_accessor_setter_thunk as crate::codegen_abi::JsBody1<crate::closure::ClosureHeader>,
    );

/// Trampoline giving a raw vtable getter func_ptr (`fn(this) -> f64`) the
/// closure calling convention. The receiver is the `this` argument passed
/// by the method-call dispatch the closure value travels through.
extern "C" fn class_accessor_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let raw = crate::closure::js_closure_get_capture_ptr(closure, 0) as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = this.as_f64();
    let f = unsafe { crate::closure::body_call::js_method_body_fn!(raw as *const u8;) };
    f(this)
}

/// Trampoline for a raw vtable setter func_ptr (`fn(this, value) -> f64`).
extern "C" fn class_accessor_setter_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let raw = crate::closure::js_closure_get_capture_ptr(closure, 0) as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = this.as_f64();
    let f = unsafe { crate::closure::body_call::js_method_body_fn!(raw as *const u8; a0) };
    f(this, value)
}

/// Return the raw getter/setter body wrapped by a descriptor-reflection
/// accessor closure. The wrapper has its own calling-convention thunk, while
/// Function.prototype.toString must consult the original MethodDefinition.
pub(crate) unsafe fn class_accessor_source_func_ptr(
    closure: *const crate::closure::ClosureHeader,
) -> Option<usize> {
    if closure.is_null() || crate::closure::real_capture_count((*closure).capture_count) < 1 {
        return None;
    }
    let thunk = (*closure).code();
    if thunk != class_accessor_getter_thunk as *const u8
        && thunk != class_accessor_setter_thunk as *const u8
    {
        return None;
    }
    let raw = crate::closure::js_closure_get_capture_ptr(closure, 0) as usize;
    (raw != 0).then_some(raw)
}

/// Wrap a raw class accessor func_ptr as a callable function VALUE for
/// descriptor reflection (`Object.getOwnPropertyDescriptor(C.prototype,
/// "x").get`). Built-in-shaped: `.length` 0/1, no `.prototype`, native
/// `toString` form. `prop_name` is the accessor's property key — the spec
/// `.name` of a `get`/`set` accessor is the key prefixed with `"get "`/`"set "`
/// (Function Definitions: SetFunctionName with the "get"/"set" prefix), e.g.
/// `Object.getOwnPropertyDescriptor(C.prototype, "x").get.name === "get x"`.
///
/// `setter_length` is the setter's spec `.length` as the class accessor table
/// recorded it ([`AccessorDecl::set_length`]); ignored for a getter.
pub(crate) fn class_accessor_function_value(
    raw_ptr: usize,
    is_setter: bool,
    prop_name: &str,
    setter_length: Option<u32>,
) -> f64 {
    if raw_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let thunk = if is_setter {
        &CLASS_ACCESSOR_SETTER_THUNK_INFO
    } else {
        &CLASS_ACCESSOR_GETTER_THUNK_INFO
    };
    let closure = crate::closure::js_closure_alloc(thunk, 1);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_closure_set_capture_ptr(closure, 0, raw_ptr as i64);
    // Spec `.length`: params before the first default/rest. A getter takes no
    // params (0); a setter takes exactly one formal param — but `set m(x = 42)`
    // has `.length === 0` (defaults don't count). Codegen records the setter's
    // default-aware spec length with the setter in the class accessor table
    // (`js_register_class_setter`'s `spec_length`). Fall back to the fixed 1
    // when none was recorded (e.g. a `defineProperty`-installed half).
    let spec_length = if is_setter {
        setter_length.unwrap_or(1)
    } else {
        0
    };
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
    crate::closure::closure_set_dynamic_prop(closure as usize, "name", name_value);
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
        *registry = Some(crate::fast_hash::new_ptr_hash_map());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_default();
    vtable.declare_accessor_half(&name, func_ptr as usize, false);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
    super::verdict_classes::note_verdict_class_accessor_change(class_id as u32);
    drop(registry);
    super::decl_accessors::note_instance_accessor_registered(class_id as u32, &name);
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
///
/// `spec_length` is the setter's spec `.length` (0 for `set m(x = 1)`), kept
/// with the setter for descriptor reflection.
pub unsafe extern "C" fn js_register_class_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    spec_length: i32,
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
        *registry = Some(crate::fast_hash::new_ptr_hash_map());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_default();
    vtable.declare_accessor_half_with_length(
        &name,
        func_ptr as usize,
        true,
        u32::try_from(spec_length).ok(),
    );
    VTABLE_GEN.fetch_add(1, Ordering::Release);
    super::verdict_classes::note_verdict_class_accessor_change(class_id as u32);
    drop(registry);
    super::decl_accessors::note_instance_accessor_registered(class_id as u32, &name);
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
    register_class_static_accessor_half(class_id, name_ptr, name_len, func_ptr, true, None);
}

/// Register a `static set name(v)` accessor. See `js_register_class_static_getter`.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    spec_length: i32,
) {
    register_class_static_accessor_half(
        class_id,
        name_ptr,
        name_len,
        func_ptr,
        false,
        u32::try_from(spec_length).ok(),
    );
}

// These two are only ever called from codegen-emitted module-init IR (no Rust
// caller), so the auto-optimize whole-program-LLVM build would dead-strip them
// without an anchor. Pin each via a `#[used]` static (mirrors node_v8.rs).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_STATIC_GETTER: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_getter;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_STATIC_SETTER: unsafe extern "C" fn(i64, *const u8, i64, i64, i32) =
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

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
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

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_STATIC_METHOD_BIND_LENGTH: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_method_bind_length;

unsafe fn register_class_static_accessor_half(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    is_getter: bool,
    set_length: Option<u32>,
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
    {
        let mut guard = CLASS_STATIC_ACCESSORS.write().unwrap();
        if guard.is_none() {
            *guard = Some(crate::fast_hash::new_ptr_hash_map());
        }
        let entry = guard
            .as_mut()
            .unwrap()
            .entry(class_id as u32)
            .or_default()
            .entry(name.clone())
            .or_default();
        if is_getter {
            entry.get = func_ptr as usize;
        } else {
            entry.set = func_ptr as usize;
            entry.set_length = set_length;
        }
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
    crate::object::class_value::note_intrinsic_registration(class_id as u32, &name);
}
