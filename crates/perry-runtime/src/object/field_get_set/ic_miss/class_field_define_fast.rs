// DefineField for a class instance whose layout the constructor's inline
// stores do not cover (#10508).
//
// `js_class_field_add` is `CreateDataPropertyOrThrow(receiver, key, value)`.
// Its general route builds a descriptor object and runs the whole
// `Object.defineProperty` algorithm, about 23,000 instructions per field; a
// `class Q extends EventEmitter` with twelve declared fields paid that twelve
// times per `new Q()`, because after `super()` the instance is no longer on the
// layout the compiler's inline field stores assume.
//
// For the common receiver the specification's answer is fixed, and it consults
// nothing on the prototype chain: on an ordinary, extensible object,
// OrdinaryDefineOwnProperty either creates `key` as a data property with
// `{ writable, enumerable, configurable }` all true, or — when `key` is already
// an own data property with exactly those attributes (a declared field the
// constructor pre-allocated, a key a base constructor assigned) — replaces its
// value and leaves the attributes as they are. Both are what
// `define_property_force_store_value` does (ensure the key, store the value);
// all-true attributes are the default that needs no attribute entry. Anything
// this file cannot prove — a Proxy, an exotic or non-extensible object, an own
// key with any attribute or accessor entry, a prototype object — takes the
// general route unchanged.

/// Define `key = value` on `receiver` per DefineField when the receiver and key
/// qualify; `false` means "not handled", never "failed".
pub(super) fn try_define_new_class_field(receiver: f64, key: f64, value: f64) -> bool {
    unsafe {
        let key_value = crate::value::JSValue::from_bits(key.to_bits());
        if !key_value.is_string() {
            return false;
        }
        let key_str = key_value.as_string_ptr();
        if key_str.is_null() {
            return false;
        }
        let key_len = (*key_str).byte_len as usize;
        let key_bytes = std::slice::from_raw_parts(
            (key_str as *const u8).add(std::mem::size_of::<crate::string::StringHeader>()),
            key_len,
        );
        // Index-like names take the general route (array-index semantics).
        if key_bytes.is_empty() || key_bytes[0].is_ascii_digit() {
            return false;
        }
        let bits = receiver.to_bits();
        if (bits >> 48) != (crate::value::POINTER_TAG >> 48) {
            return false;
        }
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        if !crate::value::addr_class::is_above_handle_band(addr)
            || crate::object::exotic_expando::exotic_expando_kind(addr).is_some()
        {
            return false;
        }
        let Some(obj) = crate::object::prototype_chain::meta_capable_object(addr) else {
            return false;
        };
        if !crate::object::object_is_regular(obj) || crate::object::dictionary::is_dictionary(obj) {
            return false;
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return false;
        };
        let immutability =
            crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & immutability != 0
        {
            return false;
        }
        // Only instances of a declared class: never a prototype object, an
        // object literal or a class object.
        let class_id = (*obj).class_id;
        if class_id == 0
            || crate::object::class_registry::is_anon_shape_class_id(class_id)
            || crate::object::class_registry::is_registered_class_prototype_object(addr)
            || crate::object::class_registry::class_id_for_decl_prototype_object(addr).is_some()
        {
            return false;
        }
        // No metadata beyond overflow storage, and no possible descriptor entry
        // for this key (a clear summary bit is authoritative).
        let meta = (*obj).meta;
        if !meta.is_null() {
            let key_bit = 1u64 << (crate::object::key_bytes_hash(key_bytes.as_ptr(), key_len) & 63);
            if (*meta).prototype != 0
                || (*meta).flags != 0
                || (*meta).private_evaluation_brand != 0
                || (*meta).attr_key_bits & key_bit != 0
                || (*meta).accessor_key_bits & key_bit != 0
            {
                return false;
            }
        }
        // The receiver shape answers the definition when it can: the key-add
        // edge for a key the shape lacks, or the own data slot of a key it
        // holds with default attributes (`define_own_data`). No byte compare.
        if crate::object::define_own_data::define_own_data_from_shape(receiver, key, value)
            .is_some()
        {
            return true;
        }
        // The shape's key list names every own key, inline or overflow, data or
        // accessor. An absent key is created; a present one must be a data
        // property with the default attributes (the constructor allocated the
        // declared field, or a base constructor assigned it), which the define
        // only overwrites.
        let Some(descriptor) = crate::object::shapes::object_shape_descriptor(obj) else {
            return false;
        };
        let mut present = false;
        let keys = descriptor.keys as usize as *mut crate::array::ArrayHeader;
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0
                || !crate::value::addr_class::is_above_handle_band(keys_ptr)
            {
                return false;
            }
            let key_count = descriptor.logical_key_count as usize;
            let (slots, slot_len) = crate::object::keys_array_dense_slots_resolved(keys);
            if key_count > slot_len {
                return false;
            }
            for i in 0..key_count {
                let stored = crate::JSValue::from_bits((*slots.add(i)).to_bits());
                if crate::string::js_string_key_matches_bytes(stored, key_bytes) {
                    present = true;
                    break;
                }
            }
        }
        if present {
            let Ok(name) = std::str::from_utf8(key_bytes) else {
                return false;
            };
            if crate::object::descriptor_state::get_property_attrs(addr, name).is_some()
                || crate::object::descriptor_state::get_accessor_descriptor(addr, name).is_some()
            {
                return false;
            }
        }
        crate::object::object_ops::define_property_force_store_value(
            obj,
            key_str as *const crate::StringHeader,
            value,
        );
        true
    }
}
