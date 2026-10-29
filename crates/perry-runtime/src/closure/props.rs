//! A function object's own properties, stored IN the function object (D1).
//!
//! `ClosureHeader::props` points at the function's BAG: a runtime-internal,
//! null-prototype `ObjectHeader` whose keys and slots are the function's own
//! string and symbol properties, with attributes and accessor pairs, in creation
//! order. It is created on
//! the first own-property write and is a traced, rewritten raw-pointer child
//! edge of the closure (`gc::layout`'s `ClosureCaptures` arm), so it moves
//! and dies with the function — no address-keyed table, no re-key hook, no
//! dead-owner prune, no young log.
//!
//! The bag's own `ObjectMeta.expando` (an ordinary child edge of the bag)
//! holds the function's rare internal STATE record, another null-prototype
//! object: the #3655 deleted-synthesized-key markers (`"d:" + key`) and the
//! recorded `[[Prototype]]` (`"p"`). Its keys are namespaced, and it is never
//! reachable from JS.
//!
//! Writers run under `GcSuppressScope`: the ~100 callers of
//! `closure_set_dynamic_prop` hold raw closure addresses across the call, as
//! they always could when the store was a Rust `HashMap`, so the allocations
//! here (bag, key string, state record) must not move anything.
use super::ClosureHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The intrinsic own data keys a function's bag is born with, in creation
/// order: every function's `length` and `name`, then a generator function's
/// `prototype`.
const INTRINSIC_OWN_KEYS: [&str; 3] = ["length", "name", "prototype"];

/// A generator function's `prototype`: writable, non-enumerable,
/// non-configurable (ECMA-262 §15.5.5 / §15.6.5 InstantiateGeneratorFunction*).
pub(crate) const GENERATOR_PROTOTYPE_ATTRS: crate::object::PropertyAttrs =
    crate::object::PropertyAttrs::new(true, false, false);

/// Does the closure at `ptr` have a (sync or async) generator body? Those
/// declare an own `prototype` that their bag is born with.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn closure_has_generator_body(closure: *const ClosureHeader) -> bool {
    let info = (*closure).info;
    !info.is_null()
        && !super::shape::is_class_info(info)
        && (*info).code != super::BOUND_FUNCTION_FUNC_PTR
        && (*info).code != super::BOUND_METHOD_FUNC_PTR
        && (*info).flags & (super::FN_GENERATOR | super::FN_ASYNC_GENERATOR) != 0
}

const STATE_PROTO: &str = "p";
const DELETED_PREFIX: &str = "d:";
const INTERNAL_PREFIX: &str = "i:";

/// The bag of the closure at `ptr` (null when it never had an own property).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn bag_of(ptr: usize) -> *mut ObjectHeader {
    (*(ptr as *const ClosureHeader)).props
}

/// Allocate the bag if absent and install it with the store barrier.
pub(crate) unsafe fn bag_ensure(ptr: usize) -> *mut ObjectHeader {
    let existing = bag_of(ptr);
    if !existing.is_null() {
        return existing;
    }
    if bag_born_with_attrs(ptr, &[], &[]) {
        return bag_of(ptr);
    }
    // Reading a lazy intrinsic name can itself materialize the bag.
    let existing = bag_of(ptr);
    if !existing.is_null() {
        return existing;
    }
    let bag = crate::object::js_object_alloc_null_proto(0, 0);
    install_bag(ptr, bag);
    bag
}

/// Give the closure at `ptr` its bag `bag` with the store barrier.
pub(crate) unsafe fn install_bag(ptr: usize, bag: *mut ObjectHeader) {
    let closure = ptr as *mut ClosureHeader;
    // A closure is born `GC_LAYOUT_POINTER_FREE` when its captures hold no
    // pointer; some collector paths treat that state as "no child edge at
    // all". It now has one, so it leaves that state (#7630's tag-checked
    // `UNKNOWN`, the state a pointer capture store would also produce).
    crate::gc::layout_note_closure_edge_installed(ptr as *mut u8);
    // GC_STORE_AUDIT(BARRIERED): header raw-pointer edge store + object-slot
    // barrier, mirroring `object_meta_ensure`'s `meta` install.
    (*closure).props = bag;
    crate::gc::runtime_write_barrier_slot(ptr, &(*closure).props as *const _ as usize, bag as u64);
}

