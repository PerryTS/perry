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

/// `module_name`'s `<mod>.default` namespace, born with the default vtable
/// the module's own vtable `base` carries (unkeyed when it carries none).
fn create_cjs_default_namespace(
    module_name: &str,
    base: Option<&'static NativeModuleVtable>,
) -> Option<f64> {
    let name = cjs_default_namespace_name(module_name)?;
    let vt = base.map(|base| base.default_namespace.unwrap_or(&NATIVE_MODULE_VTABLE_IMPL));
    Some(create_native_module_namespace_in(vt, name))
}

/// The CommonJS `module.exports` of `module_name`. `base` is the module's own
/// vtable (from the receiver namespace, from codegen, or named by a literal
/// site; `None` for a runtime string the program has no row for); the
/// `<mod>.default` namespace is born from its `default_namespace`.
pub(crate) fn cjs_default_export_value(
    module_name: &str,
    base: Option<&'static NativeModuleVtable>,
) -> Option<f64> {
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
        "module" => Some(bound_native_callable_export_value("module", "Module")),
        "process" => Some(
            crate::object::native_module::create_native_module_namespace(
                &crate::object::native_module::namespaces::process::NM_PROCESS,
                "process",
            ),
        ),
        "wasi" => Some(
            crate::object::native_module::create_native_module_namespace(
                &crate::object::native_module::namespaces::wasi::NM_WASI_DEFAULT,
                "wasi.default",
            ),
        ),
        // #9500: every remaining row of the shared CJS-default table gets its
        // `<mod>.default` namespace — the arms above are the modules whose
        // default export is NOT that namespace (a callable, or the plain
        // namespace itself) and must keep winning.
        other if perry_dispatch::has_cjs_default_namespace(other) => {
            create_cjs_default_namespace(other, base)
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
    cjs_default_export_value(module_name, native_module_vtable_by_name(module_name))
        .unwrap_or_else(|| native_namespace_by_name(module_name))
}
