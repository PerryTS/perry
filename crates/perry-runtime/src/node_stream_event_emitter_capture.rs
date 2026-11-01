//! Capture defaults use EventEmitter.prototype's ordinary symbol property.
//! Site memos hold only layout facts; live values always come from those slots.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy)]
struct CaptureSite {
    bag: u32,
    proto: u32,
    value_slot: u32,
    value_live: u32,
}
thread_local! {
    static CAPTURE_DEFAULT: Cell<Option<CaptureSite>> = const { Cell::new(None) };
}

pub(super) fn native_capture_key(ctor: f64) -> Option<f64> {
    let ptr = JSValue::from_bits(ctor.to_bits()).as_pointer::<ClosureHeader>();
    if !crate::closure::is_closure_ptr(ptr as usize) {
        return None;
    }
    let acc = crate::object::get_accessor_descriptor(ptr as usize, "captureRejections")?;
    let getter = JSValue::from_bits(acc.get).as_pointer::<ClosureHeader>();
    if !crate::closure::is_closure_ptr(getter as usize)
        || crate::closure::get_valid_func_ptr(getter)
            != crate::object::native_module::constructor_shapes::capture_rejections_get as *const u8
    {
        return None;
    }
    Some(crate::closure::js_closure_get_capture_f64(getter, 1))
}

/// One shared leaf read on a shape match. Both accessor and default retain
/// their canonical property slots; descriptor edits or prototype replacement
/// invalidate the proof. A scalar write is observed directly, including a
/// user write through the reflected symbol.
#[cfg(test)]
#[inline(never)]
pub(crate) fn emitter_capture_default() -> bool {
    emitter_capture_default_with_birth(None)
}
#[inline(never)]
pub(super) fn emitter_capture_default_with_birth(birth: Option<(f64, f64)>) -> bool {
    let ctor = if let Some((ctor, _)) = birth {
        ctor
    } else {
        let Some(ctor) =
            crate::object::native_module::minted_native_callable_export("events\0EventEmitter")
        else {
            return false;
        };
        ctor
    };
    let closure = JSValue::from_bits(ctor.to_bits()).as_pointer::<ClosureHeader>();
    let bag = unsafe { (*closure).props };
    let stamp = unsafe { (*bag).shape_word() };
    let proto = birth
        .map(|(_, proto)| proto)
        .or_else(|| crate::node_stream::constructors::own_prototype_by_bag(ctor));
    if let (Some(site), Some(proto)) = (CAPTURE_DEFAULT.with(Cell::get), proto) {
        if site.bag == stamp {
            if let Some(obj) =
                crate::object::field_get_set::runtime_read_site::object_receiver(proto)
            {
                if unsafe { (*obj).parent_class_id } == site.proto {
                    let value = unsafe {
                        crate::object::object_field_at_with_live(
                            obj,
                            site.value_slot,
                            site.value_live,
                        )
                    };
                    return crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0;
                }
            }
        }
    }
    read_default_slow(ctor, proto)
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn read_default_slow(ctor: f64, proto: Option<f64>) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(ctor);
    let proto = scope.root_nanbox_f64(proto.unwrap_or_else(|| {
        crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64())
    }));
    let Some(key) = native_capture_key(ctor.get_nanbox_f64()) else {
        return crate::value::js_is_truthy(emitter_constructor_property("captureRejections")) != 0;
    };
    let key = scope.root_nanbox_f64(key);
    if let Some(obj) =
        crate::object::field_get_set::runtime_read_site::object_receiver(proto.get_nanbox_f64())
    {
        unsafe {
            if let Some(slot) = crate::object::shaped_symbols::position(
                obj,
                (key.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            ) {
                let shape = crate::object::shapes::object_shape_descriptor(obj);
                if let Some(shape) = shape {
                    if !crate::object::key_attrs::key_is_accessor_at(
                        shape.keys as usize as *const crate::array::ArrayHeader,
                        slot,
                    ) {
                        let slot = crate::object::key_attrs::key_storage_slot(
                            shape.keys as usize as *const crate::array::ArrayHeader,
                            slot,
                        );
                        let closure =
                            JSValue::from_bits(ctor.get_nanbox_u64()).as_pointer::<ClosureHeader>();
                        let bag = (*closure).props;
                        CAPTURE_DEFAULT.with(|memo| {
                            memo.set(Some(CaptureSite {
                                bag: (*bag).shape_word(),
                                proto: (*obj).shape_word(),
                                value_slot: slot,
                                value_live: shape.live_inline_slot_count,
                            }))
                        });
                        let value = crate::object::object_field_at_with_live(
                            obj,
                            slot,
                            shape.live_inline_slot_count,
                        );
                        return crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0;
                    }
                }
            }
        }
    }
    crate::value::js_is_truthy(
        crate::object::native_module::constructor_shapes::capture_property_get(
            proto.get_nanbox_f64(),
            key.get_nanbox_f64(),
        ),
    ) != 0
}
