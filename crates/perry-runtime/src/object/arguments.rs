//! ECMAScript Arguments objects.
//!
//! The storage is an ordinary `ObjectHeader` so existing own-key enumeration,
//! descriptors, and Object APIs keep working. Sloppy mapped arguments add a
//! side table from numeric indices to Perry mutable-capture boxes.

use super::*;

#[derive(Default)]
struct ArgumentsMeta {
    mapped: HashMap<u32, usize>,
    restricted_callee: bool,
}

thread_local! {
    static ARGUMENTS_OBJECTS: RefCell<crate::fast_hash::PtrHashMap<usize, ArgumentsMeta>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

pub fn scan_arguments_object_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut moved = Vec::new();
    ARGUMENTS_OBJECTS.with(|m| {
        let mut map = m.borrow_mut();
        for (&owner, meta) in map.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            // 2026-07-09 GC audit wave 2: the mapped-arguments capture BOXES
            // are real heap references (`js_arguments_object_map_index`
            // stores raw `Box` pointers) and were never visited — a moving
            // GC could sweep or relocate a box out from under the next
            // `arguments[i]` read/write. Visit them STRONGLY so they stay
            // live and get rewritten to their post-move addresses.
            for box_ptr in meta.mapped.values_mut() {
                visitor.visit_usize_slot(box_ptr);
            }
        }
        for (old_owner, new_owner) in moved.drain(..) {
            if let Some(meta) = map.remove(&old_owner) {
                map.insert(new_owner, meta);
            }
        }
    });
}