/// The first own data properties of a function that has none yet, all at
/// once: its bag is born holding `entries` in order, in ONE shape
/// ([`crate::object::alloc::object_alloc_null_proto_with_key_attrs`]), instead of growing
/// one key-add transition per key. This is a define: no setter or attribute is
/// consulted, as for every builtin install. False, with nothing done, when the
/// function already has a bag (or a declared static could hold one of the
/// keys); the caller then installs the ordinary way.
///
/// # Safety
/// `ptr` is a proven, live closure cell, and the keys of `entries` are
/// distinct.
pub(crate) unsafe fn bag_born_with_attrs(
    ptr: usize,
    entries: &[(&str, f64)],
    attrs: &[u8],
) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    if !bag_of(ptr).is_null()
        || entries.iter().any(|(key, _)| {
            crate::object::class_value::holds_declared_static_method(ptr, key).is_some()
        })
    {
        return false;
    }
    debug_assert!(entries
        .iter()
        .enumerate()
        .all(|(i, (a, _))| entries[..i].iter().all(|(b, _)| a != b)));
    // A function's intrinsic data slots precede subsequently defined keys.
    // Materializing them at bag birth preserves their creation order when
    // a later definition makes either slot enumerable. Classes already birth
    // their own declaration layout and use that layout unchanged.
    let class = super::shape::is_class_info((*(ptr as *const ClosureHeader)).info);
    let bag = if class {
        crate::object::alloc::object_alloc_null_proto_with_key_attrs(entries, attrs)
    } else {
        let intrinsic_attrs = crate::object::key_attrs::attr_bits_to_entry(
            crate::object::PropertyAttrs::new(false, false, true).bits,
        );
        // A generator function also declares `prototype` (writable,
        // non-enumerable, non-configurable): its bag is born with the slot
        // and its original prototype object, so the slot is a fact of the
        // function's keyed shape, never a later key-add or attribute edit.
        // Its original prototype also stays in the bag's private
        // `[[GeneratorPrototype]]` slot, which no later write of `prototype`
        // replaces: an instance born before the bag names the original
        // through its owner (`generator_state::generator_record_prototype_word`).
        let generator = closure_has_generator_body(ptr as *const ClosureHeader);
        let count = if generator { 4 } else { 2 };
        let original_key = crate::object::generator_original_prototype_key();
        let names = [
            INTRINSIC_OWN_KEYS[0],
            INTRINSIC_OWN_KEYS[1],
            INTRINSIC_OWN_KEYS[2],
            original_key,
        ];
        let names = &names[..count];
        let mut own = [
            ("length", 0.0),
            ("name", 0.0),
            ("prototype", 0.0),
            (original_key, 0.0),
        ];
        let mut own_attrs = [
            intrinsic_attrs,
            intrinsic_attrs,
            crate::object::key_attrs::attr_bits_to_entry(GENERATOR_PROTOTYPE_ATTRS.bits),
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        ];
        let original = if generator {
            crate::object::generator_original_prototype(ptr)
        } else {
            0.0
        };
        for (index, &key) in names.iter().enumerate() {
            if let Some(i) = entries.iter().position(|(name, _)| *name == key) {
                own[index] = entries[i];
                own_attrs[index] = attrs.get(i).copied().unwrap_or(own_attrs[index]);
            } else if index >= 2 {
                own[index].1 = original;
            } else {
                let closure = ptr as *const ClosureHeader;
                let value = if index == 0 {
                    crate::object::bound_native_callable_value_arity(
                        crate::value::js_nanbox_pointer(ptr as i64),
                    )
                    .or_else(|| super::bound_function_length(ptr))
                    .or_else(|| super::closure_length(closure))
                    .unwrap_or(0) as f64
                } else if (*closure).code() == super::BOUND_FUNCTION_FUNC_PTR {
                    super::bound_function_lazy_name(ptr)
                } else {
                    let name =
                        crate::builtins::function_name_for_closure(closure).unwrap_or_default();
                    let name =
                        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    crate::value::js_nanbox_string(name as i64)
                };
                own[index].1 = value;
            }
        }
        if !bag_of(ptr).is_null() {
            return false;
        }
        let own = &own[..count];
        let own_attrs = &own_attrs[..count];
        if entries.len() >= count
            && attrs.len() == entries.len()
            && entries[..count]
                .iter()
                .zip(own)
                .all(|(&(key, value), &(intrinsic, expected))| {
                    key == intrinsic && value.to_bits() == expected.to_bits()
                })
        {
            crate::object::alloc::object_alloc_null_proto_with_key_attrs(entries, attrs)
        } else if entries.iter().all(|(key, _)| names.contains(key)) {
            crate::object::alloc::object_alloc_null_proto_with_key_attrs(own, own_attrs)
        } else {
            let mut all = Vec::with_capacity(entries.len() + count);
            let mut all_attrs = Vec::with_capacity(entries.len() + count);
            all.extend_from_slice(own);
            all_attrs.extend_from_slice(own_attrs);
            for (i, &(key, value)) in entries.iter().enumerate() {
                if names.contains(&key) {
                    continue;
                }
                all.push((key, value));
                all_attrs.push(attrs.get(i).copied().unwrap_or(0));
            }
            crate::object::alloc::object_alloc_null_proto_with_key_attrs(&all, &all_attrs)
        }
    };
    install_bag(ptr, bag);
    true
}

