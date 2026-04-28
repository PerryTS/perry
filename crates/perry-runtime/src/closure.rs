//! Closure runtime support for Perry
//!
//! A closure is a function pointer plus captured environment.
//! Layout:
//!   - ClosureHeader at the start
//!   - Followed by captured values (as f64 or i64 pointers)

/// Magic value stored in ClosureHeader._reserved to identify closures at runtime.
/// Used by js_value_typeof to return "function" instead of "object" for closures.
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53; // "CLOS" in ASCII

/// Sentinel func_ptr value indicating this closure is a "bound method" on a native module.
/// When js_closure_callN detects this, it extracts captures and dispatches via js_native_call_method.
/// Captures layout: [0] = namespace_obj (f64), [1] = method_name_ptr (i64), [2] = method_name_len (i64)
pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;

/// Flag stored in the high bit of capture_count to indicate that capture slot 0
/// holds `this` (i.e., this closure is an object literal method that captures `this`).
/// When the closure is detached from the object (assigned to a variable via PropertyGet),
/// `js_closure_unbind_this` clones it and clears slot 0 so `this` becomes undefined.
pub const CAPTURES_THIS_FLAG: u32 = 0x8000_0000;

/// Extract the real capture count (masking out the CAPTURES_THIS_FLAG).
#[inline(always)]
pub fn real_capture_count(capture_count: u32) -> u32 {
    capture_count & !CAPTURES_THIS_FLAG
}

/// Header for heap-allocated closures
#[repr(C)]
pub struct ClosureHeader {
    /// Function pointer (the actual compiled function)
    pub func_ptr: *const u8,
    /// Number of captured values
    pub capture_count: u32,
    /// Type tag: set to CLOSURE_MAGIC to identify closures at runtime
    pub type_tag: u32,
}

/// Allocate a closure with space for captured values.
/// The high bit of `capture_count` may contain CAPTURES_THIS_FLAG to indicate
/// that slot 0 is reserved for `this`. The flag is preserved in the header
/// for later use by `js_closure_unbind_this`, but the actual allocation size
/// uses only the lower 31 bits.
/// Returns pointer to ClosureHeader
#[no_mangle]
pub extern "C" fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader {
    let actual_count = real_capture_count(capture_count) as usize;
    let captures_size = actual_count * 8; // Each capture is 8 bytes (f64 or i64)
    let total_size = std::mem::size_of::<ClosureHeader>() + captures_size;

    let raw = crate::gc::gc_malloc(total_size, crate::gc::GC_TYPE_CLOSURE);
    let ptr = raw as *mut ClosureHeader;

    unsafe {
        (*ptr).func_ptr = func_ptr;
        (*ptr).capture_count = capture_count; // Preserve flag in high bit
        (*ptr).type_tag = CLOSURE_MAGIC;
    }

    ptr
}

/// Get the function pointer from a closure
#[no_mangle]
pub extern "C" fn js_closure_get_func(closure: *const ClosureHeader) -> *const u8 {
    unsafe { (*closure).func_ptr }
}

