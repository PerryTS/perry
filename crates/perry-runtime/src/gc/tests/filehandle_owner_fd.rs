//! A FileHandle's private initial fd is POD on its metadata record.
use super::super::*;
use super::support::*;
#[test]
fn filehandle_fd_moves_with_the_owner_metadata() {
    check_owned_fd(false);
}
#[test]
#[ignore = "negative control: erase the moved FileHandle's private fd"]
fn negative_control_objtables_filehandle_fd() {
    check_owned_fd(true);
}
fn check_owned_fd(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::object::js_object_alloc(crate::fs::CLASS_ID_FS_FILEHANDLE, 0);
    unsafe {
        crate::fs::set_filehandle_owner_fd(owner, 4242);
    }
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    assert_ne!(owner, moved);
    if erase {
        unsafe {
            (*(*moved).meta).native_state = 0;
        }
    }
    assert_eq!(
        crate::fs::filehandle_object_fd(f64::from_bits(js_shadow_slot_get(0))),
        Some(4242)
    );
    let fresh = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        crate::fs::filehandle_object_fd(f64::from_bits(crate::value::POINTER_TAG | fresh as u64)),
        None
    );
    let detached = crate::object::js_object_alloc(crate::fs::CLASS_ID_FS_FILEHANDLE, 0);
    unsafe {
        crate::fs::set_filehandle_owner_fd(detached, -1);
    }
    assert_eq!(
        crate::fs::filehandle_object_fd(f64::from_bits(
            crate::value::POINTER_TAG | detached as u64
        )),
        Some(-1)
    );
}