/// Own data lookup in an ordinary (or dictionary-mode) bag by key bytes.
unsafe fn object_own_get(obj: *const ObjectHeader, key: &[u8]) -> Option<f64> {
    // One shape lookup for an ordinary bag: keys, count and inline bound all
    // come from the same descriptor. The layout answers alone unless the
    // shadow bit says the bag's own keys extend past it (§3.6).
    if let Some(d) = crate::object::shapes::object_shape_descriptor(obj) {
        if d.object_kind.is_ordinary_layout()
            && d.keys != 0
            && !crate::object::overflow::shadowed(obj)
        {
            let keys = d.keys as usize as *const crate::array::ArrayHeader;
            let slot = crate::object::keys_find_property_slot_by_bytes(
                d.keys_view(),
                d.logical_key_count,
                key,
            )?;
            // An accessor key's slot holds its getter/setter pair, never a
            // data value.
            if d.summary & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
                && crate::object::key_attrs::key_is_accessor_at(keys, slot as u32)
            {
                return None;
            }
            let value = crate::object::object_key_field_at_with_keys(
                obj,
                keys,
                slot,
                d.live_inline_slot_count,
            );
            if value.bits() == crate::value::TAG_HOLE {
                return None;
            }
            return Some(f64::from_bits(value.bits()));
        }
    }
    // With an overflow, the entry's position is not a field slot: the entry
    // holds the value or a reference to its fixed slot (§3.4).
    let t = crate::object::overflow::table(obj);
    if !t.is_null() {
        let i = crate::object::overflow::find_bytes(t, key)?;
        if crate::object::key_attrs::key_is_accessor_at(t.cast(), i) {
            return None;
        }
        let bits = crate::object::overflow::get_from(t, obj, i)?;
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        return Some(f64::from_bits(bits));
    }
    let keys = crate::object::object_keys(obj);
    let arr = keys.arr();
    if arr.is_null() {
        return None;
    }
    let slot = crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key)?;
    if crate::object::key_attrs::key_is_accessor_at(arr, slot as u32) {
        return None;
    }
    let live = crate::object::object_live_slot_count(obj);
    let value = crate::object::object_key_field_at_with_keys(obj, arr, slot, live);
    if value.bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(f64::from_bits(value.bits()))
}

/// The function's own data property `key`, if present.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_get(ptr: usize, key: &[u8]) -> Option<f64> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    object_own_get(bag, key)
}

unsafe fn object_own_set(obj: *mut ObjectHeader, key: &str, value: f64) {
    object_own_set_bytes(obj, key.as_bytes(), value);
}

unsafe fn object_own_set_bytes(obj: *mut ObjectHeader, key: &[u8], value: f64) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    // These are ordinary properties of the bag, even when their spelling
    // matches an internal private storage key. Do not reinterpret the string
    // as a compiled PrivateReference.
    crate::object::set_field_by_name_object_tail(obj, key, value);
}

