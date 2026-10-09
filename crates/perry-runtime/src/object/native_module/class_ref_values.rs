// Class constructor/prototype REF values and the prototype-method lookups
// keyed off them.
//
// A "class ref" is an INT32-tagged NaN box carrying a class id (plus a flag bit
// for the `C.prototype` half), which is how compiled code names a class without
// materializing an object. Extracted from `native_module.rs` to keep that file
// under the repository's 2000-line cap. Textually `include!`d (like
// `class_method_values.rs`), so every item keeps the module path and visibility
// it had before -- hence line comments, not `//!` inner docs, which are illegal
// away from the top of a module.
pub(crate) const CLASS_PROTOTYPE_REF_FLAG: u64 = 1u64 << 32;

/// The VALUE of class `class_id`'s constructor: its function object.
pub(crate) fn class_constructor_ref_value(class_id: u32) -> f64 {
    super::class_value::class_value(class_id)
}

/// A stable, non-moving KEY for class `class_id`'s constructor, for side
/// tables that key by value bits (the legacy immediate's bits; never a value
/// handed to user code).
pub(crate) fn class_constructor_key_bits(class_id: u32) -> u64 {
    0x7FFE_0000_0000_0000u64 | (class_id as u64 & 0xFFFF_FFFF)
}

pub(crate) fn class_prototype_ref_value(class_id: u32) -> f64 {
    f64::from_bits(
        0x7FFE_0000_0000_0000u64 | CLASS_PROTOTYPE_REF_FLAG | (class_id as u64 & 0xFFFF_FFFF),
    )
}

#[inline]
pub(crate) fn class_prototype_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE && (bits & CLASS_PROTOTYPE_REF_FLAG) != 0 {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
}

/// A class constructor OR its `C.prototype` reference -> the class id. The
/// constructor half is [`super::class_value::class_value_id`] (both forms);
/// callers that mean only the constructor ask that directly.
#[inline]
pub(crate) fn class_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    match bits >> 48 {
        // The legacy immediates: constructor or `C.prototype` reference.
        0x7FFE => {
            super::class_value::class_value_id_bits(bits).or_else(|| class_prototype_ref_id(value))
        }
        // A class function object (one pre-filter for any other pointer).
        0x7FFD => {
            super::class_value::class_closure_id((bits & crate::value::POINTER_MASK) as usize)
        }
        _ => None,
    }
}

pub(crate) unsafe fn metadata_key_to_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}

pub(crate) fn class_has_own_method(class_id: u32, method_name: &str) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    if crate::object::native_call_method::class_holder::name_is_not_a_prototype_method(
        method_name.as_bytes(),
    ) {
        if class_non_property_method_value(class_id, method_name).is_some() {
            return true;
        }
    }
    let existing = crate::object::class_holder_prototype(class_id);
    let start = if existing.is_null() {
        let proto =
            JSValue::from_bits(crate::object::class_decl_prototype_value(class_id).to_bits());
        if !proto.is_pointer() {
            return false;
        }
        proto.as_pointer::<ObjectHeader>() as *const ObjectHeader
    } else {
        existing as *const ObjectHeader
    };
    let key =
        crate::object::native_call_method::class_holder::MethodKey::bytes(method_name.as_bytes());
    matches!(unsafe { crate::object::native_call_method::class_holder::chain_method(start, &key) },
        crate::object::native_call_method::class_holder::ChainMethod::Data { holder, .. } if holder == start)
}

/// A method membership query is a data slot read from the actual chain.
pub(crate) fn class_instance_has_method(class_id: u32, name: &str) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    let existing = crate::object::class_holder_prototype(class_id);
    let start = if existing.is_null() {
        let proto =
            JSValue::from_bits(crate::object::class_decl_prototype_value(class_id).to_bits());
        if !proto.is_pointer() {
            return false;
        }
        proto.as_pointer::<ObjectHeader>() as *const ObjectHeader
    } else {
        existing as *const ObjectHeader
    };
    let key = crate::object::native_call_method::class_holder::MethodKey::bytes(name.as_bytes());
    !matches!(
        unsafe { crate::object::native_call_method::class_holder::chain_method(start, &key) },
        crate::object::native_call_method::class_holder::ChainMethod::Absent
    )
}

