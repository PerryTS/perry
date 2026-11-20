//! Segment records register their existing per-thread keys from the installer.
use crate::feature_hooks::Hook;
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register()
    }
}
fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::intl::segmenter::scan_segment_record_keys_roots_mut",
        crate::intl::segmenter::scan_segment_record_keys_roots_mut,
    );
}
pub(crate) fn install() {
    REGISTER_ROOTS.set(register_roots);
    register_installed_roots();
}
