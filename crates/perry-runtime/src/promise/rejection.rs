//! Unhandled-rejection tracking and the per-checkpoint rejection report
//! (HostPromiseRejectionTracker + Node's `processPromiseRejections`).
//!
//! A promise that rejects with NO reaction attached at rejection time is
//! "currently unhandled". Node decides whether that rejection is *really*
//! unhandled at the END OF THE MICROTASK CHECKPOINT in which it rejected —
//! not at process exit. A `.catch` attached in a later macrotask (a timer, an
//! I/O callback) is too late: the rejection has already been reported, and
//! attaching a handler then fires `'rejectionHandled'` instead of retroactively
//! suppressing it.
//!
//! The lifecycle of a tracked promise mirrors Node's
//! (`lib/internal/process/promises.js`):
//!
//! ```text
//!   reject with no reaction  ──► unhandled   (Node: pendingUnhandledRejections)
//!   checkpoint              ──► reported     (Node: maybeUnhandledPromises, warned=true)
//!     • 'unhandledRejection' listener  → emit(reason, promise), no crash
//!     • else 'uncaughtException' listener → emit(reason, "unhandledRejection")
//!     • else                            → print diagnostic + exit(1)
//!   handler attached after report ──► pending_handled (Node: asyncHandledRejections)
//!   next checkpoint          ──► emit 'rejectionHandled'
//! ```
//!
//! A handler attached BEFORE the checkpoint (a synchronous `.catch`, an
//! `await`, a `.then` from a microtask that runs in the same drain) removes
//! the promise from the set via `mark_rejection_handled` and nothing is
//! reported — that is the overwhelmingly common case, and it costs the reject
//! path nothing but an empty-vec check.
//!
//! The checkpoint itself is driven by codegen: `js_promise_process_rejections`
//! is called from the entry's event loop right after each microtask drain and
//! before the timer queues get a turn (Node's `processTicksAndRejections`
//! ordering — a `setTimeout(0)` scheduled before the rejection still runs
//! *after* the `unhandledRejection` handler). `js_promise_report_unhandled_rejections`
//! is the same checkpoint run one final time from the loop-exit epilogue.

use super::*;

thread_local! {
    static REJECTIONS: RefCell<RejectionTracker> = RefCell::new(RejectionTracker::default());
    /// Re-entrancy guard: a listener invoked from a checkpoint can run
    /// arbitrary JS (including code that drains microtasks); it must not
    /// re-enter the checkpoint and process the same promise twice.
    static PROCESSING_REJECTIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Default)]
struct RejectionTracker {
    /// Rejected with no reaction attached — candidates for the next checkpoint.
    /// Raw promise addresses; rooted + rewritten by
    /// `scan_unhandled_rejection_roots_mut` (#6077 part 3, PR #6209).
    unhandled: Vec<usize>,
    /// Already reported at a checkpoint (`warned` in Node). A handler attached
    /// to one of these fires `'rejectionHandled'`.
    reported: Vec<usize>,
    /// Reported promises that have since had a handler attached —
    /// `'rejectionHandled'` fires for them at the next checkpoint (Node defers
    /// it the same way: the `.catch` callback runs first, the event after).
    pending_handled: Vec<usize>,
    /// Promises the runtime owns and observes through internal channels — a
    /// WHATWG reader/writer `closed` promise, a `[[closeRequest]]`, etc. Node
    /// marks these `markPromiseAsHandled` at creation so that an abort / error
    /// / cancel that later rejects them is never surfaced as an unhandled
    /// rejection. We mirror that with a persistent membership set consulted at
    /// rejection-track time. Stays empty for non-stream programs, so the hot
    /// reject path pays nothing (#1545).
    internally_handled: std::collections::HashSet<usize>,
}

/// Cap on the reported (`warned`) set. Node keys this state off a
/// `SafeWeakMap`, so a reported promise costs nothing once it is garbage; our
/// set roots its entries (the `'rejectionHandled'` emit dereferences them), so
/// an unbounded set would be an unbounded leak for a long-lived server that
/// logs unhandled rejections. Past the cap the oldest entries are dropped:
/// their rejection was still reported, they just no longer fire
/// `'rejectionHandled'` if a handler shows up thousands of rejections later.
const MAX_REPORTED_REJECTIONS: usize = 1024;

