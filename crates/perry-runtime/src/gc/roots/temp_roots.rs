//! Precise GC roots for generated-code expression temporaries (#6951).
//!
//! # What this is for
//!
//! The shadow stack ([`super::shadow_stack`]) roots *named locals*: codegen
//! reserves one slot per pointer-typed local and binds it to that local's
//! alloca. It has no slots for the values that only ever exist between two
//! instructions — the accumulator array a variadic call builds while it is
//! still evaluating its arguments, an already-evaluated operand waiting for its
//! sibling, a freshly allocated receiver waiting for its consumer. Those live
//! in LLVM SSA registers, and an SSA register is not a root.
//!
//! Until #6951 that was covered — accidentally — by the conservative native
//! stack scan, which finds whatever LLVM happened to spill. `gc_check_trigger`
//! forces that scan on both automatic arms, so the gap was invisible in
//! practice while `gc::roots::conservative_stack_scan_mode` nominally resolves
//! `Auto -> SkipDisabled` in shipped builds. Run a compiled program with
//! `PERRY_CONSERVATIVE_STACK_SCAN=off` and the gap is a live use-after-free:
//! `console.log("label", allocatingCall())` loses its label, and the harder
//! shapes segfault.
//!
//! # The contract
//!
//! A temp root is a **stack** of words, per thread. Generated code pushes the
//! word it needs kept alive, evaluates whatever may collect, then reads the
//! word back out of the root and truncates. Reading back is not optional
//! bookkeeping: this is a *mutable* root, so an evacuating cycle rewrites the
//! slot, and the pre-collection SSA register is stale afterwards. Slots are
//! visited with [`RuntimeRootVisitor::visit_heap_word_u64_slot`], the same
//! decoder the shadow stack uses, so a slot may hold either form the
//! `gc::root_words` contract admits:
//!
//! - a NaN-boxed value (`POINTER_TAG` / `STRING_TAG` / `BIGINT_TAG`), or
//! - a bare heap address, which is what the raw `i64` array pointers threaded
//!   through `js_array_alloc` / `js_array_push_f64` are.
//!
//! Immediates (numbers, `undefined`, small ints) decode to nothing and cost a
//! push slot and no more, so callers do not have to prove pointer-ness.
//!
//! # Balance
//!
//! `js_gc_temp_root_truncate(base)` drops `base` and everything above it, so a
//! missed truncate is bounded by the next one rather than leaking forever.
//! Non-local exits are covered too: [`super::shadow_stack::ShadowSavepoint`]
//! carries the temp-root depth, so the `longjmp` unwind that already restores
//! the shadow stack (`crate::exception`) restores this stack with it.

use super::*;

/// Initial capacity, in words. One `console.log` argument list needs one slot;
/// this is sized so ordinary nesting never reallocates.
const TEMP_ROOT_RESERVE: usize = 64;

thread_local! {
    /// Safety mirrors [`super::shadow_stack::SHADOW`]: these ops run only from
    /// compiled code on this thread and from GC scanner/rewriter passes, and
    /// the two never overlap (GC is stop-the-world relative to this TLS, and
    /// the scanner allocates nothing).
    pub(crate) static TEMP_ROOTS: std::cell::UnsafeCell<Vec<u64>> =
        std::cell::UnsafeCell::new(Vec::with_capacity(TEMP_ROOT_RESERVE));
}

/// Report a temp-root stack overflow without unwinding.
///
/// Same defect class as #7145's `js_shadow_frame_pop`: a `debug_assert!(false)`
/// inside an `extern "C"` fn aborts the process in a debug build instead of
/// unwinding. Unreachable in practice (it needs `u32::MAX` live temp roots), so
/// no test drives it — which is exactly why it should not be left as a latent
/// abort. One line on stderr, once per process.
#[cold]
fn report_temp_root_overflow(depth: usize) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[perry-gc] temp-root stack overflow at depth {depth}: refusing to grow \
(codegen dropped a js_gc_temp_root_truncate). The returned index still addresses \
a live slot. Reported once per process."
    );
}

