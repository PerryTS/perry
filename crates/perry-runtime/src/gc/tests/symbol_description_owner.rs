//! A fresh symbol owns its description edge, without native description maps.
use super::super::*;
use super::support::*;
#[test]
fn symbol_description_moves_with_its_symbol_owner() {
    check_description(false);
}
#[test]
#[ignore = "negative control: detach the moved symbol's description edge"]
fn negative_control_objtables_symbol_description() {
    check_description(true);
}
fn check_description(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let desc = crate::string::js_string_from_bytes(b"owned-symbol".as_ptr(), 12);
    let owner = unsafe { crate::symbol::alloc_symbol(desc, false) };
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::symbol::SymbolHeader;
    assert_ne!(moved, owner);
    let desc_after = unsafe { (*moved).description };
    assert_ne!(desc_after, desc);
    if erase {
        unsafe {
            (*moved).description = std::ptr::null_mut();
        }
    }
    assert_eq!(
        unsafe { crate::symbol::symbol_description_text(moved) }.as_deref(),
        Some(b"owned-symbol".as_slice())
    );
    let empty = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };
    assert!(unsafe { crate::symbol::symbol_description_text(empty) }.is_none());
}
