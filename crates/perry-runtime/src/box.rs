//! GC-managed mutable capture cells. Generated frames and closure captures
//! root the cell itself; the collector traces and rewrites its value slot.
//! There are no per-cell registries, malloc pools, or registry root scans.
use std::sync::atomic::{AtomicU64, Ordering};
#[path = "box/activation.rs"]
mod activation;
pub(crate) use activation::*;
static BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
/// A box is simply a heap-allocated JSValue bit slot.
#[repr(C)]
pub struct Box {
    pub value: u64,
}

#[repr(C, align(8))]
pub struct I32Box {
    pub value: i32,
}

#[repr(C, align(8))]
pub struct BoolBox {
    pub value: bool,
}

#[no_mangle]
pub extern "C" fn js_box_alloc_bits(initial_value: i64) -> *mut Box {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_u64(initial_value as u64);
    let ptr = crate::arena::arena_alloc_gc(std::mem::size_of::<Box>(), 8, crate::gc::GC_TYPE_BOX)
        as *mut Box;
    unsafe {
        (*ptr).value = value.get_nanbox_u64();
    }
    ptr
}

#[no_mangle]
pub extern "C" fn js_i32_box_alloc(initial_value: i32) -> *mut I32Box {
    let ptr =
        crate::arena::arena_alloc_gc(std::mem::size_of::<I32Box>(), 8, crate::gc::GC_TYPE_I32_BOX)
            as *mut I32Box;
    unsafe {
        (*ptr).value = initial_value;
    }
    ptr
}

#[no_mangle]
pub extern "C" fn js_bool_box_alloc(initial_value: i32) -> *mut BoolBox {
    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<BoolBox>(),
        8,
        crate::gc::GC_TYPE_BOOL_BOX,
    ) as *mut BoolBox;
    unsafe {
        (*ptr).value = initial_value != 0;
    }
    ptr
}

#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    js_box_alloc_bits(initial_value.to_bits() as i64)
}

#[no_mangle]
pub extern "C" fn js_box_release(_ptr: *mut Box) {
    finish_async_box_activation(crate::promise::current_async_box_activation());
}
#[no_mangle]
pub extern "C" fn js_box_scope_release(_ptr: *mut Box) {}

#[no_mangle]
pub extern "C" fn js_i32_box_release(_ptr: *mut I32Box) {
    finish_async_box_activation(crate::promise::current_async_box_activation());
}
#[no_mangle]
pub extern "C" fn js_i32_box_scope_release(_ptr: *mut I32Box) {}

#[no_mangle]
pub extern "C" fn js_bool_box_release(_ptr: *mut BoolBox) {
    finish_async_box_activation(crate::promise::current_async_box_activation());
}
#[no_mangle]
pub extern "C" fn js_bool_box_scope_release(_ptr: *mut BoolBox) {}

#[inline]
fn has_box_type(addr: usize, kind: u8) -> bool {
    let Some(header_addr) = addr.checked_sub(crate::gc::GC_HEADER_SIZE) else {
        return false;
    };
    let Some((_, base, starts)) = crate::arena::classify_heap_space_in_range(header_addr) else {
        return false;
    };
    if !crate::arena::arena_header_is_object_start(header_addr, base, starts) {
        return false;
    }
    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(addr).is_some_and(|header| {
            let header = header.as_ref();
            header.obj_type == kind
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && header.size as usize == crate::gc::GC_HEADER_SIZE + 8
        })
    }
}
fn is_registered_box_ptr(ptr: *mut Box) -> bool {
    has_box_type(ptr as usize, crate::gc::GC_TYPE_BOX)
}
fn is_registered_i32_box_ptr(ptr: *mut I32Box) -> bool {
    has_box_type(ptr as usize, crate::gc::GC_TYPE_I32_BOX)
}
fn is_registered_bool_box_ptr(ptr: *mut BoolBox) -> bool {
    has_box_type(ptr as usize, crate::gc::GC_TYPE_BOOL_BOX)
}

/// Thread serialization unwraps an explicitly captured mutable cell.
pub fn box_slot_contents_bits(bits: u64) -> Option<u64> {
    let ptr = bits as usize as *mut Box;
    is_registered_box_ptr(ptr).then(|| unsafe { (*ptr).value })
}

