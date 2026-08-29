//! `Object.defineProperty` fast path for the get-only descriptor literal
//! `{ get: <expr>, enumerable: true }`.
//!
//! esbuild emits `__export(target, { name: () => binding, … })` re-export
//! blocks at module TOP LEVEL — always executed at startup. pi's 13 MB bundle
//! carries 44 such sites totalling ~1,245 getter installs, and its CJS
//! interop blocks add `Object.defineProperty(exports, "X", { enumerable:
//! true, get: … })` on top. Every one of those previously allocated a
//! two-field descriptor object and re-decoded it by field name inside
//! `js_object_define_property`. Codegen recognises the literal descriptor
//! shape (`perry-codegen/src/expr/misc_methods.rs`) and calls this entrypoint
//! with the getter value directly, skipping the descriptor allocation.
//!
//! ## Contract
//!
//! Observable behaviour is byte-for-byte
//! `js_object_define_property(obj, key, { get, enumerable: true })`, split in
//! two arms:
//!
//! * the **fast arm** reproduces the generic ordinary-object accessor arm's
//!   exact effects for the one case it admits — plain extensible
//!   `GC_TYPE_OBJECT` receiver, plain non-numeric string key, brand-new own
//!   property, callable-or-`undefined` getter, unpolluted `Object.prototype`;
//! * **everything else** materialises the two-field descriptor and delegates
//!   to `js_object_define_property`, so exotic receivers (proxy / handle /
//!   class-ref / closure / typed array / buffer / frozen / sealed), symbol
//!   and numeric keys, redefinitions and the descriptor-validation
//!   TypeErrors are decided by exactly the code that decides them today.
//!
//! Two generic-path arms need no admission probe because they are no-ops for
//! this descriptor shape: the declared-class prototype-object arm
//! (`class_id_for_decl_prototype_object`) only routes descriptors that carry
//! a `value` field, and `arguments_object_after_define` only acts on
//! canonical array-index keys, which the key probe already rejects.
use super::descriptor_helpers::{object_prototype_has_desc_field, value_is_callable};
use super::*;

/// Can the fast arm reproduce the generic path bit-for-bit for this
/// receiver / key / getter triple? Pure probes — no allocation, no user code.
unsafe fn fast_path_admissible(obj_value: f64, key_value: f64, getter: f64) -> bool {
    // Receiver: a plain, extensible, ordinary heap object. `extract_obj_ptr`
    // rejects the handle bands (zlib/fetch/timer/proxy registry ids) and
    // accepts both the tagged and the raw-I64 pointer encodings, mirroring
    // the generic entry classification.
    let obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return false;
    }
    let addr = obj as usize;
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return false, // array / typed array / closure / Map / Date / …
    }
    // RegExp is an OBJECT-typed exotic cell; Date/Error route their defines
    // through the expando side tables.
    if super::super::exotic_expando::exotic_expando_kind(addr).is_some() {
        return false;
    }
    // ArrayBuffer/SharedArrayBuffer/DataView keep named-property descriptors
    // in the buffer expando tables.
    if crate::buffer::is_registered_buffer(addr) {
        return false;
    }
    // Frozen / sealed / preventExtensions receivers must throw
    // "Cannot define property …, object is not extensible" — generic arm.
    if (*gc_header_for(obj))._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
        return false;
    }
    // Key: a plain string (heap or SSO) that is neither numeric (canonical
    // index semantics: the Object.prototype hole flag, arguments-object
    // index mapping, the Array-subclass elements deopt) nor `length` (the
    // other elements-deopt key).
    let kv = crate::JSValue::from_bits(key_value.to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(kv, &mut sso) else {
        return false; // symbol / number / object keys → generic
    };
    if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if bytes == b"length" {
        return false;
    }
    // Getter: `{ get: undefined }` installs an undefined getter; any other
    // non-callable value must raise `ToPropertyDescriptor`'s exact
    // "Getter must be a function: …" TypeError — generic arm.
    let g = crate::JSValue::from_bits(getter.to_bits());
    if !g.is_undefined() && !value_is_callable(getter) {
        return false;
    }
    // `ToPropertyDescriptor` reads absent fields through the prototype chain,
    // so `Object.prototype.set = …`-style pollution must take the generic
    // decode (the same guard `try_decode_descriptor` applies).
    if object_prototype_has_desc_field() {
        return false;
    }
    true
}

