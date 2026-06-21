//! `Object.*` static methods and descriptor machinery:
//! `Object.fromEntries`/`groupBy`/`is`/`hasOwn`/`create`/`freeze`/`seal`/
//! `defineProperty`/`getOwnPropertyDescriptor`/`getPrototypeOf`/... plus
//! the `js_object_*` helpers backing them.
use super::*;
fn throw_from_entries_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
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
unsafe fn value_is_callable(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        return ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr);
    }
    // Class refs (INT32-tagged, top16 == 0x7FFE) are callable constructors.
    (value.to_bits() >> 48) == 0x7FFE
}

unsafe fn registered_buffer_index_own_property_present(
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
    let idx = super::has_own_helpers::str_from_string_header(key_str)
        .and_then(super::canonical_array_index)?;
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
            if super::has_own_helpers::closure_own_key_present(ptr, key_str) {
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
            if super::has_own_helpers::closure_own_key_present(ptr, key_str) {
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
unsafe fn validate_property_descriptor(descriptor_value: f64) {
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
unsafe fn enforce_define_property_invariants(
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

    let exists = own_key_present(obj, key);

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
unsafe fn define_property_force_store_value(
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

fn throw_from_entries_not_iterable() -> ! {
    throw_from_entries_type_error(b"undefined is not iterable")
}

fn throw_from_entries_non_object_entry() -> ! {
    throw_from_entries_type_error(b"Iterator value is not an entry object")
}

unsafe fn object_from_entries_gc_type(raw_ptr: i64) -> Option<u8> {
    if raw_ptr < crate::gc::GC_HEADER_SIZE as i64 + 0x1000 {
        return None;
    }
    let addr = raw_ptr as usize;
    if crate::symbol::is_registered_symbol(addr) {
        return None;
    }
    if crate::set::is_registered_set(addr) {
        return Some(crate::gc::GC_TYPE_SET);
    }
    if crate::map::is_registered_map(addr) {
        return Some(crate::gc::GC_TYPE_MAP);
    }
    let ptr = raw_ptr as *const u8;
    if !crate::object::is_valid_obj_ptr(ptr) {
        return None;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    Some((*gc_header).obj_type)
}

unsafe fn object_from_entries_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = crate::value::js_nanbox_get_pointer(value);
    let gc_type = object_from_entries_gc_type(raw);
    if gc_type != Some(crate::gc::GC_TYPE_ARRAY) && gc_type != Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        throw_from_entries_not_iterable();
    }
    raw as *mut ArrayHeader
}

unsafe fn object_from_entries_has_iterator(value: f64, raw: i64, gc_type: Option<u8>) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    match gc_type {
        Some(crate::gc::GC_TYPE_ARRAY)
        | Some(crate::gc::GC_TYPE_LAZY_ARRAY)
        | Some(crate::gc::GC_TYPE_MAP)
        | Some(crate::gc::GC_TYPE_SET) => return true,
        Some(crate::gc::GC_TYPE_OBJECT) => {
            let obj = raw as *mut ObjectHeader;
            if crate::url::try_read_as_search_params(obj).is_some() {
                return true;
            }
            if !obj.is_null() && (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return true;
            }
        }
        _ => {}
    }

    let iter_sym = crate::symbol::well_known_symbol("iterator");
    if !iter_sym.is_null() {
        let sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
        let iter_fn = crate::symbol::js_object_get_symbol_property(value, sym_value);
        let iter_fn_ptr = crate::value::js_nanbox_get_pointer(iter_fn);
        if iter_fn_ptr != 0 && crate::closure::is_closure_ptr(iter_fn_ptr as usize) {
            return true;
        }
    }

    crate::array::has_iterator_next(value)
}

unsafe fn object_from_entries_materialize_entries(entries_value: f64) -> *mut ArrayHeader {
    let jv = crate::value::JSValue::from_bits(entries_value.to_bits());
    if jv.is_null() || jv.is_undefined() || jv.is_bool() || jv.is_number() || jv.is_int32() {
        throw_from_entries_not_iterable();
    }
    if jv.is_bigint() {
        throw_from_entries_not_iterable();
    }

    let raw = crate::value::js_nanbox_get_pointer(entries_value);
    let gc_type = object_from_entries_gc_type(raw);

    if !jv.is_any_string() && raw == 0 {
        throw_from_entries_not_iterable();
    }

    if !object_from_entries_has_iterator(entries_value, raw, gc_type) {
        throw_from_entries_not_iterable();
    }

    if gc_type == Some(crate::gc::GC_TYPE_MAP) {
        return crate::map::js_map_entries(raw as *const crate::map::MapHeader);
    }

    if gc_type == Some(crate::gc::GC_TYPE_OBJECT) {
        let obj = raw as *mut ObjectHeader;
        if crate::url::try_read_as_search_params(obj).is_some() {
            let boxed = crate::url::js_url_search_params_entries_arr(obj);
            return object_from_entries_array_ptr(boxed);
        }
    }

    let boxed = crate::array::js_for_of_to_array(entries_value);
    object_from_entries_array_ptr(boxed)
}

unsafe fn object_from_entries_entry_values(entry_val: f64) -> (f64, f64) {
    let jv = crate::value::JSValue::from_bits(entry_val.to_bits());
    if jv.is_null()
        || jv.is_undefined()
        || jv.is_bool()
        || jv.is_number()
        || jv.is_int32()
        || jv.is_any_string()
        || jv.is_bigint()
    {
        throw_from_entries_non_object_entry();
    }

    let raw = crate::value::js_nanbox_get_pointer(entry_val);
    let gc_type = object_from_entries_gc_type(raw);
    if raw == 0 {
        throw_from_entries_non_object_entry();
    }

    if gc_type == Some(crate::gc::GC_TYPE_ARRAY) || gc_type == Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        let arr = raw as *const ArrayHeader;
        return (
            crate::array::js_array_get_f64(arr, 0),
            crate::array::js_array_get_f64(arr, 1),
        );
    }

    let obj = raw as *const ObjectHeader;
    if obj.is_null() {
        throw_from_entries_non_object_entry();
    }
    let key0 = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let key1 = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
    (
        js_object_get_field_by_name_f64(obj, key0),
        js_object_get_field_by_name_f64(obj, key1),
    )
}

/// Object.fromEntries(entries) — build an object from iterable [key, value] entries.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    unsafe {
        let arr_ptr = object_from_entries_materialize_entries(entries_value);
        let length = crate::array::js_array_length(arr_ptr) as usize;

        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        for i in 0..length {
            let entry_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
            let (key_val, val_val) = object_from_entries_entry_values(entry_val);
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
            js_object_set_field_by_name(obj, key_str, val_val);
        }

        crate::value::js_nanbox_pointer(obj as i64)
    }
}

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits {
                return f64::from_bits(TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison. #1781: accept inline SSO short
    // strings on either side. Two SSO operands with equal content already
    // match via the bit-pattern fallback below, but a mixed SSO/heap pair
    // (same content, different representation — e.g. a JSON-parsed value vs
    // a heap literal) would not. Materialize via the unified decoder so the
    // comparison is representation-independent.
    if a_jsval.is_any_string() && b_jsval.is_any_string() {
        let result = crate::string::js_string_equals(
            crate::value::js_get_string_pointer_unified(f64::from_bits(a_bits))
                as *const crate::StringHeader,
            crate::value::js_get_string_pointer_unified(f64::from_bits(b_bits))
                as *const crate::StringHeader,
        );
        if result != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// Object.hasOwn(obj, key) - check if obj has its own property `key`.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_js = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_js.is_undefined() || obj_js.is_null() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // A Proxy is a small registered id, not a heap object — route
        // `hasOwnProperty` through `[[GetOwnProperty]]` (a present own property
        // is one whose descriptor is not undefined) rather than dereferencing
        // the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            return f64::from_bits(if desc.to_bits() != crate::value::TAG_UNDEFINED {
                TAG_TRUE
            } else {
                TAG_FALSE
            });
        }

        // Symbol-keyed lookup: route through SYMBOL_PROPERTIES side table.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            // ClassRef receivers carry class_id in the low 32 bits.
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                let class_id = (bits & 0xFFFF_FFFF) as u32;
                let present =
                    crate::symbol::class_static_symbol_lookup(class_id, key_value).is_some();
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        if obj_js.is_any_string() {
            let present =
                super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(class_id) = super::class_ref_id(obj_value) {
            let present = super::has_own_helpers::str_from_string_header(key_str)
                .map(|key| {
                    if key.starts_with('#') {
                        // Private static elements are never reflectable own
                        // properties of the class constructor.
                        false
                    } else if super::class_registry::class_is_key_deleted(class_id, key) {
                        false
                    } else if matches!(key, "length" | "prototype") {
                        true
                    } else if key == "name"
                        && super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_none()
                    {
                        super::class_registry::class_name_for_id(class_id).is_some()
                    } else {
                        CLASS_DYNAMIC_PROPS.with(|m| {
                            m.borrow()
                                .get(&class_id)
                                .is_some_and(|props| props.contains_key(key))
                        }) || super::class_registry::lookup_static_method_in_chain(class_id, key)
                            .is_some()
                            // A static accessor (`static get x()`) is an own
                            // property of the constructor — own-only, mirroring
                            // getOwnPropertyDescriptor (class/definition/
                            // {getters,setters}-prop-desc `staticX`).
                            || super::class_registry::class_own_static_accessor_ptrs(class_id, key)
                                .is_some()
                    }
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let present = crate::typedarray_props::typed_array_has_own_property(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures carry built-in own `name`/`length`
        // (and `prototype` for constructors) plus any user-attached props.
        // Route them here instead of through `extract_obj_ptr`/`own_key_present`,
        // which would read `keys_array` off a closure (out of bounds).
        if obj_js.is_pointer() {
            let ptr = obj_js.as_pointer::<u8>() as usize;
            if crate::buffer::is_registered_buffer(ptr) {
                let present = super::has_own_helpers::buffer_own_key_present(
                    ptr as *const crate::buffer::BufferHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            // Date / RegExp / Error exotic instances: own expando props
            // (side tables) + per-kind builtin own slots.
            if let Some(kind) = super::exotic_expando::exotic_expando_kind(ptr) {
                use super::exotic_expando::ExoticKind;
                let present = super::has_own_helpers::str_from_string_header(key_str)
                    .map(|key| {
                        super::exotic_expando::exotic_has_own_property(kind, ptr, key)
                            || match kind {
                                ExoticKind::RegExp => key == "lastIndex",
                                ExoticKind::Error => crate::error::js_error_has_own_property(
                                    ptr as *mut crate::error::ErrorHeader,
                                    key,
                                ),
                                ExoticKind::Date | ExoticKind::Temporal | ExoticKind::Promise => {
                                    false
                                }
                            }
                    })
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::closure::is_closure_ptr(ptr) {
                let present = super::has_own_helpers::str_from_string_header(key_str)
                    .map(|k| super::has_own_helpers::closure_own_key_present(ptr, k))
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
                let present = crate::typedarray_props::typed_array_has_own_property(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            if ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
                && crate::object::is_valid_obj_ptr(ptr as *const u8)
            {
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let present = super::has_own_helpers::str_from_string_header(key_str)
                        .map(|key| {
                            crate::error::js_error_has_own_property(
                                ptr as *mut crate::error::ErrorHeader,
                                key,
                            )
                        })
                        .unwrap_or(false);
                    return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
                }
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }

        if (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID {
            let present = super::native_module::read_native_module_name(obj)
                .as_deref()
                .zip(super::has_own_helpers::str_from_string_header(key_str))
                .map(|(module, key)| {
                    super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module, key))
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let present = super::has_own_helpers::array_own_key_present(
                    obj as *const crate::array::ArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                return f64::from_bits(TAG_FALSE);
            };
            let present = read_native_module_name(obj)
                .as_deref()
                .is_some_and(|module_name| {
                    super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module_name, key_name))
                });
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // Private elements (`#x`) sit in a class instance's keys_array but are
        // never reflectable own properties. Plain literals keep class_id 0.
        if (*obj).class_id != 0 {
            if let Some(key) = super::has_own_helpers::str_from_string_header(key_str) {
                if key.starts_with('#') {
                    return f64::from_bits(TAG_FALSE);
                }
            }
        }

        if own_key_present(obj, key_str) {
            return f64::from_bits(TAG_TRUE);
        }

        // A class-declaration prototype object: instance accessors (`get x()`)
        // and methods live in the class vtable, not the object's keys_array, yet
        // they ARE own properties of `C.prototype` — `getOwnPropertyDescriptor`
        // already reflects them, so `hasOwnProperty` must agree (test262
        // class/definition/{getters,setters}-prop-desc, which assert via
        // `verifyProperty` → `hasOwnProperty`).
        if let Some(cid) = super::class_registry::class_id_for_decl_prototype_object(obj as usize) {
            if let Some(key) = super::has_own_helpers::str_from_string_header(key_str) {
                if !super::class_registry::class_is_key_deleted(cid, key)
                    && (key == "constructor"
                        || super::class_registry::class_own_accessor_ptrs(cid, key).is_some()
                        || super::native_module::class_has_own_method(cid, key))
                {
                    return f64::from_bits(TAG_TRUE);
                }
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// `Object.prototype.propertyIsEnumerable.call(obj, key)` (#2891).
#[no_mangle]
pub extern "C" fn js_object_property_is_enumerable(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // Proxy receiver: resolve the descriptor via `[[GetOwnProperty]]` and
        // report its `enumerable` attribute (absent property → false) rather
        // than dereferencing the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            if desc.to_bits() == crate::value::TAG_UNDEFINED {
                return f64::from_bits(TAG_FALSE);
            }
            let desc_ptr = extract_obj_ptr(desc);
            if desc_ptr.is_null() {
                return f64::from_bits(TAG_FALSE);
            }
            let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
            let enum_v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, enum_key);
            return f64::from_bits(
                if crate::value::js_is_truthy(f64::from_bits(enum_v.bits())) != 0 {
                    TAG_TRUE
                } else {
                    TAG_FALSE
                },
            );
        }

        // Symbol-keyed lookup: route through the SYMBOL_PROPERTIES side
        // table (mirrors js_object_has_own) — string-coercing a Symbol key
        // below would never match and reported every symbol prop as
        // non-enumerable.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                // ClassRef receivers: statics live in the class registry and
                // are non-enumerable like builtin statics.
                return f64::from_bits(TAG_FALSE);
            }
            if !crate::symbol::js_object_has_own_symbol(obj_value, key_value) {
                return f64::from_bits(TAG_FALSE);
            }
            let owner = (obj_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let sym = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let enumerable = crate::symbol::symbol_property_is_enumerable(owner, sym);
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        // ClassRef receiver (INT32-tagged constructor, not a heap object): the
        // only enumerable own string keys are the static FIELDS recorded in
        // CLASS_DYNAMIC_PROPS — `length`/`name`/`prototype` and static
        // methods/accessors are non-enumerable. `extract_obj_ptr` below would
        // null out on the INT32 payload and report every key non-enumerable, so
        // `verifyProperty(C, "f", …)`'s isEnumerable check failed (test262
        // class/elements static-field-declaration & friends).
        if let Some(class_id) = super::class_ref_id(obj_value) {
            if super::class_prototype_ref_id(obj_value).is_none() {
                if let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) {
                    let is_static_field = !key_name.starts_with('#')
                        && super::class_registry::class_own_static_field_value(class_id, key_name)
                            .is_some();
                    return f64::from_bits(if is_static_field { TAG_TRUE } else { TAG_FALSE });
                }
            }
        }

        // String primitives: index keys in range are enumerable own props;
        // "length" is a non-enumerable own prop; everything else absent.
        if obj_jv.is_any_string() {
            let present =
                super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            if !present {
                return f64::from_bits(TAG_FALSE);
            }
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let is_length = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                .map(|s| s == "length")
                .unwrap_or(false);
            return f64::from_bits(if is_length { TAG_FALSE } else { TAG_TRUE });
        }

        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // Date / RegExp / Error exotic instances: expando/accessor own props
        // report their side-table enumerability (default true for plain
        // expando writes); builtin own slots are non-enumerable.
        if let Some((addr, kind)) = super::exotic_expando::exotic_expando_kind_of_value(obj_value) {
            let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                return f64::from_bits(TAG_FALSE);
            };
            if !super::exotic_expando::exotic_has_own_property(kind, addr, key_name) {
                return f64::from_bits(TAG_FALSE);
            }
            let enumerable = super::get_property_attrs(addr, key_name)
                .map(|a| a.enumerable())
                .unwrap_or_else(|| {
                    super::exotic_expando::exotic_default_enumerable(kind, key_name)
                });
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures. Built-in `name`/`length`/`prototype` are
        // non-enumerable; user-attached props default to enumerable.
        if obj_jv.is_pointer() {
            let ptr = obj_jv.as_pointer::<u8>() as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                    return f64::from_bits(TAG_FALSE);
                };
                if !super::has_own_helpers::closure_own_key_present(ptr, key_name) {
                    return f64::from_bits(TAG_FALSE);
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    return f64::from_bits(TAG_FALSE);
                }
                let enumerable = super::get_property_attrs(ptr, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
                let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    key_str,
                );
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        let key_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s,
            Err(_) => return f64::from_bits(TAG_FALSE),
        };
        if let Some(result) = super::array_property_is_enumerable(obj, key_str, key_name) {
            return result;
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_FALSE);
        }
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(module_name) = read_native_module_name(obj) {
                return f64::from_bits(
                    if native_module_has_enumerable_key(&module_name, key_name) {
                        TAG_TRUE
                    } else {
                        TAG_FALSE
                    },
                );
            }
        }
        if !own_key_present(obj, key_str) {
            return f64::from_bits(TAG_FALSE);
        }
        let enumerable = super::get_property_attrs(obj as usize, key_name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(true);
        f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE })
    }
}

#[used]
static KEEP_PROPERTY_IS_ENUMERABLE: extern "C" fn(f64, f64) -> f64 =
    js_object_property_is_enumerable;

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
pub(crate) unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        // Raw-I64-pointer fallback (module-level array/object vars store the
        // untagged pointer directly). Every GC allocation is `align.max(8)`-
        // aligned, so a real object pointer always has its low 3 bits clear.
        // Requiring alignment here rejects non-object values whose raw bits
        // merely *land* in the address range — e.g. a native-module namespace
        // sentinel (`require('buffer')`) reaching a generic object op like
        // `hasOwnProperty`. Without it, callers deref `[ptr-8]` for the
        // GcHeader on a misaligned garbage address → SIGBUS (#3527).
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 && bits & 0x7 == 0 {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
pub(super) unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

/// #2159 helper: install a `target_cid.method` entry from an
/// `Object.defineProperty(C.prototype, name, descriptor)` call.
///
/// The descriptor's `value` came in two main shapes in practice:
///
/// 1. A `BOUND_METHOD_FUNC_PTR` closure returned by `getOwnPropertyDescriptor`
///    on a sibling class (drizzle's `applyMixins(Base, [Mixin])`: the
///    `getOwnPropertyDescriptor(Mixin.prototype, name)` value reads as
///    `js_class_method_bind(Mixin_class_ref, name)`). Dispatching that bound
///    closure would re-enter `js_native_call_method` against the class-ref —
///    a class object reaches the *static* dispatch arm, not the instance
///    method, so calling it would return the wrong thing. Instead we look up
///    the raw vtable entry on the source class and copy it onto the target
///    class's vtable directly, so future `inst.method(args)` dispatches via
///    the regular chain walk with `this = inst`.
///
/// 2. A user-supplied closure (e.g. `Object.defineProperty(C.prototype, "m",
///    { value: function () { … } })`). Route through the same per-class
///    prototype-method side table that `js_register_prototype_method` (#838)
///    uses, so the `inst.m` / `inst.m()` lookup paths in
///    `field_get_set.rs` / `native_call_method.rs` find it after the regular
///    vtable miss.
unsafe fn define_class_prototype_method(target_cid: u32, name: &str, value_bits: u64) {
    use crate::closure::{ClosureHeader, BOUND_METHOD_FUNC_PTR, CLOSURE_MAGIC};
    use crate::object::class_registry::{ClassVTable, VTableMethodEntry, CLASS_VTABLE_REGISTRY};

    // Reject undefined / null / numeric values up front — those aren't
    // methods and shouldn't make it onto the prototype side tables.
    let value = f64::from_bits(value_bits);
    let jsv = crate::JSValue::from_bits(value_bits);
    if !jsv.is_pointer() {
        return;
    }
    let ptr = jsv.as_pointer::<u8>() as usize;
    if ptr < 0x1000 {
        return;
    }

    // Shape (1): BOUND_METHOD closure. Extract source class-ref + method
    // name from the captures (see `js_class_method_bind`), then copy the
    // source class's vtable entry (or any inherited entry up the parent
    // chain) onto `target_cid`.
    if crate::closure::is_closure_ptr(ptr) {
        let closure = ptr as *const ClosureHeader;
        if (*closure).type_tag == CLOSURE_MAGIC && (*closure).func_ptr == BOUND_METHOD_FUNC_PTR {
            let recv = crate::closure::js_closure_get_capture_f64(closure, 0);
            let recv_value = crate::JSValue::from_bits(recv.to_bits());
            let source_cid = super::class_ref_id(recv).or_else(|| {
                recv_value.is_pointer().then(|| {
                    super::class_registry::class_id_for_decl_prototype_object(
                        recv_value.as_pointer::<u8>() as usize,
                    )
                })?
            });
            if let Some(source_cid) = source_cid {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    super::lookup_class_method_in_chain(source_cid, name)
                {
                    let mut guard = CLASS_VTABLE_REGISTRY.write().unwrap();
                    if guard.is_none() {
                        *guard = Some(std::collections::HashMap::new());
                    }
                    let reg = guard.as_mut().unwrap();
                    let vtable = reg.entry(target_cid).or_insert_with(|| ClassVTable {
                        methods: std::collections::HashMap::new(),
                        getters: std::collections::HashMap::new(),
                        setters: std::collections::HashMap::new(),
                    });
                    vtable.methods.insert(
                        name.to_string(),
                        VTableMethodEntry {
                            func_ptr,
                            param_count,
                            has_synthetic_arguments,
                            has_rest,
                        },
                    );
                    drop(guard);
                    super::class_registry::js_register_class_id(target_cid);
                    crate::typed_feedback::invalidate_method_change(target_cid);
                    return;
                }
            }
        }
    }

    // Shape (2): any other callable value (user closure, regular function).
    // Mirror the `Class.prototype.method = fn` direct-assignment path so the
    // existing `lookup_prototype_method` walks find it.
    super::class_registry::js_register_prototype_method(
        target_cid,
        name.as_ptr(),
        name.len(),
        value,
    );
}

/// Object.defineProperty(obj, key, descriptor) — set the value AND record the
/// `writable` / `enumerable` / `configurable` attribute flags in the side table.
/// Returns the object (NaN-boxed pointer).
///
/// IMPORTANT: writes the value via `js_object_set_field_by_name` BEFORE recording
/// the descriptor — otherwise a `writable: false` descriptor would block its own
/// initial value from being stored.
#[no_mangle]
pub extern "C" fn js_object_define_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
    unsafe {
        // A Proxy receiver is a small registered id, not a heap object — it
        // fails the `value_is_object_like` test below (so it would wrongly throw
        // "called on non-object") and the ordinary paths would deref the fake
        // pointer and segfault. Per spec, Object.defineProperty(proxy, …):
        // validate the descriptor (ToPropertyDescriptor), invoke the
        // `[[DefineOwnProperty]]` trap, and throw a TypeError if it reports
        // failure. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            if !value_is_object_like(descriptor_value)
                || crate::symbol::js_is_symbol(descriptor_value) != 0
            {
                let desc = describe_value_for_type_error(descriptor_value);
                throw_object_type_error_with_suffix(
                    "Property description must be an object: ",
                    &desc,
                );
            }
            validate_property_descriptor(descriptor_value);
            let ok =
                crate::proxy::js_reflect_define_property(obj_value, key_value, descriptor_value);
            if crate::value::js_is_truthy(ok) == 0 {
                throw_object_type_error(b"'defineProperty' on proxy: trap returned falsish");
            }
            return obj_value;
        }

        // A numeric key defined on `Object.prototype` (data or accessor) shows
        // through array hole/OOB reads — flip the global flag.
        {
            let kb = key_value.to_bits();
            let is_numeric_key =
                (kb >> 48) == 0x7FFE || crate::value::JSValue::from_bits(kb).is_number() || {
                    let sp = crate::value::js_get_string_pointer_unified(key_value)
                        as *const crate::StringHeader;
                    !sp.is_null()
                        && super::has_own_helpers::str_from_string_header(sp)
                            .map(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                            .unwrap_or(false)
                };
            if is_numeric_key {
                let ob = obj_value.to_bits();
                if (ob >> 48) == 0x7FFD {
                    crate::array::note_object_prototype_index_write(
                        (ob & crate::value::POINTER_MASK) as usize,
                    );
                }
            }
        }

        // #2817: ES Object.defineProperty validation.
        //   1. Target must be an object (or class-ref / function — all objects
        //      in Node). Primitives / null / undefined throw.
        //   2. Descriptor must be an object; otherwise
        //      `Property description must be an object: <desc>`.
        //   3. Accessor + data fields can't be mixed.
        //   4. Present `get`/`set` must be callable.
        let target_is_class_ref = super::class_ref_id(obj_value).is_some();
        if !target_is_class_ref && !value_is_object_like(obj_value) {
            // A native HANDLE target (a small pointer-tagged id — e.g. an http
            // ServerResponse, Headers, a timer) is not a heap object, so Perry
            // can't attach an arbitrary own property to it the way V8 can. Node
            // framework code nonetheless calls `Object.defineProperty(handle, …)`:
            // Next.js `patchSetHeaderWithCookieSupport` marks `res` with a Symbol
            // (`Object.defineProperty(res, PATCHED_SET_HEADER, { value: true })`).
            // Throwing here aborts the whole request (HTTP 500). Instead treat the
            // define as a best-effort success: for a string key with a data
            // descriptor, route the value through the handle property-set so
            // `res[key]` round-trips; symbol keys / accessor descriptors degrade
            // to a no-op (the framework's patch is idempotent, so re-running is
            // harmless). Matches how `js_object_set_field_by_name` already tolerates
            // small-handle receivers.
            let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
            let handle_id = if jv.is_pointer() {
                let p = jv.as_pointer::<u8>() as usize;
                if p >= 1 && p < 0x10000 {
                    Some(p)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(hid) = handle_id {
                // Best-effort: store a string-keyed data-descriptor value on the
                // handle via the same dispatch `obj.key = value` uses.
                let ks = crate::value::js_get_string_pointer_unified(key_value)
                    as *const crate::StringHeader;
                if !ks.is_null() {
                    if let Some(dispatch) = super::class_handles::handle_property_set_dispatch() {
                        let dval = if desc_has_field(descriptor_value, b"value") {
                            Some(f64::from_bits(
                                desc_read_field(descriptor_value, b"value").bits(),
                            ))
                        } else {
                            None
                        };
                        if let Some(v) = dval {
                            let name_ptr =
                                (ks as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*ks).byte_len as usize;
                            dispatch(hid as i64, name_ptr, name_len, v);
                        }
                    }
                }
                return obj_value;
            }
            throw_object_type_error(b"Object.defineProperty called on non-object");
        }
        // A descriptor must be an Object; a Symbol is pointer-tagged but not an
        // object, so `ToPropertyDescriptor(Symbol())` throws (test262
        // property-description-must-be-an-object-not-symbol).
        if !value_is_object_like(descriptor_value)
            || crate::symbol::js_is_symbol(descriptor_value) != 0
        {
            let desc = describe_value_for_type_error(descriptor_value);
            throw_object_type_error_with_suffix("Property description must be an object: ", &desc);
        }
        validate_property_descriptor(descriptor_value);

        // TypedArrays are Integer-Indexed exotic objects: a canonical numeric
        // index key bypasses ordinary define entirely (validate the index, then
        // either write the element or reject with a TypeError).
        match super::typed_array_define_own_property(obj_value, key_value, descriptor_value) {
            super::TypedArrayDefineOutcome::Defined => return obj_value,
            super::TypedArrayDefineOutcome::Rejected => {
                throw_object_type_error(b"Cannot redefine property")
            }
            super::TypedArrayDefineOutcome::NotTypedArray => {}
        }

        // Date / RegExp / Error instances are exotic cells, not
        // `ObjectHeader`s — the ordinary define path below would bit-cast
        // them and corrupt memory. Route through the expando-aware
        // [[DefineOwnProperty]] (side-table storage + attrs + accessors).
        if let Some((addr, kind)) = super::exotic_expando::exotic_expando_kind_of_value(obj_value) {
            if crate::symbol::js_is_symbol(key_value) != 0 {
                let value_field = desc_read_field(descriptor_value, b"value");
                crate::symbol::js_object_set_symbol_property(
                    obj_value,
                    key_value,
                    f64::from_bits(value_field.bits()),
                );
                return obj_value;
            }
            if let Some(name) = super::metadata_key_to_string(key_value) {
                super::exotic_expando::exotic_define_own_property(
                    addr,
                    kind,
                    &name,
                    descriptor_value,
                );
            }
            return obj_value;
        }

        // #2159: when the receiver is a class-ref (`Class.prototype` evaluates
        // back to the class itself in Perry — see `class_ref_id` /
        // `js_object_get_own_property_descriptor`'s class-ref arm), route the
        // descriptor through the class-vtable / prototype-method side tables
        // so instance lookups (`new C().method`) see the new entry. Drizzle's
        // `applyMixins(Base, [Mixin])` copies methods between class
        // prototypes via `Object.defineProperty(Base.prototype, name,
        // Object.getOwnPropertyDescriptor(Mixin.prototype, name))` — pre-fix
        // the call hit `extract_obj_ptr → null` (a class-ref isn't a pointer)
        // and silently dropped the descriptor, so `await
        // db.select().from(x)` saw `instance.then === undefined` and `await`
        // unwrapped the builder unchanged.
        if let Some(target_cid) = super::class_ref_id(obj_value) {
            if let Some(name) = super::metadata_key_to_string(key_value) {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                    let value_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                    if !value_field.is_undefined() {
                        // #5024 followup: a `defineProperty` data descriptor is
                        // non-enumerable unless it explicitly sets
                        // `enumerable: true`. Record that so the prototype-object
                        // mirror (reflective `Object.keys`/`for-in`) doesn't
                        // surface it — `Class.prototype.m = fn` assignment, which
                        // routes through the same side table, stays enumerable.
                        super::class_registry::class_prototype_method_set_enumerable(
                            target_cid,
                            &name,
                            descriptor_enumerable(descriptor_value),
                        );
                        define_class_prototype_method(target_cid, &name, value_field.bits());
                    }
                }
            }
            return obj_value;
        }

        // Closures are object-like but not ObjectHeader-backed, so descriptor
        // writes have to route through the closure property side tables.
        let target_closure_ptr = {
            let value = crate::value::JSValue::from_bits(obj_value.to_bits());
            let raw = if value.is_pointer() {
                value.as_pointer::<u8>() as usize
            } else {
                let bits = obj_value.to_bits();
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
        };
        if let Some(closure_ptr) = target_closure_ptr {
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return obj_value;
            }
            let key_rust: Option<String> = {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
            };
            let Some(key_rust) = key_rust else {
                return obj_value;
            };
            let desc_ptr = extract_obj_ptr(descriptor_value);
            if desc_ptr.is_null() {
                return obj_value;
            }

            // Spec retention: redefining an existing own property keeps the
            // attributes the descriptor omits (see the object-path comment).
            let existing_attrs: Option<PropertyAttrs> =
                if super::has_own_helpers::closure_own_key_present(closure_ptr, &key_rust) {
                    Some(
                        super::get_property_attrs(closure_ptr, &key_rust)
                            .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                    )
                } else {
                    None
                };

            // ValidateAndApplyPropertyDescriptor: a non-configurable existing own
            // property of a function object can only be redefined within the
            // spec-permitted bounds (#2843). The built-in `name`/`length` slots
            // are configurable per spec, so a redefine of those still flows
            // through unguarded. The shared core mirrors the plain-object path.
            if let Some(cur_attrs) = existing_attrs {
                if !cur_attrs.configurable() {
                    let cur_accessor = super::get_accessor_descriptor(closure_ptr, &key_rust);
                    let cur_value = if cur_accessor.is_none() {
                        crate::closure::closure_get_dynamic_prop(closure_ptr, &key_rust)
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    };
                    validate_nonconfigurable_redefine(
                        &key_rust,
                        cur_attrs,
                        cur_accessor,
                        cur_value,
                        descriptor_value,
                    );
                }
            }

            let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
            let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
            let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
            let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

            if has_accessor {
                let get_bits = if get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                };
                let set_bits = if set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                };
                set_accessor_descriptor(
                    closure_ptr,
                    key_rust.clone(),
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            } else {
                let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                let value_field =
                    js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(closure_ptr, key_rust.clone()));
                });
                if !value_field.is_undefined() {
                    crate::closure::closure_set_dynamic_prop(
                        closure_ptr,
                        &key_rust,
                        f64::from_bits(value_field.bits()),
                    );
                }
            }

            let read_bool = |name: &[u8]| -> Option<bool> {
                let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                if v.is_undefined() {
                    None
                } else {
                    Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                }
            };
            let writable = read_bool(b"writable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.writable()).unwrap_or(has_accessor));
            let enumerable = read_bool(b"enumerable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.enumerable()).unwrap_or(false));
            let configurable = read_bool(b"configurable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.configurable()).unwrap_or(false));
            set_property_attrs(
                closure_ptr,
                key_rust,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
            return obj_value;
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            // A Symbol key on a TypedArray is an ORDINARY define — store it in
            // the symbol side tables (string-coercing it would file the value
            // under a "Symbol(x)" string name, unreachable via `ta[sym]`),
            // honoring accessor descriptors and recording the attributes
            // (defineProperty defaults absent fields to false, unlike a plain
            // `ta[sym] = v` write). Mirrors the generic symbol-define block.
            if crate::symbol::js_is_symbol(key_value) != 0 {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if desc_ptr.is_null() {
                    return obj_value;
                }
                let has_get = desc_has_field(descriptor_value, b"get");
                let has_set = desc_has_field(descriptor_value, b"set");
                let has_accessor = has_get || has_set;
                if has_accessor {
                    let get_field = desc_read_field(descriptor_value, b"get");
                    let set_field = desc_read_field(descriptor_value, b"set");
                    let get_bits = if !has_get || get_field.is_undefined() {
                        0
                    } else {
                        crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                    };
                    let set_bits = if !has_set || set_field.is_undefined() {
                        0
                    } else {
                        crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                    };
                    crate::symbol::set_symbol_accessor_property(
                        obj_value, key_value, get_bits, set_bits,
                    );
                } else {
                    let value_field = desc_read_field(descriptor_value, b"value");
                    crate::symbol::js_object_set_symbol_property(
                        obj_value,
                        key_value,
                        f64::from_bits(value_field.bits()),
                    );
                }
                let read_flag = |name: &[u8]| -> Option<bool> {
                    if !desc_has_field(descriptor_value, name) {
                        return None;
                    }
                    let v = desc_read_field(descriptor_value, name);
                    Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                };
                let owner = crate::symbol::obj_key_from_f64(obj_value);
                let sym_key = crate::symbol::sym_key_from_f64(key_value);
                crate::symbol::set_symbol_property_attrs(
                    owner,
                    sym_key,
                    PropertyAttrs::new(
                        read_flag(b"writable").unwrap_or(has_accessor),
                        read_flag(b"enumerable").unwrap_or(false),
                        read_flag(b"configurable").unwrap_or(false),
                    ),
                );
                return obj_value;
            }
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return obj_value;
            }
            let key_rust: Option<String> = {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
            };
            if let Some(ref key_name) = key_rust {
                return crate::typedarray_props::typed_array_define_own_property(
                    obj_value,
                    addr as *mut crate::typedarray::TypedArrayHeader,
                    key_str,
                    key_name,
                    descriptor_value,
                );
            }
            return obj_value;
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // #1250: when the key is a Symbol, route into the symbol side
        // table (`SYMBOL_PROPERTIES`) the same way `obj[sym] = value`
        // does. Without this, `Object.defineProperty(obj, sym, ...)`
        // would drop the symbol and try to coerce it to a string,
        // which is exactly the failure mode reported for
        // `Object.defineProperty(obj, inspect.custom, …)`.
        let key_bits = key_value.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        if key_tag == 0x7FFD_0000_0000_0000 {
            let raw_ptr = (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::symbol::SymbolHeader;
            if !raw_ptr.is_null()
                && (raw_ptr as usize) >= 0x1000
                && (*raw_ptr).magic == crate::symbol::SYMBOL_MAGIC
            {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
                    let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
                    let get_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
                    let set_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
                    let has_get = own_key_present(desc_ptr, get_key);
                    let has_set = own_key_present(desc_ptr, set_key);
                    let has_accessor = has_get || has_set;
                    if has_accessor {
                        let get_bits = if !has_get || get_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                        };
                        let set_bits = if !has_set || set_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                        };
                        crate::symbol::set_symbol_accessor_property(
                            obj_value, key_value, get_bits, set_bits,
                        );
                    } else {
                        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                        if own_key_present(desc_ptr, value_key) {
                            let value_field = js_object_get_field_by_name(
                                desc_ptr as *const ObjectHeader,
                                value_key,
                            );
                            crate::symbol::js_object_set_symbol_property(
                                obj_value,
                                key_value,
                                f64::from_bits(value_field.bits()),
                            );
                        }
                    }
                    let read_bool = |name: &[u8]| -> Option<bool> {
                        let k =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                        if v.is_undefined() {
                            None
                        } else {
                            Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                        }
                    };
                    let writable = read_bool(b"writable").unwrap_or(has_accessor);
                    let enumerable = read_bool(b"enumerable").unwrap_or(false);
                    let configurable = read_bool(b"configurable").unwrap_or(false);
                    crate::symbol::set_symbol_property_attrs(
                        obj as usize,
                        raw_ptr as usize,
                        PropertyAttrs::new(writable, enumerable, configurable),
                    );
                }
                return obj_value;
            }
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract the key as a Rust string for the descriptor side-table lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        // #4949 / #2159 follow-up: `ClassExprFresh.prototype` now materializes
        // the declared-class prototype object. Keep `Object.defineProperty` on
        // that live object wired to the same prototype-method side tables used
        // by the historical ClassRef path, so instances observe decorator/mixin
        // method replacements.
        if let Some(target_cid) =
            super::class_registry::class_id_for_decl_prototype_object(obj as usize)
        {
            if let Some(ref name) = key_rust {
                if desc_has_field(descriptor_value, b"value") {
                    let value_field = desc_read_field(descriptor_value, b"value");
                    if !value_field.is_undefined() {
                        // #5024 followup: defineProperty data descriptor is
                        // non-enumerable unless it sets `enumerable: true`. Mark
                        // it so the prototype-method enumeration mirror honours
                        // the descriptor instead of defaulting to enumerable
                        // (the `Class.prototype.m = fn` assignment default).
                        super::class_registry::class_prototype_method_set_enumerable(
                            target_cid,
                            name,
                            descriptor_enumerable(descriptor_value),
                        );
                        define_class_prototype_method(target_cid, name, value_field.bits());
                    }
                }
            }
        }
        if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
            if let Some(ref key_name) = key_rust {
                return crate::typedarray_props::typed_array_define_own_property(
                    obj_value,
                    obj as *mut crate::typedarray::TypedArrayHeader,
                    key_str,
                    key_name,
                    descriptor_value,
                );
            }
            return obj_value;
        }
        if let Some(ok) = super::define_array_property(
            obj,
            obj_value,
            key_str,
            key_rust.as_deref(),
            descriptor_value,
        ) {
            if ok {
                return obj_value;
            }
            // A rejected array `[[DefineOwnProperty]]` (e.g. redefining the
            // non-configurable / non-writable `length`, or a forbidden change to
            // a non-configurable index property) throws under
            // `Object.defineProperty`.
            let k = key_rust.as_deref().unwrap_or("length");
            throw_object_type_error_with_suffix("Cannot redefine property: ", k);
        }
        // #2843: enforce frozen / sealed / non-extensible invariants BEFORE any
        // mutation, so a rejected definition leaves the object untouched and the
        // thrown TypeError matches Node.
        if let Some(ref k) = key_rust {
            enforce_define_property_invariants(obj, key_str, k, descriptor_value);
        }
        super::mark_object_dynamic_shape_unknown(obj);
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }

        // Spec (OrdinaryDefineOwnProperty / ValidateAndApplyPropertyDescriptor):
        // when the property ALREADY EXISTS as an own property, attribute fields
        // the descriptor omits must RETAIN the property's current values — they do
        // NOT reset to the new-property `false` default. Capture the current
        // attributes before any mutation below. `None` ⇒ the key is new, so the
        // historical all-`false` (writable defaults to `has_accessor`) applies.
        let existing_attrs: Option<PropertyAttrs> = key_rust.as_ref().and_then(|k| {
            if super::obj_value_has_own_key(obj_value, key_value) {
                Some(
                    super::get_property_attrs(obj as usize, k)
                        .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                )
            } else {
                None
            }
        });

        // Detect accessor descriptor (has `get` and/or `set`) vs. data
        // descriptor (has `value`/`writable`) by `ToPropertyDescriptor` field
        // PRESENCE (HasProperty — own OR inherited) on the descriptor object,
        // not by `is_undefined`: `{ get: undefined }` is an explicit (present)
        // accessor field, and an *inherited* `value`/`get` counts as present.
        let desc_has_get = desc_has_field(descriptor_value, b"get");
        let desc_has_set = desc_has_field(descriptor_value, b"set");
        let get_field = desc_read_field(descriptor_value, b"get");
        let set_field = desc_read_field(descriptor_value, b"set");
        let has_accessor = desc_has_get || desc_has_set;

        // The existing accessor (if the property is currently an accessor) —
        // used to retain `get`/`set` fields the redefining descriptor omits.
        let existing_accessor: Option<AccessorDescriptor> = key_rust
            .as_ref()
            .and_then(|k| super::get_accessor_descriptor(obj as usize, k));

        if has_accessor {
            // Store the accessor closures in the side table. Ensure the key is present
            // in the object's keys_array so lookups (hasOwn, getOwnPropertyDescriptor,
            // keys) can see it.
            ensure_key_in_keys_array(obj, key_str);
            if let Some(k) = key_rust.clone() {
                // Issue #450: spec says the getter/setter runs with `this === obj`
                // (the property access target). The user's descriptor literal
                // `{ get() {...}, set() {...} }` was lowered with `captures_this: true`
                // and had its reserved `this` slot patched to point to the *descriptor*
                // object at construction time — that's what every other object-literal
                // method does. Clone the closure once at defineProperty time and
                // rebind `this` to `obj`, so every subsequent get/set call sees the
                // correct receiver. Closures without CAPTURES_THIS_FLAG (e.g. arrow-form
                // `get: () => this._backing` written as a field rather than a method
                // shorthand) pass through unchanged.
                //
                // Spec retention (ValidateAndApplyPropertyDescriptor): redefining
                // an existing accessor with a descriptor that omits `get` (or
                // `set`) keeps the current accessor's `get` (or `set`). When the
                // current property is a data property being converted to an
                // accessor, omitted fields default to `undefined` (0).
                let recv_box = crate::value::js_nanbox_pointer(obj as i64);
                let prior = existing_accessor;
                let get_bits = if desc_has_get {
                    if get_field.is_undefined() {
                        0u64
                    } else {
                        crate::closure::clone_closure_rebind_this(get_field.bits(), recv_box)
                    }
                } else {
                    prior.map(|a| a.get).unwrap_or(0)
                };
                let set_bits = if desc_has_set {
                    if set_field.is_undefined() {
                        0u64
                    } else {
                        crate::closure::clone_closure_rebind_this(set_field.bits(), recv_box)
                    }
                } else {
                    prior.map(|a| a.set).unwrap_or(0)
                };
                set_accessor_descriptor(
                    obj as usize,
                    k,
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            }
        } else {
            // Either a data descriptor (`value`/`writable` present) or a generic
            // descriptor (only `enumerable`/`configurable`). Detect by own-field
            // presence so `{ value: undefined }` (present) stores `undefined`,
            // while a generic descriptor on an existing accessor leaves it intact.
            let desc_has_value = desc_has_field(descriptor_value, b"value");
            let desc_has_writable = desc_has_field(descriptor_value, b"writable");
            let is_data = desc_has_value || desc_has_writable;

            if is_data {
                // Converting to / redefining as a data property. Clear any
                // existing accessor for this key so the write doesn't fire the
                // setter, and clear any stale per-key descriptor so a prior
                // `writable: false` doesn't reject the forced store below. The
                // final attributes are (re)applied a few lines down.
                if let Some(ref k) = key_rust {
                    ACCESSOR_DESCRIPTORS.with(|m| {
                        m.borrow_mut().remove(&(obj as usize, k.clone()));
                    });
                    clear_property_attrs(obj as usize, k);
                }
                let value_field = desc_read_field(descriptor_value, b"value");
                // Ensure the key exists; store the (possibly `undefined`) value
                // via `[[DefineOwnProperty]]`, bypassing the `[[Set]]` writability
                // / frozen guard (invariants already enforced above). When
                // `value` is omitted (a `{ writable: ... }`-only descriptor on a
                // brand-new property) the value defaults to `undefined`.
                if desc_has_value {
                    define_property_force_store_value(
                        obj,
                        key_str,
                        f64::from_bits(value_field.bits()),
                    );
                } else if existing_accessor.is_some() {
                    // Accessor → data with no `value`: the value becomes the
                    // data default `undefined`.
                    define_property_force_store_value(
                        obj,
                        key_str,
                        f64::from_bits(crate::value::TAG_UNDEFINED),
                    );
                } else {
                    ensure_key_in_keys_array(obj, key_str);
                }
            } else {
                // Generic descriptor: no value/writable/get/set. It only adjusts
                // enumerable/configurable and never converts the property kind.
                // Leave any existing accessor / data value untouched; just make
                // sure the key is present (for a brand-new generic define).
                ensure_key_in_keys_array(obj, key_str);
            }
        }

        // Read attribute flags from descriptor. JS defaults when omitted in
        // `Object.defineProperty` are `false` (NOT `true` like for direct assignment).
        let read_bool = |name: &[u8]| -> Option<bool> {
            let v = desc_read_field(descriptor_value, name);
            if v.is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
            }
        };
        // Omitted attributes default to the EXISTING property's value when
        // redefining (spec retention, see `existing_attrs` above), else to
        // `false` for a new property. Accessor descriptors don't carry
        // `writable`; for a brand-new accessor we leave it `true` (via
        // `has_accessor`) so data lookups before the accessor override don't
        // reject a legitimate fallthrough write.
        //
        // Accessor → data conversion: the current property has no
        // [[Writable]], so an omitted `writable` defaults to FALSE (the
        // retained-attrs rule doesn't apply across the kind switch).
        let accessor_to_data = existing_accessor.is_some()
            && !has_accessor
            && (desc_has_field(descriptor_value, b"value")
                || desc_has_field(descriptor_value, b"writable"));
        let writable = read_bool(b"writable").unwrap_or_else(|| {
            if accessor_to_data {
                false
            } else {
                existing_attrs.map(|a| a.writable()).unwrap_or(has_accessor)
            }
        });
        let enumerable = read_bool(b"enumerable")
            .unwrap_or_else(|| existing_attrs.map(|a| a.enumerable()).unwrap_or(false));
        let configurable = read_bool(b"configurable")
            .unwrap_or_else(|| existing_attrs.map(|a| a.configurable()).unwrap_or(false));

        if let Some(k) = key_rust {
            set_property_attrs(
                obj as usize,
                k,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
        }
        super::arguments_object_after_define(obj, key_str, descriptor_value);
        // Return the object
        obj_value
    }
}

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
#[allow(unused_assignments)]
pub(crate) unsafe fn ensure_key_in_keys_array(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    macro_rules! refresh_define_property_roots {
        () => {{
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        }};
    }
    // If no keys array exists, create one with this key.
    let keys = (*obj).keys_array;
    if keys.is_null() {
        let new_keys = crate::array::js_array_alloc(4);
        refresh_define_property_roots!();
        let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
        refresh_define_property_roots!();
        set_object_keys_array(obj, new_keys);
        if (*obj).field_count == 0 {
            (*obj).field_count = 1;
        }
        return;
    }
    // Validate keys array pointer. The bare high-bits/low-address checks let
    // through values that are non-null and tag-free yet still not real heap
    // pointers (e.g. a stray `0x20_0000_0203` left in a miscompiled object's
    // keys_array slot), which then fault inside `js_array_length`'s GC-header
    // read. Gate on the arena-bounds predicate (same one `js_object_create`
    // uses for prototype validation) so a garbage slot is treated as "no keys
    // array" instead of crashing the process. (#321: defends against the
    // Effect `makeGenericTag` mis-tagged-receiver corruption.)
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || !is_valid_obj_ptr(keys as *const u8) {
        return;
    }
    // Check if key already exists
    let key_count = crate::array::js_array_length(keys) as usize;
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — pre-fix an existing inline-SSO key
        // wasn't seen here, so `Object.defineProperty(obj, "id", ...)`
        // on an object that already had `id` as an SSO key
        // double-inserted instead of overwriting.
        if crate::string::js_string_key_matches(stored, key) {
            return; // already present
        }
    }
    // Clone shared keys array if needed, then append.
    let owned_keys = if key_count == (*obj).field_count as usize {
        let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
        refresh_define_property_roots!();
        let keys = (*obj).keys_array;
        let src_data = (keys as *const u8).add(8) as *const f64;
        let dst_data = (cloned as *mut u8).add(8) as *mut f64;
        for i in 0..key_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *dst_data.add(i) = *src_data.add(i);
        }
        (*cloned).length = key_count as u32;
        super::rebuild_array_layout_from_slots(cloned);
        set_object_keys_array(obj, cloned);
        cloned
    } else {
        keys
    };
    let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
    let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
    let _owned_keys = owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>();
    refresh_define_property_roots!();
    set_object_keys_array(obj, new_keys);
    // `field_count` is the inline/overflow boundary consulted by the read path
    // (`js_object_get_field`: index < field_count ⇒ read inline slot, else the
    // overflow map). It must never exceed the object's physically-allocated
    // inline capacity, which is `max(field_count, 8)` (see `js_object_alloc`).
    // Only bump it when this key genuinely lands in an in-bounds inline slot.
    //
    // A keys-only entry — a built-in accessor like `Map.prototype.size`, or a
    // key whose data spilled to the overflow map — must NOT push field_count
    // past the inline region. Doing so reclassifies already-overflowed (or
    // out-of-bounds) slots as inline, so later reads dereference past the
    // allocation into adjacent-heap garbage. That is what made
    // `Map.prototype.set` / `.values` read back as raw non-pointer values and
    // crash the reflective `.call` dispatch (#4099): installing the `size`
    // getter here bumped field_count from 8 (the proto's physical capacity) to
    // 11, exposing the overflowed `values` slot and corrupting the boundary.
    let new_index = key_count as u32;
    let inline_capacity = std::cmp::max((*obj).field_count, 8);
    if new_index < inline_capacity && new_index >= (*obj).field_count {
        (*obj).field_count = new_index + 1;
    }
}

/// Install a built-in *getter-only* accessor on a prototype object so that
/// `Object.getOwnPropertyDescriptor(proto, key)` reflects it as a real
/// accessor descriptor `{ get, set: undefined, enumerable, configurable }`.
///
/// `getter_bits` is the NaN-boxed `f64` bits of the getter closure (0 = none).
/// The descriptor is non-enumerable and configurable, matching the ECMA-262
/// shape for `%TypedArray%.prototype` accessors like `length` / `byteLength` /
/// `byteOffset` / `buffer`. Reflection-only: this does NOT flip the hot-path
/// descriptor gate (see `set_builtin_accessor_descriptor`). #2060.
pub(crate) unsafe fn install_builtin_getter(proto: *mut ObjectHeader, key: &str, getter_bits: u64) {
    if proto.is_null() || (proto as usize) < 0x10000 {
        return;
    }
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_str.is_null() {
        return;
    }
    // Make the key discoverable by `own_key_present` / `getOwnPropertyNames`.
    ensure_key_in_keys_array(proto, key_str);
    // Spec: an accessor getter's `.name` is `"get " + key` (e.g.
    // `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get.name
    // === "get byteLength"`). Register it against the getter closure's func_ptr;
    // without this the `.name` read returned `""`.
    let getter_ptr = (getter_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if getter_ptr >= 0x1000 && crate::closure::is_closure_ptr(getter_ptr) {
        let func_ptr = (*(getter_ptr as *const crate::closure::ClosureHeader)).func_ptr as usize;
        crate::builtins::register_function_name_if_absent(func_ptr, &format!("get {key}"));
    }
    set_builtin_accessor_descriptor(
        proto as usize,
        key.to_string(),
        AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        // writable is N/A for an accessor; enumerable=false, configurable=true.
        PropertyAttrs::new(true, false, true),
    );
}

/// Helper: does `key` appear in `obj.keys_array`?
pub(crate) unsafe fn own_key_present(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    // Every GC allocation is `align.max(8)`-aligned, so a real object pointer
    // has its low 3 bits clear. Rejecting misaligned `obj` keeps a non-object
    // value (e.g. a native-module namespace sentinel reaching `hasOwnProperty`
    // via a caller that didn't route through `extract_obj_ptr`) from being
    // dereferenced as an ObjectHeader. (#3527)
    if obj.is_null() || (obj as usize) < 0x10000 || (obj as usize) & 0x7 != 0 || key.is_null() {
        return false;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    // Same alignment invariant for the keys_array pointer: when `obj` is not a
    // genuine object its `keys_array` field holds garbage that may land in the
    // address range yet be misaligned. Without this guard the `[keys-8]`
    // GcHeader read below SIGBUSes on that garbage. (#3527)
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || keys_ptr & 0x7 != 0 {
        return false;
    }
    // Validate keys_array GC header
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        // #1781: SSO-aware match — `hasOwnProperty("id")` previously
        // returned false when "id" lived as an inline SSO key.
        if crate::string::js_string_key_matches(stored, key) {
            return true;
        }
    }
    false
}

/// `Object.prototype.__defineGetter__(key, getter)` (Annex B §B.2.2.2).
/// Installs an accessor with the given getter and `enumerable: true,
/// configurable: true`. A non-callable getter throws a TypeError. Returns
/// `undefined`.
#[no_mangle]
pub extern "C" fn js_object_define_getter(this: f64, key: f64, getter: f64) -> f64 {
    unsafe { define_accessor_annexb(this, key, getter, true) }
}

/// `Object.prototype.__defineSetter__(key, setter)` (Annex B §B.2.2.3).
#[no_mangle]
pub extern "C" fn js_object_define_setter(this: f64, key: f64, setter: f64) -> f64 {
    unsafe { define_accessor_annexb(this, key, setter, false) }
}

/// Shared `__defineGetter__`/`__defineSetter__` body. Builds an accessor
/// descriptor `{ [get|set]: func, enumerable: true, configurable: true }` and
/// delegates to `js_object_define_property`, so the function's `this`-binding
/// and the closure/class-ref/symbol-key paths all behave like a normal
/// accessor define.
unsafe fn define_accessor_annexb(this: f64, key: f64, func: f64, is_getter: bool) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if !value_is_callable(func) {
        let which = if is_getter {
            "__defineGetter__"
        } else {
            "__defineSetter__"
        };
        throw_object_type_error(format!("Object.prototype.{which}: Expecting function").as_bytes());
    }
    let desc = js_object_alloc(0, 3);
    if desc.is_null() {
        return undef;
    }
    let field = if is_getter { "get" } else { "set" };
    let fkey = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    js_object_set_field_by_name(desc, fkey, func);
    let true_v = f64::from_bits(crate::value::JSValue::bool(true).bits());
    let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
    js_object_set_field_by_name(desc, enum_key, true_v);
    let cfg_key = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
    js_object_set_field_by_name(desc, cfg_key, true_v);
    let desc_val = f64::from_bits(crate::value::JSValue::pointer(desc as *const u8).bits());
    js_object_define_property(this, key, desc_val);
    undef
}

/// `Object.prototype.__lookupGetter__(key)` (Annex B §B.2.2.4). Walks the
/// receiver's own + prototype chain; returns the getter of the first own
/// accessor property found (or `undefined`).
#[no_mangle]
pub extern "C" fn js_object_lookup_getter(this: f64, key: f64) -> f64 {
    unsafe { lookup_accessor_annexb(this, key, true) }
}

/// `Object.prototype.__lookupSetter__(key)` (Annex B §B.2.2.5).
#[no_mangle]
pub extern "C" fn js_object_lookup_setter(this: f64, key: f64) -> f64 {
    unsafe { lookup_accessor_annexb(this, key, false) }
}

/// Shared `__lookupGetter__`/`__lookupSetter__` body. Walks own + proto chain
/// via `getOwnPropertyDescriptor`/`getPrototypeOf`; the first own property
/// found stops the walk — its `get`/`set` field is returned (`undefined` for a
/// data property or the opposite-only accessor case).
unsafe fn lookup_accessor_annexb(this: f64, key: f64, want_getter: bool) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let field = if want_getter { "get" } else { "set" };
    let fkey = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let mut cur = this;
    // Cap the walk so a pathological/cyclic prototype can't spin forever.
    for _ in 0..100_000 {
        let jv = crate::value::JSValue::from_bits(cur.to_bits());
        if jv.is_null() || jv.is_undefined() {
            return undef;
        }
        let desc = js_object_get_own_property_descriptor(cur, key);
        if !crate::value::JSValue::from_bits(desc.to_bits()).is_undefined() {
            let desc_ptr = extract_obj_ptr(desc);
            if desc_ptr.is_null() {
                return undef;
            }
            let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, fkey);
            return f64::from_bits(v.bits());
        }
        cur = js_object_get_prototype_of(cur);
    }
    undef
}

