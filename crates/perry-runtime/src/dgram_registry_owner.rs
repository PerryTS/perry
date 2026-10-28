//! Existing UDP binding state; its initializer reaches the backend by slot.
use crate::dgram::DgramRegistry;
use crate::feature_hooks::Hook;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
static REGISTER_EXIT: Hook<fn()> = Hook::empty();
pub(crate) fn install() {
    REGISTER_EXIT.set(crate::dgram::register_thread_exit_hook);
}
pub(crate) static DGRAM_REGISTRY: LazyLock<Mutex<DgramRegistry>> = LazyLock::new(|| {
    // #11541: register the thread-exit hook when the registry is first
    // touched (before any binding can exist) instead of naming it in
    // `arena::thread_exit`'s dispatcher.
    if let Some(register) = REGISTER_EXIT.get() {
        register()
    }
    Mutex::new(DgramRegistry {
        next_port: 49152,
        bound: HashMap::new(),
    })
});
