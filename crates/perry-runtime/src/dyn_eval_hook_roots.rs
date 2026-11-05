//! The evaluator's installer registers its existing per-thread roots.
use crate::feature_hooks::Hook;
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register()
    }
}
pub(crate) fn install(register: fn()) {
    REGISTER_ROOTS.set(register);
    register_installed_roots();
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static ACTIVE_FUNCTIONS_STORAGE: Hook<TlsResolver> = Hook::empty();
pub(crate) fn set_active_functions_storage(resolve: TlsResolver) {
    ACTIVE_FUNCTIONS_STORAGE.set(resolve)
}
pub(crate) fn resolve_active_functions_storage(
    idx: u32,
) -> Result<*mut u8, crate::tls_hot::AccessError> {
    ACTIVE_FUNCTIONS_STORAGE
        .get()
        .expect("Evaluator installer must precede its function pin storage")(idx)
}
static PIN_TOP_STORAGE: Hook<TlsResolver> = Hook::empty();
pub(crate) fn set_pin_top_storage(resolve: TlsResolver) {
    PIN_TOP_STORAGE.set(resolve)
}
pub(crate) fn resolve_pin_top_storage(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    PIN_TOP_STORAGE
        .get()
        .expect("Evaluator installer must precede its function pin storage")(idx)
}
