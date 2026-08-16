//! GC registration for perry-ext-fetch's request registry (split out of
//! `lib.rs` for the 2,000-line lint gate; a child module reaches the
//! crate-private tables via `use super::*`).

use super::*;

static FETCH_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Called from `store_request`, so the scanner is installed before the
/// first signal is ever parked. Registers through `perry_ffi`'s named
/// wrapper, which is itself the stable C-ABI
/// `perry_ffi_gc_register_mutable_root_scanner_named` — the route every
/// ext staticlib provider uses, so a trimmed image still installs into the
/// process-wide runtime.
pub(crate) fn ensure_gc_scanner_registered() {
    FETCH_GC_REGISTERED.call_once(|| {
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-fetch", scan_fetch_roots);
    });
}

/// GC root scanner: every stored `Request`'s `signal` is a NaN-boxed
/// AbortSignal OBJECT (user-passed, or freshly built by
/// `default_abort_signal_value()` — in which case this table is its ONLY
/// holder). It lives from `new Request(...)` until the last
/// `request.signal` read, across arbitrary user JS; without this scanner a
/// full collection frees it and a copying minor leaves the table pointing
/// at the old address.
///
/// Locking contract: this runs DURING a collection on the mutator thread
/// and takes `REQUEST_HANDLES`, so no reader may hold that guard across a
/// GC allocation (`alloc_string` et al.) — see the try_lock probe and
/// source-scan tests in `tests.rs`.
fn scan_fetch_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    if let Ok(mut requests) = REQUEST_HANDLES.lock() {
        for request in requests.values_mut() {
            visitor.visit_nanbox_f64_slot(&mut request.signal);
        }
    }
}
