//! Registry-backed Node module lookups: the `node:stream/promises` callable
//! behind `stream.pipeline` / `stream.finished`, and the real implementation of
//! `module.name` for modules the submodule registry implements without a
//! method-dispatch bucket.
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.

use super::{
    find_export, js_node_submod_install_stream_promises, js_node_submodule_export_as_function,
    js_node_submodule_namespace_member, special_export_value, SubmoduleSpec,
};

/// #6692: the `node:stream/promises` `pipeline` / `finished` callable — the
/// promise-based implementations Node exposes via
/// `stream.pipeline[util.promisify.custom]`. Installs the submodule registry
/// entry first so the lookup succeeds even when the program never imported
/// `node:stream/promises` itself. Returns NaN-boxed `TAG_TRUE` (the historical
/// unlisted-export sentinel) if `name` is unknown — callers should verify the
/// result is a callable closure before using it.
pub(crate) fn stream_promises_export_callable(name: &str) -> f64 {
    js_node_submod_install_stream_promises();
    unsafe {
        js_node_submodule_export_as_function(
            b"stream_promises".as_ptr(),
            "stream_promises".len() as u32,
            name.as_ptr(),
            name.len() as u32,
        )
    }
}

/// The submodule registry key of a Node module id that has no method-dispatch
/// bucket because the registry implements it (`timers/promises`,
/// `stream/web`, ...). `module` may carry the `node:` prefix. `None` for every
/// module with a dispatch bucket and for `test`/`test/reporters`, whose
/// exports are decorated by the `mod-node-test` hooks.
pub(crate) fn registry_backed_module_key(module: &str) -> Option<&'static str> {
    match module.strip_prefix("node:").unwrap_or(module) {
        "fs/promises" => Some("fs_promises"),
        "timers/promises" => Some("timers_promises"),
        "readline/promises" => Some("readline_promises"),
        "stream/promises" => Some("stream_promises"),
        "stream/consumers" => Some("stream_consumers"),
        "stream/web" => Some("stream_web"),
        "diagnostics_channel" => Some("diagnostics_channel"),
        "trace_events" => Some("trace_events"),
        _ => None,
    }
}

/// The real implementation of `module.name` for a registry-backed module (see
/// `registry_backed_module_key`): the registry's export value, a callable
/// singleton for functions. `None` when `module` is not registry-backed, the
/// registry does not list `name`, or the registry was never installed (codegen
/// installs it wherever the module is named; dynamic `require` arms the
/// install-all hook).
pub(crate) fn registry_backed_module_export(module: &str, name: &str) -> Option<f64> {
    let key = registry_backed_module_key(module)?;
    let value = unsafe {
        js_node_submodule_namespace_member(
            key.as_ptr(),
            key.len() as u32,
            name.as_ptr(),
            name.len() as u32,
        )
    };
    (value.to_bits() != crate::value::TAG_UNDEFINED).then_some(value)
}

/// `registry_backed_module_export` restricted to callable closures.
pub(crate) fn registry_backed_module_callable(module: &str, name: &str) -> Option<f64> {
    let value = registry_backed_module_export(module, name)?;
    let bits = value.to_bits();
    ((bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize))
    .then_some(value)
}

/// The cached native-module namespace of a registry-backed submodule that has no
/// default export object of its own (`None` for every other submodule).
pub(super) fn native_namespace_as_default(submod: &SubmoduleSpec) -> Option<f64> {
    if find_export(submod, "default").is_some()
        || special_export_value(submod.key, "default").is_some()
    {
        return None;
    }
    let module = match submod.key {
        "timers_promises" => "timers/promises",
        "readline_promises" => "readline/promises",
        "stream_consumers" => "stream/consumers",
        "stream_web" => "stream/web",
        _ => return None,
    };
    Some(crate::object::js_create_native_module_namespace(
        module.as_ptr(),
        module.len(),
    ))
}