pub fn class_prototype_method_value_for_name(class_id: u32, method_name: &str) -> f64 {
    if let Some(bits) = CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        if let Some(bits) = cache
            .get(&(
                class_id,
                method_name.to_string(),
                ClassDeclarationValueKind::Method,
            ))
            .or_else(|| {
                cache.get(&(
                    class_id,
                    method_name.to_string(),
                    ClassDeclarationValueKind::NonPropertyMethod,
                ))
            })
            .copied()
        {
            return Some(bits);
        }
        None
    }) {
        return f64::from_bits(bits);
    }

    // The registry is read only to materialize this declaration's function
    // object. Calls and probes subsequently read the holder's slot.
    let declaration = {
        let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
        registry
            .as_ref()
            .and_then(|r| r.get(&class_id))
            .and_then(|c| c.methods.get(method_name))
            .map(|m| {
                (
                    m.func_ptr,
                    m.param_count,
                    m.has_synthetic_arguments,
                    m.has_rest,
                    m.entry,
                )
            })
    };
    let Some((body, params, synthetic, rest, entry)) = declaration else {
        if crate::object::native_call_method::class_holder::name_is_not_a_prototype_method(
            method_name.as_bytes(),
        ) {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        return crate::object::class_method_slot_value(class_id, method_name)
            .map(f64::from_bits)
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    };
    let value = class_method_declaration_value(
        class_prototype_ref_value(class_id),
        method_name,
        body,
        params,
        synthetic,
        rest,
        entry,
    );
    // The immutable declaration input distinguishes a literal alias-looking
    // string key from a private or symbol member at materialization. Readers
    // use this typed function-value entry, never reclassify a deleted key.
    let kind =
        if crate::object::class_registry::proto_member_has_no_string_key(class_id, method_name) {
            ClassDeclarationValueKind::NonPropertyMethod
        } else {
            ClassDeclarationValueKind::Method
        };
    crate::object::class_registry::class_declaration_value_root_store(
        class_id,
        method_name.to_string(),
        kind,
        value.to_bits(),
    );
    value
}

/// A materialized lexical/symbol member, separate from public properties.
pub(crate) fn class_non_property_method_value(class_id: u32, name: &str) -> Option<u64> {
    let _ = class_prototype_method_value_for_name(class_id, name);
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        cache
            .borrow()
            .get(&(
                class_id,
                name.to_string(),
                ClassDeclarationValueKind::NonPropertyMethod,
            ))
            .copied()
    })
}

/// The function object of declared class `class_id`'s method `name` whose
/// closure-convention entry is `code` (its JsFunctionInfo): capture 0 is
/// `C.prototype`'s ref. Built once per method (the caller caches it), which
/// is also when the entry's code is given the method's name: module init
/// registers none, so a class costs nothing per method until a method value
/// exists.
/// Materialize a method from immutable image input. Capture 0 is its home;
/// the remaining non-pointer captures retain the legacy body ABI for private
/// calls, native declarations and source reflection, without registry reads.
pub(crate) fn class_method_declaration_value(
    home: f64,
    name: &str,
    body: usize,
    params: u32,
    synthetic: bool,
    rest: bool,
    entry: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_nanbox_f64(home);
    let info = if entry != 0 {
        entry as *const crate::closure::JsFunctionInfo
    } else {
        &NATIVE_CLASS_METHOD_INFO
    };
    let f = crate::closure::js_closure_alloc(info, 4);
    if f.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let entry_code = unsafe { (*f).code() } as usize;
    if entry != 0
        && !name.is_empty()
        && crate::builtins::function_name_for_ptr(entry_code).is_none()
    {
        unsafe {
            crate::builtins::js_register_function_name(
                entry_code as *const u8,
                name.as_ptr(),
                name.len() as u32,
            );
        }
    }
    let facts = u64::from(params) | (u64::from(synthetic) << 32) | (u64::from(rest) << 33);
    unsafe {
        crate::closure::closure_install_boxed_captures(
            f,
            &[
                home.get_nanbox_f64().to_bits(),
                crate::value::TAG_HOLE | ((body as u64 & 0xffff_ffff) << 8),
                crate::value::TAG_HOLE | (((body as u64 >> 32) & 0xffff_ffff) << 8),
                (facts as f64).to_bits(),
            ],
        );
    }
    let f = scope.root_raw_mut_ptr(f);
    if entry == 0 {
        let length = params.saturating_sub(u32::from(synthetic) + u32::from(rest));
        f.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
            crate::object::set_builtin_closure_length(ptr as usize, length);
        });
        f.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
            crate::object::set_builtin_closure_non_constructable(ptr as usize);
        });
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = scope.root_string_ptr(key);
        f.with_mut_ptr::<crate::closure::ClosureHeader, _>(|ptr| {
            let value = key.with_const_ptr::<crate::StringHeader, _>(|key| {
                crate::value::js_nanbox_string(key as i64)
            });
            crate::closure::closure_define_data_with_attrs(
                ptr as usize,
                "name",
                value,
                crate::object::PropertyAttrs::new(false, false, true),
            );
        });
    }
    crate::value::js_nanbox_pointer(f.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64)
}

