//! Compiled class entry and argument extraction from the current evaluation.
use super::*;

/// Fixed-arity captures occupy their compiled parameter slots. The memo names
/// a data slot, never an environment: refresh the evaluation after allocation,
/// then root the inline argument cells while array reads can realize functions.
pub(super) unsafe fn call_class_object_captured_entry(
    classobj_value: f64,
    class_cid: u32,
    inst: *mut ObjectHeader,
    args_ptr: *const f64,
    args_len: usize,
    entry: (usize, u32, u32),
    capture_slot: (u32, u32),
) -> Option<f64> {
    use std::cell::UnsafeCell;
    const INLINE_ARGS: usize = 24;
    let (ctor_ptr, total_params, sig_caps) = entry;
    if total_params as usize > INLINE_ARGS || sig_caps == 0 || sig_caps > total_params {
        return None;
    }
    let object =
        crate::value::JSValue::from_bits(classobj_value.to_bits()).as_pointer::<ObjectHeader>();
    if crate::object::shapes::object_shape_stamp(object) != capture_slot.0
        || (*object).class_id != class_cid
    {
        return None;
    }
    let caps = crate::object::js_object_get_field(object, capture_slot.1);
    if !caps.is_pointer() {
        return None;
    }
    let array = caps.as_pointer::<crate::array::ArrayHeader>();
    if crate::value::addr_class::try_read_gc_header(array as usize)?.obj_type
        != crate::gc::GC_TYPE_ARRAY
        || crate::array::js_array_length(array) < sig_caps
    {
        return None;
    }
    let user_params = (total_params - sig_caps) as usize;
    // Existing canonical-array admission proves callback-free dense reads.
    // With no safepoint between extraction and entry, use the same flat
    // argument ABI as a declared constructor. The caller owns class/receiver
    // roots; the compiled body roots its live formal parameters before GC.
    if let Some(dense) = crate::array::plain_call_spread_source(f64::from_bits(caps.bits())) {
        if (*dense).capacity >= sig_caps {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let mut slots = [undef; INLINE_ARGS];
            for (i, slot) in slots[..user_params].iter_mut().enumerate() {
                *slot = if !args_ptr.is_null() && i < args_len {
                    *args_ptr.add(i)
                } else {
                    undef
                };
            }
            let words = crate::array::array_elements_ptr(dense) as *const f64;
            let captures = &mut slots[user_params..total_params as usize];
            for (i, slot) in captures.iter_mut().enumerate() {
                *slot = *words.add(i);
            }
            // A hole needs the generic prototype-aware read, even in an
            // otherwise canonical array. No values were published yet.
            if captures
                .iter()
                .all(|slot| slot.to_bits() != crate::value::TAG_HOLE)
            {
                return Some(call_class_object_constructor_entry(
                    classobj_value,
                    inst,
                    slots.as_ptr(),
                    total_params as usize,
                    ctor_ptr,
                    total_params,
                ));
            }
        }
    }
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let slots: [UnsafeCell<f64>; INLINE_ARGS] = std::array::from_fn(|_| UnsafeCell::new(undef));
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_nanbox_f64(classobj_value);
    let instance = scope.root_raw_mut_ptr(inst);
    let captures = scope.root_raw_const_ptr(array);
    for (i, slot) in slots[..user_params].iter().enumerate() {
        *slot.get() = if !args_ptr.is_null() && i < args_len {
            *args_ptr.add(i)
        } else {
            undef
        };
    }
    for slot in &slots[..total_params as usize] {
        scope.root_nanbox_cell(slot);
    }
    for i in 0..sig_caps as usize {
        *slots[user_params + i].get() =
            captures.with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                crate::array::js_array_get_f64(array, i as u32)
            });
    }
    Some(instance.with_mut_ptr::<ObjectHeader, _>(|inst| {
        call_class_object_constructor_entry(
            class.get_nanbox_f64(),
            inst,
            slots.as_ptr().cast(),
            total_params as usize,
            ctor_ptr,
            total_params,
        )
    }))
}

pub(super) unsafe fn call_class_object_constructor_entry(
    classobj_value: f64,
    inst: *mut ObjectHeader,
    args_ptr: *const f64,
    args_len: usize,
    ctor_ptr: usize,
    total_params: u32,
) -> f64 {
    let _active_evaluation =
        crate::object::class_registry::push_active_class_evaluation(classobj_value);
    crate::object::class_registry::call_vtable_method_with_private_brand(
        ctor_ptr,
        inst as i64,
        args_ptr,
        args_len,
        total_params,
        false,
        false,
        classobj_value,
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn count_and_capture(_: f64, user: f64, capture: f64) -> f64 {
        crate::gc::RuntimeHandleScope::active_len_for_tests() as f64 + user + capture
    }

    #[test]
    fn dense_captures_use_the_flat_entry_without_a_replay_buffer() {
        unsafe {
            let cid = 0x6f28;
            let scope = crate::gc::RuntimeHandleScope::new();
            let class = scope.root_raw_mut_ptr(crate::object::js_object_alloc(cid, 0));
            let instance = scope.root_raw_mut_ptr(crate::object::js_object_alloc(cid, 0));
            let caps = scope.root_raw_mut_ptr(crate::array::js_array_alloc(1));
            caps.with_mut_ptr::<crate::array::ArrayHeader, _>(|caps| {
                crate::array::js_array_push_f64(caps, 41.0);
            });
            let bytes = b"__perry_ctor_caps";
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                bytes.as_ptr(),
                bytes.len() as u32,
            ));
            class.with_mut_ptr::<ObjectHeader, _>(|class| {
                crate::object::class_registry::js_object_mark_class(class as i64);
                key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_set_field_by_name(
                        class,
                        key,
                        crate::value::js_nanbox_pointer(
                            caps.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                        ),
                    );
                });
            });
            let class_value = class.with_mut_ptr::<ObjectHeader, _>(|class| {
                crate::value::js_nanbox_pointer(class as i64)
            });
            let shape = class.with_mut_ptr::<ObjectHeader, _>(|class| {
                crate::object::shapes::object_shape_stamp(class)
            });
            let code = count_and_capture as *const () as usize;
            let expected = instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
                call_class_object_constructor_entry(
                    class_value,
                    instance,
                    [7.0, 41.0].as_ptr(),
                    2,
                    code,
                    2,
                )
            });
            let actual = instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
                call_class_object_captured_entry(
                    class_value,
                    cid,
                    instance,
                    [7.0].as_ptr(),
                    1,
                    (code, 2, 1),
                    (shape, 0),
                )
            });
            assert_eq!(
                actual,
                Some(expected),
                "dense extraction must not open the generic capture frame"
            );
        }
    }
}
