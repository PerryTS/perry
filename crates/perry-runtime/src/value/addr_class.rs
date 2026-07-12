//! Centralized handle-vs-heap-pointer address classification.
//!
//! Perry NaN-boxes JS values; `POINTER_TAG` (0x7FFD) carries a 48-bit payload
//! that is USUALLY a heap pointer to a GC-managed allocation (8-byte
//! [`crate::gc::GcHeader`] at `addr - GC_HEADER_SIZE`), but several
//! subsystems smuggle small integer *registry handles* under the same tag.
//! Handles are NOT addresses: dereferencing one reads unmapped low memory and
//! segfaults on Linux (macOS mimalloc page retention masks the class — see
//! #4665, #4800). Runtime code therefore classifies a payload by MAGNITUDE
//! before any dereference. This module is the single owner of the band
//! boundaries and the classification predicates; do not re-type the literals
//! at call sites (the `scripts/addr_class_inventory.py` lint gate enforces
//! this).
//!
//! ## Band map (who owns which id range)
//!
//! | Range                  | Owner                                                            |
//! |------------------------|------------------------------------------------------------------|
//! | `0`                    | null / INVALID_HANDLE                                            |
//! | `[1, 0x40000)`         | perry-stdlib `common/handle.rs` registry (net.Socket, node:http, |
//! |                        | crypto, fastify, ioredis, UI widgets, timers, …)                 |
//! | `[0x40000, 0xE0000)`   | Web Fetch family (Request/Response/Headers/Blob), perry-stdlib   |
//! |                        | `fetch/mod.rs` `FETCH_HANDLE_ID_{START,END}` (#3973/#3974/#4004) |
//! | `[0xE0000, 0xF0000)`   | zlib streams, perry-stdlib `zlib.rs` (#1843)                     |
//! | `[0xF0000, 0x100000)`  | revocable Proxy ids, perry-runtime `proxy.rs` `PROXY_TAG_BASE`   |
//! |                        | (#2846 crash cluster)                                            |
//! | `>= 0x100000`          | plausible heap addresses (see [`is_valid_obj_ptr`] for the       |
//! |                        | platform heap floor/ceiling)                                     |
//! | `[0x100000, 0x200000)` | EXCEPTION: Web Streams ids (perry-stdlib `streams.rs`) are RAW   |
//! |                        | NUMERIC `f64` values — never `POINTER_TAG`-boxed — deliberately  |
//! |                        | placed above the pointer-tagged handle band (#1545). Only probe  |
//! |                        | this band on values that arrived as plain finite numbers.        |
//!
//! The `0x100000` ceiling was established by #1843 (zlib handle deref'd as
//! heap object), #4004 (fetch handles moved to 0x40000), and #4800
//! (`is_builtin_iterator_class_id` used an 0x1008 floor and deref'd a Headers
//! handle on every hono response). All four sub-bands must stay below
//! [`HANDLE_BAND_MAX`]; perry-stdlib re-exports these constants and its unit
//! tests assert the containment.

use crate::gc::{GcHeader, GC_HEADER_SIZE};

/// Exclusive upper bound of the small-handle id space. Payloads below this are
/// registry handles (or null/garbage), never dereferenceable heap pointers.
/// Raising any sub-band past this value requires auditing every
/// `is_handle_band` caller.
pub const HANDLE_BAND_MAX: usize = 0x100000;

/// Exclusive end of the generic perry-stdlib `common/handle.rs` registry band
/// (`[1, COMMON_HANDLE_BAND_END)`). The registry panics rather than allocate
/// into the fetch band above it.
pub const COMMON_HANDLE_BAND_END: usize = 0x40000;

/// Web Fetch handle band `[FETCH_HANDLE_BAND_START, FETCH_HANDLE_BAND_END)`,
/// owned by perry-stdlib `fetch/mod.rs` (#4004 moved it here, out of the
/// common registry's way).
pub const FETCH_HANDLE_BAND_START: usize = 0x40000;
pub const FETCH_HANDLE_BAND_END: usize = 0xE0000;

/// zlib stream handle band `[ZLIB_HANDLE_BAND_START, ZLIB_HANDLE_BAND_END)`,
/// owned by perry-stdlib `zlib.rs` (#1843 established that these ids must not
/// be dereferenced as heap objects).
pub const ZLIB_HANDLE_BAND_START: usize = 0xE0000;
pub const ZLIB_HANDLE_BAND_END: usize = 0xF0000;

/// Revocable Proxy id band `[PROXY_ID_BAND_START, HANDLE_BAND_MAX)`, owned by
/// perry-runtime `proxy.rs` (`PROXY_TAG_BASE`). Kept at the top of the handle
/// band so fetch ids below never collide with a proxy id (#2846).
pub const PROXY_ID_BAND_START: usize = 0xF0000;