/// Mark a promise as internally handled (Node's `markPromiseAsHandled`): a
/// later rejection of it is never reported as unhandled. Used by the WHATWG
/// stream implementation for the internal `closed` / `closeRequest` promises it
/// settles on abort/error/cancel without a user-attached reaction (#1545).
#[no_mangle]
pub extern "C" fn js_promise_mark_internally_handled(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    REJECTIONS.with(|t| {
        t.borrow_mut().internally_handled.insert(promise as usize);
    });
    // If it already rejected before being marked, drop it from the set now.
    mark_rejection_handled(promise);
}

/// Keep the stdlib-facing marker alive through the dead-strip pass on the
/// PERRY_NO_AUTO_OPTIMIZE prebuilt-lib link (same pattern as the checkpoint
/// hook anchors below).
#[used]
static KEEP_PROMISE_MARK_INTERNALLY_HANDLED: extern "C" fn(*mut Promise) =
    js_promise_mark_internally_handled;

pub(super) fn is_internally_handled(promise: *mut Promise) -> bool {
    REJECTIONS.with(|t| {
        let t = t.borrow();
        !t.internally_handled.is_empty() && t.internally_handled.contains(&(promise as usize))
    })
}

/// Record a rejection that has no reaction attached yet.
pub(super) fn track_unhandled_rejection(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    REJECTIONS.with(|t| t.borrow_mut().unhandled.push(promise as usize));
}

/// A handler was attached to `promise` — it is no longer an unhandled
/// rejection. If the rejection was ALREADY reported at an earlier checkpoint,
/// the attach is too late to suppress it and instead queues Node's
/// `'rejectionHandled'` event for the next checkpoint.
///
/// Cheap no-op for the common case (both sets are empty on the hot async path).
pub(crate) fn mark_rejection_handled(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let key = promise as usize;
    REJECTIONS.with(|t| {
        let mut t = t.borrow_mut();
        if !t.unhandled.is_empty() {
            t.unhandled.retain(|p| *p != key);
        }
        if !t.reported.is_empty() {
            if let Some(pos) = t.reported.iter().position(|p| *p == key) {
                t.reported.remove(pos);
                t.pending_handled.push(key);
            }
        }
    });
}

/// GC root scanner for the rejection sets (#6077). They store raw promise
/// addresses. A regular promise is allocated in the MOVABLE arena
/// (`js_promise_new_with_parent_impl`), so without this scanner a tracked
/// promise could be swept or evacuated before its checkpoint — making the
/// report's `(*pr).state` / `(*pr).reason` deref a stale/use-after-free read
/// (arena pages stay mapped, so a silent misreport is likelier than a hard
/// fault). Visiting each address marks the promise live AND rewrites the stored
/// pointer on evacuation, so the report always sees a valid promise. Entries
/// leave the sets (and are thus unrooted) as soon as a reaction is wired
/// (`mark_rejection_handled`) or the rejection is reported, so this never keeps
/// a genuinely-dead promise alive beyond its report.
pub fn scan_unhandled_rejection_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    REJECTIONS.with(|t| {
        let mut t = t.borrow_mut();
        for slot in t.unhandled.iter_mut() {
            visitor.visit_usize_slot(slot);
        }
        for slot in t.reported.iter_mut() {
            visitor.visit_usize_slot(slot);
        }
        for slot in t.pending_handled.iter_mut() {
            visitor.visit_usize_slot(slot);
        }
    });
}

/// Program-end hook (emitted by codegen's event-loop exit block, after the
/// final microtask/timer drain). The same checkpoint, run one last time so a
/// rejection raised by a `beforeExit` listener — or by the epilogue's final
/// timer tick — still surfaces before the process returns 0.
#[no_mangle]
pub extern "C" fn js_promise_report_unhandled_rejections() {
    process_rejections();
}

// #4876: keep the codegen-emitted hook alive through the auto-optimize
// whole-program-bitcode link. It is emitted unconditionally into `_main` but is
// reachable only from generated `.o`; without a `#[used]` anchor the
// internalize+dead-strip pass drops it and every native link fails with
// "undefined symbol" (see the error.rs/combinators.rs anchors for the same
// pattern).
#[used]
static KEEP_PROMISE_REPORT_UNHANDLED_REJECTIONS: extern "C" fn() =
    js_promise_report_unhandled_rejections;

