//! A writer is private, non-transferable and affine to its guest instance.
use super::*;
use crate::object::owned_state_bag::COMPONENT_GUEST_WRITER;

pub(super) fn data(wrapper: f64) -> f64 {
    let pointer = object_pointer(wrapper).unwrap_or_else(|| fail("Expected a guest writer"));
    let fields = unsafe { bag::load(pointer as usize, COMPONENT_GUEST_WRITER) };
    if array_pointer(fields).is_none() {
        fail("Expected a guest writer");
    }
    fields
}
pub(super) fn live(wrapper: f64) -> f64 {
    let fields = data(wrapper);
    let owner = get(fields, OWNER);
    if owner.to_bits() != super::super::maybe_registry().to_bits()
        || !unsafe { (*context::get(owner)).alive }
    {
        fail("Guest writer belongs to another or disposed instance");
    }
    if get(fields, STATE) != OPEN {
        fail("Guest writer is closed or consumed");
    }
    fields
}
pub(super) fn wrap(ty: u32, handle: u32) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(super::super::registry());
    let schema = schema(owner.get_nanbox_f64(), ty).unwrap_or_else(|message| fail(message));
    let stream = schema["kind"] == "stream";
    drop(schema);
    let fields = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(6) as i64,
    ));
    for (slot, value) in [
        owner.get_nanbox_f64(),
        ty as f64,
        handle as f64,
        OPEN,
        0.0,
        WRITER,
    ]
    .into_iter()
    .enumerate()
    {
        set(fields.get_nanbox_f64(), slot as u32, value);
    }
    let wrapper = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    unsafe {
        bag::store(
            object_pointer(wrapper.get_nanbox_f64()).unwrap() as usize,
            COMPONENT_GUEST_WRITER,
            fields.get_nanbox_f64(),
        );
    }
    set_field(wrapper.get_nanbox_f64(), b"then", undefined());
    for (name, info) in [
        (b"write".as_slice(), crate::fn_info!(write, 1)),
        (b"cancel".as_slice(), crate::fn_info!(cancel, 0)),
        (b"dispose".as_slice(), crate::fn_info!(dispose, 0)),
    ] {
        let method = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
        crate::closure::js_closure_set_capture_f64(
            method.get_raw_mut_ptr(),
            0,
            wrapper.get_nanbox_f64(),
        );
        crate::object::set_builtin_closure_non_constructable(
            method.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        );
        set_field(
            wrapper.get_nanbox_f64(),
            name,
            crate::value::js_nanbox_pointer(
                method.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
    }
    if stream {
        let close = scope.root_nanbox_f64(field(wrapper.get_nanbox_f64(), b"dispose"));
        set_field(wrapper.get_nanbox_f64(), b"close", close.get_nanbox_f64());
    }
    register(wrapper.get_nanbox_f64(), fields.get_nanbox_f64());
    wrapper.get_nanbox_f64()
}
extern "C" fn write(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    input: f64,
) -> f64 {
    io::write(
        crate::closure::js_closure_get_capture_f64(closure, 0),
        input,
    )
}
extern "C" fn cancel(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let fields = live(crate::closure::js_closure_get_capture_f64(closure, 0));
    let operation = get(fields, PENDING) as u32;
    if operation != 0 {
        producer::notify(
            get(fields, TYPE) as u32,
            get(fields, HANDLE) as u32,
            operation,
            0,
        );
    }
    undefined()
}
extern "C" fn dispose(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let wrapper = crate::closure::js_closure_get_capture_f64(closure, 0);
    let fields = data(wrapper);
    if get(fields, STATE) == CLOSED {
        return undefined();
    }
    live(wrapper);
    release(fields);
    crate::weakref::js_finreg_unregister(finalization(get(fields, OWNER)), wrapper);
    undefined()
}
pub(super) fn release(fields: f64) {
    if get(fields, STATE) != OPEN {
        return;
    }
    let owner = get(fields, OWNER);
    if owner.to_bits() != super::super::maybe_registry().to_bits()
        || !unsafe { (*context::get(owner)).alive }
    {
        return;
    }
    // C knows whether the future has completed. Releasing an unfinished one
    // requests Store retirement, never an illegal canonical writer drop.
    set(fields, STATE, CLOSED);
    producer::notify(get(fields, TYPE) as u32, get(fields, HANDLE) as u32, 0, 1);
}
