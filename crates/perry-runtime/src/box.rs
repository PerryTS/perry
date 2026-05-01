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

/// Allocate a new box with an initial value
#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    unsafe {
        let layout = Layout::new::<Box>();
        let ptr = alloc(layout) as *mut Box;
        if ptr.is_null() {
            eprintln!("[PERRY WARN] js_box_alloc: allocation failed — returning null");
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value;
        ptr
    }
}

/// Get the value from a box
///
/// Same robustness as `js_box_set`: invalid pointers return NaN
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    unsafe {
        if !is_plausible_box_ptr(ptr) {
            let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
            if count < 3 {
                eprintln!(
                    "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                    ptr, count
                );
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
            let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
            if count < 3 {
                eprintln!(
                    "[PERRY WARN] js_box_set: invalid box pointer {:p} #{} (value bits: 0x{:016x})",
                    ptr,
                    count,
                    value.to_bits()
                );
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
