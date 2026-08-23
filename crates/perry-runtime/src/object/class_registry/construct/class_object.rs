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
        super::super::prototype_chain::object_link_class_default_prototype(
            instance as usize,
            prototype.get_heap_word_u64(),
        )
    });
}