/// Death pruning (2026-07-09 GC audit wave 2): one entry was inserted per
/// CALL of any function referencing `arguments` and never removed, so the
/// table grew at call rate and `is_arguments_object` misfired on a fresh
/// object at a recycled address. `is_dead_owner` is one of the GC's
/// deadness predicates (`gc::dead_owner`).
pub(crate) fn prune_dead_arguments_object_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    ARGUMENTS_OBJECTS.with(|m| {
        let mut map = m.borrow_mut();
        if !map.is_empty() {
            map.retain(|owner, _| !is_dead_owner(*owner));
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_arguments_object_roots() {
    ARGUMENTS_OBJECTS.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_arguments_object_registered(addr: usize) -> bool {
    ARGUMENTS_OBJECTS.with(|m| m.borrow().contains_key(&addr))
}

#[cfg(test)]
pub(crate) fn test_arguments_mapped_box(addr: usize, index: u32) -> Option<usize> {
    ARGUMENTS_OBJECTS.with(|m| {
        m.borrow()
            .get(&addr)
            .and_then(|meta| meta.mapped.get(&index).copied())
    })
}

fn key_name(key: *const crate::StringHeader) -> Option<String> {
    if key.is_null() || (key as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

fn intern_key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

extern "C" fn arguments_throw_type_error(_closure: *const crate::closure::ClosureHeader) -> f64 {
    super::throw_object_type_error(
        b"'caller', 'callee', and 'arguments' properties may not be accessed",
    );
}

fn thrower_closure_value() -> f64 {
    let closure =
        crate::closure::js_closure_alloc_singleton(arguments_throw_type_error as *const u8);
    crate::value::js_nanbox_pointer(closure as i64)
}

#[no_mangle]
pub extern "C" fn js_arguments_object_alloc(
    raw_args: f64,
    callee: f64,
    restricted_callee: i32,
) -> *mut ObjectHeader {
    let arr_ptr = crate::array::clean_arr_ptr(
        crate::value::js_nanbox_get_pointer(raw_args) as *const crate::array::ArrayHeader
    );
    let len = if arr_ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr_ptr)
    };

    let obj = js_object_alloc(0, len.saturating_add(2));
    for i in 0..len {
        let name = i.to_string();
        let key = intern_key(&name);
        let value = if arr_ptr.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            crate::array::js_array_get_f64(arr_ptr, i)
        };
        js_object_set_field_by_name(obj, key, value);
        set_property_attrs(obj as usize, name, PropertyAttrs::new(true, true, true));
    }

    let length_key = intern_key("length");
    js_object_set_field_by_name(obj, length_key, len as f64);
    set_property_attrs(
        obj as usize,
        "length".to_string(),
        PropertyAttrs::new(true, false, true),
    );

    let callee_key = intern_key("callee");
    if restricted_callee != 0 {
        js_object_set_field_by_name(obj, callee_key, f64::from_bits(crate::value::TAG_UNDEFINED));
        let thrower = thrower_closure_value();
        set_accessor_descriptor(
            obj as usize,
            "callee".to_string(),
            AccessorDescriptor {
                get: thrower.to_bits(),
                set: thrower.to_bits(),
            },
        );
        set_property_attrs(
            obj as usize,
            "callee".to_string(),
            PropertyAttrs::new(false, false, false),
        );
    } else {
        js_object_set_field_by_name(obj, callee_key, callee);
        set_property_attrs(
            obj as usize,
            "callee".to_string(),
            PropertyAttrs::new(true, false, true),
        );
    }

    ARGUMENTS_OBJECTS.with(|m| {
        m.borrow_mut().insert(
            obj as usize,
            ArgumentsMeta {
                mapped: HashMap::new(),
                restricted_callee: restricted_callee != 0,
            },
        );
    });
    obj
}

#[no_mangle]
pub extern "C" fn js_arguments_object_map_index(
    obj: *mut ObjectHeader,
    index: u32,
    box_ptr: *mut crate::r#box::Box,
) {
    if obj.is_null() || box_ptr.is_null() {
        return;
    }
    ARGUMENTS_OBJECTS.with(|m| {
        if let Some(meta) = m.borrow_mut().get_mut(&(obj as usize)) {
            meta.mapped.insert(index, box_ptr as usize);
        }
    });
}

pub(crate) fn is_arguments_object(obj: *const ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    ARGUMENTS_OBJECTS.with(|m| m.borrow().contains_key(&(obj as usize)))
}

pub(crate) unsafe fn arguments_object_get_index(
    obj: *const ObjectHeader,
    index: u32,
) -> Option<f64> {
    if !is_arguments_object(obj) {
        return None;
    }
    let name = index.to_string();
    let key = intern_key(&name);
    Some(
        arguments_object_get_field(obj, key)
            .map(|value| f64::from_bits(value.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
    )
}

pub(crate) unsafe fn arguments_object_set_index(
    obj: *mut ObjectHeader,
    index: u32,
    value: f64,
) -> bool {
    if !is_arguments_object(obj) {
        return false;
    }
    let name = index.to_string();
    let key = intern_key(&name);
    arguments_object_set_field(obj, key, value)
}

pub(crate) fn arguments_object_to_string_tag(value: f64) -> Option<f64> {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
    if !is_arguments_object(ptr) {
        return None;
    }
    let bytes = b"[object Arguments]";
    let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    Some(crate::value::js_nanbox_string(s as i64))
}

pub(crate) unsafe fn arguments_object_get_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let name = key_name(key)?;
    let (mapped_box, restricted_callee) = ARGUMENTS_OBJECTS.with(|m| {
        let map = m.borrow();
        let meta = map.get(&(obj as usize))?;
        let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
            if super::own_key_present(obj as *mut ObjectHeader, key) {
                meta.mapped.get(&idx).copied()
            } else {
                None
            }
        });
        Some((mapped_box, meta.restricted_callee))
    })?;

    if name == "callee" && restricted_callee {
        arguments_throw_type_error(std::ptr::null());
    }
    if let Some(box_ptr) = mapped_box {
        let value = crate::r#box::js_box_get(box_ptr as *mut crate::r#box::Box);
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if super::own_key_present(obj as *mut ObjectHeader, key) {
        if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
            if acc.get != 0 {
                let closure =
                    (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                if !closure.is_null() {
                    let value = crate::closure::js_closure_call0(closure);
                    return Some(JSValue::from_bits(value.to_bits()));
                }
            }
            return Some(JSValue::undefined());
        }
        return Some(read_ordinary_own_value(obj, key));
    }
    None
}

pub(crate) unsafe fn arguments_object_set_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let Some(name) = key_name(key) else {
        return false;
    };
    let Some((mapped_box, restricted_callee)) = ARGUMENTS_OBJECTS.with(|m| {
        let map = m.borrow();
        let meta = map.get(&(obj as usize))?;
        let mapped_box = super::canonical_array_index(&name).and_then(|idx| {
            if super::own_key_present(obj, key) {
                meta.mapped.get(&idx).copied()
            } else {
                None
            }
        });
        Some((mapped_box, meta.restricted_callee))
    }) else {
        return false;
    };

    if name == "callee" && restricted_callee {
        arguments_throw_type_error(std::ptr::null());
    }
    if !super::own_key_present(obj, key) {
        return false;
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        if acc.set != 0 {
            let closure =
                (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !closure.is_null() {
                crate::closure::js_closure_call1(closure, value);
            }
        }
        return true;
    }
    if let Some(attrs) = get_property_attrs(obj as usize, &name) {
        if !attrs.writable() {
            crate::error::throw_immutable_write(0, &name);
        }
    }
    write_ordinary_own_value(obj, key, value);
    if let Some(box_ptr) = mapped_box {
        crate::r#box::js_box_set(box_ptr as *mut crate::r#box::Box, value);
    }
    true
}

pub(crate) unsafe fn arguments_object_before_delete(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<i32> {
    let name = key_name(key)?;
    ARGUMENTS_OBJECTS.with(|m| {
        let mut map = m.borrow_mut();
        let meta = map.get_mut(&(obj as usize))?;
        if name == "callee" && meta.restricted_callee {
            return Some(0);
        }
        if let Some(index) = super::canonical_array_index(&name) {
            meta.mapped.remove(&index);
        }
        None
    })
}

pub(crate) unsafe fn arguments_object_after_define(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    descriptor_value: f64,
) {
    let Some(name) = key_name(key) else {
        return;
    };
    let Some(index) = super::canonical_array_index(&name) else {
        return;
    };
    let desc_ptr = super::extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return;
    }
    let value_key = intern_key("value");
    let value = if super::own_key_present(desc_ptr, value_key) {
        Some(f64::from_bits(
            js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key).bits(),
        ))
    } else {
        None
    };
    let get_key = intern_key("get");
    let set_key = intern_key("set");
    let writable_key = intern_key("writable");
    let has_accessor =
        super::own_key_present(desc_ptr, get_key) || super::own_key_present(desc_ptr, set_key);
    let writable_false = if super::own_key_present(desc_ptr, writable_key) {
        let writable = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, writable_key);
        crate::value::js_is_truthy(f64::from_bits(writable.bits())) == 0
    } else {
        false
    };
    ARGUMENTS_OBJECTS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(meta) = map.get_mut(&(obj as usize)) else {
            return;
        };
        let Some(box_ptr) = meta.mapped.get(&index).copied() else {
            return;
        };
        if let Some(value) = value {
            crate::r#box::js_box_set(box_ptr as *mut crate::r#box::Box, value);
        }
        if has_accessor || writable_false {
            meta.mapped.remove(&index);
        }
    });
}