/// Web Streams id band `[STREAM_ID_BAND_START, STREAM_ID_BAND_END)`, owned by
/// perry-stdlib `streams.rs`. NOT part of the pointer-tagged handle band:
/// stream ids travel as raw numeric `f64`s (#1545), so they sit just above
/// `HANDLE_BAND_MAX` and only number-typed probe paths may classify into it.
pub const STREAM_ID_BAND_START: usize = 0x100000;
pub const STREAM_ID_BAND_END: usize = 0x200000;

/// True when `addr` lies in the small-handle band (including 0/null). A
/// payload in this band must never be dereferenced; route it to the handle
/// dispatch tables instead.
#[inline(always)]
pub fn is_handle_band(addr: usize) -> bool {
    addr < HANDLE_BAND_MAX
}

/// True for a plausible *live* handle id: non-zero and inside the handle
/// band. Mirrors the widespread `addr > 0 && addr < 0x100000` shape (0 is
/// null / INVALID_HANDLE, not a handle).
#[inline(always)]
pub fn is_small_handle(addr: usize) -> bool {
    (1..HANDLE_BAND_MAX).contains(&addr)
}

/// Complement of [`is_handle_band`]: the payload is above the handle band and
/// may be treated as a candidate heap address (subject to
/// [`is_valid_obj_ptr`] / registry checks as the call site requires). Note
/// `0`/null is NOT above the band.
#[inline(always)]
pub fn is_above_handle_band(addr: usize) -> bool {
    addr >= HANDLE_BAND_MAX
}

/// True when `addr` is a revocable-Proxy id. Callers must still confirm
/// registration via `proxy::js_proxy_is_proxy` before routing — a heap-free
/// check, so do it before any dereference.
#[inline(always)]
pub fn is_proxy_id_band(addr: usize) -> bool {
    (PROXY_ID_BAND_START..HANDLE_BAND_MAX).contains(&addr)
}

/// True when `id` is in the raw-numeric Web Streams id band. Only meaningful
/// for values that arrived as plain finite numbers (never for `POINTER_TAG`
/// payloads — heap pointers live in this range too).
#[inline(always)]
pub fn is_stream_id_band(id: usize) -> bool {
    (STREAM_ID_BAND_START..STREAM_ID_BAND_END).contains(&id)
}

/// Check if a pointer is a valid heap object (safe to dereference GcHeader).
/// Values below 0x100000 (1MB) are likely INT32_TAG extracts, small handles,
/// or null. The upper bound filters out NaN-box tag bits that leaked through.
///
/// Issue #73 follow-up: raised the lower bound from 1 MB to 2 TB to reject
/// corrupted NaN-boxes whose 48-bit handle lands in the 1-2 TB window
/// (e.g. `0x00FF_0000_0000` from an `ArrayHeader { length: 0, capacity:
/// 255 }` read as u64). Real macOS mimalloc + arena allocations all
/// land in the 3-5 TB range; anything below 2 TB is certainly bogus on
/// that platform. Linux glibc and Windows mimalloc allocate well below
/// 2 TB though (often in the GB-to-tens-of-GB range), so the macOS floor
/// silently rejects every legitimate object pointer there — issues
/// #385/#386/#387 traced back to this exact filter on Windows.
///
/// #1136 / #1129: iOS-family *device* targets (aarch64-apple-ios,
/// -tvos, -watchos, -visionos) ship without mimalloc and use
/// libsystem_malloc, whose user allocations land in the same low range
/// as Android/Linux/Windows. Treat them like those platforms — the
/// downstream `GcHeader.obj_type` check is the real liveness guard.
/// The simulator (e.g. ios + target_abi = "sim") runs on the macOS
/// host's mimalloc so its allocations still land above 2 TB; lowering
/// the floor here is safe because the obj_type validation does the
/// work.
///
/// NOTE: the platform `HEAP_MIN` floor on Linux/Android/iOS/Windows
/// (`0x1000`) is BELOW the handle band, so this predicate alone does NOT
/// reject small handles there — pair it with [`is_handle_band`] (or use
/// [`try_read_gc_header`], which does both) when the input can carry a
/// handle id. `scripts/addr_class_inventory.py` ratchets this: the
/// `lone-valid-obj-ptr` rule fails the build on any NEW unpaired call site.
///
/// #6279 — DO NOT "fix" this by raising `HEAP_MIN` to [`HANDLE_BAND_MAX`].
/// It looks like a one-character typo (the doc above says 1 MB, the constant
/// says 4 KB) and it is tempting, but the permissive floor is load-bearing:
/// several callers pass a handle-band id through here on purpose and rely on
/// it answering `true` so they can route the value onward. Raising the floor
/// was tried and measurably regressed `Object.defineProperty` on native
/// handles (started throwing TypeError) and the Proxy `apply` trap (lost its
/// arguments) — caught by `test_gap_handle_band_object_ops` and
/// `test_gap_proxy_reflect` on Linux. Those dependents must be migrated to an
/// explicit band check FIRST; only then can this floor be raised.
#[inline(always)]
pub(crate) fn is_valid_obj_ptr(ptr: *const u8) -> bool {
    let addr = ptr as u64;
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    const HEAP_MIN: u64 = 0x1000;
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
    const HEAP_MIN: u64 = 0x200_0000_0000;
    (HEAP_MIN..0x8000_0000_0000).contains(&addr)
}

