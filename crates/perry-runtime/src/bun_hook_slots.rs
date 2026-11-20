//! Bun CLI objects are reached through their program-selected installer.
use crate::feature_hooks::Hook;
static JS_BUN_STDIN: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_bun_stdin() -> f64 {
    JS_BUN_STDIN.get().map_or_else(|| undefined(), |run| run())
}
static JS_BUN_STDOUT: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_bun_stdout() -> f64 {
    JS_BUN_STDOUT.get().map_or_else(|| undefined(), |run| run())
}
static JS_BUN_STDERR: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_bun_stderr() -> f64 {
    JS_BUN_STDERR.get().map_or_else(|| undefined(), |run| run())
}
static DECORATE_BUN_PLUGIN: Hook<fn(f64) -> f64> = Hook::empty();
pub(crate) fn decorate_bun_plugin(value: f64) -> f64 {
    DECORATE_BUN_PLUGIN
        .get()
        .map_or_else(|| value, |run| run(value))
}
fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
pub(crate) fn install() {
    JS_BUN_STDIN.set(crate::bun_compat::js_bun_stdin);
    JS_BUN_STDOUT.set(crate::bun_compat::js_bun_stdout);
    JS_BUN_STDERR.set(crate::bun_compat::js_bun_stderr);
    DECORATE_BUN_PLUGIN.set(crate::bun_compat::decorate_bun_plugin);
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static SELF_TLS_RESOLVER: Hook<TlsResolver> = Hook::empty();
pub(crate) fn install_self_resolver(resolve: TlsResolver) {
    SELF_TLS_RESOLVER.set(resolve)
}
pub(crate) fn resolve_self_guard(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    SELF_TLS_RESOLVER
        .get()
        .expect("Bun platform bootstrap must precede self alias storage")(idx)
}
