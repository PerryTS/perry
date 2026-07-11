//! OS memory-pressure → GC bridge for macOS (#6184, 2026-07-09 GC audit).
//!
//! AppKit has no `applicationDidReceiveMemoryWarning:` analog; the system-wide
//! signal is libdispatch's `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`. We install a
//! source on the **main** queue (the thread that owns the JS arena and precise
//! GC roots) at app startup and forward the pressure level to the runtime:
//!
//!   - `DISPATCH_MEMORYPRESSURE_WARN`     → `js_gc_memory_pressure(1)` (minor).
//!   - `DISPATCH_MEMORYPRESSURE_CRITICAL` → `js_gc_memory_pressure(2)` (full
//!     collect + trigger clamp).
//!
//! libdispatch is part of libSystem, so the C symbols below resolve at the
//! final link of any Perry-produced macOS binary without an extra crate.

use std::ffi::c_void;
use std::os::raw::c_ulong;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Once;

// Opaque libdispatch handles.
type DispatchSourceType = *const c_void;
type DispatchObject = *mut c_void;

/// Placeholder for the opaque `struct dispatch_source_type_s` / queue structs
/// we only ever take the address of.
#[repr(C)]
struct DispatchOpaque {
    _private: [u8; 0],
}

extern "C" {
    /// The memory-pressure source "type" token. The public
    /// `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` macro expands to a pointer to
    /// this symbol.
    static _dispatch_source_type_memorypressure: DispatchOpaque;
    /// The process-wide main serial queue; `dispatch_get_main_queue()` expands
    /// to `&_dispatch_main_q`.
    static _dispatch_main_q: DispatchOpaque;

    fn dispatch_source_create(
        type_: DispatchSourceType,
        handle: usize,
        mask: c_ulong,
        queue: DispatchObject,
    ) -> DispatchObject;
    fn dispatch_source_set_event_handler_f(
        source: DispatchObject,
        handler: extern "C" fn(*mut c_void),
    );
    fn dispatch_source_get_data(source: DispatchObject) -> c_ulong;
    fn dispatch_resume(object: DispatchObject);

    // perry-runtime's OS-memory-pressure entry point: level 1 = collect-if-safe
    // (minor), level 2+ = full collect + clamp trigger. Always lowers+arms the
    // arena trigger so an unsafe delivery still collects at the next allocation.
    fn js_gc_memory_pressure(level: u32) -> u32;
}

// <dispatch/source.h> DISPATCH_MEMORYPRESSURE_* masks.
const DISPATCH_MEMORYPRESSURE_WARN: c_ulong = 0x02;
const DISPATCH_MEMORYPRESSURE_CRITICAL: c_ulong = 0x04;

/// The live source pointer, so the event handler can read its pressure level
/// via `dispatch_source_get_data`. Set once, never cleared (the source is
/// intentionally leaked for the app's lifetime).
static PRESSURE_SOURCE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

static INSTALL_ONCE: Once = Once::new();

extern "C" fn on_memory_pressure(_context: *mut c_void) {
    let source = PRESSURE_SOURCE.load(Ordering::Acquire);
    if source.is_null() {
        return;
    }
    let data = unsafe { dispatch_source_get_data(source) };
    // CRITICAL is the last notice before the OS starts terminating processes,
    // so escalate to a full collect; WARN is advisory, so a minor collect.
    let level: u32 = if data & DISPATCH_MEMORYPRESSURE_CRITICAL != 0 {
        2
    } else if data & DISPATCH_MEMORYPRESSURE_WARN != 0 {
        1
    } else {
        0
    };
    if level != 0 {
        unsafe {
            js_gc_memory_pressure(level);
        }
    }
}

/// Install the main-queue memory-pressure dispatch source. Idempotent; safe to
/// call once from `app_run`.
pub fn install() {
    INSTALL_ONCE.call_once(|| unsafe {
        let source = dispatch_source_create(
            &_dispatch_source_type_memorypressure as *const _ as DispatchSourceType,
            0,
            DISPATCH_MEMORYPRESSURE_WARN | DISPATCH_MEMORYPRESSURE_CRITICAL,
            &_dispatch_main_q as *const _ as DispatchObject,
        );
        if source.is_null() {
            return;
        }
        PRESSURE_SOURCE.store(source, Ordering::Release);
        dispatch_source_set_event_handler_f(source, on_memory_pressure);
        // Dispatch sources start suspended.
        dispatch_resume(source);
        // Intentionally never released — the source must outlive `app_run`.
    });
}
