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

pub(crate) fn class_constructor_ref_value(class_id: u32) -> f64 {
    f64::from_bits(0x7FFE_0000_0000_0000u64 | (class_id as u64 & 0xFFFF_FFFF))
}

pub(crate) fn class_prototype_ref_value(class_id: u32) -> f64 {
    f64::from_bits(
        0x7FFE_0000_0000_0000u64 | CLASS_PROTOTYPE_REF_FLAG | (class_id as u64 & 0xFFFF_FFFF),
    )
}

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

pub(crate) fn class_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
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
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .and_then(|reg| reg.get(&class_id))
        .map(|vtable| vtable.methods.contains_key(method_name))
        .unwrap_or(false)
}

/// Does the class chain rooted at `class_id` DECLARE a prototype method,
/// getter or setter named `name` (one `delete` has not removed)? A filter over
/// class metadata ("may this chain resolve `name`?") for paths that must not
/// materialize a prototype; it never answers a property query itself.
pub(crate) fn class_instance_has_member(class_id: u32, name: &str) -> bool {
    class_chain_declares(class_id, name, true)
}

/// Wall 10 — `name in instance` for a class instance whose walk found nothing:
/// true when `name` is a prototype METHOD anywhere in the instance's class
/// chain. Methods registered in `CLASS_VTABLE_REGISTRY` may have no physical
/// key the ordinary own-key + prototype walk in `js_object_has_property` can
/// see, which made `'method' in instance` wrongly `false` (NestJS's app Proxy
/// gates routing on `'listen' in receiver`). Accessors are not consulted: they
/// are real properties of the class prototype, which that walk visits.
pub(crate) fn class_instance_has_method(class_id: u32, name: &str) -> bool {
    class_chain_declares(class_id, name, false)
}

fn class_chain_declares(class_id: u32, name: &str, accessors: bool) -> bool {
    if class_id == 0 {
        return false;
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(reg) = registry.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 32 {
        if let Some(vtable) = reg.get(&cid) {
            // Honor `delete C.prototype.m`: a deleted key must report `false`
            // from `'m' in new C()`, matching the descriptor/static lookup paths.
            if !super::class_registry::class_is_key_deleted(cid, name)
                && (vtable.methods.contains_key(name)
                    || (accessors && vtable.accessor_decl(name).is_some()))
            {
                return true;
            }
        }
        match super::class_registry::get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

pub fn class_prototype_method_value_for_name(class_id: u32, method_name: &str) -> f64 {
    if let Some(bits) = CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        if let Some(bits) = cache.get(&(class_id, method_name.to_string())).copied() {
            return Some(bits);
        }
        None
    }) {
        return f64::from_bits(bits);
    }

    // Bounded leak: `js_class_method_bind` keeps the byte pointer for the
    // lifetime of the bound closure (it's stashed inside the closure's
    // capture frame). We leak one allocation per unique
    // `(class_id, method_name)` pair the program ever asks for, so the
    // total leak is bounded by the static set of decorated method
    // descriptors. The cache below short-circuits repeat queries.
    let leaked = intern_class_method_name(class_id, method_name);
    let class_ref = class_prototype_ref_value(class_id);
    // Build the closure DIRECTLY (not via `js_class_method_bind`, whose
    // canonical short-circuit would call back into this function and recurse).
    // The captured receiver is the prototype-ref, which doubles as the
    // "canonical class method" marker that `dispatch_bound_method` keys on.
    let value = build_bound_method_closure(class_ref, leaked.as_ptr(), leaked.len());
    class_prototype_method_value_cache_root_store(
        class_id,
        method_name.to_string(),
        value.to_bits(),
    );
    value
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
    class_prototype_method_value_for_name(class_id, &method_name)
}