/// Issue #620: returns the OWN-property value at `name` if one exists in the
/// receiver's own keys_array (a string-keyed data property), otherwise
/// returns TAG_UNDEFINED. Used by class-method dispatch to detect override
/// patterns like `this.method = X` (hono's SmartRouter.match rebinds itself
/// on first call). Distinct from `js_object_get_field_by_name` because it
/// does NOT walk the class vtable's getter chain — we only want a raw own
/// data-property read, not a side-effecting getter invocation.
#[no_mangle]
pub extern "C" fn js_object_get_own_field_or_undef(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    const TAG_UNDEF: u64 = 0x7FFC_0000_0000_0001;
    if name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        // Reject anything in the native / Web-Fetch small-handle band (see
        // `value::addr_class`). Headers/Request/Response/Blob and node:http
        // handles are NaN-boxed POINTER_TAG values holding a small registry
        // id, not heap object pointers. The old `< 0x10000` floor let a
        // Headers handle (first id = 0x40000) through; this fn then
        // dereferenced `[handle - GC_HEADER_SIZE]` as a GcHeader and
        // segfaulted. macOS's `is_valid_obj_ptr` floor (0x200_0000_0000)
        // masked this, but on Linux/Android/iOS the floor is 0x1000, so the
        // bad deref reached.
        if !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
            return f64::from_bits(TAG_UNDEF);
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return f64::from_bits(TAG_UNDEF);
        }
        // Skip closures sharing the GC_TYPE_OBJECT slot (CLOSURE_MAGIC at +12).
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
            return f64::from_bits(TAG_UNDEF);
        }
        let key_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_count = crate::array::js_array_length(keys) as usize;
        if key_count > 65536 {
            return f64::from_bits(TAG_UNDEF);
        }
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: SSO-aware match by byte slice — the
            // own-property-or-undef path was the route through which
            // hono's `c.req.X` dispatch decided to invoke the vtable
            // getter, and pre-fix a SSO-stored `X` was invisible here.
            if crate::string::js_string_key_matches_bytes(key_val, key_bytes) {
                let val = if i < alloc_limit {
                    js_object_get_field(obj, i as u32)
                } else {
                    match overflow_get(obj as usize, i) {
                        Some(bits) => crate::JSValue::from_bits(bits),
                        None => return f64::from_bits(TAG_UNDEF),
                    }
                };
                return f64::from_bits(val.bits());
            }
        }
        f64::from_bits(TAG_UNDEF)
    }
}