/// Define/overwrite the function's own data property `key` (plain `[[Set]]`
/// on the null-prototype bag: no inherited setter can run).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_set(ptr: usize, key: &str, value: f64) {
    bag_set_bytes(ptr, key.as_bytes(), value);
}

pub(crate) unsafe fn bag_set_bytes(ptr: usize, key: &[u8], value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    object_own_set_bytes(bag, key, value);
    declared_value_replaced_bytes(ptr, key, declared);
}

unsafe fn declared_value_replaced_bytes(ptr: usize, key: &[u8], declared: Option<f64>) {
    let Some(old) = declared else { return };
    if bag_get(ptr, key).is_some_and(|v| v.to_bits() == old.to_bits()) {
        return;
    }
    crate::object::shapes::transition_object_shape_semantics(bag_of(ptr));
}

/// [[DefineOwnProperty]] of own data property `key` with just a value: the
/// value is stored and the key keeps (or, when new, gets default) attributes.
/// Unlike [`bag_set`] it ignores the key's `writable` attribute; a caller
/// that is performing a [[Set]] has checked it (a class function object's
/// statics, whose attributes live with these keys).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_define_value(ptr: usize, key: &str, value: f64) {
    bag_define_value_bytes(ptr, key.as_bytes(), value);
}

pub(crate) unsafe fn bag_define_value_bytes(ptr: usize, key: &[u8], value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    if let Ok(key) = std::str::from_utf8(key) {
        if bag_of(ptr).is_null() && bag_born_with_attrs(ptr, &[(key, value)], &[]) {
            return;
        }
    }
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    if !bag_has_own(ptr, key) {
        // A NEW property — including one `delete` removed earlier: it is
        // appended as any new key is, so it enumerates last. The in-place
        // store below would find the deleted key's tombstoned entry and
        // bring the property back at its old position.
        object_own_set_bytes(bag, key, value);
        declared_value_replaced_bytes(ptr, key, declared);
        return;
    }
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
    declared_value_replaced_bytes(ptr, key, declared);
}

/// [[DefineOwnProperty]] of own data property `key` with its value AND its
/// attributes (`attrs`, `PropertyAttrs` bits) in one step: a new key is
/// claimed carrying its final key entry (one key-add, no attribute rewrite
/// after it); an existing key keeps its position, takes the value, and its
/// entry is rewritten only when it differs. The caller has validated the
/// descriptor and refreshes the function's shape.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_define_data_with_attrs_bytes(
    ptr: usize,
    key: &[u8],
    value: f64,
    attrs: u8,
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    if let Ok(key) = std::str::from_utf8(key) {
        if bag_of(ptr).is_null()
            && bag_born_with_attrs(
                ptr,
                &[(key, value)],
                &[crate::object::key_attrs::attr_bits_to_entry(attrs)],
            )
        {
            return;
        }
    }
    let entry = crate::object::key_attrs::attr_bits_to_entry(attrs);
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let bag = bag_ensure(ptr);
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let present = bag_has_own(ptr, key);
    if !present
        && entry != 0
        && crate::object::shapes::object_shape_descriptor(bag).is_some_and(|d| d.hole_count == 0)
    {
        // A fresh key on a tombstone-free bag: claimed with its entry, as an
        // ordinary object's builtin define claims it.
        crate::object::object_ops::ensure_key_in_keys_array_with_entry(bag, key_hdr, entry);
        crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
        // The bag's descriptor bit, without flipping any process gate.
        crate::object::descriptor_state::note_descriptor_target_edits(bag as usize, &[]);
    } else {
        if present {
            crate::object::object_ops::define_property_force_store_value(bag, key_hdr, value);
        } else {
            // New (possibly after a delete): appended, so it enumerates last.
            object_own_set_bytes(bag, key, value);
        }
        if crate::object::key_attrs::object_key_entry(bag, key) != entry {
            crate::object::descriptor_state::note_descriptor_target_edits(
                bag as usize,
                &[crate::object::key_attrs::AttrsEdit::Data(key, attrs)],
            );
        }
    }
    declared_value_replaced_bytes(ptr, key, declared);
}

/// Remove the function's own data property `key`; true when it existed.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_remove(ptr: usize, key: &str) -> bool {
    bag_remove_bytes(ptr, key.as_bytes())
}

