//! Exception handling runtime for Perry
//!
//! Uses setjmp/longjmp for exception unwinding.
//! The key insight is that setjmp must be called directly from the generated code,
//! not from inside a Rust function (because the stack frame would be invalid when longjmp returns).

// Platform-specific jmp_buf size (in i32 units)
// macOS ARM64: _JBLEN = 48 (48 * 4 = 192 bytes)
// macOS x86_64: _JBLEN = 37 (37 * 4 = 148 bytes, but aligned to 156)
// Linux x86_64: __jmp_buf is 8 * i64 = 64 bytes
// Windows MSVC x86_64: _JBLEN = 16 doubles = 256 bytes
// We use a conservative size that works for all
const JMP_BUF_SIZE: usize = 64; // 64 * i32 = 256 bytes, enough for any platform

// jmp_buf must be properly aligned
#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct JmpBuf {
    data: [i32; JMP_BUF_SIZE],
}

impl JmpBuf {
    const fn new() -> Self {
        JmpBuf {
            data: [0; JMP_BUF_SIZE],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut i32 {
        self.data.as_mut_ptr()
    }
}

extern "C" {
    fn longjmp(env: *mut i32, val: i32) -> !;
}

// Maximum nesting depth for try blocks
const MAX_TRY_DEPTH: usize = 128;

/// Per-thread exception state. Exception handling uses setjmp/longjmp,
/// and a jmp_buf captured by setjmp on thread A is meaningless on thread
/// B (its stack frame doesn't exist there) — so the buffers, the depth
/// counter, the current exception, and the finally-flag all have to
/// live in TLS once `perry/thread` workers can run user code that
/// throws. Previously all five were process-wide `static mut`s and would
/// corrupt under any concurrent throw.
struct ExceptionState {
    jump_buffers: [JmpBuf; MAX_TRY_DEPTH],
    try_depth: usize,
    current_exception: f64,
    has_exception: bool,
    in_finally: bool,
}

impl ExceptionState {
    const fn new() -> Self {
        ExceptionState {
            jump_buffers: [JmpBuf::new(); MAX_TRY_DEPTH],
            try_depth: 0,
            current_exception: 0.0,
            has_exception: false,
            in_finally: false,
        }
    }
}

thread_local! {
    static EXCEPTION_STATE: std::cell::UnsafeCell<ExceptionState> =
        const { std::cell::UnsafeCell::new(ExceptionState::new()) };
}

#[inline]
fn with_exception_state<R>(f: impl FnOnce(*mut ExceptionState) -> R) -> R {
    EXCEPTION_STATE.with(|c| f(c.get()))
}

/// Push a new try block and return a pointer to its jmp_buf.
/// The generated code must call setjmp() directly with this pointer.
#[no_mangle]
pub extern "C" fn js_try_push() -> *mut i32 {
    with_exception_state(|s| unsafe {
        if (*s).try_depth >= MAX_TRY_DEPTH {
            panic!("Try block nesting too deep");
        }
        let depth = (*s).try_depth;
        (*s).try_depth += 1;
        (*s).jump_buffers[depth].as_mut_ptr()
    })
}

/// End a try block (just decrements depth, does NOT clear exception)
/// The exception is cleared explicitly by js_clear_exception() in catch blocks
#[no_mangle]
pub extern "C" fn js_try_end() {
    with_exception_state(|s| unsafe {
        (*s).try_depth = (*s).try_depth.saturating_sub(1);
    });
}

/// Throw an exception with the given value
#[no_mangle]
pub extern "C" fn js_throw(value: f64) -> ! {
    // Pull the jmp_buf pointer out under the TLS borrow, then drop the
    // borrow before calling longjmp (longjmp doesn't return, so leaving
    // the TLS access "open" would leave the cell permanently borrowed
    // on this thread; in practice UnsafeCell tolerates it but the
    // shorter scope keeps things tidy).
    let jb_ptr: *mut i32 = with_exception_state(|s| unsafe {
        (*s).current_exception = value;
        (*s).has_exception = true;

        if (*s).in_finally {
            eprintln!("Cannot throw during finally block");
            std::process::abort();
        }

        if (*s).try_depth == 0 {
            print_uncaught(value);
            std::process::exit(1);
        }

        let depth = (*s).try_depth - 1;
        (*s).jump_buffers[depth].as_mut_ptr()
    });
    unsafe { longjmp(jb_ptr, 1) }
}

/// Get the current exception value
#[no_mangle]
pub extern "C" fn js_get_exception() -> f64 {
    with_exception_state(|s| unsafe { (*s).current_exception })
}

/// Check if there's an active exception
#[no_mangle]
pub extern "C" fn js_has_exception() -> i32 {
    with_exception_state(|s| unsafe {
        if (*s).has_exception {
            1
        } else {
            0
        }
    })
}

/// Clear the current exception
#[no_mangle]
pub extern "C" fn js_clear_exception() {
    with_exception_state(|s| unsafe {
        (*s).has_exception = false;
        (*s).current_exception = 0.0;
    });
}

/// Mark entering a finally block
#[no_mangle]
pub extern "C" fn js_enter_finally() {
    with_exception_state(|s| unsafe {
        (*s).in_finally = true;
    });
}

/// Mark leaving a finally block
#[no_mangle]
pub extern "C" fn js_leave_finally() {
    with_exception_state(|s| unsafe {
        (*s).in_finally = false;
    });
}

/// Read a StringHeader into an owned Rust String (empty on null/garbage).
unsafe fn string_header_to_string(ptr: *const crate::string::StringHeader) -> String {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    // Guard against corrupt lengths — StringHeader lengths above ~1GB
    // indicate a stale/bogus pointer (e.g. misread via a wrong tag).
    if len > 1 << 30 {
        return String::new();
    }
    let bytes_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(bytes_ptr, len))
        .unwrap_or("?")
        .to_string()
}

/// Best-effort display of a thrown value for uncaught-exception reporting.
/// Matches Node semantics roughly: Errors print `name: message` + stack,
/// regular objects probe for `.message`/`.stack`, everything else goes
/// through the generic `js_jsvalue_to_string` (which handles strings,
/// numbers, booleans, arrays, user `[Symbol.toPrimitive]`, etc.).
fn print_uncaught(value: f64) {
    let bits = value.to_bits();
    let top16 = bits >> 48;

    if top16 == 0x7FFD {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if ptr >= 0x10000 {
            let object_type = unsafe { *(ptr as *const u32) };
            if object_type == crate::error::OBJECT_TYPE_ERROR {
                // ErrorHeader: object_type, error_kind, message, name, stack, cause, errors
                let eh = ptr as *const crate::error::ErrorHeader;
                let name_str = unsafe { string_header_to_string((*eh).name) };
                let msg_str = unsafe { string_header_to_string((*eh).message) };
                let stack_str = unsafe { string_header_to_string((*eh).stack) };
                let name_display = if name_str.is_empty() {
                    "Error"
                } else {
                    &name_str
                };
                // Issue #616: Node formats an uncaught throw as
                //   <Name>: <message>
                //       at <frame>
                //       ...
                // (no `Uncaught exception:` prefix). Perry's `stack` field
                // already starts with `<Name>: <message>` per Error.stack
                // convention, so emit just the stack — matches Node format
                // for this header. When the stack is empty (defensive), fall
                // back to the bare `<Name>: <message>` line.
                if !stack_str.is_empty() {
                    eprintln!("{}", stack_str);
                } else if msg_str.is_empty() {
                    eprintln!("{}", name_display);
                } else {
                    eprintln!("{}: {}", name_display, msg_str);
                }
                return;
            }
            if object_type == crate::error::OBJECT_TYPE_REGULAR {
                // Probe for `.message` and `.stack` properties the way
                // Node does for thrown non-Error objects. Users commonly
                // throw custom error shapes like `{ message, stack }` or
                // user-class instances that carry those fields.
                let msg_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
                let stack_key = crate::string::js_string_from_bytes(b"stack".as_ptr(), 5);
                let msg_val = crate::object::js_object_get_field_by_name_f64(
                    ptr as *const crate::object::ObjectHeader,
                    msg_key as *const crate::string::StringHeader,
                );
                let stack_val = crate::object::js_object_get_field_by_name_f64(
                    ptr as *const crate::object::ObjectHeader,
                    stack_key as *const crate::string::StringHeader,
                );
                let msg_str_ptr = crate::value::js_jsvalue_to_string(msg_val);
                let msg_str = unsafe { string_header_to_string(msg_str_ptr) };
                if !msg_str.is_empty() && msg_str != "undefined" {
                    eprintln!("Uncaught exception: {}", msg_str);
                } else {
                    let obj_str_ptr = crate::value::js_jsvalue_to_string(value);
                    let obj_str = unsafe { string_header_to_string(obj_str_ptr) };
                    if obj_str.is_empty() || obj_str == "[object Object]" {
                        eprintln!("Uncaught exception: [object] (bits=0x{:016X})", bits);
                    } else {
                        eprintln!("Uncaught exception: {}", obj_str);
                    }
                }
                let stack_str_ptr = crate::value::js_jsvalue_to_string(stack_val);
                let stack_str = unsafe { string_header_to_string(stack_str_ptr) };
                if !stack_str.is_empty() && stack_str != "undefined" {
                    eprintln!("{}", stack_str);
                }
                return;
            }
            // Fall through to generic stringify for arrays, promises,
            // bigints, maps, etc. — js_jsvalue_to_string handles them all.
        }
    }

    let s_ptr = crate::value::js_jsvalue_to_string(value);
    let s = unsafe { string_header_to_string(s_ptr) };
    if s.is_empty() {
        eprintln!("Uncaught exception: (bits=0x{:016X})", bits);
    } else {
        eprintln!("Uncaught exception: {}", s);
    }
}

/// GC root scanner: mark the current exception value
pub fn scan_exception_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_exception_roots_mut(&mut visitor);
}

pub fn scan_exception_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    with_exception_state(|s| unsafe {
        if (*s).has_exception {
            visitor.visit_nanbox_f64_raw_slot(&raw mut (*s).current_exception);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_set_exception(value: f64) {
    with_exception_state(|s| unsafe {
        (*s).current_exception = value;
        (*s).has_exception = true;
    });
}