/// Get a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64 {
    if closure.is_null() { return 0.0; }
    unsafe {
        let captures_ptr = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64) {
    if closure.is_null() { return; }
    unsafe {
        let captures_ptr = (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        *captures_ptr.add(index as usize) = value;
    }
}

/// Get a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64 {
    if closure.is_null() { return 0; }
    unsafe {
        let captures_ptr = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const i64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64) {
    if closure.is_null() { return; }
    unsafe {
        let captures_ptr = (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut i64;
        *captures_ptr.add(index as usize) = value;
    }
}

/// Dispatch a bound method call with the given arguments.
/// Extracts the namespace object and method name from the closure captures,
/// then calls js_native_call_method with the packed arguments.
#[inline]
unsafe fn dispatch_bound_method(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let namespace_obj = js_closure_get_capture_f64(closure, 0);
    let method_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const i8;
    let method_name_len = js_closure_get_capture_ptr(closure, 2) as usize;
    crate::object::js_native_call_method(
        namespace_obj,
        method_name_ptr,
        method_name_len,
        args.as_ptr(),
        args.len(),
    )
}

/// Validate a closure pointer and return its func_ptr if the closure is valid.
///
/// Uses `read_volatile` for type_tag + `compiler_fence` to GUARANTEE that:
/// 1. CLOSURE_MAGIC is checked BEFORE func_ptr is ever read
/// 2. The optimizer cannot hoist the func_ptr read before the type_tag check
///
/// Background: `#[inline(never)]` on `is_valid_closure_ptr` is insufficient — LLVM
/// still speculatively hoists the non-volatile func_ptr load before the CLOSURE_MAGIC
/// check in the caller. This produces code that only checks CLOSURE_MAGIC when func_ptr==0,
/// allowing non-closure heap objects (Box<JSValue>, BigInt structs) to bypass validation
/// and execute their data as code via `br x1` → SIGBUS.
///
/// Returns null pointer if invalid (address out of range, wrong CLOSURE_MAGIC, bad func_ptr).
#[inline(always)]
fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
    let addr = closure as u64;
    if addr < 0x1000 || addr >= 0x0001_0000_0000_0000 {
        return std::ptr::null();
    }
    let type_tag = unsafe {
        std::ptr::read_volatile((closure as *const u8).add(12) as *const u32)
    };
    if type_tag != CLOSURE_MAGIC {
        return std::ptr::null();
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let func_ptr = unsafe {
        std::ptr::read_volatile(closure as *const *const u8)
    };
    let func_ptr_addr = func_ptr as usize;
    if func_ptr_addr == 0 {
        return std::ptr::null();
    }
    // Validate func_ptr is in a reasonable code address range.
    // macOS ARM64: .text starts at 0x100000000, typically < 0x400000000
    // Windows x86_64: typically 0x7FF7_xxxx_xxxx (ASLR), so we allow up to 0x8000_0000_0000
    // Linux x86_64 PIE: .text is typically in 0x55xxxxxxxxxx range
    // Skip this check on Linux since PIE addresses vary widely and CLOSURE_MAGIC
    // already provides strong validation.
    #[cfg(target_os = "macos")]
    if func_ptr_addr < 0x100000000 || func_ptr_addr > 0x400000000 {
        return std::ptr::null();
    }
    #[cfg(target_os = "windows")]
    if func_ptr_addr < 0x10000 || func_ptr_addr > 0x800000000000 {
        return std::ptr::null();
    }
    func_ptr
}

/// Call a closure with 0 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[]) };
    }
    let func: extern "C" fn(*const ClosureHeader) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure)
}

/// Call a closure with 1 argument, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0)
}

/// Call a closure with 2 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1)
}

/// Call a closure with 3 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call3(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2)
}

/// Call a closure with 4 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call4(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3)
}

/// Call a closure with 5 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call5(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4)
}

/// Call a closure with 6 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call6(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5)
}

/// Call a closure with 7 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call7(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6)
}

/// Call a closure with 8 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call8(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
}

/// Call a closure with 9 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call9(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)
}

/// Call a closure with 10 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call10(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9)
}

/// Call a closure with 11 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call11(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10)
}

/// Call a closure with 12 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call12(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11)
}

/// Call a closure with 13 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call13(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12)
}

/// Call a closure with 14 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call14(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13)
}

/// Call a closure with 15 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call15(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64, arg14: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14)
}

/// Call a closure with 16 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call16(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64, arg3: f64, arg4: f64, arg5: f64, arg6: f64, arg7: f64, arg8: f64, arg9: f64, arg10: f64, arg11: f64, arg12: f64, arg13: f64, arg14: f64, arg15: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14, arg15]) };
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13, arg14, arg15)
}

/// Call a JavaScript function value with variable arguments
/// This is the native implementation for dynamic function dispatch.
/// func_value: NaN-boxed f64 containing a closure pointer
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_value to avoid symbol collision
/// with js_call_value in perry-jsruntime which handles V8 JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let jsval = JSValue::from_bits(func_value.to_bits());

    // Get the closure pointer from the value
    // For native compilation, function values are stored as NaN-boxed pointers
    let closure: *const ClosureHeader = if jsval.is_pointer() {
        jsval.as_pointer()
    } else if jsval.is_undefined() || jsval.is_null() || func_value.is_nan() {
        // TAG_UNDEFINED, TAG_NULL, or other NaN values are not callable
        return f64::from_bits(JSValue::undefined().bits());
    } else {
        // Try treating the value directly as a pointer (for i64 representation)
        func_value.to_bits() as *const ClosureHeader
    };

    if closure.is_null() {
        // Return undefined for null/invalid closures
        return f64::from_bits(JSValue::undefined().bits());
    }

    // Call with the appropriate arity
    match args_len {
        0 => js_closure_call0(closure),
        1 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            js_closure_call1(closure, arg0)
        }
        2 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            js_closure_call2(closure, arg0, arg1)
        }
        3 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            js_closure_call3(closure, arg0, arg1, arg2)
        }
        4 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            js_closure_call4(closure, arg0, arg1, arg2, arg3)
        }
        5 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            js_closure_call5(closure, arg0, arg1, arg2, arg3, arg4)
        }
        6 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            js_closure_call6(closure, arg0, arg1, arg2, arg3, arg4, arg5)
        }
        7 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            let arg6 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(6) };
            js_closure_call7(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6)
        }
        8 => {
            let arg0 = if args_ptr.is_null() { 0.0 } else { *args_ptr };
            let arg1 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(1) };
            let arg2 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(2) };
            let arg3 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(3) };
            let arg4 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(4) };
            let arg5 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(5) };
            let arg6 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(6) };
            let arg7 = if args_ptr.is_null() { 0.0 } else { *args_ptr.add(7) };
            js_closure_call8(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
        }
        9 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call9(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8))
        }
        10 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call10(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9))
        }
        11 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call11(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10))
        }
        12 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call12(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11))
        }
        13 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call13(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12))
        }
        14 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call14(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13))
        }
        15 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call15(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14))
        }
        16 => {
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call16(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14), a(15))
        }
        _ => {
            eprintln!("Warning: js_native_call_value called with {} args, only supporting up to 16", args_len);
            let a = |i: usize| if args_ptr.is_null() { 0.0 } else { *args_ptr.add(i) };
            js_closure_call16(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14), a(15))
        }
    }
}