pub(crate) unsafe fn bag_remove_bytes(ptr: usize, key: &[u8]) -> bool {
    let bag = bag_of(ptr);
    if bag.is_null() || !bag_has_own(ptr, key) {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let declared = crate::object::class_value::holds_declared_static_method_bytes(ptr, key);
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_delete_field(bag, key_hdr);
    // Deleting the declaration must not let a re-add reach the shape that
    // proved it (a removed last key re-added lands on the same key list).
    declared_value_replaced_bytes(ptr, key, declared);
    true
}

/// Does the function own `key` — a data OR an accessor property?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_has_own(ptr: usize, key: &[u8]) -> bool {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return false;
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return false;
    }
    let Some(slot) = crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key)
    else {
        return false;
    };
    crate::object::key_attrs::key_is_accessor_at(arr, slot as u32)
        || crate::object::object_key_field_at_with_keys(
            bag,
            arr,
            slot,
            crate::object::object_live_slot_count(bag),
        )
        .bits()
            != crate::value::TAG_HOLE
}

/// The function's own ACCESSOR property names, in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_accessor_names(ptr: usize) -> Vec<String> {
    bag_accessor_names_bytes(ptr)
        .into_iter()
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect()
}

pub(crate) unsafe fn bag_accessor_names_bytes(ptr: usize) -> Vec<Vec<u8>> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..keys.count() {
        if !crate::object::key_attrs::key_is_accessor_at(arr, i) {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
                continue;
            }
            out.push(bytes.to_vec());
        }
    }
    out
}

/// Every own property name (data AND accessor) in creation order — the
/// order of the bag's key list. A deleted key is gone from that order; a
/// key defined again after a delete is a new key and comes last.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_own_key_names(ptr: usize) -> Vec<String> {
    bag_own_key_names_bytes(ptr)
        .into_iter()
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect()
}

pub(crate) unsafe fn bag_own_key_names_bytes(ptr: usize) -> Vec<Vec<u8>> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let live = crate::object::object_live_slot_count(bag);
    let mut out = Vec::new();
    for i in 0..keys.count() {
        if !crate::object::key_attrs::key_is_accessor_at(arr, i)
            && crate::object::object_key_field_at_with_keys(bag, arr, i, live).bits()
                == crate::value::TAG_HOLE
        {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
                continue;
            }
            out.push(bytes.to_vec());
        }
    }
    out
}

/// Every own data property in ECMA-262 own-key order: integer indices
/// ascending, then other strings in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_snapshot(ptr: usize) -> Vec<(String, f64)> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let live = crate::object::object_live_slot_count(bag);
    let mut indexed: Vec<(u32, String, f64)> = Vec::new();
    let mut strings: Vec<(String, f64)> = Vec::new();
    for i in 0..keys.count() {
        let value = crate::object::object_key_field_at_with_keys(bag, arr, i, live);
        if value.bits() == crate::value::TAG_HOLE
            || crate::object::key_attrs::key_is_accessor_at(arr, i)
        {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) else {
            continue;
        };
        if crate::object::field_get_set::own_key_hidden_bytes(bag, bytes) {
            continue;
        }
        let name = String::from_utf8_lossy(bytes).into_owned();
        let v = f64::from_bits(value.bits());
        match crate::object::canonical_array_index(&name) {
            Some(index) => indexed.push((index, name, v)),
            None => strings.push((name, v)),
        }
    }
    crate::cold_sort::sort_by_key(&mut indexed, |(index, _, _)| *index);
    indexed
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(strings)
        .collect()
}

/// The closure's internal state record (null when none).
unsafe fn state_of(ptr: usize) -> *mut ObjectHeader {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return std::ptr::null_mut();
    }
    crate::object::cell_expando_get(bag as usize).unwrap_or(std::ptr::null_mut())
}

