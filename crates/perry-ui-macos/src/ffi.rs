//! Canonical `extern "C"` declarations shared across this crate.
//!
//! These symbols were previously re-declared per-file with divergent signatures,
//! triggering ~44 `clashing_extern_declarations` warnings — and, for
//! `js_string_from_bytes`, a real ABI mismatch (`len: i64` vs the runtime's
//! `len: u32`) that only worked by register luck. Declare each exactly once here,
//! matching perry-runtime's real ABI (or the system ABI for objc/CoreGraphics),
//! and have every call site `use crate::ffi::*`.

use std::ffi::c_void;

use crate::string_header::StringHeader;

// ---------------------------------------------------------------------------
// perry-runtime symbols
// ---------------------------------------------------------------------------

extern "C" {
    /// Canonical: perry-runtime `js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader`.
    pub fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;

    /// Canonical: perry-runtime returns the array header pointer. Opaque here.
    pub fn js_array_push_f64(arr: *mut c_void, value: f64) -> *mut c_void;

    /// Canonical: perry-runtime `js_get_string_pointer_unified(value: f64) -> i64`.
    pub fn js_get_string_pointer_unified(value: f64) -> i64;
}

// ---------------------------------------------------------------------------
// Objective-C runtime symbols (system ABI)
// ---------------------------------------------------------------------------

extern "C" {
    /// objc `sel_registerName(const char *) -> SEL`. SEL is an opaque pointer.
    pub fn sel_registerName(name: *const i8) -> *const c_void;

    /// objc `class_addMethod(Class, SEL, IMP, const char *types) -> BOOL`.
    /// `imp` and `sel` are opaque pointers; pass function pointers via `as *const c_void`.
    pub fn class_addMethod(
        cls: *mut c_void,
        sel: *const c_void,
        imp: *const c_void,
        types: *const i8,
    ) -> bool;
}

// ---------------------------------------------------------------------------
// CoreGraphics symbols (system ABI)
//
// The context is an opaque `CGContextRef` (`*mut c_void`); pass any objc
// `*mut AnyObject` handle via `as *mut c_void`. Only the functions declared by
// more than one file live here — file-specific ones (gradients, image drawing,
// arcs) stay local to their single caller.
// ---------------------------------------------------------------------------

extern "C" {
    pub fn CGContextSetRGBFillColor(c: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextSetRGBStrokeColor(c: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextSetLineWidth(c: *mut c_void, width: f64);
    pub fn CGContextFillRect(c: *mut c_void, rect: objc2_core_foundation::CGRect);
    pub fn CGContextStrokeRect(c: *mut c_void, rect: objc2_core_foundation::CGRect);
    pub fn CGContextBeginPath(c: *mut c_void);
    pub fn CGContextMoveToPoint(c: *mut c_void, x: f64, y: f64);
    pub fn CGContextAddLineToPoint(c: *mut c_void, x: f64, y: f64);
    pub fn CGContextStrokePath(c: *mut c_void);
    pub fn CGContextFillPath(c: *mut c_void);
    pub fn CGContextClosePath(c: *mut c_void);
}
