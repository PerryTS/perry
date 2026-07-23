//! Array-specific branches for `Object.*` operations.
//!
//! Split out of `object_ops.rs` to keep that file under the repository
//! line-count guard while preserving the public FFI entry points there.

use super::*;

unsafe fn is_array_object(obj: *const ObjectHeader) -> bool {
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
}

/// Apply `Object.freeze` / `Object.seal` to an array's OWN index + named data
/// properties. The generic `mark_all_keys` walks `(*obj).keys_array`, but an
/// array's indices live in the dense element store and its named props in the
/// `ARRAY_NAMED_PROPS` side table — neither appears in `keys_array` — so
/// freeze/seal historically missed them, leaving a frozen array's elements
/// writable/configurable. Returns `true` when `obj` is an array (handled here),
/// `false` otherwise so the caller can fall back to the ordinary key walk.
pub(crate) unsafe fn mark_all_array_props(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    drop_configurable: bool,
) -> bool {
    if !is_array_object(obj) {
        return false;
    }
    // Any explicit per-index/named attribute override makes the raw numeric
    // fast paths ineligible — gate them on the descriptor flag so the recorded
    // non-writable/non-configurable attrs are actually honored on read/write.
    {
        let gc = gc_header_for(obj);
        (*gc)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
    }
    let arr = obj as *const crate::array::ArrayHeader;
    let addr = obj as usize;
    let apply = |key: String| {
        let mut attrs =
            super::get_property_attrs(addr, &key).unwrap_or(PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
        super::set_property_attrs(addr, key, attrs);
    };
    let len = (*arr).length;
    for i in 0..len {
        apply(i.to_string());
    }
    for name in crate::array::array_named_property_names(arr, false) {
        apply(name);
    }
    true
}

pub(crate) unsafe fn array_property_is_enumerable(
    obj: *mut ObjectHeader,
    key_str: *const crate::StringHeader,
    key_name: &str,
) -> Option<f64> {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if !is_array_object(obj) {
        return None;
    }
    if key_name == "length" {
        return Some(f64::from_bits(TAG_FALSE));
    }
    let arr = obj as *const crate::array::ArrayHeader;
    if !super::has_own_helpers::array_own_key_present(arr, key_str) {
        return Some(f64::from_bits(TAG_FALSE));
    }
    // Both index and named properties default to enumerable when no explicit
    // descriptor was recorded; an index redefined via
    // `Object.defineProperty(arr, i, { enumerable: false })` carries a
    // side-table entry that must be honored (it previously hard-coded `true`
    // for canonical indices, so a non-enumerable index still reported `true`).
    let enumerable = super::get_property_attrs(obj as usize, key_name)
        .map(|attrs| attrs.enumerable())
        .unwrap_or(true);
    Some(f64::from_bits(if enumerable {
        TAG_TRUE
    } else {
        TAG_FALSE
    }))
}

/// ToUint32 (ECMA-262 §7.1.6) of an already-`ToNumber`-coerced value.
fn to_uint32(number: f64) -> u32 {
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    number.trunc().rem_euclid(4_294_967_296.0) as u32
}

/// `ArraySetLength(A, Desc)` (ECMA-262 §10.4.2.4): the array exotic
/// `[[DefineOwnProperty]]` for the `"length"` property. The `length` property
/// is a non-configurable, non-enumerable data property; its writability is
/// tracked in the property-attrs side table (absent ⇒ writable). Returns `true`
/// if the definition succeeds, `false` if it must be rejected (the caller turns
/// that into a thrown `TypeError` for `Object.defineProperty` or a `false`
/// return for `Reflect.defineProperty`). A non-integer / out-of-range length
/// throws a `RangeError`, which propagates through both callers.
pub(crate) unsafe fn array_set_length_from_descriptor(
    obj: *mut ObjectHeader,
    descriptor_value: f64,
) -> bool {
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return true;
    }
    // A customized `length` (e.g. writable:false) gates the raw numeric
    // fast paths — see OBJ_FLAG_ARRAY_DESCRIPTORS in define_array_property.
    // Set here too so the `Reflect.defineProperty` entry point is covered.
    {
        let gc = gc_header_for(obj);
        (*gc)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
    }
    let arr = obj as *mut crate::array::ArrayHeader;

    let read_present = |name: &[u8]| -> bool {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        own_key_present(desc_ptr, k)
    };
    let read_bool = |name: &[u8]| -> bool {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
        crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0
    };

    let has_get = read_present(b"get");
    let has_set = read_present(b"set");
    let has_accessor = has_get || has_set;
    let has_value = read_present(b"value");
    let has_writable = read_present(b"writable");
    let new_writable = has_writable && read_bool(b"writable");
    let has_enumerable = read_present(b"enumerable");
    let new_enumerable = has_enumerable && read_bool(b"enumerable");
    let has_configurable = read_present(b"configurable");
    let new_configurable = has_configurable && read_bool(b"configurable");

    // Steps 3-5 (only when a value is supplied): ToUint32 then ToNumber, in that
    // order — each runs `ToNumber` on the descriptor's `value`, so a `valueOf`
    // observer is invoked exactly twice and may mutate the array between calls.
    // Read the current `length` descriptor AFTER both coercions so such a
    // mutation (e.g. flipping `writable` to false) is honored.
    let new_len: Option<u32> = if has_value {
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
        let value = f64::from_bits(value_field.bits());
        let uint = to_uint32(crate::builtins::js_number_coerce(value));
        let number = crate::builtins::js_number_coerce(value);
        // SameValueZero(newLen, numberLen): a fractional / out-of-range length
        // is a RangeError.
        if (uint as f64) != number {
            crate::array::array_length_range_error();
        }
        Some(uint)
    } else {
        None
    };

    let old_len = (*arr).length;
    // `length` is non-configurable, non-enumerable; writable defaults to true
    // until explicitly set otherwise via the side table.
    let cur_writable = super::get_property_attrs(obj as usize, "length")
        .map(|a| a.writable())
        .unwrap_or(true);

    // ValidateAndApplyPropertyDescriptor against the current (non-configurable
    // data) `length` descriptor.
    if has_configurable && new_configurable {
        return false; // can't make a non-configurable property configurable
    }
    if has_enumerable && new_enumerable {
        return false; // can't make a non-enumerable property enumerable
    }
    if has_accessor {
        return false; // can't turn a non-configurable data prop into an accessor
    }
    if !cur_writable {
        if has_writable && new_writable {
            return false; // can't re-enable writability on a non-configurable prop
        }
        if let Some(n) = new_len {
            if n != old_len {
                return false; // can't change the value of a non-writable length
            }
        }
    }

    // Apply. Growing pads with holes. Shrinking must delete the now-out-of-range
    // indices from the TOP down (ECMA-262 10.4.2.4 ArraySetLength steps 15-17):
    // if a deletion target is a NON-configurable index, `length` can only shrink
    // to just above it and the operation is rejected. `js_array_set_length`
    // doesn't consult the per-index descriptor side table, so do the spec walk
    // here. The new writability (if `writable:false` was requested) is persisted
    // even on the reject path, matching the spec's step-16/17 ordering.
    let mut rejected = false;
    if let Some(n) = new_len {
        if n < old_len {
            // Find the highest non-configurable index in [n, old_len): length
            // can shrink no further than one past it.
            let mut target = n;
            let mut i = old_len;
            while i > n {
                i -= 1;
                let key = i.to_string();
                let configurable = super::get_property_attrs(obj as usize, &key)
                    .map(|a| a.configurable())
                    .unwrap_or(true);
                if !configurable {
                    target = i + 1;
                    rejected = true;
                    break;
                }
            }
            // Drop the per-index descriptor entries for the indices actually
            // removed so stale attrs can't resurrect a deleted index.
            let mut j = old_len;
            while j > target {
                j -= 1;
                super::clear_property_attrs(obj as usize, &j.to_string());
            }
            crate::array::js_array_set_length(arr, target as f64);
        } else {
            crate::array::js_array_set_length(arr, n as f64);
        }
    }
    if has_writable && !new_writable {
        // A `writable:false` length define is applied even when a shrink was
        // rejected (the property becomes non-writable; only the truncation
        // partially failed).
        super::set_property_attrs(
            obj as usize,
            "length".to_string(),
            PropertyAttrs::new(false, false, false),
        );
    } else if has_writable {
        super::set_property_attrs(
            obj as usize,
            "length".to_string(),
            PropertyAttrs::new(new_writable, false, false),
        );
    }
    !rejected
}