/// The fast arm. Returns `Some(obj)` when it performed the define, `None`
/// when a rooted-phase re-probe says the generic arm must decide (existing
/// own key, non-UTF-8 key, coercion surprise).
unsafe fn try_fast_install(
    scope: &crate::gc::RuntimeHandleScope,
    obj_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    getter_handle: &crate::gc::RuntimeHandle<'_>,
) -> Option<f64> {
    // Materialise the key's heap form (identity for a heap string; an SSO key
    // allocates — the first possible collection point, hence the rooted
    // re-reads below). Admission proved the key is a string, so no user
    // `toString` runs here.
    let key_str = crate::builtins::js_string_coerce(key_handle.get_nanbox_f64());
    if key_str.is_null() {
        return None;
    }
    let key_str_handle = scope.root_string_ptr(key_str);
    let mut obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
    let mut obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return None;
    }
    // The key as a Rust-heap string for the descriptor side tables — immune
    // to evacuation, safe across every call below. The accompanying indexed
    // presence probe is non-allocating, so both uses share one scoped read.
    let (key_rust, indexed_present) =
        key_str_handle.with_const_ptr(|key_str: *const crate::StringHeader| {
            let name_ptr = crate::string::string_data(key_str);
            let name_len = (*key_str).byte_len as usize;
            let key_rust = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                .ok()?
                .to_string();
            Some((key_rust, own_key_present_via_index(obj, key_str)))
        })?;

    // Brand-new keys only: a redefinition carries the non-configurable
    // validation and omitted-field retention semantics the generic arm owns.
    // Same presence probe, same order, as the generic `existing_attrs` read.
    let (present, refreshed_key_str) = match indexed_present {
        Some(present) => (present, None),
        None => {
            let (present, key_str) = key_str_handle.across_const::<crate::StringHeader, _>(|| {
                super::super::obj_value_has_own_key(obj_value, key_handle.get_nanbox_f64())
            });
            (present, Some(key_str))
        }
    };
    // (`obj_value_has_own_key` string-coerces its key internally and can
    // allocate — refresh before the next deref.)
    obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
    obj = extract_obj_ptr(obj_value);
    if present || obj.is_null() {
        return None;
    }

    // ---- Committed: mirror the generic ordinary-object accessor arm. ----
    super::super::mark_object_dynamic_shape_unknown(obj);
    // Make the key discoverable (hasOwn / keys / getOwnPropertyNames) — the
    // accessor itself lives in the side table, not in a value slot. The entry
    // point roots both arguments before its first possible allocation.
    if let Some(key_str) = refreshed_key_str {
        ensure_key_in_keys_array(obj, key_str);
    } else {
        key_str_handle.with_const_ptr(|key_str: *const crate::StringHeader| {
            ensure_key_in_keys_array(obj, key_str)
        });
    }
    obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
    obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return Some(obj_value);
    }

    // Issue #450: the getter runs with `this === obj`.
    // `clone_closure_rebind_this` clones-and-rebinds CAPTURES_THIS closures
    // and passes every other value through untouched — identical to the
    // generic arm's treatment of the descriptor's `get` field.
    let getter = getter_handle.get_nanbox_f64();
    let get_bits = if crate::JSValue::from_bits(getter.to_bits()).is_undefined() {
        0
    } else {
        // `recv_box` is derived from the CURRENT receiver, one statement
        // before the clone that can move it (the callee roots its operands).
        let recv_box = crate::value::js_nanbox_pointer(obj as i64);
        crate::closure::clone_closure_rebind_this(getter.to_bits(), recv_box)
    };
    obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
    obj = extract_obj_ptr(obj_value);
    if obj.is_null() {
        return Some(obj_value);
    }
    // New-property attributes for `{ get, enumerable: true }`: `enumerable`
    // explicit, omitted `configurable` → false, and the internal writable bit
    // stays `true` for a brand-new accessor (the generic arm's `has_accessor`
    // default) so data lookups before the accessor override don't reject a
    // legitimate fallthrough write. The one-call installer folds the
    // duplicated `set_accessor_descriptor` + `set_property_attrs` work — the
    // brand-newness it requires is exactly what the presence probe above
    // proved, with nothing allocating in between.
    super::super::install_fresh_accessor_property(
        obj as usize,
        key_rust,
        AccessorDescriptor {
            get: get_bits,
            set: 0,
        },
        PropertyAttrs::new(true, true, false),
    );
    Some(f64::from_bits(obj_handle.get_heap_word_u64()))
}

