//! The hidden per-object JS state of a payload-family instance, and the
//! own accessors a family installs on its instances.

use super::*;

/// The hidden own property holding a family's JS-side state.
pub(crate) const JS_STATE_KEY: &[u8] = b"#<perry:native-payload-js-state>";

/// The per-object JS state object of an instance: an ordinary object stored
/// under a hidden own key (invisible to `Object.keys`, `for…in`,
/// `getOwnPropertyNames`, `JSON.stringify`), so every JS value a family keeps
/// for an instance is a traced field that moves and dies with it. Created on
/// first use when `create` is true; `undefined` otherwise when absent.
pub fn js_state(value: f64, family: &NativePayloadFamily, create: bool) -> f64 {
    js_state_for_class(value, family.class_id, create)
}

pub(super) fn js_state_for_class(value: f64, class_id: u32, create: bool) -> f64 {
    let undefined = undefined();
    let Some(obj) = instance_of(value, class_id) else {
        return undefined;
    };
    if let Some(state) = unsafe { raw_js_state(obj) } {
        return crate::value::js_nanbox_pointer(state as i64);
    }
    if !create {
        return undefined;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let existing = state_field_memo(&obj, JS_STATE_KEY, &JS_STATE_MEMO);
    if crate::value::JSValue::from_bits(existing.to_bits()).is_pointer() || !create {
        return existing;
    }
    let state = crate::object::js_object_alloc(0, 6);
    if state.is_null() {
        return undefined;
    }
    let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    set_state_field(&scope, &obj, JS_STATE_KEY, state.get_nanbox_f64());
    state.get_nanbox_f64()
}

/// `obj[key] = value` for a runtime-owned ASCII key, with the key rooted
/// across the store (the store may allocate a shape transition).
pub(super) fn set_own(
    scope: &crate::gc::RuntimeHandleScope,
    obj: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: f64,
) {
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(key));
    key.with_const_ptr::<crate::StringHeader, _>(|key| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            crate::object::js_object_set_field_by_name(obj, key, value.get_nanbox_f64())
        })
    });
}

/// Read the field `key` of an instance's JS state (`undefined` when the
/// instance has no state or no such field). Never runs JS.
pub fn state_get(value: f64, family: &NativePayloadFamily, key: &[u8]) -> f64 {
    let state = js_state(value, family, false);
    if !crate::value::JSValue::from_bits(state.to_bits()).is_pointer() {
        return undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let state = root_pointer::<ObjectHeader>(&scope, state);
    state_field(&state, key)
}

/// [`state_get`] through a per-site [`StateKeyMemo`] (hot keys).
pub fn state_get_memo(
    value: f64,
    family: &NativePayloadFamily,
    key: &[u8],
    memo: &'static StateKeyMemo,
) -> f64 {
    let Some(obj) = instance_of(value, family.class_id) else {
        return undefined();
    };
    unsafe { raw_js_state(obj).map_or_else(undefined, |state| raw_field_memo(state, key, memo)) }
}

/// [`state_set`] through a per-site [`StateKeyMemo`] (hot keys).
pub fn state_set_memo(
    value: f64,
    family: &NativePayloadFamily,
    key: &[u8],
    v: f64,
    memo: &'static StateKeyMemo,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let v = scope.root_nanbox_f64(v);
    let owner = scope.root_nanbox_f64(value);
    let state = js_state(owner.get_nanbox_f64(), family, true);
    if !crate::value::JSValue::from_bits(state.to_bits()).is_pointer() {
        return;
    }
    let state = root_pointer::<ObjectHeader>(&scope, state);
    let slot = state
        .with_mut_ptr::<ObjectHeader, _>(|obj| unsafe { state_key_index_memo(obj, key, memo) });
    match slot {
        Some(i) => state.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
            state_slot_set(obj, i, v.get_nanbox_f64())
        }),
        None => set_state_field(&scope, &state, key, v.get_nanbox_f64()),
    }
    sync_callbacks(owner.get_nanbox_f64(), family, key, v.get_nanbox_f64());
}

/// Store `v` as the field `key` of an instance's JS state (created on
/// demand). An own data slot is defined directly; no setter can run.
pub fn state_set(value: f64, family: &NativePayloadFamily, key: &[u8], v: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let v = scope.root_nanbox_f64(v);
    let owner = scope.root_nanbox_f64(value);
    let state = js_state(owner.get_nanbox_f64(), family, true);
    if !crate::value::JSValue::from_bits(state.to_bits()).is_pointer() {
        return;
    }
    let state = root_pointer::<ObjectHeader>(&scope, state);
    set_state_field(&scope, &state, key, v.get_nanbox_f64());
    sync_callbacks(owner.get_nanbox_f64(), family, key, v.get_nanbox_f64());
}

/// Define an own accessor on an instance, for families whose node objects
/// carry own (not prototype) getters such as `db.isOpen`. The getter and
/// setter are per-realm singleton closures of `get` / `set`.
pub fn define_own_accessor(
    value: f64,
    name: &str,
    get: *const crate::closure::JsFunctionInfo,
    set: Option<*const crate::closure::JsFunctionInfo>,
    enumerable: bool,
    configurable: bool,
) {
    let Some(obj) = any_object(value) else {
        return;
    };
    // The key is an interned literal and the closures are realm singletons;
    // keep the heap still across the shape transition and the pair install.
    let _no_move = crate::gc::GcSuppressScope::new();
    let getter = crate::closure::js_closure_alloc_singleton(get);
    let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
    let setter_bits = set.map_or(0, |info| {
        let setter = crate::closure::js_closure_alloc_singleton(info);
        crate::value::js_nanbox_pointer(setter as i64).to_bits()
    });
    let key = crate::string::intern_ascii_literal(name.as_bytes());
    unsafe {
        crate::object::install_own_builtin_accessor(
            obj,
            key,
            name,
            getter_bits,
            setter_bits,
            crate::object::PropertyAttrs::new(true, enumerable, configurable),
        );
    }
}