/// Push `value` and return the index generated code must pass to
/// `js_gc_temp_root_get` / `js_gc_temp_root_set` / `js_gc_temp_root_truncate`.
#[no_mangle]
pub extern "C" fn js_gc_temp_root_push(value: u64) -> u32 {
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &mut *cell.get();
        let idx = s.len();
        // A depth this large means codegen dropped a truncate; refusing to grow
        // keeps a runaway from turning into unbounded retention. The returned
        // index still addresses a live slot, so get/set stay well-defined.
        if idx >= u32::MAX as usize {
            report_temp_root_overflow(idx);
            return (idx - 1) as u32;
        }
        s.push(value);
        if value != 0 {
            crate::gc::runtime_write_barrier_root_heap_word(value);
        }
        idx as u32
    })
}

/// Read slot `idx` back. Generated code must use this value, not the register
/// it pushed: an evacuating cycle rewrites the slot in place.
#[no_mangle]
pub extern "C" fn js_gc_temp_root_get(idx: u32) -> u64 {
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &*cell.get();
        s.get(idx as usize).copied().unwrap_or(0)
    })
}

/// Overwrite slot `idx`, for producers that hand back a possibly-reallocated
/// pointer (`js_array_push_f64`).
#[no_mangle]
pub extern "C" fn js_gc_temp_root_set(idx: u32, value: u64) {
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &mut *cell.get();
        if let Some(slot) = s.get_mut(idx as usize) {
            *slot = value;
            if value != 0 {
                crate::gc::runtime_write_barrier_root_heap_word(value);
            }
        }
    });
}

/// Drop slot `base` and every slot above it.
#[no_mangle]
pub extern "C" fn js_gc_temp_root_truncate(base: u32) {
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = base as usize;
        if base < s.len() {
            s.truncate(base);
        }
    });
}

/// Push a rooted value onto the array in temp-root slot `idx`, writing the
/// (possibly reallocated) array pointer back into the slot.
///
/// This is the fused form of get + `js_array_push_f64` + set: the argument
/// accumulator of a variadic call is pushed to once per argument, and the
/// three-call form would triple that traffic for no added safety. `value` is
/// rooted by `js_array_push_f64` itself (its grow path takes a
/// `RuntimeHandleScope`), so only the accumulator needs the slot.
#[no_mangle]
pub extern "C" fn js_array_push_f64_temp_rooted(idx: u32, value: f64) {
    let arr = js_gc_temp_root_get(idx) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, value);
    js_gc_temp_root_set(idx, arr as u64);
}

/// Current depth — the value a savepoint records.
pub(crate) fn temp_root_depth() -> usize {
    TEMP_ROOTS.with(|cell| unsafe { (*cell.get()).len() })
}

/// Restore a previously-recorded depth. Used by the exception unwind path via
/// [`super::shadow_stack::ShadowSavepoint`]; a `longjmp` can only have added
/// slots relative to the savepoint, so the `<=` guard is defensive.
pub(crate) fn temp_roots_restore(depth: usize) {
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &mut *cell.get();
        if depth <= s.len() {
            s.truncate(depth);
        }
    });
}

/// Test-only: drop every temp root, so an isolated GC test starts from a known
/// empty stack.
#[cfg(test)]
pub(crate) fn reset_temp_roots() {
    TEMP_ROOTS.with(|cell| unsafe {
        (*cell.get()).clear();
    });
}

pub(crate) fn scan_temp_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    TEMP_ROOTS.with(|cell| unsafe {
        for slot in (*cell.get()).iter_mut() {
            visitor.visit_heap_word_u64_slot(slot);
        }
    });
}

#[derive(Default)]
pub(crate) struct TempRootScanState {
    cursor: usize,
}

pub(crate) fn new_temp_root_scan_state() -> Box<dyn Any> {
    Box::<TempRootScanState>::default()
}

pub(crate) fn scan_temp_roots_mut_step(
    visitor: &mut RuntimeRootVisitor<'_>,
    state: &mut dyn Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<TempRootScanState>()
        .expect("temp root scanner state type");
    TEMP_ROOTS.with(|cell| unsafe {
        let s = &mut *cell.get();
        while *remaining > 0 && state.cursor < s.len() {
            visitor.visit_heap_word_u64_slot(&mut s[state.cursor]);
            state.cursor += 1;
            *remaining -= 1;
        }
        state.cursor >= s.len()
    })
}