pub(crate) unsafe fn arguments_object_descriptor(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let name = key_name(key)?;
    let restricted_callee = ARGUMENTS_OBJECTS.with(|m| {
        let map = m.borrow();
        map.get(&(obj as usize)).map(|meta| meta.restricted_callee)
    })?;
    if !super::own_key_present(obj, key) {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if name == "callee" && restricted_callee {
        let thrower = thrower_closure_value();
        return Some(build_accessor_descriptor(thrower, thrower, false, false));
    }
    if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
        let get = if acc.get != 0 {
            f64::from_bits(acc.get)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let set = if acc.set != 0 {
            f64::from_bits(acc.set)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let attrs =
            get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
        return Some(build_accessor_descriptor(
            get,
            set,
            attrs.enumerable(),
            attrs.configurable(),
        ));
    }
    let value = if let Some(index) = super::canonical_array_index(&name) {
        ARGUMENTS_OBJECTS
            .with(|m| {
                m.borrow()
                    .get(&(obj as usize))
                    .and_then(|meta| meta.mapped.get(&index).copied())
            })
            .map(|box_ptr| crate::r#box::js_box_get(box_ptr as *mut crate::r#box::Box))
            .unwrap_or_else(|| f64::from_bits(read_ordinary_own_value(obj, key).bits()))
    } else {
        f64::from_bits(read_ordinary_own_value(obj, key).bits())
    };
    let attrs =
        get_property_attrs(obj as usize, &name).unwrap_or(PropertyAttrs::new(true, true, true));
    Some(build_data_descriptor(
        value,
        attrs.writable(),
        attrs.enumerable(),
        attrs.configurable(),
    ))
}

pub(crate) unsafe fn arguments_object_to_vec(obj: *const ObjectHeader) -> Option<Vec<f64>> {
    if !is_arguments_object(obj) {
        return None;
    }
    let length_key = intern_key("length");
    let length_value = arguments_object_get_field(obj, length_key)
        .map(|v| f64::from_bits(v.bits()))
        .unwrap_or(0.0);
    let len = if length_value.is_finite() && length_value > 0.0 {
        length_value.floor().min(u32::MAX as f64) as u32
    } else {
        0
    };
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let name = i.to_string();
        let key = intern_key(&name);
        let value = arguments_object_get_field(obj, key)
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
        out.push(value);
    }
    Some(out)
}

pub(crate) unsafe fn arguments_object_to_array(
    obj: *const ObjectHeader,
) -> Option<*mut ArrayHeader> {
    let values = arguments_object_to_vec(obj)?;
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut current = arr;
    for value in values {
        current = crate::array::js_array_push_f64(current, value);
    }
    Some(current)
}

#[no_mangle]
pub extern "C" fn js_array_like_to_array(value: f64) -> *mut ArrayHeader {
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return std::ptr::null_mut();
    }
    let raw = jsv.as_pointer::<u8>();
    unsafe {
        if let Some(arr) = arguments_object_to_array(raw as *const ObjectHeader) {
            return arr;
        }
        let addr = raw as usize;
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            return crate::typedarray::typed_array_to_array(
                raw as *const crate::typedarray::TypedArrayHeader,
            );
        }
        if crate::buffer::is_registered_buffer(addr) {
            return crate::buffer::buffer_to_array(raw as *const crate::buffer::BufferHeader);
        }
        // A real Array → fast path (no protocol overhead).
        if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
            return crate::array::clean_arr_ptr(raw as *const ArrayHeader) as *mut ArrayHeader;
        }
        // Generic iterable with a user `[Symbol.iterator]` (Map/Set, generator
        // objects, hand-rolled iterables, …): spread uses the ITERATOR protocol
        // (GetIterator → IteratorStep → IteratorValue), NOT array-like
        // length/index access. Drive `.next()` to collect the yielded values so
        // `f(...iterable)` / `new C(...iterable)` / `[...iterable]` spread see
        // them, and so errors thrown by `[Symbol.iterator]` / `.next()` /
        // accessing `value`/`done` propagate (test262 spread-err-*). Plain
        // array-like objects (a `{length, 0, 1}` bag with no `@@iterator`) keep
        // the legacy reinterpret fallback below.
        if crate::collection_iter::is_iterable(value) {
            let iter = crate::symbol::js_get_iterator(value);
            let mut out = crate::array::js_array_alloc(0);
            while let Some(v) = crate::collection_iter::iterator_next_value(iter) {
                out = crate::array::js_array_push_f64(out, f64::from_bits(v.to_bits()));
            }
            return out;
        }
        crate::array::clean_arr_ptr(raw as *const ArrayHeader) as *mut ArrayHeader
    }
}

