//! Descriptor validation + throw helpers backing `Object.defineProperty` /
//! `Object.create` / `Object.defineProperties` (moved out of `object_ops.rs`).
use super::super::*;
use super::*;
/// Throw a `TypeError` with the given UTF-8 message bytes. Used by the
/// `Object.defineProperty` / `Object.create` descriptor + invariant validation
/// paths (#2817 / #2843 / #2816).
pub(crate) fn throw_object_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
/// Throw `TypeError: <prefix><suffix>` where `suffix` is a runtime-built
/// string (e.g. the offending descriptor value rendered with the same
/// formatting Node uses in its messages). #2817.
pub(crate) fn throw_object_type_error_with_suffix(prefix: &str, suffix: &str) -> ! {
    let full = format!("{prefix}{suffix}");
    let msg = crate::string::js_string_from_bytes(full.as_ptr(), full.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Render a value the way Node does inside its `Object.defineProperty`
/// descriptor TypeError messages (e.g. `Property description must be an
/// object: 1` / `... : undefined` / `Getter must be a function: 1`).
/// Primitives render via their natural string form; objects render as
/// `[object Object]` etc. — but in practice these error paths only fire on
/// primitives, so a simple coercion suffices.
pub(crate) unsafe fn describe_value_for_type_error(value: f64) -> String {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    let s = crate::value::js_jsvalue_to_string(value);
    if s.is_null() {
        return String::new();
    }
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Is `value` a non-nullish object reference that `Object.defineProperty` /
/// `Object.create` accepts as a descriptor / properties bag? (#2817)
/// Functions/closures count as objects too.
pub(crate) unsafe fn value_is_object_like(value: f64) -> bool {
    if crate::typedarray_props::typed_array_addr_from_value(value).is_some() {
        return true;
    }
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        // Module-level raw-I64 object pointers (top16 == 0) — accept if it
        // resolves to a real heap object.
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            return is_valid_obj_ptr(bits as *const u8)
                || crate::closure::is_closure_ptr(bits as usize);
        }
        return false;
    }
    let ptr = jv.as_pointer::<u8>() as usize;
    if ptr < 0x10000 {
        return false;
    }
    is_valid_obj_ptr(ptr as *const u8) || crate::closure::is_closure_ptr(ptr)
}

/// Is `value` callable (a closure / function) — used to validate `get`/`set`
/// descriptor fields. Per spec, an *omitted* (undefined) accessor is allowed;
/// only a present non-callable value throws. (#2817)
pub(crate) unsafe fn value_is_callable(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        return ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr);
    }
    // Class refs (INT32-tagged, top16 == 0x7FFE) are callable constructors.
    (value.to_bits() >> 48) == 0x7FFE
}

pub(crate) unsafe fn registered_buffer_index_own_property_present(
    obj_value: f64,
    key_str: *const crate::StringHeader,
) -> Option<bool> {
    let obj_js = crate::JSValue::from_bits(obj_value.to_bits());
    let raw_buffer_addr = if obj_js.is_pointer() {
        obj_js.as_pointer::<u8>() as usize
    } else {
        let bits = obj_value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            bits as usize
        } else {
            0
        }
    };
    if raw_buffer_addr == 0 || !crate::buffer::is_registered_buffer(raw_buffer_addr) {
        return None;
    }

    // Only answer for canonical *index* keys here. Non-index keys (e.g.
    // `length` or user-defined expandos on a typed array) are owned by the
    // `typedarray_props` registry — returning `Some(false)` for them would
    // shadow that check (`typed_array_has_own_property`) and wrongly report
    // a defined own property as absent. Fall through with `None` instead.
    let idx = super::super::has_own_helpers::str_from_string_header(key_str)
        .and_then(super::super::canonical_array_index)?;
    let buf = raw_buffer_addr as *const crate::buffer::BufferHeader;
    Some(idx < (*buf).length)
}

