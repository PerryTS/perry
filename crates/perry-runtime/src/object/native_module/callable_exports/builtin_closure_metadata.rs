//! Builtin metadata belongs to the closure: length in its props bag and
//! non-constructability in its kind-specific header flag. Neither needs an
//! owner registry, scanner, young log, or death prune.

pub(crate) fn set_bound_native_closure_name(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
) {
    define_bound_native_closure_metadata(closure, name, None);
}

/// A native function's name and spec length share one attributed birth.
pub(crate) fn set_bound_native_closure_metadata(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
    length: u32,
) {
    define_bound_native_closure_metadata(closure, name, Some(length));
}

fn define_bound_native_closure_metadata(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
    length: Option<u32>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure);
    let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_handle = scope.root_string_ptr(ptr);
    let name_value =
        f64::from_bits(crate::value::JSValue::string_ptr(name_handle.get_raw_mut_ptr()).bits());
    let closure = closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize;
    let attrs = crate::object::PropertyAttrs::new(false, false, true);
    let entries = [("name", name_value), ("length", length.unwrap_or(0) as f64)];
    let count = if length.is_some() { 2 } else { 1 };
    let entries_attrs = [crate::object::key_attrs::attr_bits_to_entry(attrs.bits); 2];
    if crate::closure::closure_define_first_props_with_attrs(
        closure,
        &entries[..count],
        &entries_attrs[..count],
    ) {
        return;
    }
    crate::closure::closure_define_data_with_attrs(closure, "name", name_value, attrs);
    if let Some(length) = length {
        set_builtin_closure_length(closure, length);
    }
}

/// The spec length is an own data property, including its attributes.
pub(crate) fn set_builtin_closure_length(closure: usize, length: u32) {
    let attrs = crate::object::PropertyAttrs::new(false, false, true);
    if crate::closure::closure_define_first_props_with_attrs(
        closure,
        &[("length", length as f64)],
        &[crate::object::key_attrs::attr_bits_to_entry(attrs.bits)],
    ) {
        return;
    }
    crate::closure::closure_define_data_with_attrs(closure, "length", length as f64, attrs);
}

/// A recorded length is read from the function bag; an unmaterialized bound
/// length remains in its capture, as before.
pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    if let Some(length) = crate::closure::closure_get_own_dynamic_prop(closure, "length") {
        if length.is_finite() && length >= 0.0 && length <= u32::MAX as f64 {
            return Some(length as u32);
        }
        return None;
    }
    unsafe { crate::closure::bound_function_length(closure) }
}

/// The birth word of a bound native export: `perry_abi::realm_header::
/// closure_reserved` of its declared constructability (the realm declaration
/// says `constructable = false`). Its receiver capture is a pointer and the
/// sentinel body states no constructability, so only the per-instance bit can
/// be set here.
pub(crate) fn publish_bound_export_birth(closure: usize, constructable: bool) {
    let word = perry_abi::realm_header::closure_reserved(perry_abi::realm_header::ClosureBirth {
        pointer_free_captures: false,
        constructable,
        body_non_constructor: false,
        integrity: perry_abi::realm_header::Integrity::Extensible,
    });
    unsafe {
        let header = crate::gc::header_from_trusted_user_ptr(closure as *const u8).cast_mut();
        debug_assert_eq!((*header).obj_type, crate::gc::GC_TYPE_CLOSURE);
        // GC_STORE_AUDIT(INIT): a fresh closure's birth bits, before any
        // reader can see it.
        (*header)._reserved |= word;
    }
}

pub(crate) fn set_builtin_closure_non_constructable(closure: usize) {
    unsafe {
        let header = crate::gc::header_from_trusted_user_ptr(closure as *const u8).cast_mut();
        debug_assert_eq!((*header).obj_type, crate::gc::GC_TYPE_CLOSURE);
        (*header)._reserved |= crate::gc::GC_CLOSURE_NON_CONSTRUCTABLE;
    }
}

pub(crate) fn builtin_closure_is_non_constructable(closure: usize) -> bool {
    // Constructor dispatch also asks about proxy handles and arbitrary values.
    // Classify before reading the closure-specific bit; a registry id has no header.
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(closure) }) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE {
        return false;
    }
    header._reserved & crate::gc::GC_CLOSURE_NON_CONSTRUCTABLE != 0
        || crate::closure::closure_body_is_non_constructor(
            closure as *const crate::closure::ClosureHeader,
        )
}
