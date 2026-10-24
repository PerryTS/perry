//! Loop interrupts: how one agent asks another to stop running JS (#12633).
//!
//! `worker.terminate()` must stop a worker wherever it is, including inside a
//! loop that makes no calls at all (`while (true) { x = (x + 1) | 0; }`).
//! Such a loop never returns to the worker's event loop, so a `Terminate`
//! command on the worker's channel alone is never read.
//!
//! The mechanism is V8's: every loop back-edge of compiled code polls. The
//! compiler places the poll after LLVM's optimization pipeline
//! (`perry-codegen/src/inprocess/loop_interrupt.rs`), at the back-edges of
//! the FINAL loops, so a vectorized or unrolled loop polls once per machine
//! iteration and no loop optimization ever sees the poll. The poll is a
//! relaxed load of [`PERRY_LOOP_INTERRUPT_ARMED`] and a branch; only when the
//! word is non-zero does it call [`js_loop_interrupt`].
//!
//! The word counts outstanding requests process-wide. A process-global word
//! keeps the poll free of TLS (a thread-local costs a `_tlv_get_addr` call per
//! back-edge on Darwin); the cost of that choice is that while ANY agent has a
//! request outstanding, every agent's back-edges take the call, read their own
//! [`TerminationHandle`] and return. Requests are rare and short-lived (a
//! request is retired when its agent stops), so that window is too.
//!
//! [`js_loop_interrupt`] never collects and never runs JS: it either returns,
//! or ends this agent's JS by unwinding straight to the worker's base trap
//! (`exception::hand_to_worker_base`), passing every `catch` and `finally` on
//! the way, as Node's termination does.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

/// Process-global count of outstanding termination requests; non-zero means
/// back-edge polls must call [`js_loop_interrupt`].
///
/// **This symbol is ABI.** `perry-codegen` declares a mirror
/// (`@PERRY_LOOP_INTERRUPT_ARMED`, `external global i32`) and loads it at every
/// loop back-edge, so the name, type and width are part of the
/// compiler/runtime contract.
#[no_mangle]
pub static PERRY_LOOP_INTERRUPT_ARMED: AtomicU32 = AtomicU32::new(0);

const RUNNING: u8 = 0;
const REQUESTED: u8 = 1;
const RETIRED: u8 = 2;

/// One agent's termination state, shared between the agent (which installs it
/// and retires it) and whoever may ask it to stop.
///
/// Each handle holds at most one arm of [`PERRY_LOOP_INTERRUPT_ARMED`]: taken
/// by the request that moves it `RUNNING -> REQUESTED`, released by the
/// `retire` that moves it out of `REQUESTED`. Every other transition arms and
/// disarms within the same call, so the count is exact whatever the
/// interleaving.
#[derive(Clone, Default)]
pub struct TerminationHandle(Arc<AtomicU8>);

impl TerminationHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the agent to stop. Returns false when it already stopped or was
    /// already asked.
    pub fn request(&self) -> bool {
        // Arm BEFORE publishing REQUESTED: an agent that can see the request
        // must also see the armed word, or its polls would skip it.
        PERRY_LOOP_INTERRUPT_ARMED.fetch_add(1, Ordering::SeqCst);
        match self
            .0
            .compare_exchange(RUNNING, REQUESTED, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => true,
            Err(_) => {
                PERRY_LOOP_INTERRUPT_ARMED.fetch_sub(1, Ordering::SeqCst);
                false
            }
        }
    }

    /// The agent has stopped running JS: release any outstanding request.
    pub fn retire(&self) {
        if self.0.swap(RETIRED, Ordering::SeqCst) == REQUESTED {
            PERRY_LOOP_INTERRUPT_ARMED.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst) == REQUESTED
    }
}

thread_local! {
    static CURRENT: RefCell<Option<TerminationHandle>> = const { RefCell::new(None) };
    static TERMINATED: Cell<bool> = const { Cell::new(false) };
}

/// Install the handle the polls on this thread answer to. A thread with no
/// handle cannot be terminated and its polls always return.
pub fn install_current(handle: TerminationHandle) {
    TERMINATED.with(|t| t.set(false));
    CURRENT.with(|c| *c.borrow_mut() = Some(handle));
}

/// Remove this thread's handle (and retire it).
pub fn clear_current() {
    if let Some(handle) = CURRENT.with(|c| c.borrow_mut().take()) {
        handle.retire();
    }
}

/// Whether this thread's JS was ended by a termination request; reading it
/// clears it. The worker's base trap sees a termination as an exception, and
/// this is how it tells the two apart.
pub fn take_terminated() -> bool {
    TERMINATED.with(|t| t.replace(false))
}

/// Whether a termination request is outstanding for this thread.
pub fn current_termination_requested() -> bool {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(TerminationHandle::is_requested)
    })
}

/// End this thread's JS if it was asked to stop; otherwise return. Called by
/// the back-edge poll when [`PERRY_LOOP_INTERRUPT_ARMED`] is non-zero, and by
/// runtime loops that run JS callbacks back to back (microtask drains).
#[no_mangle]
pub extern "C" fn js_loop_interrupt() {
    if !current_termination_requested() {
        return;
    }
    TERMINATED.with(|t| t.set(true));
    // Does not return on a worker thread: every frame between here and the
    // worker's base trap is passed, JS `catch`/`finally` included.
    crate::exception::hand_to_worker_base(f64::from_bits(crate::value::TAG_UNDEFINED));
    // Not under a worker base (cannot happen for an installed handle): leave
    // the request for the agent's event loop.
    TERMINATED.with(|t| t.set(false));
}

// Codegen-only symbols: referenced by name from the poll the compiler inserts,
// so anchor them against the whole-program dead-strip (#4876).
#[used(compiler)]
static KEEP_JS_LOOP_INTERRUPT: extern "C" fn() = js_loop_interrupt;
#[used(compiler)]
static KEEP_LOOP_INTERRUPT_ARMED: &AtomicU32 = &PERRY_LOOP_INTERRUPT_ARMED;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_interleaving_releases_its_arm() {
        let Some(_isolated) = crate::test_support::isolated_process_guard() else {
            return;
        };
        let base = PERRY_LOOP_INTERRUPT_ARMED.load(Ordering::SeqCst);
        // request then retire
        let h = TerminationHandle::new();
        assert!(h.request());
        assert!(h.is_requested());
        assert!(!h.request(), "a second request is a no-op");
        h.retire();
        // retire then request
        let g = TerminationHandle::new();
        g.retire();
        assert!(!g.request(), "a retired agent cannot be asked to stop");
        // retire twice
        g.retire();
        assert_eq!(PERRY_LOOP_INTERRUPT_ARMED.load(Ordering::SeqCst), base);
    }

    #[test]
    fn a_thread_without_a_handle_is_never_interrupted() {
        js_loop_interrupt();
        assert!(!take_terminated());
    }
}
