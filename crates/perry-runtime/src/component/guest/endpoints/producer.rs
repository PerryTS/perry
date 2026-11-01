//! Canonical creation stays in C; private JS capabilities never reveal handles.
use super::*;
use perry_abi::component_guest::{ComponentGuestProducerOps, GUEST_PRODUCER_ABI_VERSION};
use std::sync::atomic::{AtomicPtr, Ordering};

static OPS: AtomicPtr<ComponentGuestProducerOps> = AtomicPtr::new(std::ptr::null_mut());

/// # Safety
/// The immutable descriptor and its code pointers have image lifetime. The
/// notify callback only edits native metadata and never enters JS or WASI.
#[no_mangle]
pub unsafe extern "C" fn js_component_set_guest_producer_ops(
    ops: *const ComponentGuestProducerOps,
) -> i32 {
    if ops.is_null()
        || (*ops).abi_version != GUEST_PRODUCER_ABI_VERSION
        || (*ops).size != std::mem::size_of::<ComponentGuestProducerOps>()
    {
        return 0;
    }
    match OPS.compare_exchange(
        std::ptr::null_mut(),
        ops.cast_mut(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 1,
        Err(previous) => i32::from(previous == ops.cast_mut()),
    }
}
pub(super) fn ops() -> &'static ComponentGuestProducerOps {
    unsafe { OPS.load(Ordering::Acquire).as_ref() }
        .unwrap_or_else(|| fail("Guest adapter does not support producers"))
}
pub(super) fn notify(ty: u32, handle: u32, operation: u32, action: u32) {
    if let Some(ops) = unsafe { OPS.load(Ordering::Acquire).as_ref() } {
        unsafe { (ops.notify)(ty, handle, operation, action) };
    }
}

/// Create a pair from an immutable generated manifest key. No canonical handle
/// is accepted from JS; C selects the exact maintained typed factory.
#[no_mangle]
pub extern "C" fn js_component_guest_create_async_value(key: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(key);
    let owner = scope.root_nanbox_f64(super::super::registry());
    if !unsafe { (*context::get(owner.get_nanbox_f64())).alive } {
        fail("Guest instance has been disposed");
    }
    let mut name = String::new();
    let mut schema = Value::Null;
    let mut frame = Box::new(Frame::allocate_guest_frame());
    let outcome = crate::exception::catch_js_throw(|| {
        name = string(key.get_nanbox_f64());
        let index = name
            .get(1..)
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|id| *id <= u32::MAX / 2)
            .unwrap_or_else(|| fail("Invalid guest async factory type"));
        let exported = match name.as_bytes().first() {
            Some(b't') => 0,
            Some(b'e') => 1,
            _ => fail("Invalid guest async factory type"),
        };
        let ty = 2 * index + exported;
        schema = super::schema(owner.get_nanbox_f64(), ty).unwrap_or_else(|message| fail(message));
        if !unsafe { (*context::get(owner.get_nanbox_f64())).alive } {
            fail("Guest instance has been disposed");
        }
        let result = unsafe { (ops().create)((&raw mut *frame).cast(), ty) };
        if let Some(error) = frame.error() {
            crate::exception::js_throw(error);
        }
        if result == 0 {
            fail("Guest factory returned no pair");
        }
        frame.get(result)
    });
    drop((name, schema));
    // Dropping the native frame cannot collect. The returned value is rooted
    // before returning to a caller that may collect while publishing it.
    let result = outcome.map(|value| scope.root_nanbox_f64(value));
    drop(frame);
    match result {
        Ok(value) => value.get_nanbox_f64(),
        Err(error) => finish(Err(error)),
    }
}

/// # Safety
/// Called by the selected generated factory with both fresh canonical ends.
/// The caller retires the Store if publication fails after partial adoption.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_endpoint_pair(
    pointer: *mut c_void,
    ty: u32,
    reader: u32,
    writer_handle: u32,
) -> u32 {
    frame(pointer).guard(|frame| {
        let scope = RuntimeHandleScope::new();
        let readable = scope.root_nanbox_f64(wrap(ty, reader));
        let writable = scope.root_nanbox_f64(writer::wrap(ty, writer_handle));
        let pair = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set_field(pair.get_nanbox_f64(), b"value", readable.get_nanbox_f64());
        set_field(pair.get_nanbox_f64(), b"writer", writable.get_nanbox_f64());
        let schema = schema(super::super::registry(), ty).unwrap_or_else(|message| fail(message));
        let kind = if schema["kind"] == "stream" {
            b"stream".as_slice()
        } else {
            b"future".as_slice()
        };
        let kind = scope.root_nanbox_f64(crate::component::producer::managed_text(kind));
        set_field(pair.get_nanbox_f64(), b"kind", kind.get_nanbox_f64());
        drop(schema);
        frame.add(pair.get_nanbox_f64())
    })
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PRODUCER_OPS: unsafe extern "C" fn(*const ComponentGuestProducerOps) -> i32 =
    js_component_set_guest_producer_ops;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_CREATE: extern "C" fn(f64) -> f64 = js_component_guest_create_async_value;

#[cfg(test)]
mod tests;
