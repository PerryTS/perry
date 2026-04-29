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
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    unsafe {
        if ptr.is_null() {
            let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
            if count < 3 {
                eprintln!("[PERRY WARN] js_box_get: null box pointer #{}", count);
            }
            return f64::NAN;
        }
        (*ptr).value
    }
}

/// Set the value in a box
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    unsafe {
        if ptr.is_null() {
            let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
            if count < 3 {
                eprintln!(
                    "[PERRY WARN] js_box_set: null box pointer #{} (value bits: 0x{:016x})",
                    count,
                    value.to_bits()
                );
            }
            return;
        }
        (*ptr).value = value;
    }
}
