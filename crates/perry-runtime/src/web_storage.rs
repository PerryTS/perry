//! Minimal Web Storage globals (`Storage`, `localStorage`, `sessionStorage`).
//!
//! Node 22 exposes these behind `--experimental-webstorage`; newer Node
//! exposes them by default. Perry's runtime has no process flag layer here, so
//! both storage areas are process-local in-memory stores.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use crate::closure::ClosureHeader;
use crate::object::{ObjectHeader, PropertyAttrs};

const TAG_UNDEFINED: u64 = crate::value::TAG_UNDEFINED;
const TAG_NULL: u64 = crate::value::TAG_NULL;
const STORAGE_QUOTA_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StorageKind {
    Local,
    Session,
}

thread_local! {
    static LOCAL_STORE: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static SESSION_STORE: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

pub extern "C" fn storage_constructor_illegal(_closure: *const ClosureHeader) -> f64 {
    throw_error("Illegal constructor")
}

extern "C" fn storage_global_setter(_closure: *const ClosureHeader, _value: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn storage_local_global_getter(_closure: *const ClosureHeader) -> f64 {
    storage_global_value(StorageKind::Local)
}

extern "C" fn storage_session_global_getter(_closure: *const ClosureHeader) -> f64 {
    storage_global_value(StorageKind::Session)
}

extern "C" fn storage_length_getter(_closure: *const ClosureHeader) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    with_store(kind, |store| store.len() as f64)
}

extern "C" fn storage_get_item(_closure: *const ClosureHeader, key: f64) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    storage_get_item_impl(kind, key)
}

extern "C" fn storage_set_item(_closure: *const ClosureHeader, key: f64, value: f64) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    storage_set_item_impl(kind, key, value)
}

extern "C" fn storage_remove_item(_closure: *const ClosureHeader, key: f64) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    storage_remove_item_impl(kind, key)
}

extern "C" fn storage_key(_closure: *const ClosureHeader, index: f64) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    storage_key_impl(kind, index)
}

extern "C" fn storage_clear(_closure: *const ClosureHeader) -> f64 {
    let Some(kind) = receiver_kind() else {
        return throw_type_error("Illegal invocation");
    };
    storage_clear_impl(kind)
}

pub(crate) fn is_storage_value(value: f64) -> bool {
    storage_kind_from_value(value).is_some()
}

pub(crate) fn dispatch_storage_method(object: f64, method_name: &str, args: &[f64]) -> Option<f64> {
    let kind = storage_kind_from_value(object)?;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let result = match method_name {
        "clear" => storage_clear_impl(kind),
        "getItem" => storage_get_item_impl(kind, args.first().copied().unwrap_or(undefined)),
        "key" => storage_key_impl(kind, args.first().copied().unwrap_or(undefined)),
        "removeItem" => storage_remove_item_impl(kind, args.first().copied().unwrap_or(undefined)),
        "setItem" => storage_set_item_impl(
            kind,
            args.first().copied().unwrap_or(undefined),
            args.get(1).copied().unwrap_or(undefined),
        ),
        _ => return None,
    };
    Some(result)
}

fn storage_get_item_impl(kind: StorageKind, key: f64) -> f64 {
    let key = js_string(key);
    with_store(kind, |store| match store.get(&key) {
        Some(value) => boxed_str(value),
        None => f64::from_bits(TAG_NULL),
    })
}

fn storage_set_item_impl(kind: StorageKind, key: f64, value: f64) -> f64 {
    let key = js_string(key);
    let value = js_string(value);
    with_store_mut(kind, |store| {
        let old_value_len = store.get(&key).map(|v| v.len()).unwrap_or(0);
        let current_size = storage_size(store);
        let new_size = current_size
            .saturating_sub(if old_value_len == 0 && !store.contains_key(&key) {
                0
            } else {
                key.len() + old_value_len
            })
            .saturating_add(key.len())
            .saturating_add(value.len());
        if new_size > STORAGE_QUOTA_BYTES {
            return throw_quota_exceeded();
        }
        store.insert(key.clone(), value.clone());
        sync_named_property(kind, &key, Some(&value));
        update_length(kind, store.len());
        f64::from_bits(TAG_UNDEFINED)
    })
}

fn storage_remove_item_impl(kind: StorageKind, key: f64) -> f64 {
    let key = js_string(key);
    with_store_mut(kind, |store| {
        if store.remove(&key).is_some() {
            sync_named_property(kind, &key, None);
            update_length(kind, store.len());
        }
        f64::from_bits(TAG_UNDEFINED)
    })
}

fn storage_key_impl(kind: StorageKind, index: f64) -> f64 {
    let number = crate::JSValue::from_bits(index.to_bits()).to_number();
    let idx = if number.is_nan() { 0.0 } else { number.trunc() };
    if idx < 0.0 {
        return f64::from_bits(TAG_NULL);
    }
    let idx = idx as usize;
    with_store(kind, |store| match store.keys().nth(idx) {
        Some(key) => boxed_str(key),
        None => f64::from_bits(TAG_NULL),
    })
}

