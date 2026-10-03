pub(crate) fn class_evaluation_method_value_for_name(
    owner_class_id: u32,
    method_name: &str,
    evaluation_brand: f64,
) -> f64 {
    // A template compiled with method entries gives each evaluation's
    // prototype its own function object per method (`<method>__eclo`): that
    // is the evaluation's method value while the prototype still holds it.
    if let Some(value) =
        evaluation_prototype_method_value(owner_class_id, method_name, evaluation_brand)
    {
        return value;
    }
    let cache_key = format!("#<perry:class-evaluation-method:{owner_class_id}:{method_name}>");
    let cached = crate::object::js_object_get_own_field_or_undef(
        evaluation_brand,
        cache_key.as_ptr(),
        cache_key.len(),
    );
    if cached.to_bits() != crate::value::TAG_UNDEFINED {
        return cached;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let brand = scope.root_nanbox_f64(evaluation_brand);
    let leaked = intern_class_method_name(owner_class_id, method_name);
    let method = build_bound_method_closure_with_private_brand(
        class_prototype_ref_value(owner_class_id),
        leaked.as_ptr(),
        leaked.len(),
        Some(brand.get_nanbox_f64()),
    );
    let method = scope.root_nanbox_f64(method);
    let key = crate::string::js_string_from_bytes(cache_key.as_ptr(), cache_key.len() as u32);
    let key = scope.root_string_ptr(key);
    let class_obj = JSValue::from_bits(brand.get_nanbox_f64().to_bits())
        .as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    let class_obj = scope.root_raw_mut_ptr(class_obj);
    class_obj.with_mut_ptr::<ObjectHeader, _>(|class_obj| {
        key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_set_field_by_name(class_obj, key, method.get_nanbox_f64());
        });
    });
    method.get_nanbox_f64()
}

pub(crate) fn class_private_static_method_value_for_name(
    owner_class_id: u32,
    method_name: &str,
    evaluation_brand: f64,
) -> f64 {
    let cache_name = format!("#<perry:static-private-method:{method_name}>");
    if class_registry::is_class_object_value(evaluation_brand) {
        let cached = crate::object::js_object_get_own_field_or_undef(
            evaluation_brand,
            cache_name.as_ptr(),
            cache_name.len(),
        );
        if cached.to_bits() != crate::value::TAG_UNDEFINED {
            return cached;
        }

        let scope = crate::gc::RuntimeHandleScope::new();
        let brand = scope.root_nanbox_f64(evaluation_brand);
        let leaked = intern_class_method_name(owner_class_id, method_name);
        let method = build_bound_method_closure_with_private_brand(
            class_constructor_ref_value(owner_class_id),
            leaked.as_ptr(),
            leaked.len(),
            Some(brand.get_nanbox_f64()),
        );
        let method = scope.root_nanbox_f64(method);
        let key = crate::string::js_string_from_bytes(cache_name.as_ptr(), cache_name.len() as u32);
        let key = scope.root_string_ptr(key);
        let class_obj = JSValue::from_bits(brand.get_nanbox_f64().to_bits())
            .as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        let class_obj = scope.root_raw_mut_ptr(class_obj);
        class_obj.with_mut_ptr::<ObjectHeader, _>(|class_obj| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(class_obj, key, method.get_nanbox_f64());
            })
        });
        return method.get_nanbox_f64();
    }

    if let Some(bits) = CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        cache
            .borrow()
            .get(&(owner_class_id, cache_name.clone()))
            .copied()
    }) {
        return f64::from_bits(bits);
    }
    let leaked = intern_class_method_name(owner_class_id, method_name);
    let method = build_bound_method_closure_with_private_brand(
        class_constructor_ref_value(owner_class_id),
        leaked.as_ptr(),
        leaked.len(),
        Some(evaluation_brand),
    );
    class_prototype_method_value_cache_root_store(owner_class_id, cache_name, method.to_bits());
    method
}
static CLASS_METHOD_NAME_INTERNER: OnceLock<RwLock<HashMap<(u32, String), &'static [u8]>>> =
    OnceLock::new();

/// Stable storage for the method-name pointer captured by bound-method
/// closures. The key set is bounded by the program's declared class methods,
/// even when one class expression is evaluated arbitrarily many times.
pub(super) fn intern_class_method_name(class_id: u32, method_name: &str) -> &'static [u8] {
    let interner =
        crate::once_init::get_or_init(&CLASS_METHOD_NAME_INTERNER, || RwLock::new(HashMap::new()));
    let key = (class_id, method_name.to_string());
    if let Ok(guard) = interner.read() {
        if let Some(bytes) = guard.get(&key).copied() {
            return bytes;
        }
    }
    let mut guard = interner
        .write()
        .expect("class method name interner poisoned");
    if let Some(bytes) = guard.get(&key).copied() {
        return bytes;
    }
    let bytes: &'static [u8] = method_name.as_bytes().to_vec().leak();
    guard.insert(key, bytes);
    bytes
}

/// Allocate a bound-method closure for the named method. Keeping this raw
/// builder separate avoids recursion through the canonical method cache.
pub(crate) fn build_bound_method_closure(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    build_bound_method_closure_with_private_brand(instance, method_name_ptr, method_name_len, None)
}

/// The function object evaluation `brand` (a class object of template
/// `owner_class_id`) holds for method `name` in its prototype, while that
/// prototype's own `name` is still the declaration's entry-backed function at
/// home in `brand`. `None` for a template without method entries.
fn evaluation_prototype_method_value(owner_class_id: u32, name: &str, brand: f64) -> Option<f64> {
    let code = class_registry::class_method_entry(owner_class_id, name)?;
    if !class_registry::is_class_object_value(brand) {
        return None;
    }
    let class = JSValue::from_bits(brand.to_bits()).as_pointer::<ObjectHeader>();
    if class.is_null() || crate::object::js_object_get_class_id(class) != owner_class_id {
        return None;
    }
    let proto = unsafe { crate::object::field_get_set::class_object_prototype_value(class) };
    if !proto.is_pointer() {
        return None;
    }
    let class = JSValue::from_bits(brand.to_bits()).as_pointer::<ObjectHeader>();
    let value =
        class_registry::class_object_own_field_bytes(proto.as_pointer::<ObjectHeader>(), name.as_bytes())?;
    unsafe {
        crate::object::field_get_set::static_method_value_runs(value.to_bits(), code, class)
    }
    .then_some(value)
}