/// Get the raw JSValue bit pattern from a box.
///
/// Same robustness as `js_box_set`: invalid pointers return `undefined`
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get_bits(ptr: *mut Box) -> i64 {
    box_get_bits_named(ptr, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// Checked lexical read with the source binding name supplied by codegen.
/// The name is consumed only on the TDZ error path, before any GC allocation.
#[no_mangle]
pub extern "C" fn js_box_get_bits_named(ptr: *mut Box, name: f64) -> i64 {
    box_get_bits_named(ptr, name)
}

#[inline]
fn box_get_bits_named(ptr: *mut Box, name: f64) -> i64 {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: production services see these in tight bursts of
            // 3 synced with normal request handling and the operator can't
            // tell whether anything is wrong. The path is correctness-safe
            // (we already return a defined value to the caller); gate the
            // diagnostic behind `PERRY_DEBUG=1` so it only surfaces during
            // bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            // perry#4926: with codegen entry-initializing boxed slots to
            // TAG_UNDEFINED, this arm is the read-before-initialization
            // path for a boxed variable — in JS that reads as `undefined`
            // (Perry has no TDZ), not as the number NaN. TAG_UNDEFINED is
            // itself a quiet-NaN bit pattern, so numeric consumers behave
            // exactly as before; JS-level checks (`typeof`, `== null`)
            // now see `undefined`.
            return crate::value::TAG_UNDEFINED as i64;
        }
        let bits = (*ptr).value;
        // Temporal Dead Zone: a lexical `let`/`const`/`class` box seeded with
        // the TAG_TDZ sentinel at scope entry throws a spec ReferenceError when
        // read before its declaration runs (which overwrites the sentinel with
        // a real value). TAG_TDZ is a reserved bit pattern no legitimate value
        // ever holds, so this branch is only ever taken on a genuine
        // read-before-initialization — making the check zero-regression for
        // every already-initialized box. The name is passed as `undefined`
        // because this choke point is name-agnostic (it serves direct,
        // closure-captured, and compound reads alike); the resulting message is
        // the spec-generic form.
        if bits == crate::value::TAG_TDZ {
            // #6044 regression (#6052): Perry-internal materialization reads —
            // the class-capture decl-site snapshot refreshes emitted after EACH
            // captured var's assignment (`RegisterClassCaptures`, the #6037
            // refresh strategy) — legally observe sibling captures that are
            // still in their dead zone (`const _fs = ..; <refresh reads _path>;
            // const _path = ..`, the SWC CJS interop shape). Those are not user
            // reads: pre-TDZ they snapshotted `undefined` and the next refresh
            // fixed the value up. Inside the codegen-bracketed suppression
            // window, keep exactly that behavior instead of throwing.
            if TDZ_SUPPRESS_DEPTH.with(|d| d.get()) > 0 {
                return crate::value::TAG_UNDEFINED as i64;
            }
            crate::error::js_throw_reference_error_tdz(name);
        }
        bits as i64
    }
}

/// Immutable fallback for a missing capture. Generated code must root every
/// real cell address across collection points; the fallback itself never moves.
static BOX_CAPTURE_UNDEFINED_CELL: Box = Box {
    value: crate::value::TAG_UNDEFINED,
};

/// Resolve a live GC box or the immutable undefined fallback. The returned
/// address must be rooted/reloaded across a collection point.
#[no_mangle]
pub extern "C" fn js_box_capture_cell_ptr(bits: i64) -> i64 {
    let ptr = bits as usize as *mut Box;
    if is_registered_box_ptr(ptr) {
        bits
    } else {
        &BOX_CAPTURE_UNDEFINED_CELL as *const Box as i64
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_box_get_bits_trusted(ptr: *mut Box) -> i64 {
    unsafe { js_box_get_bits_trusted_named(ptr, f64::from_bits(crate::value::TAG_UNDEFINED)) }
}

/// Named counterpart of `js_box_get_bits_trusted`.
///
/// # Safety
/// `ptr` must be a live box cell, as for `js_box_get_bits_trusted`.
#[no_mangle]
pub unsafe extern "C" fn js_box_get_bits_trusted_named(ptr: *mut Box, name: f64) -> i64 {
    let bits = unsafe { (*ptr).value };
    if bits == crate::value::TAG_TDZ {
        if TDZ_SUPPRESS_DEPTH.with(|d| d.get()) > 0 {
            return crate::value::TAG_UNDEFINED as i64;
        }
        crate::error::js_throw_reference_error_tdz(name);
    }
    bits as i64
}

crate::perry_thread_local! {
    /// #6052: >0 while codegen-emitted Perry-internal materialization reads
    /// (the `RegisterClassCaptures` decl-site snapshot refresh) are running —
    /// a dead-zone box then reads as `undefined` (pre-#6044 behavior) instead
    /// of throwing. Never spans user code: the bracketed window contains only
    /// side-effect-free capture loads.
    static TDZ_SUPPRESS_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Enter a TDZ-suppression window (see `TDZ_SUPPRESS_DEPTH`). Emitted by
/// codegen immediately before a `RegisterClassCaptures` snapshot's capture
/// loads; paired with `js_tdz_suppress_end`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_begin() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

/// Leave the TDZ-suppression window opened by `js_tdz_suppress_begin`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_end() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

/// Keepalive anchors for the auto-optimize whole-program build (generated-code-
/// only callees — without these the symbols dead-strip and the app link fails).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_TDZ_SUPPRESS_BEGIN: extern "C" fn() = js_tdz_suppress_begin;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_TDZ_SUPPRESS_END: extern "C" fn() = js_tdz_suppress_end;

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    f64::from_bits(js_box_get_bits(ptr) as u64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_get(ptr: *mut I32Box) -> i32 {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        (*ptr).value
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_get(ptr: *mut BoolBox) -> i32 {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        i32::from((*ptr).value)
    }
}

/// Set the raw JSValue bit pattern in a box.
///
/// Robust against bogus pointers: in addition to the null check, we
/// reject obviously-invalid pointers (below the first user page or
/// above the 48-bit user-address ceiling) and pointers that aren't
/// 8-byte aligned. This avoids SIGSEGV on `(*ptr).value = value` when
/// upstream codegen hands us a stale/uninitialized slot — a known
/// failure mode for closure prologues at hub-scale (perry#393).
/// Boxes are heap-allocated 8-byte JSValue bit slots; a non-aligned or low/high
/// pointer is definitely wrong, so a silent skip + telemetry warning
/// is strictly safer than dereferencing it.
#[no_mangle]
pub extern "C" fn js_box_set_bits(ptr: *mut Box, value_bits: i64) {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: silent-skip is correctness-safe (caller's box
            // mutation is dropped, which is the same as no closure
            // capture having existed). Gate diagnostics behind
            // `PERRY_DEBUG=1` to keep production stderr clean.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_set: invalid box pointer {:p} #{} (value bits: 0x{:016x})",
                        ptr,
                        count,
                        value_bits as u64
                    );
                }
            }
            return;
        }
        let bits = value_bits as u64;
        (*ptr).value = bits;
        crate::gc::runtime_write_barrier_slot(ptr as usize, ptr as usize, bits);
    }
}