/// `Object.defineProperty(obj, key, { get: getter, enumerable: true })`,
/// without the descriptor allocation on the admissible path. Returns the
/// object (NaN-boxed), like `js_object_define_property`.
#[no_mangle]
pub extern "C" fn js_object_define_get_accessor(
    obj_value: f64,
    key_value: f64,
    getter_value: f64,
) -> f64 {
    unsafe {
        // ONE handle scope shared by both arms (see define_property.rs #7963:
        // an inner scope dropped while an outer one is still taking handles
        // truncates the outer container). The callee-owned scopes (string
        // coerce, keys append, the generic define) nest and drop entirely
        // inside their calls.
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_heap_word_u64(obj_value.to_bits());
        let key_handle = scope.root_nanbox_f64(key_value);
        let getter_handle = scope.root_nanbox_f64(getter_value);

        if fast_path_admissible(obj_value, key_value, getter_value) {
            if let Some(ret) = try_fast_install(&scope, &obj_handle, &key_handle, &getter_handle) {
                return ret;
            }
        }

        // Generic arm: materialise `{ get, enumerable: true }` — exactly what
        // the descriptor literal would have allocated — and let
        // `js_object_define_property` decide everything: validation
        // TypeErrors, exotic receivers, redefinition, retention. Mirrors the
        // Annex B `define_accessor_annexb` builder one file over.
        let desc = js_object_alloc(0, 2);
        if desc.is_null() {
            return f64::from_bits(obj_handle.get_heap_word_u64());
        }
        let desc_handle = scope.root_raw_mut_ptr(desc);
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        desc_handle.with_mut_ptr(|desc: *mut ObjectHeader| {
            js_object_set_field_by_name(desc, get_key, getter_handle.get_nanbox_f64())
        });
        let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        let true_v = f64::from_bits(crate::JSValue::bool(true).bits());
        desc_handle.with_mut_ptr(|desc: *mut ObjectHeader| {
            js_object_set_field_by_name(desc, enum_key, true_v)
        });
        let desc_val = desc_handle.with_mut_ptr(|desc: *mut u8| {
            f64::from_bits(crate::JSValue::pointer(desc as *const u8).bits())
        });
        js_object_define_property(
            f64::from_bits(obj_handle.get_heap_word_u64()),
            key_handle.get_nanbox_f64(),
            desc_val,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        get_accessor_descriptor, get_property_attrs, js_object_alloc, js_object_set_field_by_name,
    };
    use super::*;

    unsafe fn heap_key(name: &str) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
            as *const crate::StringHeader
    }

    unsafe fn obj_val(obj: *mut ObjectHeader) -> f64 {
        f64::from_bits(crate::JSValue::pointer(obj as *const u8).bits())
    }

    unsafe fn str_val(name: &str) -> f64 {
        f64::from_bits(crate::JSValue::string_ptr(heap_key(name) as *mut _).bits())
    }

    const UNDEF: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

    /// Fast arm: a brand-new non-numeric string key on a plain object takes
    /// the inline install, and its side-table state is identical to what the
    /// generic `js_object_define_property` arm records for the same
    /// descriptor.
    #[test]
    fn fast_arm_matches_generic_state() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            // Fast arm.
            let fast = js_object_alloc(0, 0);
            let ret = js_object_define_get_accessor(obj_val(fast), str_val("alpha"), UNDEF);
            assert_eq!(ret.to_bits(), obj_val(fast).to_bits(), "returns the object");

            // Generic reference: same shape through the descriptor decode.
            let generic = js_object_alloc(0, 0);
            let desc = js_object_alloc(0, 2);
            js_object_set_field_by_name(desc, heap_key("get"), UNDEF);
            js_object_set_field_by_name(
                desc,
                heap_key("enumerable"),
                f64::from_bits(crate::JSValue::bool(true).bits()),
            );
            js_object_define_property(obj_val(generic), str_val("alpha"), obj_val(desc));

            for (label, obj) in [("fast", fast), ("generic", generic)] {
                let acc = get_accessor_descriptor(obj as usize, "alpha")
                    .unwrap_or_else(|| panic!("{label}: accessor entry installed"));
                assert_eq!(acc.get, 0, "{label}: undefined getter records 0");
                assert_eq!(acc.set, 0, "{label}: no setter");
                let attrs = get_property_attrs(obj as usize, "alpha")
                    .unwrap_or_else(|| panic!("{label}: attrs recorded"));
                assert!(attrs.writable(), "{label}: new-accessor writable default");
                assert!(attrs.enumerable(), "{label}: explicit enumerable: true");
                assert!(!attrs.configurable(), "{label}: omitted configurable");
                assert!(
                    own_key_present(obj, heap_key("alpha")),
                    "{label}: key in keys_array"
                );
            }
        }
    }

    /// #9103 follow-up: the one-call installer's side-table state is
    /// identical to the two-call `set_accessor_descriptor` +
    /// `set_property_attrs` sequence it replaces — descriptor entry, attrs,
    /// and the owner index (exactly one entry, so enumeration reports the
    /// key once).
    #[test]
    fn combined_installer_matches_two_call_sequence() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let combined = js_object_alloc(0, 0);
            super::super::super::install_fresh_accessor_property(
                combined as usize,
                "beta".to_string(),
                AccessorDescriptor { get: 0, set: 0 },
                PropertyAttrs::new(true, true, false),
            );
            let two_call = js_object_alloc(0, 0);
            set_accessor_descriptor(
                two_call as usize,
                "beta".to_string(),
                AccessorDescriptor { get: 0, set: 0 },
            );
            set_property_attrs(
                two_call as usize,
                "beta".to_string(),
                PropertyAttrs::new(true, true, false),
            );
            for (label, obj) in [("combined", combined), ("two_call", two_call)] {
                let acc = get_accessor_descriptor(obj as usize, "beta")
                    .unwrap_or_else(|| panic!("{label}: accessor entry"));
                assert_eq!((acc.get, acc.set), (0, 0), "{label}");
                let attrs = get_property_attrs(obj as usize, "beta")
                    .unwrap_or_else(|| panic!("{label}: attrs entry"));
                assert!(
                    attrs.writable() && attrs.enumerable() && !attrs.configurable(),
                    "{label}"
                );
                let keys = super::super::super::accessor_descriptor_keys_for_obj(obj as usize);
                assert_eq!(
                    keys.iter().filter(|k| k.as_str() == "beta").count(),
                    1,
                    "{label}: owner index holds the key exactly once"
                );
            }
        }
    }

    /// A violated brand-newness precondition must degrade to overwrite, never
    /// to a duplicated owner-index entry: the second call sees the meta bit
    /// already set and takes the scanning (dedup) add.
    #[test]
    fn combined_installer_repeat_does_not_duplicate_owner_index() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = js_object_alloc(0, 0);
            for _ in 0..2 {
                super::super::super::install_fresh_accessor_property(
                    obj as usize,
                    "gamma".to_string(),
                    AccessorDescriptor { get: 0, set: 0 },
                    PropertyAttrs::new(true, true, false),
                );
            }
            let keys = super::super::super::accessor_descriptor_keys_for_obj(obj as usize);
            assert_eq!(
                keys.iter().filter(|k| k.as_str() == "gamma").count(),
                1,
                "duplicate install must dedupe via the meta prior-bit path"
            );
        }
    }

    /// Numeric keys are inadmissible (canonical-index semantics) and must
    /// flow through the materialised-descriptor generic arm — which still
    /// installs the accessor with the same attributes.
    #[test]
    fn numeric_key_takes_generic_arm() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = js_object_alloc(0, 0);
            js_object_define_get_accessor(obj_val(obj), str_val("7"), UNDEF);
            let acc =
                get_accessor_descriptor(obj as usize, "7").expect("generic arm installs accessor");
            assert_eq!((acc.get, acc.set), (0, 0));
            let attrs = get_property_attrs(obj as usize, "7").expect("attrs recorded");
            assert!(attrs.writable() && attrs.enumerable() && !attrs.configurable());
        }
    }

    /// Redefining an EXISTING own data property is inadmissible: the generic
    /// arm converts data → accessor and RETAINS the omitted `configurable`
    /// from the existing property (`true` for a plain assigned field) — the
    /// new-property `false` default must NOT apply.
    #[test]
    fn existing_key_takes_generic_arm_and_retains_configurable() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = js_object_alloc(0, 1);
            js_object_set_field_by_name(obj, heap_key("x"), 42.0);
            js_object_define_get_accessor(obj_val(obj), str_val("x"), UNDEF);
            assert!(
                get_accessor_descriptor(obj as usize, "x").is_some(),
                "data property converted to accessor"
            );
            let attrs = get_property_attrs(obj as usize, "x").expect("attrs recorded");
            assert!(
                attrs.configurable(),
                "retained from the existing plain property, not the new-property false default"
            );
            assert!(attrs.enumerable(), "explicit enumerable: true");
        }
    }
}