/// `ToPropertyDescriptor` field presence: `HasProperty(descriptor, name)` —
/// own OR inherited. Spec §6.2.6.5 reads each descriptor field with
/// `HasProperty` then `Get`, so an inherited `value`/`get`/... counts as
/// present (e.g. `Object.defineProperty(o, k, child)` where `child`'s prototype
/// carries `value`). `descriptor_value` is the NaN-boxed descriptor object.
pub(crate) unsafe fn desc_has_field(descriptor_value: f64, name: &[u8]) -> bool {
    // A function object used as a descriptor (`Object.defineProperty(o, k,
    // funObj)`, test262 15.2.3.6-3-139-1 …) is a closure, not an
    // `ObjectHeader`. `js_object_has_property` can't walk a closure's own
    // dynamic props nor its `[[Prototype]]` (`Function.prototype`), so
    // `ToPropertyDescriptor` would miss an inherited `value`/`get`/… field.
    // Route closures through the closure-aware presence check.
    if let Some(ptr) = closure_ptr_from_value(descriptor_value) {
        if let Ok(key_str) = std::str::from_utf8(name) {
            if super::super::has_own_helpers::closure_own_key_present(ptr, key_str) {
                return true;
            }
            // Inherited from `Function.prototype` (and its own chain).
            let fp = crate::object::builtin_prototype_value("Function");
            if value_is_object_like(fp) {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let key_f64 = crate::value::JSValue::string_ptr(key).bits();
                const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
                return crate::object::js_object_has_property(fp, f64::from_bits(key_f64))
                    .to_bits()
                    == TAG_TRUE;
            }
            return false;
        }
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_f64 = crate::value::JSValue::string_ptr(key).bits();
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    crate::object::js_object_has_property(descriptor_value, f64::from_bits(key_f64)).to_bits()
        == TAG_TRUE
}

/// If `value` is a closure (function object), return its heap pointer. Mirrors
/// the closure-pointer recovery used elsewhere in `js_object_define_property`:
/// closures arrive either NaN-boxed with `POINTER_TAG` (function-local) or as a
/// raw in-range I64 (module-level), and `is_closure_ptr` confirms the magic.
pub(crate) unsafe fn closure_ptr_from_value(value: f64) -> Option<usize> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let raw = if jv.is_pointer() {
        jv.as_pointer::<u8>() as usize
    } else {
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            bits as usize
        } else {
            0
        }
    };
    if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
        Some(raw)
    } else {
        None
    }
}

