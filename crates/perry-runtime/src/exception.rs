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

use crate::gc::{shadow_stack_restore, shadow_stack_savepoint, ShadowSavepoint};

extern "C" {
    fn longjmp(env: *mut i32, val: i32) -> !;
}

// Maximum nesting depth for try blocks. Backed by fixed-size per-thread
// arrays (see ExceptionState), so this directly sizes thread-local memory:
// jump_buffers is MAX_TRY_DEPTH * sizeof(JmpBuf) (256 B each). 1024 covers
// deep-but-legal recursion-through-try; genuinely unbounded recursion hits a
// native stack overflow well before this. Raised from 128 (#5065): 128
// aborted the process via panic on legal deeply-nested try/catch.
const MAX_TRY_DEPTH: usize = 1024;

/// Per-thread exception state. Exception handling uses setjmp/longjmp,
/// and a jmp_buf captured by setjmp on thread A is meaningless on thread
/// B (its stack frame doesn't exist there) — so the buffers, the depth
/// counter, the current exception, and the finally-flag all have to
/// live in TLS once `perry/thread` workers can run user code that
/// throws. Previously all five were process-wide `static mut`s and would
/// corrupt under any concurrent throw.
// arm64_32 fix: the three per-depth arrays are HEAP-allocated (`Box<[..]>`)
// instead of stored inline in TLS. At MAX_TRY_DEPTH=1024 they are ~280KB of
// initialized thread-local data (`jump_buffers` alone is 1024 * 256B = 256KB),
// which overflows ld64's 64KB `__thread_data` cap for arm64_32 (and the ILP32
// TLS layout generally). Boxing leaves only three fat pointers + scalars inline
// in TLS; the arrays live on the heap. `[T]` indexing on `Box<[T]>` is
// unchanged, so the accessors below need no edits. (Mirrors the
// TRANSITION_CACHE / VTABLE_IC / INTERN_TABLE boxing.)
struct ExceptionState {
    jump_buffers: Box<[JmpBuf]>,
    /// Shadow-stack depth captured when each `try` block was pushed, so the
    /// unwind path can drop the orphaned frames `longjmp` leaves behind (see
    /// `js_throw` / issue #1830). Indexed by try-depth, in lockstep with
    /// `jump_buffers`.
    shadow_savepoints: Box<[ShadowSavepoint]>,
    /// `js_native_call_method` recursion depth captured when each `try` was
    /// pushed. A throw `longjmp`s past the in-flight method frames, skipping
    /// their `CallMethodDepthGuard` `Drop`s; the unwind path restores this so
    /// the counter doesn't leak (see `js_throw` / `crate::object`'s
    /// `call_method_depth_*`). Indexed by try-depth, in lockstep with
    /// `jump_buffers`.
    call_method_depths: Box<[u32]>,
    try_depth: usize,
    current_exception: f64,
    has_exception: bool,
    in_finally: bool,
}

impl ExceptionState {
    // No longer `const`: `vec!` builds the arrays directly on the heap (no large
    // stack temporary), so first access lazily allocates ~280KB off the TLS.
    fn new() -> Self {
        ExceptionState {
            jump_buffers: vec![JmpBuf::new(); MAX_TRY_DEPTH].into_boxed_slice(),
            shadow_savepoints: vec![ShadowSavepoint::EMPTY; MAX_TRY_DEPTH].into_boxed_slice(),
            call_method_depths: vec![0u32; MAX_TRY_DEPTH].into_boxed_slice(),
            try_depth: 0,
            current_exception: 0.0,
            has_exception: false,
            in_finally: false,
        }
    }
}

