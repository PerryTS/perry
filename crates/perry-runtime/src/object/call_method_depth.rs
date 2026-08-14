//! Recursion depth guard for `js_native_call_method`, preventing stack
//! overflow from circular module dependencies during initialization, plus the
//! savepoint/restore pair exception handling uses to keep the counter honest
//! across `longjmp`-style throws.

use std::cell::Cell;

crate::perry_thread_local! {
    static CALL_METHOD_DEPTH: Cell<u32> = const { Cell::new(0) };
}
const MAX_CALL_METHOD_DEPTH: u32 = 512;

pub(super) struct CallMethodDepthGuard {
    depth_before: u32,
}
impl CallMethodDepthGuard {
    pub(super) fn enter(_method_name: &str) -> Option<Self> {
        CALL_METHOD_DEPTH.with(|d| {
            let v = d.get();
            if v >= MAX_CALL_METHOD_DEPTH {
                // Silently return null object to prevent stack overflow
                None
            } else {
                // Debug logging disabled for production runs
                // if v <= 10 || v % 50 == 0 {
                //     eprintln!("[DEPTH GUARD] depth={} calling method '{}'", v, method_name);
                // }
                d.set(v + 1);
                Some(CallMethodDepthGuard { depth_before: v })
            }
        })
    }
}
impl Drop for CallMethodDepthGuard {
    fn drop(&mut self) {
        CALL_METHOD_DEPTH.with(|d| {
            let current = d.get();
            // `js_throw` restores the counter before transporting a generated
            // exception. The fast transport installs the catch context
            // directly, but its system-unwinder fallback subsequently runs
            // Rust cleanups. In that fallback this guard has already been
            // accounted for by the restore, so its Drop must be idempotent.
            // An unconditional subtraction wrapped the counter to u32::MAX
            // after a caught Next.js manifest probe and permanently tripped
            // the recursion guard on every later method call.
            if current > self.depth_before {
                d.set(current - 1);
            }
        });
    }
}

/// Snapshot the current `js_native_call_method` recursion depth. Exception
/// handling (`js_try_push`) records this at each `try` so the unwind path can
/// restore it: a `js_throw` `longjmp`s past the in-flight method frames and
/// skips their `CallMethodDepthGuard` `Drop`s, so without an explicit restore
/// the counter leaks one per caught throw and — after `MAX_CALL_METHOD_DEPTH`
/// throw/catch cycles — wedges every subsequent method call into the
/// stack-overflow fallback (returning the empty null-object instead of
/// dispatching). System unwinding does run those drops; guards remember their
/// entry depths so the eager restore makes their later cleanup a no-op instead
/// of a second decrement. See `crate::exception::{js_try_push, js_throw}`.
pub(crate) fn call_method_depth_savepoint() -> u32 {
    CALL_METHOD_DEPTH.with(|d| d.get())
}

/// Restore the `js_native_call_method` recursion depth captured by
/// [`call_method_depth_savepoint`]. Called on the `longjmp` unwind path so the
/// frames the throw skips don't leak their depth increments (see above).
pub(crate) fn call_method_depth_restore(depth: u32) {
    CALL_METHOD_DEPTH.with(|d| d.set(depth));
}
