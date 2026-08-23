pub(crate) fn class_evaluation_method_value_for_name(
    owner_class_id: u32,
    method_name: &str,
    evaluation_brand: f64,
) -> f64 {
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
    let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
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
        let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
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
    let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
    let method = build_bound_method_closure_with_private_brand(
        class_constructor_ref_value(owner_class_id),
        leaked.as_ptr(),
        leaked.len(),
        Some(evaluation_brand),
    );
    class_prototype_method_value_cache_root_store(owner_class_id, cache_name, method.to_bits());
    method
}
