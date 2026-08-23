/// True when `class_id` or an ancestor extends the intrinsic Promise.
pub(crate) fn promise_parent_in_chain(class_id: u32) -> bool {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 && cid != 0 {
        let parent = js_get_dynamic_parent_value(cid);
        if matches!(identify_global_builtin_constructor(parent), Some("Promise")) {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(parent_id) if parent_id != 0 && parent_id != cid => {
                cid = parent_id;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

/// Install the hidden Promise backing on a dynamically-constructed subclass
/// unless an explicit `super(executor)` already did so.
unsafe fn ensure_promise_subclass_backing(
    instance: &crate::gc::RuntimeHandle<'_>,
    class_id: u32,
    args_ptr: *const f64,
    args_len: usize,
) {
    if !promise_parent_in_chain(class_id) {
        return;
    }
    let has_backing = instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        crate::promise::subclass_backing_promise(crate::value::js_nanbox_pointer(
            instance as i64,
        ))
        .is_some()
    });
    if has_backing {
        return;
    }
    let executor = if args_len >= 1 && !args_ptr.is_null() {
        *args_ptr
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        crate::promise::js_promise_subclass_init(
            crate::value::js_nanbox_pointer(instance as i64),
            executor,
        )
    });
}