pub(crate) fn class_method_entry_declaration_value(class_id: u32, entry: usize, home: f64) -> f64 {
    let declaration = {
        let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
        registry
            .as_ref()
            .and_then(|r| r.get(&class_id))
            .and_then(|c| c.methods.iter().find(|(_, m)| m.entry == entry))
            .map(|(name, m)| {
                (
                    name.clone(),
                    m.func_ptr,
                    m.param_count,
                    m.has_synthetic_arguments,
                    m.has_rest,
                )
            })
    };
    let Some((name, body, params, synthetic, rest)) = declaration else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_method_declaration_value(home, &name, body, params, synthetic, rest, entry)
}

static NATIVE_CLASS_METHOD_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of_native_args(native_class_method, 0);

unsafe extern "C" fn native_class_method(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    args: *const f64,
    len: usize,
) -> f64 {
    let (body, params, synthetic, rest) =
        class_method_value_target(crate::value::POINTER_TAG | closure as u64)
            .expect("materialized native method carries its body");
    super::class_registry::call_vtable_method_value(
        body,
        this.as_f64(),
        args,
        len,
        params,
        synthetic,
        rest,
        None,
    )
}

/// A declaration function object's retained raw-body ABI. Arbitrary function
/// values have no legacy target and are called through their JsFunctionInfo.
pub(crate) unsafe fn class_method_value_target(value: u64) -> Option<(usize, u32, bool, bool)> {
    let v = JSValue::from_bits(value);
    if !v.is_pointer() || !crate::closure::is_closure_ptr(v.as_pointer::<u8>() as usize) {
        return None;
    }
    let closure = v.as_pointer::<crate::closure::ClosureHeader>();
    if crate::closure::real_capture_count((*closure).capture_count) != 4 {
        return None;
    }
    let home = f64::from_bits(crate::closure::js_closure_get_capture_bits(closure, 0));
    if class_prototype_ref_id(home).is_none() && !super::class_registry::is_class_object_value(home)
    {
        return None;
    }
    let low = crate::closure::js_closure_get_capture_bits(closure, 1);
    let high = crate::closure::js_closure_get_capture_bits(closure, 2);
    // Opaque ABI words in the existing non-pointer hole band. They never
    // escape the function object. A JavaScript value cannot contain a hole
    // with a payload, and two canonical holes would name the rejected null
    // body, so an ordinary capture list cannot forge this convention.
    let payload = 0xffff_ffffu64 << 8;
    if low & !payload != crate::value::TAG_HOLE || high & !payload != crate::value::TAG_HOLE {
        return None;
    }
    let body = ((low >> 8) & 0xffff_ffff) | (((high >> 8) & 0xffff_ffff) << 32);
    if body == 0 {
        return None;
    }
    let facts = f64::from_bits(crate::closure::js_closure_get_capture_bits(closure, 3)) as u64;
    Some((
        body as usize,
        facts as u32,
        facts & (1 << 32) != 0,
        facts & (1 << 33) != 0,
    ))
}

/// The method body a class method's function object runs, for its retained
/// source text: `closure` runs the closure-convention entry of a method of the
/// class its home capture names (`C.prototype`'s ref for a declared class, the
/// evaluation's class object for a per-evaluation template).
pub(crate) unsafe fn class_method_entry_source_func_ptr(
    closure: *const crate::closure::ClosureHeader,
) -> Option<usize> {
    class_method_value_target(crate::value::POINTER_TAG | closure as u64).map(|m| m.0)
}

#[no_mangle]
pub extern "C" fn js_class_prototype_method_value(class_ref: f64, method_key: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let method_name = unsafe { metadata_key_to_string(method_key) };
    let Some(method_name) = method_name else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    crate::object::class_method_slot_value(class_id, &method_name)
        .map(f64::from_bits)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}