/// One rejection checkpoint.
///
/// Node's `processPromiseRejections` drains `asyncHandledRejections` first, then
/// the pending unhandled list, and its caller re-runs the tick queue while
/// either produced work (`do { runNextTicks() } while (processPromiseRejections())`).
/// We mirror that: emit, drain the microtasks/ticks the listeners queued, and
/// re-check — so a `.catch` attached inside an `unhandledRejection` listener
/// gets its `'rejectionHandled'` in the same turn Node would deliver it.
pub(super) fn process_rejections() {
    if PROCESSING_REJECTIONS.with(|c| c.replace(true)) {
        return;
    }
    loop {
        let mut emitted = false;
        while let Some(promise) = take_pending_handled() {
            emit_rejection_handled(promise);
            emitted = true;
        }
        // Snapshot the budget the way Node snapshots `pendingUnhandledRejections.length`:
        // a promise rejected *by a listener* waits for the next checkpoint.
        let mut budget = REJECTIONS.with(|t| t.borrow().unhandled.len());
        while budget > 0 {
            budget -= 1;
            let Some(promise) = take_next_unhandled() else {
                break;
            };
            report_unhandled(promise);
            emitted = true;
        }
        if !emitted {
            break;
        }
        // Listener callbacks can queue microtasks/ticks (`.catch(...)` attached
        // inside the handler, a logger flush, …). Node runs them before the
        // next rejection pass; without this the `'rejectionHandled'` for a
        // handler attached inside an `'unhandledRejection'` listener would slip
        // to the next event-loop turn (or be lost entirely at loop exit).
        super::microtasks::js_promise_run_microtasks_checkpoint();
    }
    PROCESSING_REJECTIONS.with(|c| c.set(false));
}

/// Pop the next promise that is *still* genuinely unhandled, moving it into the
/// reported set before any listener runs (so a handler attached from inside the
/// listener queues `'rejectionHandled'` rather than being ignored).
///
/// Backstop re-check: a tracked promise is only *still* unhandled if it is
/// rejected AND no reaction was ever wired onto it. Any consumer —
/// `then`/`catch`/`finally`, chaining (`resolve_with_promise`), or
/// `attach_handlers` — sets `on_rejected` or `next` on the promise, so
/// re-reading those fields catches handlers attached through direct-field paths
/// we don't explicitly hook. (Settle-listener consumers don't touch these
/// fields, so `attach_settle_listener` removes them from the set at attach
/// time.) This makes the detector robust to internal machinery (async
/// generators, async-from-sync iterators) adopting a rejection.
fn take_next_unhandled() -> Option<*mut Promise> {
    REJECTIONS.with(|t| {
        let mut t = t.borrow_mut();
        while !t.unhandled.is_empty() {
            let addr = t.unhandled.remove(0);
            let pr = addr as *const Promise;
            let still_unhandled = unsafe {
                (*pr).state == PromiseState::Rejected
                    && (*pr).on_rejected.is_null()
                    && (*pr).next.is_null()
            };
            if !still_unhandled {
                continue;
            }
            if t.reported.len() >= MAX_REPORTED_REJECTIONS {
                t.reported.remove(0);
            }
            t.reported.push(addr);
            return Some(addr as *mut Promise);
        }
        None
    })
}

fn take_pending_handled() -> Option<*mut Promise> {
    REJECTIONS.with(|t| {
        let mut t = t.borrow_mut();
        if t.pending_handled.is_empty() {
            None
        } else {
            Some(t.pending_handled.remove(0) as *mut Promise)
        }
    })
}

/// `process.emit('rejectionHandled', promise)` — a handler showed up for a
/// rejection we already reported. Node emits a `PromiseRejectionHandledWarning`
/// when nobody listens; we stay silent (the warning carries Node's pid and
/// rejection id, neither of which is reproducible).
fn emit_rejection_handled(promise: *mut Promise) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(promise);
    let promise_value = box_promise_ptr(handle.get_raw_mut_ptr::<Promise>());
    with_listener_uncaught_trap(|| {
        let _ = crate::os::emit_process_event("rejectionHandled", &[promise_value]);
    });
}