fn storage_global_value(kind: StorageKind) -> f64 {
    let obj = storage_object(kind);
    if obj.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

fn storage_clear_impl(kind: StorageKind) -> f64 {
    with_store_mut(kind, |store| {
        let keys: Vec<String> = store.keys().cloned().collect();
        store.clear();
        for key in keys {
            sync_named_property(kind, &key, None);
        }
        update_length(kind, 0);
        f64::from_bits(TAG_UNDEFINED)
    })
}

pub(crate) fn install_storage_globals(
    global: *mut ObjectHeader,
    storage_ctor: *mut ClosureHeader,
    storage_proto: *mut ObjectHeader,
    ctor_value: f64,
) {
    if global.is_null() || storage_ctor.is_null() || storage_proto.is_null() {
        return;
    }
    crate::object::set_builtin_property_attrs(
        global as usize,
        "Storage".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    install_method(storage_proto, "clear", storage_clear as *const u8, 0);
    install_method(storage_proto, "getItem", storage_get_item as *const u8, 1);
    install_method(storage_proto, "key", storage_key as *const u8, 1);
    install_method(
        storage_proto,
        "removeItem",
        storage_remove_item as *const u8,
        1,
    );
    install_method(storage_proto, "setItem", storage_set_item as *const u8, 2);
    install_storage_length_accessor(storage_proto);

    let constructor_key = string("constructor");
    crate::object::js_object_set_field_by_name(storage_proto, constructor_key, ctor_value);
    crate::object::set_builtin_property_attrs(
        storage_proto as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );

    let local = make_storage_object(StorageKind::Local, storage_proto);
    let session = make_storage_object(StorageKind::Session, storage_proto);
    crate::gc::runtime_store_root_atomic_raw_i64(
        &crate::object::LOCAL_STORAGE_PTR,
        local as i64,
        Ordering::Release,
    );
    crate::gc::runtime_store_root_atomic_raw_i64(
        &crate::object::SESSION_STORAGE_PTR,
        session as i64,
        Ordering::Release,
    );

    set_global_storage_property(global, "localStorage", local);
    set_global_storage_property(global, "sessionStorage", session);
}

fn install_method(proto: *mut ObjectHeader, name: &str, func_ptr: *const u8, arity: u32) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::object::js_object_set_field_by_name(
        proto,
        string(name),
        crate::value::js_nanbox_pointer(closure as i64),
    );
    crate::object::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        PropertyAttrs::new(true, true, true),
    );
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
}

fn install_storage_length_accessor(proto: *mut ObjectHeader) {
    let getter = crate::closure::js_closure_alloc(storage_length_getter as *const u8, 0);
    if getter.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(storage_length_getter as *const u8, 0);
    crate::object::set_bound_native_closure_name(getter, "get length");
    crate::object::js_object_set_field_by_name(
        proto,
        string("length"),
        f64::from_bits(TAG_UNDEFINED),
    );
    crate::object::set_builtin_accessor_descriptor(
        proto as usize,
        "length".to_string(),
        crate::object::AccessorDescriptor {
            get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
            set: 0,
        },
        PropertyAttrs::new(false, true, false),
    );
}

fn make_storage_object(kind: StorageKind, proto: *mut ObjectHeader) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 8);
    if obj.is_null() {
        return obj;
    }
    let proto_bits = crate::value::js_nanbox_pointer(proto as i64).to_bits();
    crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto_bits);
    update_length_on_obj(obj, 0);
    for (name, arity) in [
        ("clear", 0),
        ("getItem", 1),
        ("key", 1),
        ("removeItem", 1),
        ("setItem", 2),
    ] {
        let func_ptr = match name {
            "clear" => storage_clear as *const u8,
            "getItem" => storage_get_item as *const u8,
            "key" => storage_key as *const u8,
            "removeItem" => storage_remove_item as *const u8,
            _ => storage_set_item as *const u8,
        };
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if !closure.is_null() {
            crate::closure::js_register_closure_arity(func_ptr, arity);
            crate::object::set_bound_native_closure_name(closure, name);
            crate::object::set_builtin_closure_length(closure as usize, 0);
            crate::object::js_object_set_field_by_name(
                obj,
                string(name),
                crate::value::js_nanbox_pointer(closure as i64),
            );
            crate::object::set_builtin_property_attrs(
                obj as usize,
                name.to_string(),
                PropertyAttrs::new(true, false, true),
            );
        }
    }
    match kind {
        StorageKind::Local => LOCAL_STORE.with(|s| s.borrow_mut().clear()),
        StorageKind::Session => SESSION_STORE.with(|s| s.borrow_mut().clear()),
    }
    obj
}