unsafe fn read_ordinary_own_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    let keys = (*obj).keys_array;
    let key_count = crate::array::js_array_length(keys) as usize;
    let alloc_limit = std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            if i < alloc_limit {
                return js_object_get_field(obj, i as u32);
            }
            return overflow_get(obj as usize, i)
                .map(JSValue::from_bits)
                .unwrap_or_else(JSValue::undefined);
        }
    }
    JSValue::undefined()
}

unsafe fn write_ordinary_own_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let keys = (*obj).keys_array;
    let key_count = crate::array::js_array_length(keys) as usize;
    let alloc_limit = std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches(key_val, key) {
            if i < alloc_limit {
                js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
            } else {
                overflow_set(obj as usize, i, value.to_bits());
            }
            return;
        }
    }
}

unsafe fn build_data_descriptor(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    let packed = b"value\0writable\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_A6_50, 4, packed.as_ptr(), packed.len() as u32);
    let fields = (desc as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
    // GC_STORE_AUDIT(BARRIERED): fresh descriptor fields are replayed by the rebuild below.
    *fields = value;
    *fields.add(1) = bool_value(writable);
    *fields.add(2) = bool_value(enumerable);
    *fields.add(3) = bool_value(configurable);
    super::rebuild_object_field_layout(desc, 4);
    crate::value::js_nanbox_pointer(desc as i64)
}

unsafe fn build_accessor_descriptor(
    get: f64,
    set: f64,
    enumerable: bool,
    configurable: bool,
) -> f64 {
    let packed = b"get\0set\0enumerable\0configurable";
    let desc = js_object_alloc_with_shape(0x0D_A6_51, 4, packed.as_ptr(), packed.len() as u32);
    let fields = (desc as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
    // GC_STORE_AUDIT(BARRIERED): fresh descriptor fields are replayed by the rebuild below.
    *fields = get;
    *fields.add(1) = set;
    *fields.add(2) = bool_value(enumerable);
    *fields.add(3) = bool_value(configurable);
    super::rebuild_object_field_layout(desc, 4);
    crate::value::js_nanbox_pointer(desc as i64)
}
