//! Shared payload attachment for in-tree and binding families.

use super::*;

/// Attach a payload to an existing ordinary object. This is the `super()`
/// half of the pattern: a source-compiled subclass keeps its own class id and
/// prototype, while its traced `native_state` owns the same typed cell as a
/// direct instance.
pub fn attach_to_object<T: 'static>(
    value: f64,
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
) -> bool {
    attach_to_object_with(value, family, payload, plain_vtable::<T>(), external_bytes)
}

/// [`attach_to_object`] for a stream family (`super()` of
/// `class X extends zlib.Gzip`): the cell's vtable carries `T::HOOKS`.
pub fn attach_stream_to_object<T: StreamPayload>(
    value: f64,
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
) -> bool {
    attach_to_object_with(value, family, payload, hooked_vtable::<T>(), external_bytes)
}

pub(super) fn attach_to_object_with<T: 'static>(
    value: f64,
    family: &'static NativePayloadFamily,
    payload: T,
    vtable: &'static PayloadVTable,
    external_bytes: usize,
) -> bool {
    let Some(obj) = any_object(value) else {
        return false;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let meta = obj
        .with_mut_ptr::<ObjectHeader, _>(|obj| unsafe { crate::object::object_meta_ensure(obj) });
    if meta.is_null() {
        return false;
    }
    let previous = unsafe { (*meta).native_state };
    if is_payload_state_word(previous) {
        let cell = (previous & crate::value::POINTER_MASK) as *mut NativeHandleHeader;
        return attach_cell(
            obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64)),
            cell,
            family,
            payload,
            vtable,
            external_bytes,
        )
        .is_ok();
    }
    attach_rooted(&obj, family, Some(payload), vtable, external_bytes);
    true
}

pub(super) fn attach_rooted<T: 'static>(
    obj: &crate::gc::RuntimeHandle<'_>,
    family: &'static NativePayloadFamily,
    payload: Option<T>,
    vtable: &'static PayloadVTable,
    external_bytes: usize,
) {
    // A closed cell has no Rust layout yet. Its first attach establishes T.
    let type_id = if payload.is_some() {
        type_tag::<T>(family.class_id)
    } else {
        family.class_id as u64
    };
    let resource = payload.map_or(std::ptr::null_mut(), |p| {
        Box::into_raw(Box::new(p)) as *mut c_void
    });
    attach_external_rooted(
        obj,
        resource,
        type_id,
        vtable,
        family.name,
        family.links_owner,
        external_bytes,
    );
}

/// Shared attachment for in-tree and C-ABI families; the caller owns the
/// resource until this call transfers it to the object's traced cell.
pub(crate) fn attach_external_rooted(
    obj: &crate::gc::RuntimeHandle<'_>,
    resource: *mut c_void,
    type_id: u64,
    vtable: &'static PayloadVTable,
    name: &str,
    links_owner: bool,
    external_bytes: usize,
) {
    let meta = obj
        .with_mut_ptr::<ObjectHeader, _>(|obj| unsafe { crate::object::object_meta_ensure(obj) });
    if meta.is_null() {
        return;
    }
    let cell = unsafe {
        crate::native_handle::native_handle_new_rust_payload(
            resource,
            type_id,
            vtable,
            name,
            links_owner,
        )
    };
    let word = crate::value::JSValue::pointer(cell as *const u8).bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        let meta = (*obj).meta;
        debug_assert!(!meta.is_null(), "object_meta_ensure ran above");
        (*meta).native_state = word;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            word,
        );
    });
    if links_owner {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
            let owner = crate::value::js_nanbox_pointer(obj as i64).to_bits();
            // GC_STORE_AUDIT(BARRIERED): malloc cell -> nursery owner.
            (*cell).owner = owner;
            if let Some(state) = raw_js_state(obj) {
                store_callbacks(cell, raw_field_memo(state, b"callbacks", &CALLBACKS_MEMO));
            }
            #[cfg(test)]
            if callback_sabotage("barrier") {
                return;
            }
            crate::gc::runtime_write_barrier_external_slot(
                cell as usize,
                &(*cell).owner as *const _ as usize,
                owner,
            );
        });
    }
    // Only now, with the cell reachable from the rooted object: reporting the
    // bytes can start a collection.
    if external_bytes != 0 {
        unsafe { crate::native_handle::native_handle_set_external_bytes(cell, external_bytes) };
    }
}

/// `obj[key] = value` for a runtime-owned ASCII key, with the key rooted
/// across the store (the store may allocate a shape transition).
pub(crate) fn set_own(
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

/// `%IteratorPrototype%` of this realm, for a family prototype to inherit.
pub fn iterator_prototype() -> f64 {
    crate::object::iterator_prototypes::ensure_iterator_prototypes();
    let ptr = crate::object::iterator_prototypes::ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        undefined()
    } else {
        crate::value::js_nanbox_pointer(ptr)
    }
}

/// A `{ done, value }` iterator result in node:sqlite's key order.
pub fn iter_result_done_value(done: bool, value: f64) -> f64 {
    unsafe {
        crate::iter_result::make_sqlite_iter_result(
            crate::value::JSValue::from_bits(value.to_bits()),
            done,
        )
    }
}

/// The family's per-realm prototype, materialized now. A family whose
/// constructor is a module export calls this when the export is created, so
/// `Export.prototype` carries the methods before any instance exists.
pub fn prototype(family: &NativePayloadFamily) -> f64 {
    let proto = family_prototype(family);
    if proto.is_null() {
        undefined()
    } else {
        crate::value::js_nanbox_pointer(proto as i64)
    }
}

#[inline]
pub(crate) fn any_object(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    Some(addr as *mut ObjectHeader)
}
