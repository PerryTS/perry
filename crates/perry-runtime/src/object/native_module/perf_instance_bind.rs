//! Receiver binding for the internal `perf_*` namespace tags.
//!
//! Split out of `native_module.rs` to keep that file under the 2k-LOC gate
//! (`scripts/check_file_size.sh`).

use super::build_bound_method_closure;

/// Method names of the internal `perf_*` namespace tags whose objects carry
/// PER-INSTANCE state: a registry index in field[1] for a histogram or an
/// observer, and the in-flight flush buffer for an observer entry list.
///
/// A method read on one of these must bind THIS receiver.
/// `bound_native_callable_export_value` mints a *fresh* namespace object for
/// the module and captures that instead, which loses the instance id — and for
/// a tag that is not a real importable module it does not resolve back to a
/// dispatch bucket at all. Both failure modes are silent: `h.record(5)` and
/// `list.getEntries()` returned `undefined` and did nothing, because
/// `h.record` / `list.getEntries` read as `Call { callee: PropertyGet }` (a
/// value read then an indirect call), not as a statically lowered
/// NativeMethodCall.
fn instance_bound_perf_method_names(module_name: &str) -> Option<&'static [&'static str]> {
    match module_name {
        "perf_histogram" => Some(&[
            "add",
            "disable",
            "enable",
            "percentile",
            "percentileBigInt",
            "record",
            "recordDelta",
            "reset",
            "toJSON",
        ]),
        "perf_observer" => Some(&["disconnect", "observe", "takeRecords"]),
        "perf_observer_list" => Some(&["getEntries", "getEntriesByName", "getEntriesByType"]),
        _ => None,
    }
}

/// Bind `property_name` to `receiver` when the module is one of the
/// per-instance `perf_*` tags. `&'static str` bytes give the closure the stable
/// method-name pointer `build_bound_method_closure` requires.
pub(crate) fn instance_bound_perf_method(
    module_name: &str,
    property_name: &str,
    receiver: f64,
) -> Option<f64> {
    let name = instance_bound_perf_method_names(module_name)?
        .iter()
        .find(|candidate| **candidate == property_name)?;
    Some(build_bound_method_closure(
        receiver,
        name.as_ptr(),
        name.len(),
    ))
}
