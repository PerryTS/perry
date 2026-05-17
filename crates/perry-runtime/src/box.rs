//! Box runtime for mutable captured variables
//!
//! When a closure captures a variable that is modified (either in the closure
//! or in the outer scope), we need to store it in a heap-allocated "box" so
//! both scopes share the same storage location.

use std::alloc::{alloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

static BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);

/// A box is simply a heap-allocated f64
#[repr(C)]
pub struct Box {
    pub value: f64,
}

thread_local! {
    /// Registry of every active box pointer. GC traces the contained
    /// f64 value so that NaN-boxed heap pointers stored in boxes (e.g.
    /// the generator state machine's iter object held in `__iter`'s
    /// mutable-capture box) keep the referenced heap object alive
    /// across collections. Without this, captures stored as raw box
    /// pointers in closure capture slots fail the `valid_ptrs.contains`
    /// check during `trace_closure` (boxes come from `std::alloc::alloc`
    /// directly, not the GC arena), so the box pointer is never marked
    /// AND the f64 value inside is never scanned — heap objects
    /// referenced only through box-captures can be swept mid-await.
    pub(crate) static BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        // Pre-size for promise-heavy workloads: `promise_all_chains`
        // allocates ~150 k boxes per kernel run (one per closure
        // mutable capture). Starting at 128 k buckets (~2 MB) covers
        // the full working set in one alloc — without it, hashbrown
        // rehashes from 0 → 256 k buckets across the alloc history,
        // showing up as ~3 % CPU in `hash_one` / `reserve_rehash`.
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            128 * 1024,
            crate::fast_hash::PtrHasher,
        ));
}

/// Allocate a new box with an initial value
#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    unsafe {
        let layout = Layout::new::<Box>();
        let ptr = alloc(layout) as *mut Box;
        if ptr.is_null() {
            // perry#924: oom is rare enough that operators see the
            // downstream crash and react to that; keep the diagnostic
            // available under `PERRY_DEBUG=1` for bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value;
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        ptr
    }
}

/// GC root scanner: walk every registered box and `mark` the f64
/// value inside. Heap pointers stored inside boxes (e.g. the generator
/// state machine's iter object held in a mutable-capture box) must be
/// kept alive across collections. The box pointer itself is _not_ a
/// heap value the runtime tracks — `BOX_REGISTRY` is the source of
/// truth for "every live box right now" — so we use the standard root
/// scanner protocol: dispatch every stored f64 to `mark` and let the
/// GC trace into it.
pub fn scan_box_roots(mark: &mut dyn FnMut(f64)) {
    BOX_REGISTRY.with(|r| {
        let r = r.borrow();
        for &addr in r.iter() {
            let ptr = addr as *const Box;
            // Defensive: the registry should only contain valid live
            // pointers, but if a stale entry slipped through we'd
            // segfault on the deref. The tight bounds check on the
            // address (alloc gives 8-aligned pointers in user space)
            // matches `is_plausible_box_ptr` to keep this a no-op for
            // any pathological entry.
            if addr >= 0x1000 && addr < 0x0001_0000_0000_0000 && addr % 8 == 0 {
                let v = unsafe { (*ptr).value };
                mark(v);
            }
        }
    });
}

/// Get the value from a box
///
/// Same robustness as `js_box_set`: invalid pointers return NaN
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    unsafe {
        if !is_plausible_box_ptr(ptr) {
            // perry#924: production services see these in tight bursts of
            // 3 synced with normal request handling and the operator can't
            // tell whether anything is wrong. The path is correctness-safe
            // (we already return NaN to the caller); gate the diagnostic
            // behind `PERRY_DEBUG=1` so it only surfaces during bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return f64::NAN;
        }
        (*ptr).value
    }
}

/// Set the value in a box
///
/// Robust against bogus pointers: in addition to the null check, we
/// reject obviously-invalid pointers (below the first user page or
/// above the 48-bit user-address ceiling) and pointers that aren't
/// 8-byte aligned. This avoids SIGSEGV on `(*ptr).value = value` when
/// upstream codegen hands us a stale/uninitialized slot — a known
/// failure mode for closure prologues at hub-scale (perry#393).
/// Boxes are heap-allocated 8-byte f64s; a non-aligned or low/high
/// pointer is definitely wrong, so a silent skip + telemetry warning
/// is strictly safer than dereferencing it.
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    unsafe {
        if !is_plausible_box_ptr(ptr) {
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
                        value.to_bits()
                    );
                }
            }
            return;
        }
        (*ptr).value = value;
    }
}

/// Cheap pointer-sanity test — same threat model as `get_valid_func_ptr`
/// in closure.rs, adapted for box-shaped allocations.
///
/// A `*mut Box` from `js_box_alloc` is a Rust-`alloc()` heap pointer,
/// which on x86_64 Linux/macOS lives in the 47-bit user-address half
/// of the address space and (because `Layout::new::<Box>()` yields
/// `align = 8`) is 8-byte aligned. Pointers below the first user page
/// or above the user-address ceiling, or unaligned ones, can only come
/// from stale/uninitialized stack slots reinterpreted as box pointers.
#[inline]
fn is_plausible_box_ptr(ptr: *mut Box) -> bool {
    let addr = ptr as usize;
    if addr == 0 {
        return false;
    }
    if addr < 0x1000 {
        return false;
    }
    if addr >= 0x0001_0000_0000_0000 {
        return false;
    }
    if addr % std::mem::align_of::<Box>() != 0 {
        return false;
    }
    true
}
