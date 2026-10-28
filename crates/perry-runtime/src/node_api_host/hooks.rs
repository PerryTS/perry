//! The always-live call sites' only way into the Node-API host.
//!
//! The event pump, process exit, agent retirement, `require` and
//! `process.dlopen` run in every program. They reach the host through one
//! slot that [`install`] fills (`js_runtime_install_node_api_host`, called by
//! perry's generated installer for a program with an approved native addon),
//! so a program without an addon links none of the host. An empty slot is the
//! behavior of an executable without an addon manifest.

use std::path::PathBuf;

use crate::feature_hooks::Hook;

/// The host entry points the always-live call sites use.
pub(crate) struct NodeApiHost {
    process_pending: fn() -> i32,
    has_active_work: fn() -> bool,
    shutdown_current_env: fn(),
    resolve_addon_request: fn(&str) -> Result<PathBuf, String>,
    load_addon_or_throw: fn(&str) -> f64,
    dlopen: fn(f64, f64, f64) -> f64,
}

static HOST: Hook<&'static NodeApiHost> = Hook::empty();

static ENTRY_POINTS: NodeApiHost = NodeApiHost {
    process_pending: super::process_pending,
    has_active_work: super::has_active_work,
    shutdown_current_env: super::shutdown_current_env,
    resolve_addon_request: super::resolve_addon_request,
    load_addon_or_throw: super::load_addon_or_throw,
    dlopen: super::process_dlopen,
};

/// Fill the slot and register the host's GC roots.
pub(crate) fn install() {
    crate::native_finalizer_hook::install();
    HOST.set(&ENTRY_POINTS);
    crate::feature_hooks::NODE_API_ROOT_SCANNER.set(super::scan_node_api_roots_mut);
    crate::gc::gc_register_named_mutable_root_scanner(
        super::ROOT_SCANNER_NAME,
        super::scan_node_api_roots_mut,
    );
}

/// Whether the program installed the host.
pub(crate) fn installed() -> bool {
    HOST.get().is_some()
}

/// Drain addon callbacks owed to this agent (the event pump).
pub(crate) fn process_pending() {
    if let Some(host) = HOST.get() {
        (host.process_pending)();
    }
}

/// Whether an addon still holds work that keeps the process alive.
pub(crate) fn has_active_work() -> bool {
    HOST.get().is_some_and(|host| (host.has_active_work)())
}

/// Finish native-addon ownership of the current agent.
pub(crate) fn shutdown_current_env() {
    if let Some(host) = HOST.get() {
        (host.shutdown_current_env)();
    }
}

/// The payload an allowlisted addon request resolves to.
pub(crate) fn resolve_addon_request(request: &str) -> Option<PathBuf> {
    HOST.get()
        .and_then(|host| (host.resolve_addon_request)(request).ok())
}

/// Load an addon `require` resolved to a `.node` file. `None` without the host.
pub(crate) fn load_addon(filename: &str) -> Option<f64> {
    HOST.get().map(|host| (host.load_addon_or_throw)(filename))
}

/// `process.dlopen(module, filename, flags)`. `None` without the host.
pub(crate) fn dlopen(module: f64, filename: f64, flags: f64) -> Option<f64> {
    HOST.get()
        .map(|host| (host.dlopen)(module, filename, flags))
}