/// Look up the canonical NaN-boxed value of a built-in constructor /
/// namespace stored on `globalThis` (the singleton populated by
/// `populate_global_this_builtins`). Used by `instance.constructor`
/// reads and by bare `Date`/`Array`/`Object` identifier resolution so
/// both forms produce the same closure-pointer value — that's what
/// `instance.constructor === Date` (date-fns's `constructFrom`,
/// drizzle's `is(value, ctor)` duck checks, ...) hinges on.
///
/// Returns NaN-boxed undefined if the name isn't one of the populated
/// built-ins or the singleton hasn't been initialized yet.
#[no_mangle]
pub extern "C" fn js_get_global_this_builtin_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    // Force the singleton init the first time so the lookup below has
    // a populated field bag.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(global_obj, key);
    let bits = value.bits();
    f64::from_bits(bits)
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(proto_value: f64) -> f64 {
    // #809: actually wire up the prototype. Pre-fix this ignored its
    // argument entirely, so `Object.create(Proto)` returned a bare empty
    // object — `inst.method()` / `inst.prop` saw nothing and threw
    // `TypeError: <m> is not a function`. Reuse the #711 prototype-object
    // machinery: allocate a synthetic class_id, map it to `proto` in
    // CLASS_PROTOTYPE_OBJECTS, and stamp the new object with that id. The
    // chain walk in `js_object_get_field_by_name` (the `class_id != 0`
    // branch) then resolves missing own props/methods off `proto`.
    //
    // `Object.create(null)` (or a non-object proto / a builtin-backed
    // Set/Map/Regex source Perry can't model as a prototype) falls back
    // to the original behavior: a plain prototype-less object.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let mut class_id: u32 = 0;
    let proto_bits = proto_value.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto_value) as *mut ObjectHeader;
        if !proto_ptr.is_null() && (proto_ptr as usize) > 0x10000 {
            let proto_addr = proto_ptr as usize;
            let modellable = !(crate::set::is_registered_set(proto_addr)
                || crate::map::is_registered_map(proto_addr)
                || crate::regex::is_regex_pointer(proto_ptr as *const u8));
            let valid = modellable && is_valid_obj_ptr(proto_ptr as *const u8);
            if valid {
                let cid =
                    NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                class_prototype_object_root_store(cid, proto_ptr);
                unsafe { js_register_class_id(cid) };
                // #1805: link the synthetic class_id into the original class's
                // inheritance chain. `Object.getPrototypeOf(instance)` returns
                // the instance pointer itself in Perry's model (see
                // `js_object_get_prototype_of`), so `proto_ptr` here is a real
                // class instance whose `class_id` field IS the user class's
                // id. Registering it as the synthetic cid's parent lets
                // `js_instanceof`'s `get_parent_class_id` walk reach the
                // original class and match — without this, the chain stopped
                // at the unregistered synthetic id and `Object.create(proto)
                // instanceof C` was always false even though property /
                // getter dispatch through the chain worked correctly.
                let parent_class_id = unsafe { (*proto_ptr).class_id };
                if parent_class_id != 0 && parent_class_id != cid {
                    register_class(cid, parent_class_id);
                }
                class_id = cid;
            }
        }
    }
    // #1175: when `proto_value` is null/undefined/non-object, the resulting
    // object has no [[Prototype]]. Stamp OBJ_FLAG_NULL_PROTO so
    // `Object.getPrototypeOf(Object.create(null))` returns null (it
    // previously returned the object itself).
    let null_proto = class_id == 0;
    let obj = if null_proto {
        js_object_alloc_null_proto(class_id, 0)
    } else {
        js_object_alloc(class_id, 0)
    };
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.getPrototypeOf(obj):
/// - For an INT32-tagged class ref (top16 == 0x7FFE) — return the parent
///   class ref via CLASS_REGISTRY's parent_class_id chain, or null at
///   the root. Drizzle's `is(value, type)` chain walks this.
/// - For an object instance with a registered class_id — return the
///   class ref. Conceptually JS returns `Class.prototype`; Perry doesn't
///   maintain prototype objects, but drizzle's chain consumes
///   `Object.getPrototypeOf(value).constructor`, and class_ref's
///   `.constructor` synthesizes back to the same class ref via the
///   constructor intercept (v0.5.746). So returning the class ref here
///   makes that chain produce `value.constructor` as Node would.
/// - Other receivers — null.
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    // #2820: `Object.getPrototypeOf(null | undefined)` throws TypeError
    // (`Cannot convert undefined or null to object`). Class refs and heap
    // objects fall through to the existing resolution below.
    {
        let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
    }
    // A Proxy is a small registered id, NOT a heap object — the handle path
    // below would mis-read it and return `null`. Route it to the proxy
    // `[[GetPrototypeOf]]` (handler trap, else the target's prototype) so
    // `Object.getPrototypeOf(proxy)` matches the target. drizzle aliases columns
    // as `new Proxy(column, …)` and `is(value, type)` reads
    // `getPrototypeOf(value).constructor`, which crashed on `null.constructor`.
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return crate::proxy::js_proxy_get_prototype_of(obj_value);
    }
    // A Temporal value is a NaN-boxed opaque cell, not an `ObjectHeader` — the
    // heap-object resolution below would deref its boxed payload as a class id
    // and crash. The reflective prototype is reachable directly as
    // `Temporal.<Type>.prototype`, so for a cell receiver return `null` rather
    // than faulting on the cell.
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(obj_value) {
        return f64::from_bits(TAG_NULL);
    }
    // ES2015 ToObject(primitive): `Object.getPrototypeOf(0 | "s" | true |
    // 1n | sym)` resolves to the wrapper class prototype, not a TypeError /
    // null (15.2.3.2-1*).
    {
        let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
        // An INT32-tagged value may be a class ref (same 0x7FFE tag as small
        // integers) — those must keep flowing to the class resolution below.
        let is_class_ref =
            (obj_value.to_bits() >> 48) == 0x7FFE && super::class_ref_id(obj_value).is_some();
        let wrapper = if is_class_ref {
            None
        } else if jv.is_number() {
            Some("Number")
        } else if jv.is_any_string() {
            Some("String")
        } else if jv.is_bool() {
            Some("Boolean")
        } else if jv.is_bigint() {
            Some("BigInt")
        } else if unsafe { crate::symbol::js_is_symbol(obj_value) } != 0 {
            Some("Symbol")
        } else {
            None
        };
        if let Some(name) = wrapper {
            let proto = crate::object::builtin_prototype_value(name);
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return proto;
            }
            return f64::from_bits(TAG_NULL);
        }
    }
    let bits = obj_value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if crate::value::addr_class::is_small_handle(raw_addr as usize) {
            if let Some(dispatch) = super::class_registry::handle_prototype_dispatch() {
                let proto = unsafe { dispatch(raw_addr as i64) };
                if proto.to_bits() != crate::value::TAG_UNDEFINED {
                    return proto;
                }
            }
            return f64::from_bits(TAG_NULL);
        }
    }
    let collection_prototype = |addr: usize| -> Option<f64> {
        if crate::map::is_registered_map(addr) {
            let proto = crate::object::builtin_prototype_value("Map");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        if crate::set::is_registered_set(addr) {
            let proto = crate::object::builtin_prototype_value("Set");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        None
    };
    let buffer_backed_prototype = |addr: usize| -> Option<f64> {
        let name = if crate::buffer::is_array_buffer(addr) {
            "ArrayBuffer"
        } else if crate::buffer::is_shared_array_buffer(addr) {
            "SharedArrayBuffer"
        } else {
            return None;
        };
        let proto = crate::object::builtin_prototype_value(name);
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let buffer_backed_uint8array_prototype = |addr: usize| -> Option<f64> {
        if !crate::buffer::is_uint8array_buffer(addr) {
            return None;
        }
        let proto = crate::object::builtin_prototype_value("Uint8Array");
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let typed_array_instance_prototype = |addr: usize| -> Option<f64> {
        let kind = crate::typedarray::lookup_typed_array_kind(addr)?;
        // A `Reflect.construct(TA, …, newTarget)` view with a custom
        // `[[Prototype]]` (spec `GetPrototypeFromConstructor`) resolves to the
        // recorded prototype rather than the default per-kind prototype. The
        // link is stored in the GC-tracked static-prototype side table.
        if let Some(proto_bits) = super::prototype_chain::object_static_prototype(addr) {
            if proto_bits != crate::value::TAG_NULL {
                return Some(f64::from_bits(proto_bits));
            }
        }
        let proto = crate::object::builtin_prototype_value(crate::typedarray::name_for_kind(kind));
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let function_prototype_or_null = || {
        let proto = crate::object::builtin_prototype_value("Function");
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            proto
        } else {
            f64::from_bits(TAG_NULL)
        }
    };
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(parent_id) = get_parent_class_id(class_id) {
            if parent_id != 0 {
                let parent_bits = 0x7FFE_0000_0000_0000u64 | (parent_id as u64);
                return f64::from_bits(parent_bits);
            }
        }
        return f64::from_bits(TAG_NULL);
    }
    // Heap-pointer receiver — return the input value itself. For
    // class-id-tagged instances, `.constructor` then returns the class
    // ref (via the constructor intercept in js_object_get_field_by_name,
    // v0.5.746), making `getPrototypeOf(v).constructor === v.constructor`.
    // For object literals / arrays / other non-class-tagged heap values,
    // `.constructor` returns undefined, which collapses drizzle's
    // `if (cls)` chain to false safely (instead of throwing on
    // `null.constructor` if we returned null). Drizzle's
    // `is(value, type)` chain calls this on every chunk including
    // arrays of values, so the array case is load-bearing.
    //
    // Two NaN-shapes cover the heap-pointer case:
    //  - top16 == 0x7FFD: NaN-boxed POINTER_TAG (typical function-local).
    //  - top16 == 0x0000 with raw_addr large enough: module-level object
    //    literals get stored as raw I64 pointers (no NaN-boxing) per the
    //    "Module-level variables" note in CLAUDE.md, so we accept that
    //    form here too.
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            if let Some(proto) = typed_array_instance_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = buffer_backed_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = buffer_backed_uint8array_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = collection_prototype(raw_addr as usize) {
                return proto;
            }
            // #2820: an explicit `Object.setPrototypeOf(obj, proto)` recorded
            // in the side-table takes precedence — return exactly what was set
            // (including `null`).
            if let Some(proto_bits) =
                super::prototype_chain::object_static_prototype(raw_addr as usize)
            {
                return f64::from_bits(proto_bits);
            }
            unsafe {
                let obj = raw_addr as *const ObjectHeader;
                let gc = gc_header_for(obj);
                // #1175: objects allocated with a null prototype
                // (Object.create(null), querystring.parse) report null here.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                // #2145: per-kind typed-array `.prototype` objects share a
                // single `%TypedArray%.prototype` parent. Resolved off the
                // cached intrinsic pointer (also a GC root) so the chain holds
                // through copying GC.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let err = raw_addr as *const crate::error::ErrorHeader;
                    if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                        return proto;
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    if let Some(proto) = super::array_get_prototype_of_addr(raw_addr as usize) {
                        return proto;
                    }
                }
                // #489 / #2145: a function/constructor receiver has no
                // walkable [[Prototype]] in Perry's model UNLESS its
                // closure-static-prototype side-table has been set
                // (`Object.setPrototypeOf(closure, parent)` — effect's
                // TagClass and Perry's `%TypedArray%`-chain typed-array
                // constructors use this). Returning the recorded parent
                // satisfies drizzle's `cls = getPrototypeOf(cls)` walk
                // (which terminates when the parent has no further
                // recorded proto) and the test262 `__proto__` chain. When
                // no static prototype is recorded, return null to break
                // the would-be `getPrototypeOf(cls) === cls` self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(raw_addr as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    // #3664: a generator/async-generator function's
                    // [[Prototype]] is `%Generator%` / `%AsyncGenerator%`.
                    if let Some(proto) =
                        crate::object::generator_function_proto_of(raw_addr as usize)
                    {
                        return proto;
                    }
                    return function_prototype_or_null();
                }
                // Fast [[Prototype]] for a DECLARED-class instance: resolve
                // directly from the class id instead of the generic
                // `constructor_dynamic_prototype` probe, which reads the
                // `constructor` field by name and therefore does a LINEAR scan
                // over the instance's own keys (O(own-key-count)) before missing
                // and continuing to the prototype. On a wide build —
                // `const o = new C(); for (i) o["k"+i] = i` — that scan grows by
                // one each iteration, making any reflective getPrototypeOf on the
                // instance O(n²). The class-id table at line ~2810 below already
                // returns this exact prototype for the same instances; hoisting it
                // here is semantically identical (same declared-class prototype
                // object) but O(1). Gated on a REAL declared class id only
                // (`class_decl_prototype_value_for_instance_class` returns None for
                // class_id 0 / anonymous-shape / unregistered ids), so synthetic
                // function-ctor instances and plain objects keep the existing
                // `constructor`-based resolution unchanged.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*obj).class_id != 0
                    && !is_anon_shape_class_id((*obj).class_id)
                {
                    if let Some(proto) =
                        super::class_registry::class_decl_prototype_value_for_instance_class(
                            (*obj).class_id,
                        )
                    {
                        return proto;
                    }
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
                {
                    if let Some(proto_bits) =
                        super::prototype_chain::default_object_prototype_for_owner(
                            raw_addr as usize,
                        )
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                // Built-in iterator instances (Array/Map/Set/String iterators)
                // share a `%...IteratorPrototype%` singleton. Their instances
                // normally carry it as a recorded static prototype (returned
                // above), but resolve by class id too so the chain holds even if
                // the static-prototype side-table entry was dropped.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    if let Some(proto) = super::iterator_prototype_for_class_id((*obj).class_id) {
                        return proto;
                    }
                    if let Some(proto) =
                        super::class_registry::class_decl_prototype_value_for_instance_class(
                            (*obj).class_id,
                        )
                    {
                        return proto;
                    }
                    // #3986: `Object.create(proto)` and `new F()` (a plain
                    // function ctor, whose instances carry a synthetic
                    // function-prototype class id) record the actual
                    // [[Prototype]] object pointer in CLASS_PROTOTYPE_OBJECTS
                    // keyed by that synthetic class id. Return the exact stored
                    // pointer so `Object.getPrototypeOf(o) === proto` holds by
                    // identity (test262 built-ins/Object/create/15.2.3.5-*,
                    // S9.9 ToObject identity). Declared ES classes use the
                    // separate CLASS_DECL_PROTOTYPE_OBJECTS table handled just
                    // above, so this does not perturb the
                    // `getPrototypeOf(instance) === instance` model their
                    // `.constructor` resolution relies on. Without this the
                    // synthetic-class instance fell through to the
                    // `return obj_value` self-prototype fallback below.
                    let synth_proto =
                        super::class_registry::class_prototype_object((*obj).class_id);
                    if !synth_proto.is_null() {
                        return f64::from_bits(
                            crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                        );
                    }
                }
                // A native-module namespace object (`require("path")` etc.,
                // class_id NATIVE_MODULE_CLASS_ID, the `__module__`-tagged
                // object) is an ordinary object whose [[Prototype]] is
                // %Object.prototype% — NOT itself. The `return obj_value` self-
                // prototype fallback below makes turbopack's `interopEsm`
                // proto-chain walk (`for(cur=raw; !LEAF.includes(cur);
                // cur=getProto(cur))`) never terminate — getProto keeps
                // returning the same object, so it creates export getters
                // forever (the Next.js standalone startup runaway: unbounded
                // memory growth, no `✓ Ready`). Return Object.prototype so the
                // walk reaches a LEAF_PROTOTYPE and stops.
                if (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID {
                    let proto = crate::object::builtin_prototype_value("Object");
                    if proto.to_bits() != crate::value::TAG_UNDEFINED {
                        return proto;
                    }
                    return f64::from_bits(TAG_NULL);
                }
            }
            return obj_value;
        }
    }
    if top16 == 0 && bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
        if let Some(proto) = typed_array_instance_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = buffer_backed_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = buffer_backed_uint8array_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = collection_prototype(bits as usize) {
            return proto;
        }
        // #2820: explicit setPrototypeOf side-table takes precedence.
        if let Some(proto_bits) = super::prototype_chain::object_static_prototype(bits as usize) {
            return f64::from_bits(proto_bits);
        }
        unsafe {
            let obj = bits as *const ObjectHeader;
            let gc = gc_header_for(obj);
            if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                return f64::from_bits(TAG_NULL);
            }
            if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                let p = crate::object::typed_array_intrinsic_proto_ptr();
                if !p.is_null() {
                    return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                }
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                let err = bits as *const crate::error::ErrorHeader;
                if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                    return proto;
                }
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                if let Some(proto) = super::array_get_prototype_of_addr(bits as usize) {
                    return proto;
                }
            }
            // #489 / #2145: function/constructor receiver — see the
            // 0x7FFD branch above. Return the recorded static
            // prototype if any, else null to break the chain-walk
            // self-cycle.
            if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                if let Some(proto_bits) = crate::closure::closure_static_prototype(bits as usize) {
                    return f64::from_bits(proto_bits);
                }
                // #3664: generator/async-generator [[Prototype]] resolution.
                if let Some(proto) = crate::object::generator_function_proto_of(bits as usize) {
                    return proto;
                }
                return function_prototype_or_null();
            }
            if let Some(proto) = constructor_dynamic_prototype(obj) {
                return proto;
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
            {
                if let Some(proto_bits) =
                    super::prototype_chain::default_object_prototype_for_owner(bits as usize)
                {
                    return f64::from_bits(proto_bits);
                }
                return f64::from_bits(TAG_NULL);
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                if let Some(proto) = super::iterator_prototype_for_class_id((*obj).class_id) {
                    return proto;
                }
                if let Some(proto) =
                    super::class_registry::class_decl_prototype_value_for_instance_class(
                        (*obj).class_id,
                    )
                {
                    return proto;
                }
                // #3986: `Object.create(proto)` and `new F()` (a plain
                // function ctor, whose instances carry a synthetic
                // function-prototype class id) record the actual
                // [[Prototype]] object pointer in CLASS_PROTOTYPE_OBJECTS
                // keyed by that synthetic class id. Return the exact stored
                // pointer so `Object.getPrototypeOf(o) === proto` holds by
                // identity (test262 built-ins/Object/create/15.2.3.5-*,
                // S9.9 ToObject identity). Declared ES classes use the
                // separate CLASS_DECL_PROTOTYPE_OBJECTS table handled just
                // above, so this does not perturb the
                // `getPrototypeOf(instance) === instance` model their
                // `.constructor` resolution relies on. Without this the
                // synthetic-class instance fell through to the
                // `return obj_value` self-prototype fallback below.
                let synth_proto = super::class_registry::class_prototype_object((*obj).class_id);
                if !synth_proto.is_null() {
                    return f64::from_bits(
                        crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                    );
                }
                // A native-module namespace object (`require("path")` etc.,
                // class_id NATIVE_MODULE_CLASS_ID, the `__module__`-tagged
                // object) is an ordinary object whose [[Prototype]] is
                // %Object.prototype% — NOT itself. The `return obj_value` self-
                // prototype fallback below makes turbopack's `interopEsm`
                // proto-chain walk (`for(cur=raw; !LEAF.includes(cur);
                // cur=getProto(cur))`) never terminate — getProto keeps
                // returning the same object, so it creates export getters
                // forever (the Next.js standalone startup runaway: unbounded
                // memory growth, no `✓ Ready`). Return Object.prototype so the
                // walk reaches a LEAF_PROTOTYPE and stops.
                if (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID {
                    let proto = crate::object::builtin_prototype_value("Object");
                    if proto.to_bits() != crate::value::TAG_UNDEFINED {
                        return proto;
                    }
                    return f64::from_bits(TAG_NULL);
                }
            }
        }
        return obj_value;
    }
    f64::from_bits(TAG_NULL)
}

