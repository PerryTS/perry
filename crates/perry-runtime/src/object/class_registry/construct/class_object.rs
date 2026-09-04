/// Link an instance constructed through a fresh class value to that
/// evaluation's distinct prototype object. Class-id dispatch alone follows
/// the shared template and cannot preserve per-evaluation inheritance.
fn link_class_object_instance_prototype(class_value: f64, instance: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_nanbox_f64(class_value);
    let instance = scope.root_raw_mut_ptr(instance);
    let class_obj = crate::value::JSValue::from_bits(class.get_nanbox_f64().to_bits())
        .as_pointer::<ObjectHeader>();
    let prototype =
        unsafe { super::super::field_get_set::class_object_prototype_value(class_obj) };
    let prototype = scope.root_heap_word_u64(prototype.bits());
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        super::super::prototype_chain::object_link_class_evaluation_prototype(
            instance as usize,
            prototype.get_heap_word_u64(),
        )
    });
}

/// Object's constructor has special newTarget semantics: when invoked as the
/// super-constructor of a derived class it ignores `value` and performs
/// OrdinaryCreateFromConstructor(newTarget). Calling the ordinary Object thunk
/// would instead coerce and return the first argument, binding an unrelated
/// object as the derived `this` and losing its class prototype and brand.
unsafe fn construct_object_with_new_target(new_target: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let new_target = scope.root_nanbox_f64(new_target);
    let instance_cid = new_target_class_id(new_target.get_nanbox_f64())
        .unwrap_or_else(|| synthetic_class_id_for_function(new_target.get_nanbox_f64()));
    let instance =
        if let Some((keys_array, field_count)) = registered_class_keys_array(instance_cid) {
            js_object_alloc_class_inline_keys(instance_cid, 0, field_count, keys_array)
        } else {
            js_object_alloc(
                instance_cid,
                crate::object::learned_inline_field_count(instance_cid),
            )
        };
    let instance = scope.root_raw_mut_ptr(instance);
    let prototype = new_target_custom_object_prototype(new_target.get_nanbox_f64())
        .or_else(global_object_prototype_bits)
        .map(|bits| scope.root_heap_word_u64(bits));
    let new_target_value = new_target.get_nanbox_f64();
    if is_class_object_value(new_target_value) {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::field_get_set::stamp_private_evaluation_brand(
                instance,
                new_target_value,
            )
        });
    }
    if let Some(prototype) = prototype {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::prototype_chain::object_set_static_prototype(
                instance as usize,
                prototype.get_heap_word_u64(),
            )
        });
    }
    crate::value::js_nanbox_pointer(instance.get_raw_mut_ptr::<ObjectHeader>() as i64)
}