/// True when `addr` is outside every handle band AND inside the platform
/// heap range — i.e. plausible to dereference as a GC allocation. This is the
/// canonical `addr >= 0x100000 && is_valid_obj_ptr(addr)` pairing.
#[inline(always)]
pub(crate) fn is_plausible_heap_addr(addr: usize) -> bool {
    is_above_handle_band(addr) && is_valid_obj_ptr(addr as *const u8)
}

/// Validated GcHeader read: magnitude-classify FIRST (reject the handle band
/// and implausible heap addresses), only then dereference
/// `addr - GC_HEADER_SIZE`. Returns `None` without touching memory for
/// handles, null, tag remnants, and out-of-range garbage.
///
/// # Safety
/// `addr` must either be a live GC allocation's user address or arbitrary
/// non-pointer bits; a STALE heap address that passes the magnitude checks is
/// still dereferenced (same contract as every existing call site — the
/// registries/`obj_type` checks layered above this are what catch reuse).
#[inline(always)]
pub(crate) unsafe fn try_read_gc_header(addr: usize) -> Option<&'static GcHeader> {
    if !is_plausible_heap_addr(addr) {
        return None;
    }
    // Small-buffer slab allocations are heap-plausible but carry NO GcHeader —
    // `addr - GC_HEADER_SIZE` is the previous slab entry's data bytes, so a
    // brand probe (Temporal/Date/Map/Set `obj_type` check) would read a
    // content-dependent fake header and misroute (observed: `String(buffer)`
    // on a zlib result took the Temporal path and deref'd buffer bytes).
    if crate::buffer::is_small_buf_slab_addr(addr) {
        return None;
    }
    Some(&*((addr - GC_HEADER_SIZE) as *const GcHeader))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_layout_is_contiguous_and_contained() {
        assert!(COMMON_HANDLE_BAND_END <= FETCH_HANDLE_BAND_START);
        assert!(FETCH_HANDLE_BAND_START < FETCH_HANDLE_BAND_END);
        assert!(FETCH_HANDLE_BAND_END <= ZLIB_HANDLE_BAND_START);
        assert!(ZLIB_HANDLE_BAND_END <= PROXY_ID_BAND_START);
        assert!(PROXY_ID_BAND_START < HANDLE_BAND_MAX);
        assert!(STREAM_ID_BAND_START >= HANDLE_BAND_MAX);
    }

    #[test]
    fn handle_band_predicates() {
        // The #4800 shape: a first-allocation fetch Headers handle.
        assert!(is_handle_band(0x40000));
        assert!(is_small_handle(0x40000));
        // Proxy ids (#2846), zlib (#1843), common registry, null.
        assert!(is_proxy_id_band(0xF0000));
        assert!(is_proxy_id_band(0xF_FFF8));
        assert!(!is_proxy_id_band(0x40000));
        assert!(is_handle_band(0xE0000));
        assert!(is_handle_band(1));
        assert!(is_handle_band(0));
        assert!(!is_small_handle(0));
        // First heap-plausible address.
        assert!(!is_handle_band(HANDLE_BAND_MAX));
        assert!(is_above_handle_band(HANDLE_BAND_MAX));
        assert!(!is_small_handle(HANDLE_BAND_MAX));
    }

    #[test]
    fn try_read_gc_header_rejects_handles_without_deref() {
        // Would SIGSEGV on Linux if dereferenced (#4665/#4800) — must be None
        // purely from the magnitude check.
        for addr in [
            0usize, 1, 0x1008, 0x10000, 0x40000, 0x4000c, 0xF0000, 0xF_FFF8,
        ] {
            assert!(unsafe { try_read_gc_header(addr) }.is_none());
        }
        // Tag remnants / out-of-range bits.
        assert!(unsafe { try_read_gc_header(0x7FFD_0000_0000_0000) }.is_none());
    }

    #[test]
    fn stream_id_band_is_above_pointer_handles() {
        assert!(is_stream_id_band(STREAM_ID_BAND_START));
        assert!(!is_stream_id_band(HANDLE_BAND_MAX - 1));
        assert!(!is_handle_band(STREAM_ID_BAND_START));
    }
}