unsafe fn state_ensure(ptr: usize) -> Option<*mut ObjectHeader> {
    let bag = bag_ensure(ptr);
    let existing = state_of(ptr);
    if !existing.is_null() {
        return Some(existing);
    }
    // A null-prototype record (its `"p"` key is not a JS property, and no
    // inherited setter may run on it), hung off the bag's `meta.expando` —
    // an ordinary traced edge of the bag.
    let meta = crate::object::object_meta_ensure(
        crate::meta_obj_runtime_born!(bag),
        crate::object::MetaReason::ClosureProps,
    );
    if meta.is_null() {
        return None;
    }
    let state = crate::object::js_object_alloc_null_proto(0, 0);
    let meta = (*bag).meta;
    let boxed = crate::value::js_nanbox_pointer(state as i64).to_bits();
    // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object barrier.
    (*meta).expando = boxed;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).expando as *const _ as usize,
        boxed,
    );
    Some(state)
}

/// Is the #3655 deleted marker for `key` set on the closure at `ptr`?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_is_deleted(ptr: usize, key: &str) -> bool {
    state_is_deleted_bytes(ptr, key.as_bytes())
}

pub(crate) unsafe fn state_is_deleted_bytes(ptr: usize, key: &[u8]) -> bool {
    let state = state_of(ptr);
    if state.is_null() {
        return false;
    }
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    object_own_get(state, &marker).is_some()
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_mark_deleted(ptr: usize, key: &str) {
    state_mark_deleted_bytes(ptr, key.as_bytes());
}

pub(crate) unsafe fn state_mark_deleted_bytes(ptr: usize, key: &[u8]) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    object_own_set_bytes(state, &marker, f64::from_bits(crate::value::TAG_TRUE));
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_clear_deleted(ptr: usize, key: &str) {
    state_clear_deleted_bytes(ptr, key.as_bytes());
}

pub(crate) unsafe fn state_clear_deleted_bytes(ptr: usize, key: &[u8]) {
    if !state_is_deleted_bytes(ptr, key) {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    let mut marker = Vec::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.extend_from_slice(DELETED_PREFIX.as_bytes());
    marker.extend_from_slice(key);
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
}

/// The recorded `[[Prototype]]` bits, if any.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_prototype(ptr: usize) -> Option<u64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    object_own_get(state, STATE_PROTO.as_bytes()).map(f64::to_bits)
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_set_prototype(ptr: usize, proto_bits: u64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, STATE_PROTO, f64::from_bits(proto_bits));
}

/// A runtime-internal own slot of the function object — never a JS property
/// (class private statics, computed-key records, class captures): kept in the
/// state record under `"i:" + key`, so no reflection or enumeration of the
/// function can reach it.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_get(ptr: usize, key: &str) -> Option<f64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    let mut marker = String::with_capacity(INTERNAL_PREFIX.len() + key.len());
    marker.push_str(INTERNAL_PREFIX);
    marker.push_str(key);
    object_own_get(state, marker.as_bytes())
}

/// [`state_internal_get`] of the key `head + tail`, spelled without a heap
/// allocation for short keys.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_get_joined(
    ptr: usize,
    head: &[u8],
    tail: &[u8],
) -> Option<f64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    let len = INTERNAL_PREFIX.len() + head.len() + tail.len();
    let mut stack = [0u8; 96];
    let mut heap = Vec::new();
    let buf: &mut [u8] = if len <= stack.len() {
        &mut stack[..len]
    } else {
        heap.resize(len, 0);
        &mut heap[..]
    };
    let (prefix, rest) = buf.split_at_mut(INTERNAL_PREFIX.len());
    prefix.copy_from_slice(INTERNAL_PREFIX.as_bytes());
    let (h, t) = rest.split_at_mut(head.len());
    h.copy_from_slice(head);
    t.copy_from_slice(tail);
    object_own_get(state, buf)
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_set(ptr: usize, key: &str, value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, &format!("{INTERNAL_PREFIX}{key}"), value);
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_remove(ptr: usize, key: &str) -> bool {
    if state_internal_get(ptr, key).is_none() {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    let marker = format!("{INTERNAL_PREFIX}{key}");
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
    true
}

/// The number of keys of the closure's internal state record (tests).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cfg(test)]
pub(crate) unsafe fn state_key_count(ptr: usize) -> usize {
    let state = state_of(ptr);
    if state.is_null() {
        return 0;
    }
    crate::object::object_keys(state).count() as usize
}

/// True when the closure carries internal state a base/keyed Function shape
/// cannot describe (a deleted marker or a recorded prototype).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn has_state(ptr: usize) -> bool {
    !state_of(ptr).is_null()
}