use std::collections::HashMap;
use std::sync::{OnceLock, Mutex};

static CLOSURE_PROPS: OnceLock<Mutex<HashMap<usize, HashMap<String, f64>>>> = OnceLock::new();

fn get_closure_props() -> &'static Mutex<HashMap<usize, HashMap<String, f64>>> {
    CLOSURE_PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Check if a raw pointer points to a ClosureHeader by checking CLOSURE_MAGIC at offset 12.
/// Safe to call with any non-null, sufficiently aligned pointer >= 0x10000.
pub fn is_closure_ptr(ptr: usize) -> bool {
    if ptr < 0x10000 {
        return false;
    }
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == CLOSURE_MAGIC
    }
}

/// Get a dynamic property stored on a closure.
/// Returns TAG_UNDEFINED if not found.
pub fn closure_get_dynamic_prop(ptr: usize, prop: &str) -> f64 {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get(&ptr) {
            if let Some(&val) = closure_props.get(prop) {
                return val;
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Set a dynamic property on a closure.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if let Ok(mut props) = get_closure_props().lock() {
        props.entry(ptr).or_insert_with(HashMap::new).insert(prop.to_string(), value);
    }
}

/// Unbind `this` from a detached method closure.
///
/// When a method is read from an object via PropertyGet (e.g., `const fn = holder.getX`),
/// this function is called on the result. If the value is a closure whose capture_count
/// has CAPTURES_THIS_FLAG set (indicating slot 0 is `this`), it allocates a new closure
/// with the same func_ptr and captures but slot 0 set to undefined.
///
/// For non-closure values (numbers, strings, objects, arrays), this is a no-op.
#[no_mangle]
pub extern "C" fn js_closure_unbind_this(val: f64) -> f64 {
    let bits = val.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // Only process POINTER_TAG values (closures are NaN-boxed with POINTER_TAG)
    if tag != 0x7FFD_0000_0000_0000 {
        return val;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return val;
    }
    // Check CLOSURE_MAGIC
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return val;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // Only unbind if the closure has the CAPTURES_THIS_FLAG
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return val;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return val;
        }
        // Clone the closure with slot 0 set to undefined
        let new_closure = js_closure_alloc((*header).func_ptr, raw_count);
        let src_captures = (ptr as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        let dst_captures = (new_closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        // Set slot 0 to undefined
        *dst_captures = f64::from_bits(crate::value::TAG_UNDEFINED);
        // Copy remaining captures (slots 1..count)
        for i in 1..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        // NaN-box the new closure pointer
        let new_ptr = new_closure as u64;
        f64::from_bits(0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF))
    }
}

/// Adapter for V8's `native_callback_trampoline` (perry-jsruntime).
///
/// `js_create_callback(func_ptr, closure_env, param_count)` registers a JS
/// callable whose trampoline invokes `func_ptr(closure_env, args_ptr,
/// args_len)`. Perry closure bodies have signature
/// `(closure_ptr, arg0, arg1, ...)` per arity instead, so the codegen
/// arm for `Expr::JsCreateCallback` (issue #248 Phase 2B) passes
/// `js_closure_call_array` as the trampoline `func_ptr` and the raw
/// `*const ClosureHeader` (NaN-boxing stripped) as `closure_env`. The
/// trampoline then ends up calling THIS function, which dispatches to
/// the right `js_closure_callN` per `args_len`.
///
/// Mirrors `js_native_call_value` exactly but takes an i64 closure
/// pointer (already unboxed) instead of an f64 NaN-boxed value, so the
/// SysV-x64 / Win64 first-arg register lands in rdi/rcx (integer)
/// rather than xmm0 — matching the trampoline's `extern "C"` int-arg
/// expectation.
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_array(
    closure_env: i64,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    let closure = closure_env as *const ClosureHeader;
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let n = if args_len < 0 { 0 } else { args_len as usize };
    // Perry's closure-body arithmetic uses plain `fadd`/`fmul`/etc on
    // f64 inputs and assumes its arguments arrive as plain doubles, not
    // NaN-boxed values. perry-jsruntime's `v8_to_native` (bridge.rs:215)
    // NaN-boxes JS integers with INT32_TAG=0x7FFE. If we passed those
    // bits straight through, the closure body's `fadd` would produce a
    // NaN (whose payload happens to look like one of the operands when
    // re-decoded by `console.log`'s tag-aware unbox — which is why
    // `(a, b) => a + b` with `cb(10, 20)` returned 10 instead of 30
    // pre-fix). Unbox at the dispatch boundary so the body sees a
    // plain `20.0` not the NaN-boxed `0x7FFE_0000_0000_0014`. JS
    // doubles (non-int32) already arrive as plain f64 from
    // `v8_to_native`; only the INT32_TAG case needs unboxing here.
    let a = |i: usize| {
        if args_ptr.is_null() {
            return 0.0;
        }
        let raw = *args_ptr.add(i);
        let bits = raw.to_bits();
        if (bits & 0xFFFF_0000_0000_0000) == 0x7FFE_0000_0000_0000 {
            let int_val = (bits & 0xFFFF_FFFF) as i32;
            return int_val as f64;
        }
        raw
    };
    match n {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, a(0)),
        2 => js_closure_call2(closure, a(0), a(1)),
        3 => js_closure_call3(closure, a(0), a(1), a(2)),
        4 => js_closure_call4(closure, a(0), a(1), a(2), a(3)),
        5 => js_closure_call5(closure, a(0), a(1), a(2), a(3), a(4)),
        6 => js_closure_call6(closure, a(0), a(1), a(2), a(3), a(4), a(5)),
        7 => js_closure_call7(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6)),
        8 => js_closure_call8(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7)),
        9 => js_closure_call9(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8)),
        10 => js_closure_call10(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9)),
        11 => js_closure_call11(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10)),
        12 => js_closure_call12(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11)),
        13 => js_closure_call13(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12)),
        14 => js_closure_call14(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13)),
        15 => js_closure_call15(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8), a(9), a(10), a(11), a(12), a(13), a(14)),
        _ => js_closure_call16(
            closure,
            a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7),
            a(8), a(9), a(10), a(11), a(12), a(13), a(14), a(15),
        ),
    }
}

