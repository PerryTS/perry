//! Provider-safe GC registration for the Web Fetch registries' heap values.
//!
//! The Fetch handle registries are process-global `lazy_static!` tables keyed
//! by small handle ids, and three of them hold *heap values*, not just Rust
//! data:
//!
//! * `HEADERS_METHOD_VALUE_CACHE` — the bound-method closure behind
//!   `headers.get` / `headers.entries` / … (one per `(handle, method)`),
//! * `FORM_DATA_METHOD_VALUE_CACHE` — the same for `FormData`,
//! * `RequestRecord::signal` — the `AbortSignal` object behind `request.signal`.
//!
//! Until #8163 nothing marked or rewrote those slots. `js_write_barrier_root_nanbox`
//! at the store site is only the incremental-marking shade — it does not
//! register a root — so under a moving collector the cached closure either
//! died (nothing else referenced it once the caller dropped the bound copy) or
//! moved, and the next `headers.get` read handed the pre-move address to
//! `typeof`. That is precisely the shape the production Next App Route fixture
//! hit under forced evacuation: `(await headers()).get(...)` twice per request
//! with collections between, and the second read faulting on a retired
//! from-space closure. `PERRY_GC_VERIFY_EVACUATION` cannot see it (no scanner
//! to verify), `PERRY_GC_PROTECT_FROMSPACE_HOLDERS` cannot see it (the holder is
//! a Rust `HashMap` outside the GC heap), and `scripts/gc_runtime_root_holders.py`
//! could not see it either — its declaration regex did not match
//! `lazy_static!`'s `static ref` (fixed alongside this module).
//!
//! Registration goes through the stable C ABI, exactly like `streams::gc`, so a
//! separately packaged stdlib provider installs its scanner into the
//! process-wide runtime image rather than into any runtime glue that happens to
//! be linked into the stdlib image.
//!
//! **Locking contract (load-bearing).** The scanner runs *during* a collection
//! on the mutator thread and takes each table's mutex, so **no site may hold one
//! of these guards across a GC allocation, or across a throw**. `std::sync::Mutex`
//! is not reentrant: an allocation under the guard that triggers a collection
//! deadlocks against this scanner on the same thread, and a throw under the
//! guard unwinds through the frame without running `Drop` (the `eh.rs` transport
//! is written for `panic=abort` semantics), leaving the registry locked for the
//! life of the process.
//!
//! Registering this scanner is therefore what turned a dozen pre-existing
//! "allocate under the guard" sites into real deadlocks, and they were hoisted
//! in the same change: `js_request_new` builds its whole `RequestRecord`
//! (default `AbortSignal` allocation included) before taking the lock;
//! `js_request_get_url` / `_method` / `_body`, `js_request_input_to_url`,
//! `request_string_field` and `dispatch_request_property`'s twelve string arms
//! snapshot the field bytes under the guard and allocate after dropping it; and
//! `js_request_clone` decides "unusable" under the guard but throws outside it.
//! Two tests hold the line, because the failure mode is a HANG and a test that
//! hangs is worse than one that fails:
//! `fetch::tests::request_reads_release_the_registry_guard` exercises every
//! reader and asserts the mutex is free afterwards (a guard leaked by a throw
//! shows up here), and
//! `fetch::tests::no_allocation_is_taken_off_a_live_registry_borrow` is a
//! source scan for the shape that caused it — `js_string_from_bytes(req.…)`,
//! i.e. allocating straight out of a borrow that only the guard keeps alive.

use super::*;
use std::ffi::c_void;

const FFI_SLOT_NANBOX_F64: u32 = 4;
const FFI_SLOT_NANBOX_U64: u32 = 5;

type FfiMutableRootVisitor = extern "C" fn(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool;
type FfiNamedMutableRootScanner =
    extern "C" fn(scanner_id: usize, visit: FfiMutableRootVisitor, ctx: *mut c_void);

extern "C" {
    fn perry_ffi_gc_register_mutable_root_scanner_named(
        source_ptr: *const u8,
        source_len: usize,
        scanner_id: usize,
        scanner: FfiNamedMutableRootScanner,
    );
}

/// The two slot shapes the Fetch registries hold. Both are NaN-boxed values;
/// the visitor marks the referent and rewrites the slot when it moved.
pub(super) trait FetchRootVisitor {
    fn visit_nanbox_f64_slot(&mut self, slot: &mut f64);
    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64);
}

impl FetchRootVisitor for perry_runtime::gc::RuntimeRootVisitor<'_> {
    fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) {
        perry_runtime::gc::RuntimeRootVisitor::visit_nanbox_f64_slot(self, slot);
    }

    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
        perry_runtime::gc::RuntimeRootVisitor::visit_nanbox_u64_slot(self, slot);
    }
}

struct FfiFetchRootVisitor {
    visit: FfiMutableRootVisitor,
    ctx: *mut c_void,
}

impl FetchRootVisitor for FfiFetchRootVisitor {
    fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) {
        (self.visit)(
            FFI_SLOT_NANBOX_F64,
            slot as *mut f64 as *mut c_void,
            self.ctx,
        );
    }

    fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
        (self.visit)(
            FFI_SLOT_NANBOX_U64,
            slot as *mut u64 as *mut c_void,
            self.ctx,
        );
    }
}

static GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the Fetch root scanner exactly once. Called from every site that
/// stores a heap value into one of the registries, before the store, so the
/// value is reachable from the first collection after it lands.
pub(super) fn ensure_gc_registered() {
    GC_REGISTERED.call_once(|| {
        const SOURCE: &[u8] = b"stdlib:fetch";
        unsafe {
            perry_ffi_gc_register_mutable_root_scanner_named(
                SOURCE.as_ptr(),
                SOURCE.len(),
                0,
                scan_fetch_roots_ffi,
            );
        }
    });
}

extern "C" fn scan_fetch_roots_ffi(
    _scanner_id: usize,
    visit: FfiMutableRootVisitor,
    ctx: *mut c_void,
) {
    scan_fetch_roots_with(&mut FfiFetchRootVisitor { visit, ctx });
}

#[cfg(test)]
pub(super) fn scan_fetch_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_fetch_roots_with(&mut visitor);
}

/// Visit every heap-value slot the Fetch registries own.
pub(super) fn scan_fetch_roots_with<V: FetchRootVisitor>(visitor: &mut V) {
    headers_method_value::visit_roots(visitor);
    dispatch::visit_form_data_method_value_roots(visitor);
    if let Ok(mut requests) = REQUEST_REGISTRY.lock() {
        for request in requests.values_mut() {
            visitor.visit_nanbox_f64_slot(&mut request.signal);
        }
    }
}