/// `Reflect.defineProperty` hook for the array `length` property. Returns
/// `Some(ok)` only when `obj_value` is an array and `key_value` is `"length"`,
/// so non-length array defines keep flowing through the ordinary path.
pub(crate) unsafe fn array_length_reflect_define(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> Option<bool> {
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() || !is_array_object(obj) {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(key_value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    let key_name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)).ok()?;
    if key_name != "length" {
        return None;
    }
    Some(array_set_length_from_descriptor(obj, descriptor_value))
}

pub(crate) unsafe fn define_array_property(
    obj: *mut ObjectHeader,
    obj_value: f64,
    key_str: *const crate::StringHeader,
    key_name: Option<&str>,
    descriptor_value: f64,
) -> Option<bool> {
    if !is_array_object(obj) {
        return None;
    }
    let Some(key_name) = key_name else {
        return Some(true);
    };

    // Any explicit per-index/named/length descriptor makes the raw numeric
    // fast paths ineligible for this array — they can't see accessors or
    // attribute overrides, so they must decline to the descriptor-aware
    // element get/set (OBJ_FLAG_ARRAY_DESCRIPTORS gates them).
    {
        let gc = gc_header_for(obj);
        (*gc)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
    }

    if key_name == "length" {
        return Some(array_set_length_from_descriptor(obj, descriptor_value));
    }

    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return Some(true);
    }
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    // `ToPropertyDescriptor` field presence is HasProperty (own OR inherited).
    let has_value = super::desc_has_field(descriptor_value, b"value");
    let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
    let value = if has_value {
        f64::from_bits(value_field.bits())
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };

    let arr = obj as *mut crate::array::ArrayHeader;

    let read_bool = |name: &[u8]| -> Option<bool> {
        if !super::desc_has_field(descriptor_value, name) {
            return None;
        }
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
        Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
    };

    // A NEW own property on a non-extensible array is forbidden (ECMA-262
    // OrdinaryDefineOwnProperty: `extensible` false + no current property ⇒
    // reject). `Object.preventExtensions`/`seal`/`freeze` set the flag; an
    // existing index/named property can still be redefined within the spec
    // bounds. (test262 defineProperty/15.2.3.6-4-198 and friends.)
    {
        let named_exists = super::canonical_array_index(key_name).is_none()
            && (super::get_accessor_descriptor(obj as usize, key_name).is_some()
                || crate::array::array_named_property_get_by_name(arr, key_name).is_some());
        let index_exists = super::canonical_array_index(key_name)
            .map(|_| super::has_own_helpers::array_own_key_present(arr, key_str))
            .unwrap_or(false);
        if !named_exists && !index_exists {
            let gc = gc_header_for(obj);
            if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
                super::throw_object_type_error_with_suffix(
                    "Cannot define property ",
                    &format!("{key_name}, object is not extensible"),
                );
            }
        }
    }

    if let Some(index) = super::canonical_array_index(key_name) {
        let exists = super::has_own_helpers::array_own_key_present(arr, key_str);

        // Array exotic `[[DefineOwnProperty]]` (ECMA-262 10.4.2.1) step 3.b: a
        // NEW index at or beyond `length` requires extending `length`, which is
        // forbidden when the `length` property is non-writable — reject (the
        // caller turns this into a `TypeError`).
        if !exists && index >= (*arr).length {
            let len_writable = super::get_property_attrs(obj as usize, "length")
                .map(|a| a.writable())
                .unwrap_or(true);
            if !len_writable {
                return Some(false);
            }
        }

        // Accessor descriptor on an array index: store get/set in the side table
        // (the dense element store can't hold a getter/setter). Routing this
        // through the generic object path would deref the array as an
        // ObjectHeader and corrupt it, so handle it here.
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
        let desc_has_get = super::desc_has_field(descriptor_value, b"get");
        let desc_has_set = super::desc_has_field(descriptor_value, b"set");
        if desc_has_get || desc_has_set {
            // ValidateAndApplyPropertyDescriptor for an existing non-configurable
            // index: reject the data→accessor switch AND a change to a
            // non-configurable accessor's `get`/`set` (or a forbidden
            // enumerable/configurable change). The historical check only
            // rejected the data→accessor case, so redefining a non-configurable
            // accessor index with a different setter silently succeeded.
            if exists {
                let cur = super::get_property_attrs(obj as usize, key_name)
                    .unwrap_or_else(|| PropertyAttrs::new(true, true, true));
                if !cur.configurable() {
                    let cur_accessor = super::get_accessor_descriptor(obj as usize, key_name);
                    let cur_value = if cur_accessor.is_none() {
                        crate::array::js_array_get_f64(arr, index)
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    };
                    super::validate_nonconfigurable_redefine(
                        key_name,
                        cur,
                        cur_accessor,
                        cur_value,
                        descriptor_value,
                        None,
                    );
                }
            }
            let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
            let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            let prior = super::get_accessor_descriptor(obj as usize, key_name);
            let get_bits = if desc_has_get {
                if get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), recv)
                }
            } else {
                prior.map(|a| a.get).unwrap_or(0)
            };
            let set_bits = if desc_has_set {
                if set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), recv)
                }
            } else {
                prior.map(|a| a.set).unwrap_or(0)
            };
            // Materialize BEFORE storing the accessor — the extend helper
            // dispatches accessor setters, so installing the accessor first
            // would turn this internal materialization into a setter call.
            //
            // Extending `length` past the dense capacity reallocates the array
            // (header + inline elements are one allocation), leaving a
            // forwarding pointer at the old address. The side tables
            // (ACCESSOR_DESCRIPTORS / property attrs / OBJ_FLAG) are keyed by
            // address, and every later read resolves through `clean_arr_ptr` to
            // the NEW home — so they must be keyed to that canonical address,
            // not the stale `obj`. Without this, a length-extending accessor
            // (`Object.defineProperty(arr, "20", {get})` on a short array) was
            // silently dropped: `arr[20]` / `indexOf` never fired the getter.
            let mut side_addr = obj as usize;
            if !exists {
                let new_arr = crate::array::js_array_set_f64_extend(
                    arr,
                    index,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                let canonical = crate::array::clean_arr_ptr(new_arr);
                if !canonical.is_null() && canonical as usize != side_addr {
                    let gc = gc_header_for(canonical as *const ObjectHeader);
                    (*gc)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
                    side_addr = canonical as usize;
                }
            }
            set_accessor_descriptor(
                side_addr,
                key_name.to_string(),
                AccessorDescriptor {
                    get: get_bits,
                    set: set_bits,
                },
            );
            // Retain existing attrs the descriptor omits when redefining; new
            // accessor defaults to non-enumerable / non-configurable. An
            // existing dense element with no side-table entry has default
            // all-true attributes (so data→accessor keeps enumerable:true).
            let cur = if exists {
                Some(
                    super::get_property_attrs(side_addr, key_name)
                        .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                )
            } else {
                None
            };
            let enumerable = read_bool(b"enumerable")
                .unwrap_or_else(|| cur.map(|a| a.enumerable()).unwrap_or(false));
            let configurable = read_bool(b"configurable")
                .unwrap_or_else(|| cur.map(|a| a.configurable()).unwrap_or(false));
            set_property_attrs(
                side_addr,
                key_name.to_string(),
                PropertyAttrs::new(false, enumerable, configurable),
            );
            return Some(true);
        }

        // The element's current attributes: an explicit side-table entry wins;
        // otherwise a present dense element defaults to all-true (writable,
        // enumerable, configurable).
        let cur_attrs: Option<PropertyAttrs> = if exists {
            Some(
                super::get_property_attrs(obj as usize, key_name)
                    .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
            )
        } else {
            None
        };

        // ValidateAndApplyPropertyDescriptor for the existing-non-configurable
        // case: reject the spec-forbidden changes (make configurable, flip
        // enumerable, re-enable writability, or change a non-writable value).
        if let Some(cur) = cur_attrs {
            if !cur.configurable() {
                if read_bool(b"configurable") == Some(true) {
                    return Some(false);
                }
                if let Some(want_enum) = read_bool(b"enumerable") {
                    if want_enum != cur.enumerable() {
                        return Some(false);
                    }
                }
                if !cur.writable() {
                    if read_bool(b"writable") == Some(true) {
                        return Some(false);
                    }
                    if has_value {
                        let cur_value = crate::array::js_array_get_f64(arr, index);
                        if js_object_is(value, cur_value).to_bits()
                            != crate::value::JSValue::bool(true).bits()
                        {
                            return Some(false);
                        }
                    }
                }
            }
        }

        // A GENERIC descriptor (attrs only, no value/writable/get/set) on an
        // existing ACCESSOR property just updates the attributes — it must
        // NOT convert the accessor back to data (spec ValidateAndApply step:
        // IsGenericDescriptor → no [[Get]]/[[Set]]/[[Value]] changes).
        if !has_value
            && !super::desc_has_field(descriptor_value, b"writable")
            && super::get_accessor_descriptor(obj as usize, key_name).is_some()
        {
            let cur = cur_attrs.unwrap_or(PropertyAttrs::new(false, false, false));
            let enumerable = read_bool(b"enumerable").unwrap_or_else(|| cur.enumerable());
            let configurable = read_bool(b"configurable").unwrap_or_else(|| cur.configurable());
            set_property_attrs(
                obj as usize,
                key_name.to_string(),
                PropertyAttrs::new(false, enumerable, configurable),
            );
            return Some(true);
        }

        // Redefining an index that was previously an accessor back to a data
        // property: drop the stale accessor entry.
        crate::state::state()
            .descriptors
            .accessor_descriptors
            .borrow_mut()
            .remove(&(obj as usize, key_name.to_string()));
        // [[DefineOwnProperty]] writes the slot directly — clear any stale
        // attrs first so the extend helper's [[Set]]-side writability check
        // (added for ordinary `arr[i] = v` writes) can't reject this store.
        // The final attributes are recorded below after the write.
        super::clear_property_attrs(obj as usize, key_name);

        if has_value {
            crate::array::js_array_set_f64_extend(arr, index, value);
        } else if !exists {
            // A NEW index defined with an attributes-only / generic descriptor
            // (`Object.defineProperty(arr, i, { enumerable: true })`, no `value`)
            // still becomes an own data property whose value defaults to
            // `undefined`. Materialize the slot so the index counts as an own
            // property for reflection (`hasOwnProperty`, `verifyProperty`).
            crate::array::js_array_set_f64_extend(
                arr,
                index,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
        }

        // Compute final attributes. New property: omitted ⇒ false. Redefine:
        // omitted ⇒ retain current.
        let writable = read_bool(b"writable")
            .unwrap_or_else(|| cur_attrs.map(|a| a.writable()).unwrap_or(false));
        let enumerable = read_bool(b"enumerable")
            .unwrap_or_else(|| cur_attrs.map(|a| a.enumerable()).unwrap_or(false));
        let configurable = read_bool(b"configurable")
            .unwrap_or_else(|| cur_attrs.map(|a| a.configurable()).unwrap_or(false));
        set_property_attrs(
            obj as usize,
            key_name.to_string(),
            PropertyAttrs::new(writable, enumerable, configurable),
        );
        let _ = obj_value;
        return Some(true);
    }

    // ValidateAndApplyPropertyDescriptor for an EXISTING non-configurable named
    // (non-index) own property on an array. The index path above performs this
    // check; the named path historically did not, so a redefine of a
    // non-configurable `arr.prop` (data or accessor) silently succeeded instead
    // of throwing a TypeError.
    {
        let cur_accessor = super::get_accessor_descriptor(obj as usize, key_name);
        let cur_attrs = super::get_property_attrs(obj as usize, key_name);
        if cur_attrs.is_some() || cur_accessor.is_some() {
            let attrs = cur_attrs.unwrap_or_else(|| PropertyAttrs::new(true, true, true));
            if !attrs.configurable() {
                let cur_value = if cur_accessor.is_none() {
                    crate::array::array_named_property_get_by_name(arr, key_name)
                        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                super::validate_nonconfigurable_redefine(
                    key_name,
                    attrs,
                    cur_accessor,
                    cur_value,
                    descriptor_value,
                    None,
                );
            }
        }
    }

    // Named (non-index) accessor on an array target: store get/set in the
    // side table, exactly like the index path above. Without this, a
    // `defineProperty(arr, "prop", {get,set})` silently stored `undefined`
    // as a data property and dropped the accessors.
    {
        let desc_has_get = super::desc_has_field(descriptor_value, b"get");
        let desc_has_set = super::desc_has_field(descriptor_value, b"set");
        if desc_has_get || desc_has_set {
            let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
            let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
            let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
            let recv = crate::value::js_nanbox_pointer(obj as i64);
            let prior = super::get_accessor_descriptor(obj as usize, key_name);
            let get_bits = if desc_has_get {
                if get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), recv)
                }
            } else {
                prior.map(|a| a.get).unwrap_or(0)
            };
            let set_bits = if desc_has_set {
                if set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), recv)
                }
            } else {
                prior.map(|a| a.set).unwrap_or(0)
            };
            // Retain attrs the descriptor omits when redefining; a new accessor
            // (or one replacing a data property) defaults to
            // non-enumerable/non-configurable. An existing data property with no
            // side-table entry has default all-true attributes, so a
            // data→accessor conversion keeps `enumerable`/`configurable`.
            let existed = prior.is_some()
                || super::get_property_attrs(obj as usize, key_name).is_some()
                || crate::array::array_named_property_get_by_name(arr, key_name).is_some();
            let cur = if existed {
                Some(
                    super::get_property_attrs(obj as usize, key_name)
                        .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                )
            } else {
                None
            };
            set_accessor_descriptor(
                obj as usize,
                key_name.to_string(),
                AccessorDescriptor {
                    get: get_bits,
                    set: set_bits,
                },
            );
            let enumerable = read_bool(b"enumerable")
                .unwrap_or_else(|| cur.map(|a| a.enumerable()).unwrap_or(false));
            let configurable = read_bool(b"configurable")
                .unwrap_or_else(|| cur.map(|a| a.configurable()).unwrap_or(false));
            set_property_attrs(
                obj as usize,
                key_name.to_string(),
                PropertyAttrs::new(false, enumerable, configurable),
            );
            let _ = obj_value;
            return Some(true);
        }
    }

    // ValidateAndApplyPropertyDescriptor merge for a named (non-index) data
    // property: a redefine keeps the attributes (and the value) the descriptor
    // omits, while a brand-new property defaults omitted fields to false. The
    // index path above already does this; the named path historically reset
    // every omitted field to false and overwrote the value with `undefined`
    // (dropping `arr.prop = 12` then `defineProperty(arr,"prop",{writable:false})`
    // back to `{value: undefined}`). Mirror the index-path merge here.
    let cur_accessor = super::get_accessor_descriptor(obj as usize, key_name);
    let exists = cur_accessor.is_some()
        || crate::array::array_named_property_get_by_name(arr, key_name).is_some();
    let cur_attrs: Option<PropertyAttrs> = if exists {
        Some(
            super::get_property_attrs(obj as usize, key_name)
                .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
        )
    } else {
        None
    };

    // A GENERIC descriptor (only enumerable/configurable — no value/writable,
    // and no get/set, since accessor descriptors returned above) on an existing
    // ACCESSOR property must only update the attributes; it must NOT convert the
    // accessor back to a data property (spec ValidateAndApplyPropertyDescriptor:
    // IsGenericDescriptor leaves [[Get]]/[[Set]] intact). Mirrors the index path.
    if cur_accessor.is_some() && !has_value && !super::desc_has_field(descriptor_value, b"writable")
    {
        let cur = cur_attrs.unwrap_or_else(|| PropertyAttrs::new(false, false, false));
        let enumerable = read_bool(b"enumerable").unwrap_or_else(|| cur.enumerable());
        let configurable = read_bool(b"configurable").unwrap_or_else(|| cur.configurable());
        set_property_attrs(
            obj as usize,
            key_name.to_string(),
            PropertyAttrs::new(false, enumerable, configurable),
        );
        let _ = obj_value;
        return Some(true);
    }

    // Redefining a former accessor back to a data property drops the stale
    // accessor entry (the non-configurable case already threw above).
    if cur_accessor.is_some() {
        crate::state::state()
            .descriptors
            .accessor_descriptors
            .borrow_mut()
            .remove(&(obj as usize, key_name.to_string()));
    }

    // Write the value: an explicit `value` wins; a NEW property with no value
    // defaults to `undefined`; a redefine that omits `value` keeps the current.
    if has_value {
        crate::array::array_named_property_set(arr, key_str, value);
    } else if !exists {
        crate::array::array_named_property_set(
            arr,
            key_str,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }

    let writable =
        read_bool(b"writable").unwrap_or_else(|| cur_attrs.map(|a| a.writable()).unwrap_or(false));
    let enumerable = read_bool(b"enumerable")
        .unwrap_or_else(|| cur_attrs.map(|a| a.enumerable()).unwrap_or(false));
    let configurable = read_bool(b"configurable")
        .unwrap_or_else(|| cur_attrs.map(|a| a.configurable()).unwrap_or(false));
    set_property_attrs(
        obj as usize,
        key_name.to_string(),
        PropertyAttrs::new(writable, enumerable, configurable),
    );
    let _ = obj_value;
    Some(true)
}

fn builtin_constructor_prototype_value(name: &[u8]) -> Option<f64> {
    let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
    proto_value.is_pointer().then_some(proto)
}

pub(crate) fn array_get_prototype_of_addr(raw_addr: usize) -> Option<f64> {
    if let Some(array_proto) = builtin_constructor_prototype_value(b"Array") {
        let proto_addr = crate::value::js_nanbox_get_pointer(array_proto) as usize;
        if proto_addr != raw_addr {
            return Some(array_proto);
        }
    }
    builtin_constructor_prototype_value(b"Object")
}