thread_local! {
    static EXCEPTION_STATE: std::cell::UnsafeCell<ExceptionState> =
        std::cell::UnsafeCell::new(ExceptionState::new());
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
        // Capture the shadow-stack depth now, before the protected region
        // can push any callee frames, so the unwind path can restore to
        // exactly this point and drop the frames `longjmp` orphans (#1830).
        (*s).shadow_savepoints[depth] = shadow_stack_savepoint();
        // Capture the method-dispatch recursion depth too, so a throw caught by
        // this `try` can restore it — `longjmp` skips the `CallMethodDepthGuard`
        // `Drop`s of the method frames it unwinds (#5591).
        (*s).call_method_depths[depth] = crate::object::call_method_depth_savepoint();
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

/// Current `try` nesting depth on this thread. Async-context scopes
/// (`AsyncLocalStorage#run` etc.) record this at entry so the unwind path
/// can tell which scopes a throw is about to longjmp past (#788).
pub(crate) fn current_try_depth() -> usize {
    with_exception_state(|s| unsafe { (*s).try_depth })
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
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, value);
        (*s).has_exception = true;

        if (*s).in_finally {
            eprintln!("Cannot throw during finally block");
            std::process::abort();
        }

        if (*s).try_depth == 0 {
            print_uncaught(value);
            std::process::exit(1);
        }

        // Issue #2780: this throw is going to be CAUGHT by an open `try`
        // (try_depth > 0), so it is not a runaway uncaught loop. Reset the
        // `throw_not_callable` circuit-breaker counter so that valid JS which
        // throws-and-catches a non-callable many times (e.g. a route-handler
        // / retry loop doing `try { (undefined as any)() } catch {}` 200k
        // times) completes instead of tripping the abort at 100k. The
        // breaker is meant to catch genuinely *uncaught* runaway throw loops;
        // those still hit the `try_depth == 0` path above and abort there /
        // the async-step guards in `promise/microtasks.rs` still cover
        // unbounded async re-entry.
        crate::closure::reset_throw_not_callable_counter();

        let depth = (*s).try_depth - 1;
        // Apply the deferred context restores of async-context scopes
        // (`AsyncLocalStorage#run`/`#exit`, `runInAsyncScope`) whose normal
        // restore code this longjmp skips (#788). Pure thread-local state
        // swaps — no JS runs and nothing allocates.
        crate::async_context::unwind_context_guards(depth);
        // Drop the shadow-stack frames of the functions we are about to
        // unwind past. `longjmp` skips their epilogues (and therefore their
        // `js_shadow_frame_pop` calls), so without this the next GC would
        // scan — and the copying collector would rewrite — slots living in
        // already-unwound stack frames (#1830). Restore to the depth captured
        // when this `try` was pushed.
        shadow_stack_restore((*s).shadow_savepoints[depth]);
        // Restore the method-dispatch recursion depth captured when this `try`
        // was pushed. The frames we are about to `longjmp` past never run their
        // `CallMethodDepthGuard` `Drop`s, so without this the counter leaks one
        // per caught throw and eventually wedges every method call into the
        // depth-guard fallback (#5591).
        crate::object::call_method_depth_restore((*s).call_method_depths[depth]);
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
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, 0.0);
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
pub(crate) unsafe fn string_header_to_string(ptr: *const crate::string::StringHeader) -> String {
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
pub(crate) fn print_uncaught(value: f64) {
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
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, value);
        (*s).has_exception = true;
    });
}

#[cfg(test)]
pub(crate) fn test_try_depth() -> usize {
    with_exception_state(|s| unsafe { (*s).try_depth })
}

/// Replay the shadow-stack restore that `js_throw` performs for the
/// innermost open `try`, without the `longjmp` (which can't return in a
/// unit test). Lets tests exercise the real #1830 savepoint/restore path
/// recorded by `js_try_push`.
#[cfg(test)]
pub(crate) fn test_unwind_innermost_shadow_restore() {
    with_exception_state(|s| unsafe {
        assert!((*s).try_depth > 0, "no open try to unwind");
        let depth = (*s).try_depth - 1;
        shadow_stack_restore((*s).shadow_savepoints[depth]);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        js_shadow_frame_pop, js_shadow_frame_push, js_shadow_slot_set, shadow_stack_depth,
    };

    // Issue #1830: js_try_push must capture a shadow-stack savepoint, and the
    // unwind path (js_throw, here replayed without the longjmp) must restore
    // it so the orphaned frames of the functions being unwound past are
    // dropped before any later GC scans roots. All assertions are relative to
    // the entry state so this is robust under `--test-threads=1` (shared TLS).
    #[test]
    fn js_throw_path_restores_shadow_stack_across_unwound_frames() {
        let base_depth = shadow_stack_depth();
        let base_try = test_try_depth();

        // Establish run()'s frame.
        let run_frame = js_shadow_frame_push(1);
        js_shadow_slot_set(0, 0x7FFD_0000_0000_0001);
        let depth_at_try = shadow_stack_depth();

        // try { ... } — js_try_push records the savepoint at this depth.
        let _jb = js_try_push();
        assert_eq!(test_try_depth(), base_try + 1);

        // Callees push frames and the innermost throws (their pops skipped).
        let _f1 = js_shadow_frame_push(1);
        js_shadow_slot_set(0, 0x7FFD_0000_0000_00A1);
        let _f2 = js_shadow_frame_push(2);
        js_shadow_slot_set(0, 0x7FFD_0000_0000_00B1);
        assert_eq!(shadow_stack_depth(), depth_at_try + 2);

        // Replay js_throw's shadow restore (the longjmp itself can't return in
        // a unit test), then the catch path's js_try_end().
        test_unwind_innermost_shadow_restore();
        js_try_end();

        assert_eq!(test_try_depth(), base_try);
        assert_eq!(
            shadow_stack_depth(),
            depth_at_try,
            "unwind dropped the orphaned callee frames"
        );

        js_shadow_frame_pop(run_frame);
        assert_eq!(shadow_stack_depth(), base_depth);
    }

    #[test]
    fn try_push_pop_beyond_old_limit_does_not_panic() {
        // Regression for #5065: old fixed limit was 128 and js_try_push panicked
        // (aborting the process) at the 129th simultaneously-active try frame.
        // Relative to the entry depth so it's robust under shared TLS
        // (`--test-threads=1`) alongside the other tests in this module.
        let base = current_try_depth();
        let pushes = (MAX_TRY_DEPTH - base) - 1;
        assert!(
            pushes > 128,
            "expected room for >128 frames beyond the old limit"
        );
        for _ in 0..pushes {
            let p = js_try_push();
            assert!(!p.is_null(), "js_try_push returned null jmp_buf");
        }
        assert_eq!(current_try_depth(), base + pushes);
        for _ in 0..pushes {
            js_try_end();
        }
        assert_eq!(current_try_depth(), base);
    }
}
