/// The instance `new` through fresh class value `class_value` (template
/// `class_cid`) allocates, linked to that evaluation's distinct prototype
/// object. Class-id dispatch alone follows the shared template and cannot
/// preserve per-evaluation inheritance.
///
/// The instance is born in its class's declared keys, as `new` of the shared
/// class is (`construct_registered_class_ref`), and moves to that shape's
/// facts at the prototype's identity. The template remembers the link
/// (`class_object_template::record_instance_link`), so every later instance
/// of the same evaluation is born in the linked shape directly.
fn construct_class_object_instance(
    class_value: f64,
    class_cid: u32,
    cell: Option<super::super::field_get_set::TemplateCell>,
) -> *mut ObjectHeader {
    use super::super::field_get_set::TemplateInstance;
    let template = cell.and_then(|cell| unsafe {
        super::super::field_get_set::template_instance(cell, class_value, class_cid)
    });
    let (inst, width) = match template {
        Some(TemplateInstance::Linked(inst)) => return inst,
        Some(TemplateInstance::Birth(inst, width)) => (inst, width),
        None => allocate_class_instance(class_cid),
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_nanbox_f64(class_value);
    let instance = scope.root_raw_mut_ptr(inst);
    let class_obj = || {
        crate::value::JSValue::from_bits(class.get_nanbox_f64().to_bits())
            .as_pointer::<ObjectHeader>()
    };
    let prototype =
        unsafe { super::super::field_get_set::class_object_prototype_value(class_obj()) };
    let prototype = scope.root_heap_word_u64(prototype.bits());
    let birth = instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        crate::object::shapes::object_shape_stamp(instance)
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        super::super::prototype_chain::object_link_class_evaluation_prototype(
            instance as usize,
            prototype.get_heap_word_u64(),
        )
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        super::super::field_get_set::record_instance_link(
            class_obj(),
            instance,
            birth,
            width,
            prototype.get_heap_word_u64(),
        );
        instance
    })
}

/// An instance of class `class_cid` allocated in its declared keys, and its
/// width; the learned width without keys for a class that registered none.
fn allocate_class_instance(class_cid: u32) -> (*mut ObjectHeader, u32) {
    if let Some((keys_array, field_count)) = registered_class_keys_array(class_cid) {
        // As wide as the class's instances have been learned to grow, so the
        // keys a constructor adds beyond the declared ones stay inline.
        let field_count =
            field_count.max(crate::object::learned_inline_field_count(class_cid));
        let inst = crate::object::alloc::alloc_class_instance_with_keys(
            class_cid,
            0,
            field_count,
            keys_array,
        );
        (inst, field_count)
    } else {
        let field_count = crate::object::learned_inline_field_count(class_cid);
        (js_object_alloc(class_cid, field_count), field_count)
    }
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
            crate::object::alloc::alloc_class_instance_with_keys(
                instance_cid,
                0,
                field_count,
                keys_array,
            )
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
    instance.with_mut_ptr::<ObjectHeader, _>(|i| crate::value::js_nanbox_pointer(i as i64))
}

/// #11229: `Reflect.construct(C, args, newTarget)` where `C` is a
/// per-evaluation class object (a capturing class) and `newTarget` is a
/// different constructor. Construct `C` the normal way -- that replays its
/// constructor with its own captures -- then honor
/// `GetPrototypeFromConstructor(newTarget)`, as the Date arm does. Falling
/// through to the generic tail instead ran the class as a plain function
/// against a bare object, so the result was not `instanceof newTarget`.
unsafe fn construct_class_object_with_new_target(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let nt = scope.root_nanbox_f64(new_target);
    let func = scope.root_nanbox_f64(func_value);
    let proto = new_target_custom_object_prototype(nt.get_nanbox_f64())
        .map(|bits| scope.root_heap_word_u64(bits));
    let result = js_new_function_construct(func.get_nanbox_f64(), args_ptr, args_len);
    if let Some(proto) = proto {
        let jv = crate::value::JSValue::from_bits(result.to_bits());
        if jv.is_pointer() {
            let addr = (jv.bits() & crate::value::POINTER_MASK) as usize;
            super::super::prototype_chain::object_set_static_prototype(
                addr,
                proto.get_heap_word_u64(),
            );
        }
    }
    result
}