/// Run a rejection listener under its own exception trap.
///
/// The checkpoint fires from inside `run_microtasks`, whose trap is shaped for
/// *microtask callbacks* (it rejects the running promise's `next` on a throw).
/// A throw out of an `unhandledRejection` listener has no such promise, so
/// without an inner trap it would longjmp into the microtask trap and be
/// silently swallowed. Node treats it as an ordinary uncaught exception:
/// `uncaughtException` listeners see it, otherwise the process dies. Mirrors
/// `timer::with_timer_uncaught_trap`, which does the same for timer callbacks
/// fired from the same drain.
fn with_listener_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    // SAFETY: this setjmp frame is live only for the synchronous listener
    // invocation below; `js_throw` longjmps back here before it is popped.
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    if jumped == 0 {
        f();
        crate::exception::js_try_end();
        return;
    }
    let exc = crate::exception::js_get_exception();
    crate::exception::js_clear_exception();
    crate::exception::js_try_end();
    if !crate::os::emit_process_event("uncaughtException", &[exc]) {
        crate::exception::print_uncaught(exc);
        crate::process::exit_after_current_thread_collection_teardown(1);
    }
}

/// Report one still-unhandled rejection, mirroring Node's default
/// (`--unhandled-rejections=throw`) mode:
///
/// 1. `'unhandledRejection'` listener → `(reason, promise)`, process continues.
/// 2. else the rejection is raised as an uncaught exception, so an
///    `'uncaughtException'` listener still observes it — with `origin`
///    `'unhandledRejection'` — and still suppresses the crash.
/// 3. else print the diagnostic and exit(1).
fn report_unhandled(promise: *mut Promise) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let reason_handle =
        scope.root_nanbox_f64(unsafe { (*promise_handle.get_raw_mut_ptr::<Promise>()).reason });

    let mut handled = false;
    with_listener_uncaught_trap(|| {
        let promise_value = box_promise_ptr(promise_handle.get_raw_mut_ptr::<Promise>());
        handled = crate::os::emit_process_event(
            "unhandledRejection",
            &[reason_handle.get_nanbox_f64(), promise_value],
        );
    });
    if handled {
        return;
    }

    // No `unhandledRejection` listener: Node raises the rejection as an uncaught
    // exception (`triggerUncaughtException(err, fromPromise=true)`), which an
    // `uncaughtException` listener observes with `origin === 'unhandledRejection'`
    // — and which still suppresses the crash.
    with_listener_uncaught_trap(|| {
        let origin = b"unhandledRejection";
        let origin_ptr = crate::string::js_string_from_bytes(origin.as_ptr(), origin.len() as u32);
        let origin_value = crate::value::js_nanbox_string(origin_ptr as i64);
        handled = crate::os::emit_process_event(
            "uncaughtException",
            &[reason_handle.get_nanbox_f64(), origin_value],
        );
    });
    if handled {
        return;
    }

    print_unhandled_diagnostic(reason_handle.get_nanbox_f64());
    // Match Node's unhandled-rejection exit code (1).
    crate::process::exit_after_current_thread_collection_teardown(1);
}

/// Surface the rejection reason instead of the bare, opaque
/// "Uncaught (in promise)" line (#4841): an unhandled rejection that carried a
/// `TypeError: ...` previously printed nothing useful, forcing users to wrap
/// every call in `.catch` just to learn what failed. Prefer the rejection
/// Error's `stack` (it begins with `<Name>: <message>` and carries `at <frame>`
/// file:line lines) so the throw site is visible — mirrors the synchronous
/// uncaught-throw handler (`exception::print_uncaught`). Falls back to ToString
/// for non-Error reasons / empty stacks.
fn print_unhandled_diagnostic(reason: f64) {
    let bits = reason.to_bits();
    if (bits >> 48) == 0x7FFD {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // Band+plausibility gate (2026-07-02 audit): a POINTER-tagged rejection
        // reason can be a registry HANDLE (a fetch Response id in the 0x40000
        // band — `fetch().then(r => { throw r })` uncaught) — the old bare
        // `>= 0x10000` deref'd the id as memory instead of printing the
        // fallback line.
        if crate::value::addr_class::is_plausible_heap_addr(ptr)
            && unsafe { *(ptr as *const u32) } == crate::error::OBJECT_TYPE_ERROR
        {
            let eh = ptr as *const crate::error::ErrorHeader;
            let stack_str = unsafe { crate::exception::string_header_to_string((*eh).stack) };
            if !stack_str.is_empty() {
                eprintln!("Uncaught (in promise) {stack_str}");
                return;
            }
        }
    }
    let reason_str_ptr = crate::value::js_jsvalue_to_string(reason);
    let reason_str = unsafe { crate::exception::string_header_to_string(reason_str_ptr) };
    if reason_str.is_empty() {
        eprintln!("Uncaught (in promise)");
    } else {
        eprintln!("Uncaught (in promise) {reason_str}");
    }
}
