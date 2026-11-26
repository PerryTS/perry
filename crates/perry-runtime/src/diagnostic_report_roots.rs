//! Only programs with diagnostics materialize and root a report controller.
use crate::feature_hooks::Hook;
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register()
    }
}
fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::process::scan_report_cache_roots_mut",
        crate::process::scan_report_cache_roots_mut,
    );
}
pub(crate) fn install() {
    REPORT_VALUE.set(report_value_installing_collector);
    REGISTER_ROOTS.set(register_roots);
    register_installed_roots();
}

/// `process.report.getReport()`: the first report selects the diagnostic
/// collector body (`gc::diagnostic_hooks::install_collector`), as a snapshot does.
fn report_value_installing_collector() -> f64 {
    crate::gc::diagnostic_hooks::install_collector();
    crate::process::report::process_report_value()
}

static REPORT_VALUE: Hook<fn() -> f64> = Hook::empty();
pub(crate) fn report_value() -> f64 {
    REPORT_VALUE
        .get()
        .map_or(f64::from_bits(crate::value::TAG_UNDEFINED), |read| read())
}
