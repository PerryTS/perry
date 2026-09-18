//! CJS default-export resolution for native modules, split out of
//! `native_module.rs` for the 2000-line cap. Bodies are verbatim; the one
//! `super::` path (which meant `object::`) is spelled `crate::object::` here.

use super::*;

/// `"<mod>.default"` → `mod`. #9500: a thin view over the ONE shared table
/// (`perry_dispatch::CJS_DEFAULT_NAMESPACE_MODULES`); the hand-maintained copy
/// that used to live here is what the method-call router drifted from (#9485).
pub(crate) fn cjs_default_base_module(module_name: &str) -> Option<&'static str> {
    perry_dispatch::cjs_default_base_module(module_name)
}

/// `mod` → `"<mod>.default"`, from the same shared table (#9500).
fn cjs_default_namespace_name(module_name: &str) -> Option<&'static str> {
    perry_dispatch::cjs_default_namespace_name(module_name)
}

fn create_cjs_default_namespace(module_name: &str) -> Option<f64> {
    let name = cjs_default_namespace_name(module_name)?;
    Some(js_create_native_module_namespace(name.as_ptr(), name.len()))
}

pub(crate) fn cjs_default_export_value(module_name: &str) -> Option<f64> {
    match module_name {
        "assert" | "assert/strict" => Some(callable_exports::assert_cjs_export_value(module_name)),
        "events" => Some(bound_native_callable_export_value("events", "EventEmitter")),
        // #10431: `stream`'s `module.exports` IS the legacy `Stream` constructor
        // (exports hang off it: `attach_stream_legacy_prototype`).
        "stream" => Some(bound_native_callable_export_value("stream", "Stream")),
        // #3687: `node:cluster` default import is a distinct EventEmitter-shaped
        // `cluster.default` namespace (its `on`/`emit`/… reads diverge from the
        // bare `import * as` namespace).
        "cluster" => Some(crate::cluster::cluster_default_value()),
        // #3693: `node:dgram` default === the module namespace (CJS
        // `module.exports`); a cached singleton makes `dgram === ns.default`.
        "dgram" => Some(js_create_native_module_namespace(
            b"dgram".as_ptr(),
            "dgram".len(),
        )),
        "module" => Some(bound_native_callable_export_value("module", "Module")),
        "process" => Some(js_create_native_module_namespace(
            b"process".as_ptr(),
            "process".len(),
        )),
        "wasi" => Some(js_create_native_module_namespace(
            b"wasi.default".as_ptr(),
            "wasi.default".len(),
        )),
        // #9500: every remaining row of the shared CJS-default table gets its
        // `<mod>.default` namespace — the arms above are the modules whose
        // default export is NOT that namespace (a callable, or the plain
        // namespace itself) and must keep winning.
        other if perry_dispatch::has_cjs_default_namespace(other) => {
            create_cjs_default_namespace(other)
        }
        _ => None,
    }
}

pub(crate) fn native_module_get_builtin_module_value(module_name: &str) -> f64 {
    // Devirt: this is the runtime-dynamic builtin resolver (`require(spec)`,
    // `process.getBuiltinModule(spec)`) — `module_name` is only known at runtime,
    // so codegen could not emit the per-module dispatch install. Run the
    // install-all hook so a dynamically-resolved namespace can dispatch methods.
    // The hook is an INDIRECT pointer (null unless codegen emitted
    // `js_nm_enable_install_all()` because the program actually uses dynamic
    // require/getBuiltinModule) — so this resolver, which is linked into every
    // program via the always-present `process.getBuiltinModule` method table,
    // does NOT statically reference `js_nm_install_all` and therefore does not
    // pin every bucket. Static imports keep their precise per-module installs.
    crate::object::native_module_registry::nm_run_install_all_hook();
    cjs_default_export_value(module_name).unwrap_or_else(|| {
        js_create_native_module_namespace(module_name.as_ptr(), module_name.len())
    })
}