/// V8 interop no-op stubs. Real implementations are in perry-jsruntime/src/interop.rs.
/// These stubs ensure symbols are always available even when perry-jsruntime is not linked
/// (iOS, Android, standalone builds). When perry-jsruntime IS linked, its strong symbols
/// override these stubs via linker symbol resolution order.
#[no_mangle] pub extern "C" fn js_new_from_handle(_constructor: f64, _args_ptr: i64, _args_len: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_call_function(_func: f64, _args_ptr: i64, _args_len: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_load_module(_path_ptr: i64, _path_len: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_new_instance(_class_ptr: i64, _args_ptr: i64, _args_len: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_create_callback(_func_ptr: i64, _closure_env: i64, _param_count: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_set_property(_obj: f64, _key_ptr: i64, _key_len: i64, _value: f64) {}
#[no_mangle] pub extern "C" fn js_get_export(_module: f64, _name_ptr: i64, _name_len: i64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_await_js_promise(_promise: f64) -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_runtime_init() {}

// =============================================================================
// AOT stubs for unconditionally-declared extern functions
// =============================================================================

#[no_mangle] pub extern "C" fn js_ratelimit_create() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_lodash_ends_with() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_escape() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_includes() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_lower_first() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_replace() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_split() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_start_case() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_starts_with() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_unescape() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_lodash_upper_first() -> f64 { 0.0 }
#[no_mangle] pub extern "C" fn js_axios_create() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_axios_request() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_argon2_hash_options() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_negate() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_quality() -> i64 { 0 }
#[no_mangle] pub extern "C" fn js_sharp_to_format() -> i64 { 0 }
// js_sqlite_transaction / _commit / _rollback stubs removed — the real
// implementations live in perry-stdlib/src/sqlite.rs and would collide at
// link time when both crates are present (e.g. `cargo test --workspace`).
#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn test_closure_func(closure: *const ClosureHeader) -> f64 {
        unsafe {
            let captured = js_closure_get_capture_f64(closure, 0);
            captured * 2.0
        }
    }

    #[test]
    fn test_closure_basic() {
        let closure = js_closure_alloc(test_closure_func as *const u8, 1);
        js_closure_set_capture_f64(closure, 0, 21.0);
        let result = js_closure_call0(closure);
        assert_eq!(result, 42.0);
    }
}