/// `Object.defineProperties(target, descriptors)` — iterate the descriptor
/// object's own keys and invoke `js_object_define_property` for each one.
/// Used by chalk's `Object.defineProperties(createChalk.prototype, styles)`
/// where `styles` is built via `Object.create(null)` + dynamic assignment,
/// so the static `Object(...)` literal desugar in the HIR lowering can't
/// fire and we fall here.
///
/// Returns the target. Spec also returns target — Perry's lowering relies
/// on that so `const x = Object.defineProperties(...)` still binds `x`.
#[no_mangle]
pub extern "C" fn js_object_define_properties(target: f64, descriptors: f64) -> f64 {
    // #2817: target must be an object (or class-ref). Node throws
    // `Object.defineProperties called on non-object` for primitives.
    let target_is_class_ref = super::class_ref_id(target).is_some();
    if !target_is_class_ref && !unsafe { value_is_object_like(target) } {
        throw_object_type_error(b"Object.defineProperties called on non-object");
    }
    // #2817: the properties bag must be coercible to an object. Node throws
    // `Cannot convert undefined or null to object` for null/undefined, and
    // primitives are boxed (no own enumerable keys → no-op). Match the nullish
    // case explicitly.
    {
        let jv = crate::value::JSValue::from_bits(descriptors.to_bits());
        if jv.is_undefined() || jv.is_null() {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
    }
    let desc_obj = unsafe { extract_obj_ptr(descriptors) };
    if desc_obj.is_null() || !is_valid_obj_ptr(desc_obj as *const u8) {
        return target;
    }
    // Snapshot the descriptor object's own keys array. We collect into a
    // Vec<f64> first so adding properties via `js_object_define_property`
    // (which can resize the target's keys_array) can't perturb iteration
    // — descriptors and target are usually different objects, but a
    // defensive copy costs ~ngc and protects against a user who passes
    // `Object.defineProperties(obj, obj)` aliasing.
    // Spec (ObjectDefineProperties): the property keys come from the properties
    // object's own keys, but only the ones whose own descriptor is ENUMERABLE
    // participate — and the descriptor object for each is read through `[[Get]]`
    // (so accessors on the properties bag run). Using the full own-key set is
    // wrong for native namespaces like `Math` (whose `E`/`PI`/... are
    // non-enumerable) and for any object with non-enumerable own props.
    let names_value = js_object_get_own_property_names(descriptors);
    let names_arr =
        crate::value::js_nanbox_get_pointer(names_value) as *const crate::array::ArrayHeader;
    let mut keys: Vec<f64> = Vec::new();
    if !names_arr.is_null() {
        let len = unsafe { crate::array::js_array_length(names_arr) } as usize;
        for i in 0..len {
            let k = unsafe { crate::array::js_array_get(names_arr, i as u32) };
            let k_f64 = f64::from_bits(k.bits());
            // Skip non-enumerable own keys (spec step: descriptor must be
            // enumerable). `propertyIsEnumerable` returns false for absent or
            // non-enumerable keys.
            const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
            if js_object_property_is_enumerable(descriptors, k_f64).to_bits() == TAG_TRUE {
                keys.push(k_f64);
            }
        }
    }
    for k in keys {
        // Read the descriptor through `[[Get]]` so accessors on the properties
        // bag are honored, then ToPropertyDescriptor + DefinePropertyOrThrow.
        //
        // Use the value-level getter (keyed off the `descriptors` *value*, not a
        // raw `ObjectHeader` deref): the properties bag is `ToObject(Properties)`
        // and may be ANY object — a Date, array, boxed primitive, class
        // instance, etc. `Object.create({}, new Date(0))` previously bit-cast the
        // Date's `DateCell` pointer to an `ObjectHeader` and segfaulted. The
        // dynamic getter dispatches on the receiver's real type.
        let key_str = str_from_value(k);
        let descriptor = unsafe {
            if key_str.is_null() {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                crate::value::js_dynamic_object_get_property(
                    descriptors,
                    name_ptr as *const i8,
                    name_len,
                )
            }
        };
        js_object_define_property(target, k, descriptor);
    }
    target
}

const TAG_UNDEFINED_LOCAL: u64 = 0x7FFC_0000_0000_0001;

/// Coerce an arbitrary key value (f64 — usually a STRING_TAG NaN-box) to a
/// `*const StringHeader` for use with `js_object_get_field_by_name_f64`.
/// Returns null if the value isn't string-like.
fn str_from_value(v: f64) -> *const crate::string::StringHeader {
    let bits = v.to_bits();
    let top = bits >> 48;
    if top == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader
    } else {
        // Try to coerce (handles number keys, etc.).
        crate::builtins::js_string_coerce(v) as *const crate::string::StringHeader
    }
}

