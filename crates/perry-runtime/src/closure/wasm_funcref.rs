//! A table-function wrapper owns its native funcref through capture 6.
//! The collecting native cell releases it exactly once without an owner map.
use crate::native_handle::NativeHandleHeader;
use crate::value::JSValue;
use std::ffi::c_void;
const NAME: &str = "Perry.Wasm.FunctionReference";
const TYPE_ID: u64 = 0x1179_8efb_4213_c8ab;
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_external,
    stream: None,
};
unsafe extern "C" fn drop_external(resource: *mut c_void, _hint: *mut c_void) {
    crate::webassembly::drop_host_extern_handle(resource as usize);
}
unsafe fn install(
    owner: usize,
    external: *mut c_void,
    vtable: &'static crate::native_payload::PayloadVTable,
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cell = crate::native_handle::native_handle_new_rust_payload(
        external, TYPE_ID, vtable, NAME, false,
    );
    crate::closure::js_closure_set_capture_f64(
        owner as *mut super::ClosureHeader,
        6,
        crate::value::js_nanbox_pointer(cell as i64),
    );
}
pub(crate) fn register_wasm_funcref_external(owner: usize, external: usize) {
    if owner == 0 {
        crate::webassembly::drop_host_extern_handle(external);
        return;
    }
    unsafe {
        install(owner, external as *mut c_void, &VTABLE);
    }
}
/// Ordinary instance exports still borrow their native pointer from the
/// instance. Table funcrefs instead hold a traced owning native cell.
pub(crate) fn external_from_capture(value: f64) -> *mut c_void {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return if value.is_number() {
            value.as_number() as usize as *mut c_void
        } else {
            std::ptr::null_mut()
        };
    }
    unsafe {
        crate::native_handle::native_handle_rust_payload_ptr(
            value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader,
            TYPE_ID,
        )
    }
}
#[cfg(test)]
static TEST_VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_test_payload,
    stream: None,
};
#[cfg(test)]
unsafe extern "C" fn drop_test_payload(resource: *mut c_void, _hint: *mut c_void) {
    drop(Box::from_raw(resource.cast::<u64>()));
}
#[cfg(test)]
pub(crate) unsafe fn test_install(owner: usize) {
    install(owner, Box::into_raw(Box::new(37u64)).cast(), &TEST_VTABLE);
}
#[cfg(test)]
pub(crate) unsafe fn test_read(owner: usize) -> Option<u64> {
    let value = JSValue::from_bits(
        crate::closure::js_closure_get_capture_f64(owner as *const super::ClosureHeader, 6)
            .to_bits(),
    );
    if !value.is_pointer() {
        return None;
    }
    let resource = crate::native_handle::native_handle_rust_payload_ptr(
        value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader,
        TYPE_ID,
    );
    resource.cast::<u64>().as_ref().copied()
}