fn set_global_storage_property(global: *mut ObjectHeader, name: &str, value: *mut ObjectHeader) {
    crate::object::js_object_set_field_by_name(
        global,
        string(name),
        crate::value::js_nanbox_pointer(value as i64),
    );
    crate::object::set_builtin_property_attrs(
        global as usize,
        name.to_string(),
        PropertyAttrs::new(true, true, true),
    );

    let getter_fn = match name {
        "localStorage" => storage_local_global_getter as *const u8,
        _ => storage_session_global_getter as *const u8,
    };
    let getter = crate::closure::js_closure_alloc(getter_fn, 0);
    let setter = crate::closure::js_closure_alloc(storage_global_setter as *const u8, 0);
    if !getter.is_null() && !setter.is_null() {
        crate::closure::js_register_closure_arity(getter_fn, 0);
        crate::closure::js_register_closure_arity(storage_global_setter as *const u8, 1);
        crate::object::set_bound_native_closure_name(getter, name);
        crate::object::set_builtin_accessor_descriptor(
            global as usize,
            name.to_string(),
            crate::object::AccessorDescriptor {
                get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                set: crate::value::js_nanbox_pointer(setter as i64).to_bits(),
            },
            PropertyAttrs::new(false, true, true),
        );
    }
}

fn receiver_kind() -> Option<StorageKind> {
    let this = crate::object::js_implicit_this_get();
    storage_kind_from_value(this)
}

fn storage_kind_from_value(value: f64) -> Option<StorageKind> {
    let this = value;
    let ptr = crate::value::js_nanbox_get_pointer(this);
    if ptr == 0 {
        return None;
    }
    let ptr = ptr;
    if ptr == crate::object::LOCAL_STORAGE_PTR.load(Ordering::Acquire) {
        Some(StorageKind::Local)
    } else if ptr == crate::object::SESSION_STORAGE_PTR.load(Ordering::Acquire) {
        Some(StorageKind::Session)
    } else {
        None
    }
}

fn storage_object(kind: StorageKind) -> *mut ObjectHeader {
    let ptr = match kind {
        StorageKind::Local => crate::object::LOCAL_STORAGE_PTR.load(Ordering::Acquire),
        StorageKind::Session => crate::object::SESSION_STORAGE_PTR.load(Ordering::Acquire),
    };
    ptr as *mut ObjectHeader
}

fn with_store<R>(kind: StorageKind, f: impl FnOnce(&BTreeMap<String, String>) -> R) -> R {
    match kind {
        StorageKind::Local => LOCAL_STORE.with(|s| f(&s.borrow())),
        StorageKind::Session => SESSION_STORE.with(|s| f(&s.borrow())),
    }
}

fn with_store_mut<R>(kind: StorageKind, f: impl FnOnce(&mut BTreeMap<String, String>) -> R) -> R {
    match kind {
        StorageKind::Local => LOCAL_STORE.with(|s| f(&mut s.borrow_mut())),
        StorageKind::Session => SESSION_STORE.with(|s| f(&mut s.borrow_mut())),
    }
}

fn storage_size(store: &BTreeMap<String, String>) -> usize {
    store.iter().map(|(k, v)| k.len() + v.len()).sum()
}

fn sync_named_property(kind: StorageKind, key: &str, value: Option<&str>) {
    if reserved_storage_name(key) {
        return;
    }
    let obj = storage_object(kind);
    if obj.is_null() {
        return;
    }
    let key_hdr = string(key);
    match value {
        Some(value) => crate::object::js_object_set_field_by_name(obj, key_hdr, boxed_str(value)),
        None => {
            crate::object::js_object_delete_field(obj, key_hdr);
        }
    }
}

fn update_length(kind: StorageKind, len: usize) {
    let obj = storage_object(kind);
    if !obj.is_null() {
        update_length_on_obj(obj, len);
    }
}

fn update_length_on_obj(obj: *mut ObjectHeader, len: usize) {
    crate::object::clear_property_attrs(obj as usize, "length");
    crate::object::js_object_set_field_by_name(obj, string("length"), len as f64);
    crate::object::set_builtin_property_attrs(
        obj as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
}

fn reserved_storage_name(key: &str) -> bool {
    matches!(
        key,
        "length" | "clear" | "getItem" | "key" | "removeItem" | "setItem" | "constructor"
    )
}

fn js_string(value: f64) -> String {
    let hdr = crate::builtins::js_string_coerce(value);
    if hdr.is_null() {
        return String::new();
    }
    unsafe {
        let data = (hdr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let len = (*hdr).byte_len as usize;
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn string(value: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32)
}

fn boxed_str(value: &str) -> f64 {
    crate::value::js_nanbox_string(string(value) as i64)
}

fn throw_error(message: &str) -> f64 {
    let msg = string(message);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_type_error(message: &str) -> f64 {
    let msg = string(message);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_quota_exceeded() -> f64 {
    let msg = string("The quota has been exceeded.");
    let err = crate::error::js_error_new_with_name_message(b"QuotaExceededError", msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