/// `Object.setPrototypeOf(obj, proto)` — chalk's callable-with-getter-bag
/// foundation. Perry's runtime bakes class IDs at allocation time (it
/// walks `parent_class_id` for INT32-tagged class refs), so we cannot
/// mutate an existing object's prototype chain in a fully observable
/// way. What we *can* do is satisfy the spec's "return target" contract
/// so callers like
///
/// ```text
/// const chalk = (...s) => s.join(' ');
/// Object.setPrototypeOf(chalk, Foo.prototype);
/// ```
///
/// don't crash with `TypeError: value is not a function` (which is what
/// the generic `(Object).setPrototypeOf(...)` PropertyGet → Call fallback
/// used to produce — the property lookup returned undefined and the call
/// dispatched a non-callable). chalk's module init invokes this exact
/// pattern; ms / express decorate functions with `Object.assign` instead,
/// which is already a fast path.
///
/// Pragmatically: today this returns the target and otherwise no-ops.
/// chalk's getters on `createChalk.prototype` won't actually fire under
/// Perry, but the rest of the program keeps running and chalk's
/// call-without-properties form (the most common usage) keeps working.
/// A future change can register the (obj → proto) mapping in a
/// thread-local side-table so a downstream `Object.getPrototypeOf(obj)`
/// + inherited property dispatch can consult it.
#[no_mangle]
pub extern "C" fn js_object_set_prototype_of(obj_value: f64, proto: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let obj_bits = obj_value.to_bits();
    let proto_bits = proto.to_bits();

    // A Proxy receiver is a small registered id, not a heap object — the
    // recording path below would deref the fake pointer and segfault. Route
    // through the Reflect entry (which resolves the proxy to its target) and
    // return the proxy per Object.setPrototypeOf's contract. (Proxy crash
    // cluster.)
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        crate::proxy::js_reflect_set_prototype_of(obj_value, proto);
        return obj_value;
    }

    // #2820: `Object.setPrototypeOf(null | undefined, proto)` throws
    // `TypeError: Object.setPrototypeOf called on null or undefined`.
    {
        let jv = crate::value::JSValue::from_bits(obj_bits);
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Object.setPrototypeOf called on null or undefined");
        }
    }

    // #2820: `proto` must be an object or `null`. A primitive / undefined proto
    // throws `TypeError: Object prototype may only be an Object or null`. A
    // Symbol is pointer-tagged but is NOT an object, so reject it explicitly.
    let proto_is_null = proto_bits == TAG_NULL;
    let proto_is_symbol = unsafe { crate::symbol::js_is_symbol(proto) != 0 };
    let proto_ok = proto_is_null
        || (!proto_is_symbol
            && (unsafe { value_is_object_like(proto) } || super::class_ref_id(proto).is_some()));
    if !proto_ok {
        // V8 renders the offending value: `... an Object or null: 5`.
        let rendered = unsafe { describe_value_for_type_error(proto) };
        throw_object_type_error_with_suffix(
            "Object prototype may only be an Object or null: ",
            &rendered,
        );
    }

    // OrdinarySetPrototypeOf: a non-extensible target rejects a *changing*
    // prototype. `Object.setPrototypeOf` surfaces that rejection as a
    // TypeError; `Reflect.setPrototypeOf` returns `false` without throwing
    // (handled in js_reflect_set_prototype_of, which never reaches here for the
    // reject case). A no-op set to the SAME prototype still succeeds. Primitive
    // targets are extensible-irrelevant — `obj_value_no_extend` is false for
    // non-objects, so they fall through to the no-op return below. (test262
    // Reflect/preventExtensions/prevent-extensions:
    // `Object.setPrototypeOf(o, Array.prototype)` after preventExtensions.)
    if crate::object::obj_value_no_extend(obj_value) {
        let current = js_object_get_prototype_of(obj_value);
        if current.to_bits() != proto_bits {
            throw_object_type_error(b"#<Object> is not extensible");
        }
        return obj_value;
    }

    // #2820: setting the prototype of a primitive target is a spec no-op that
    // returns the (boxed) primitive value. `value_is_object_like` is false for
    // numbers/strings/booleans, and class refs are handled by the recording
    // path below — so a non-object, non-closure target just returns unchanged.
    let obj_ptr_for_record = {
        let top = obj_bits >> 48;
        if top == 0x7FFD {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top == 0 && obj_bits > 0x10000 {
            obj_bits as usize
        } else {
            0
        }
    };

    // #36 / #321: when the target is a closure (a plain function value) and the
    // proto is an object, record the (closure → proto) link in the closure
    // static-prototype side-table. effect's `Context.Tag(id)` returns a
    // function `TagClass` whose `_op`/`[TagTypeId]`/`[EffectTypeId]` live on a
    // `TagProto` object wired in via `Object.setPrototypeOf(TagClass,
    // TagProto)`. Recording the link lets later string/symbol property reads on
    // the closure (and on a subclass that `extends TagClass`) walk to the
    // proto's own properties, so the Tag is recognized as a valid Effect.
    if (obj_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
        && (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
    {
        let obj_ptr = crate::value::js_nanbox_get_pointer(obj_value) as usize;
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as usize;
        if obj_ptr != 0 && proto_ptr != 0 && crate::closure::is_closure_ptr(obj_ptr) {
            crate::closure::closure_set_static_prototype(obj_ptr, proto_bits);
            return obj_value;
        }
    }

    // #2820: ordinary heap object — record the observable [[Prototype]] in the
    // object-prototype side-table so `Object.getPrototypeOf(obj)` and inherited
    // property reads (`obj.x` where `x` lives on `proto`) reflect it. Records
    // `TAG_NULL` for `setPrototypeOf(obj, null)`.
    if obj_ptr_for_record != 0
        && !crate::closure::is_closure_ptr(obj_ptr_for_record)
        && is_valid_obj_ptr(obj_ptr_for_record as *const u8)
    {
        super::prototype_chain::object_set_static_prototype(obj_ptr_for_record, proto_bits);
        // A grown array's local may still hold the FORWARDED (old) pointer;
        // the spec [[HasProperty]]/[[Get]] helpers look the prototype up by
        // the CLEANED address. Record under both keys so either resolves
        // (test262 copyWithin/coerced-values-start-change-* second case).
        unsafe {
            let hdr = (obj_ptr_for_record as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*hdr).obj_type == crate::gc::GC_TYPE_ARRAY
                || (*hdr).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                let cleaned = crate::array::clean_arr_ptr(
                    obj_ptr_for_record as *const crate::array::ArrayHeader,
                ) as usize;
                if cleaned != 0 && cleaned != obj_ptr_for_record {
                    super::prototype_chain::object_set_static_prototype(cleaned, proto_bits);
                }
            }
        }
    }

    // Spec: `Object.setPrototypeOf(O, proto)` returns O.
    obj_value
}