/// Raw store paired with [`js_box_get_bits_trusted`]. The generated caller
/// immediately emits the ordinary child-shading write barrier after this
/// store, so this helper deliberately performs only the cell write. Public
/// closure bodies retain the validating, self-barriering setter.
///
/// # Safety
///
/// `ptr` must be non-null and must name a live Perry box cell whose capture
/// edge remains live for the duration of the call. If `value_bits` may name a
/// GC object, the caller must shade it against this box before any operation
/// that can collect.
#[no_mangle]
pub unsafe extern "C" fn js_box_set_bits_trusted_no_barrier(ptr: *mut Box, value_bits: i64) {
    unsafe {
        (*ptr).value = value_bits as u64;
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    js_box_set_bits(ptr, value.to_bits() as i64);
}

#[no_mangle]
pub extern "C" fn js_i32_box_set(ptr: *mut I32Box, value: i32) {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value;
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_set(ptr: *mut BoolBox, value: i32) {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value != 0;
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_ALLOC_BITS: extern "C" fn(i64) -> *mut Box = js_box_alloc_bits;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_RELEASE: extern "C" fn(*mut Box) = js_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_RELEASE: extern "C" fn(*mut I32Box) = js_i32_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_RELEASE: extern "C" fn(*mut BoolBox) = js_bool_box_release;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_GET_BITS: extern "C" fn(*mut Box) -> i64 = js_box_get_bits;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_SET_BITS: extern "C" fn(*mut Box, i64) = js_box_set_bits;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_GET_BITS_TRUSTED: unsafe extern "C" fn(*mut Box) -> i64 =
    js_box_get_bits_trusted;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_GET_BITS_NAMED: extern "C" fn(*mut Box, f64) -> i64 = js_box_get_bits_named;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_GET_BITS_TRUSTED_NAMED: unsafe extern "C" fn(*mut Box, f64) -> i64 =
    js_box_get_bits_trusted_named;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_SET_BITS_TRUSTED_NO_BARRIER: unsafe extern "C" fn(*mut Box, i64) =
    js_box_set_bits_trusted_no_barrier;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_ALLOC: extern "C" fn(f64) -> *mut Box = js_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_GET: extern "C" fn(*mut Box) -> f64 = js_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_SET: extern "C" fn(*mut Box, f64) = js_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_ALLOC: extern "C" fn(i32) -> *mut I32Box = js_i32_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_GET: extern "C" fn(*mut I32Box) -> i32 = js_i32_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_SET: extern "C" fn(*mut I32Box, i32) = js_i32_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_ALLOC: extern "C" fn(i32) -> *mut BoolBox = js_bool_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_GET: extern "C" fn(*mut BoolBox) -> i32 = js_bool_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_SET: extern "C" fn(*mut BoolBox, i32) = js_bool_box_set;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_SCOPE_RELEASE: extern "C" fn(*mut Box) = js_box_scope_release;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_SCOPE_RELEASE: extern "C" fn(*mut I32Box) = js_i32_box_scope_release;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_SCOPE_RELEASE: extern "C" fn(*mut BoolBox) = js_bool_box_scope_release;

#[cfg(test)]
#[path = "box/tests.rs"]
mod tests;