/// `Get(descriptor, name)` as a value-level read. For an ordinary object the raw
/// `js_object_get_field_by_name` read is sufficient, but a closure descriptor
/// (`Object.defineProperty(o, k, funObj)`) requires reading its own dynamic
/// props and then walking its `[[Prototype]]` (`Function.prototype`) — Perry's
/// `[[Get]]` for the descriptor's `value`/`get`/`set`/attribute fields. Returns
/// `undefined` when the field is absent.
pub(crate) unsafe fn desc_read_field(descriptor_value: f64, name: &[u8]) -> crate::value::JSValue {
    if let Some(ptr) = closure_ptr_from_value(descriptor_value) {
        if let Ok(key_str) = std::str::from_utf8(name) {
            if super::super::has_own_helpers::closure_own_key_present(ptr, key_str) {
                let v = crate::closure::closure_get_dynamic_prop(ptr, key_str);
                return crate::value::JSValue::from_bits(v.to_bits());
            }
            let fp = crate::object::builtin_prototype_value("Function");
            let fp_ptr = extract_obj_ptr(fp);
            if !fp_ptr.is_null() {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                return js_object_get_field_by_name(fp_ptr as *const ObjectHeader, key);
            }
            return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
    // The descriptor may be ANY object — a Date, array, RegExp, boxed
    // primitive, typed array, class instance — not just a plain `ObjectHeader`.
    // A raw `js_object_get_field_by_name(ptr as ObjectHeader)` bit-casts e.g. a
    // Date's cell to an `ObjectHeader` and segfaults (test262
    // Object/create/15.2.3.5-4-* and defineProperties exotic-descriptor cases).
    // Read through the value-level `[[Get]]`, which dispatches on the receiver's
    // real type and — matching `desc_has_field`'s `HasProperty` and the spec
    // `ToPropertyDescriptor` — walks the prototype chain and fires accessors.
    if !value_is_object_like(descriptor_value) {
        return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_f64 = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
    let v = crate::object::js_object_get_property_key(descriptor_value, key_f64);
    crate::value::JSValue::from_bits(v.to_bits())
}

/// Whether a property descriptor is enumerable. Mirrors the spec default for
/// `Object.defineProperty` (and `defineProperties`): a descriptor that omits
/// `enumerable` defines a NON-enumerable property, so the default is `false`.
pub(crate) unsafe fn descriptor_enumerable(descriptor_value: f64) -> bool {
    desc_has_field(descriptor_value, b"enumerable")
        && crate::value::js_is_truthy(f64::from_bits(
            desc_read_field(descriptor_value, b"enumerable").bits(),
        )) != 0
}

/// Validate a property descriptor object per ES `ToPropertyDescriptor`
/// invariants that Node surfaces as `TypeError`s (#2817). Assumes
/// `descriptor_value` is already known to be an object. Throws on:
///   - mixing accessor (`get`/`set`) and data (`value`/`writable`) fields,
///   - a present, non-callable `get`,
///   - a present, non-callable `set`.
pub(crate) unsafe fn validate_property_descriptor(descriptor_value: f64) {
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return;
    }
    let desc = desc_ptr as *const ObjectHeader;

    // `ToPropertyDescriptor` field presence is HasProperty (own OR inherited).
    let has_field = |name: &[u8]| -> bool { desc_has_field(descriptor_value, name) };
    let read = |name: &[u8]| -> crate::value::JSValue {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_get_field_by_name(desc, key)
    };

    let has_get = has_field(b"get");
    let has_set = has_field(b"set");
    let has_value = has_field(b"value");
    let has_writable = has_field(b"writable");

    if (has_get || has_set) && (has_value || has_writable) {
        // Node renders the offending descriptor object after the message; for
        // the plain-object descriptors that hit this path it prints `#<Object>`.
        throw_object_type_error(
            b"Invalid property descriptor. Cannot both specify accessors and a value or writable attribute, #<Object>",
        );
    }

    if has_get {
        let g = read(b"get");
        if !g.is_undefined() && !value_is_callable(f64::from_bits(g.bits())) {
            let s = describe_value_for_type_error(f64::from_bits(g.bits()));
            throw_object_type_error_with_suffix("Getter must be a function: ", &s);
        }
    }
    if has_set {
        let s_field = read(b"set");
        if !s_field.is_undefined() && !value_is_callable(f64::from_bits(s_field.bits())) {
            let s = describe_value_for_type_error(f64::from_bits(s_field.bits()));
            throw_object_type_error_with_suffix("Setter must be a function: ", &s);
        }
    }
}

/// #2843: enforce the ordinary `[[DefineOwnProperty]]` invariants
/// (ECMA-262 10.1.6.3 `ValidateAndApplyPropertyDescriptor`) for
/// `Object.defineProperty`. `obj` is the resolved heap object, `key` the
/// coerced key string. Throws the Node `TypeError` when the definition would
/// violate an invariant; returns normally when the definition is permitted.
///
/// Rules (matching Node v25):
///   - Adding a NEW key to a non-extensible object:
///       `Cannot define property <k>, object is not extensible`
///   - Redefining an EXISTING **non-configurable** key in a way the spec
///     forbids (make it configurable, flip enumerable, switch data↔accessor,
///     re-enable writability, or change the value of a non-writable data
///     property to a different value):
///       `Cannot redefine property: <k>`
///
/// A property is non-configurable either object-wide (the object was frozen or
/// sealed — both drop `configurable` on every existing key) OR individually
/// (`Object.defineProperty(obj, k, { configurable: false })`). Both surface
/// through the per-key descriptor side table, so this validation no longer
/// gates on the object-level flags — an individually non-configurable property
/// on an otherwise-extensible object is validated the same way.
pub(crate) unsafe fn enforce_define_property_invariants(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    key_name: &str,
    descriptor_value: f64,
) {
    if obj.is_null() || (obj as usize) <= 0x10000 {
        return;
    }
    let gc = gc_header_for(obj);
    let no_extend = (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0;

    // #6743: wide objects answer via the O(1) sidecar; the linear scan is the
    // narrow-object fallback (repeated defines were O(N²) through this check).
    let exists = own_key_present_via_index(obj, key).unwrap_or_else(|| own_key_present(obj, key));

    if !exists {
        // Adding a new property to a non-extensible object always throws.
        if no_extend {
            throw_object_type_error_with_suffix(
                "Cannot define property ",
                &format!("{key_name}, object is not extensible"),
            );
        }
        return;
    }

    // Existing own property. Its configurability comes from the per-key
    // descriptor side table: no entry ⇒ the default `{configurable: true}`
    // applies ⇒ any redefinition is permitted. Frozen/sealed objects and
    // explicit `{configurable: false}` defines both populate the table.
    let Some(attrs) = get_property_attrs(obj as usize, key_name) else {
        return;
    };
    if attrs.configurable() {
        return; // still configurable — redefinition allowed
    }

    // --- ValidateAndApplyPropertyDescriptor: current is non-configurable. ---
    let cur_accessor = get_accessor_descriptor(obj as usize, key_name);
    let cur_value = if cur_accessor.is_none() {
        f64::from_bits(js_object_get_field_by_name(obj as *const ObjectHeader, key).bits())
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    validate_nonconfigurable_redefine(key_name, attrs, cur_accessor, cur_value, descriptor_value);
}

/// The non-configurable branch of `ValidateAndApplyPropertyDescriptor`, factored
/// so the plain-object, function-object (closure), and symbol-keyed define paths
/// share one spec implementation. `cur_attrs` is the existing property's
/// attributes (already known non-configurable). `cur_accessor` is `Some(_)` for
/// an accessor property (carrying its get/set closure bits) or `None` for a data
/// property whose current value is `cur_value`. Throws `TypeError: Cannot
/// redefine property: <k>` when the redefinition violates an invariant.
pub(crate) unsafe fn validate_nonconfigurable_redefine(
    key_name: &str,
    cur_attrs: PropertyAttrs,
    cur_accessor: Option<AccessorDescriptor>,
    cur_value: f64,
    descriptor_value: f64,
) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    let desc_ptr = extract_obj_ptr(descriptor_value);
    if desc_ptr.is_null() {
        return;
    }
    let reject = || throw_object_type_error_with_suffix("Cannot redefine property: ", key_name);

    // `ToPropertyDescriptor` field presence is HasProperty (own OR inherited).
    let has_field = |name: &[u8]| -> bool { desc_has_field(descriptor_value, name) };
    let read = |name: &[u8]| -> crate::value::JSValue {
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k)
    };
    let read_bool = |name: &[u8]| -> Option<bool> {
        if !has_field(name) {
            return None;
        }
        Some(crate::value::js_is_truthy(f64::from_bits(read(name).bits())) != 0)
    };

    let desc_has_get = has_field(b"get");
    let desc_has_set = has_field(b"set");
    let desc_has_value = has_field(b"value");
    let desc_has_writable = has_field(b"writable");
    let desc_is_accessor = desc_has_get || desc_has_set;
    let desc_is_data = desc_has_value || desc_has_writable;

    // Step 4: a non-configurable property cannot be made configurable, and its
    // enumerability cannot change.
    if read_bool(b"configurable") == Some(true) {
        reject();
    }
    if let Some(want_enum) = read_bool(b"enumerable") {
        if want_enum != cur_attrs.enumerable() {
            reject();
        }
    }

    // A generic descriptor (only enumerable/configurable) imposes no further
    // constraints once the two checks above pass.
    if !desc_is_accessor && !desc_is_data {
        return;
    }

    // Step: a non-configurable property cannot switch between data and accessor.
    let cur_is_accessor = cur_accessor.is_some();
    if desc_is_accessor != cur_is_accessor {
        reject();
    }

    if let Some(acc) = cur_accessor {
        // Both accessor: `get`/`set` may not change. The stored closures are
        // clones rebound to the receiver (`clone_closure_rebind_this`) but keep
        // the original `func_ptr`, so compare by underlying function pointer.
        let closure_func_ptr = |bits: u64| -> usize {
            let p = (bits & crate::value::POINTER_MASK) as usize;
            if p >= 0x1000 && crate::closure::is_closure_ptr(p) {
                (*(p as *const crate::closure::ClosureHeader)).func_ptr as usize
            } else {
                0
            }
        };
        if desc_has_get {
            let want = read(b"get");
            let want_fp = if want.is_undefined() {
                0
            } else {
                closure_func_ptr(want.bits())
            };
            if want_fp != closure_func_ptr(acc.get) {
                reject();
            }
        }
        if desc_has_set {
            let want = read(b"set");
            let want_fp = if want.is_undefined() {
                0
            } else {
                closure_func_ptr(want.bits())
            };
            if want_fp != closure_func_ptr(acc.set) {
                reject();
            }
        }
        return;
    }

    // Both data. A non-writable data property cannot be made writable, and its
    // value cannot change to a different value (SameValue). A still-writable
    // data property allows any value/writable change.
    if !cur_attrs.writable() {
        if read_bool(b"writable") == Some(true) {
            reject();
        }
        if desc_has_value {
            let new_value = f64::from_bits(read(b"value").bits());
            if js_object_is(new_value, cur_value).to_bits() != TAG_TRUE {
                reject();
            }
        }
    }
}

/// Store a data-property value for `Object.defineProperty`, bypassing the
/// ordinary `[[Set]]` writability / frozen / sealed guards. The spec writes the
/// value via `[[DefineOwnProperty]]`, which is NOT subject to the `[[Set]]`
/// writability check — so redefining a configurable-but-non-writable property's
/// value, or performing a (validation-approved) same-value redefine on a frozen
/// object, must store the value rather than throw `Cannot assign to read only`.
///
/// The object's immutability flags are lifted only across the store. `obj` is
/// rooted so a GC evacuation during the store leaves the flag restore landing
/// on the relocated header. Callers must clear any stale per-key `writable`
/// descriptor first (it is re-applied with the final attributes afterward).
pub(crate) unsafe fn define_property_force_store_value(
    obj: *mut ObjectHeader,
    key_str: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key_str);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if obj.is_null() || (obj as usize) <= 0x10000 {
        return;
    }
    let immutability =
        crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
    let gc = gc_header_for(obj);
    let saved = (*gc)._reserved;
    (*gc)._reserved &= !immutability;
    let key_str = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    js_object_set_field_by_name(obj, key_str, value);
    // Re-fetch after a possible evacuation, then restore the immutability bits.
    obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if !obj.is_null() && (obj as usize) > 0x10000 {
        let gc = gc_header_for(obj);
        (*gc)._reserved = ((*gc)._reserved & !immutability) | (saved & immutability);
    }
}
