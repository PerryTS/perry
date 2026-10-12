//! A traced capture owns the native funcref lifetime without closure keys.
use super::super::*;
use super::support::*;
extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
    0.0
}
#[test]
fn wasm_funcref_native_payload_follows_a_moving_wrapper() {
    check(false);
}
#[test]
#[ignore = "negative control: erase the function's owning native capture"]
fn negative_control_objtables_wasm_funcref() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 7);
    unsafe {
        crate::closure::wasm_funcref::test_install(owner as usize);
    }
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    assert_ne!(moved, owner);
    if erase {
        crate::closure::js_closure_set_capture_f64(
            moved,
            6,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    assert_eq!(
        unsafe { crate::closure::wasm_funcref::test_read(moved as usize) },
        Some(37)
    );
    let fresh = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 7);
    assert_eq!(
        unsafe { crate::closure::wasm_funcref::test_read(fresh as usize) },
        None
    );
}
