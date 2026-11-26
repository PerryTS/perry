//! The close-callbacks phase of link sockets.
//!
//! Node emits a socket's `'close'` from its handle's close callback, in the
//! phase after check, and libuv runs that phase newest close first because it
//! prepends each closing handle. A close the phase itself starts waits for the
//! next one. [`transport::close`] queues every link close here, in call order:
//! a handle's entry becomes ready when the driver's `Closed` arrives, and a
//! connect still resolving has no handle, so its entry is ready at once. The
//! end of each check phase turns the driver once for the `Closed` of handles
//! closed since the last poll, then delivers the ready entries queued before
//! it, newest first, each followed by a tick and microtask checkpoint. Each
//! entry holds a resource ref on its cell until it is delivered.

use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::native_payload::OwnerLink;

struct Entry {
    seq: u64,
    cell: usize,
    route: u8,
    /// The handle whose `Closed` this entry waits for; `None` when the
    /// connect was still resolving.
    handle: Option<Handle>,
    ready: bool,
}

#[derive(Default)]
struct Queue {
    next_seq: u64,
    entries: VecDeque<Entry>,
}

crate::perry_thread_local! {
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_tlnet")]
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
}

/// Owe `link`'s `Closed` on `route` to the close-callbacks phase; the caller
/// holds the ref that pins the cell.
pub(super) fn queue(link: OwnerLink, route: u8, handle: Option<Handle>) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let seq = q.next_seq;
        q.next_seq += 1;
        q.entries.push_back(Entry {
            seq,
            cell: link.0,
            route,
            handle,
            ready: handle.is_none(),
        });
    });
}

fn waits_for(e: &Entry, link: OwnerLink, handle: Option<Handle>) -> bool {
    e.cell == link.0 && !e.ready && e.handle.is_some() && e.handle == handle
}

/// Mark the entry waiting for `handle`'s `Closed` ready. Returns whether one
/// was waiting.
pub(super) fn mark_closed(link: OwnerLink, handle: Option<Handle>) -> bool {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let waiting = q.entries.iter_mut().find(|e| waits_for(e, link, handle));
        waiting.map(|e| e.ready = true).is_some()
    })
}

/// Whether an entry waits for `handle`'s `Closed` on `link`.
pub(super) fn awaits(link: OwnerLink, handle: Option<Handle>) -> bool {
    QUEUE.with(|q| {
        q.borrow()
            .entries
            .iter()
            .any(|e| waits_for(e, link, handle))
    })
}

/// Whether a close is owed at all: the loop stays alive for it.
pub(crate) fn pending() -> bool {
    QUEUE.with(|q| q.try_borrow().is_ok_and(|q| !q.entries.is_empty()))
}

/// Whether a close is ready for the next close-callbacks phase: the loop must
/// not park ahead of it.
pub(crate) fn ready() -> bool {
    QUEUE.with(|q| {
        q.try_borrow()
            .is_ok_and(|q| q.entries.iter().any(|e| e.ready))
    })
}

/// The close-callbacks phase.
pub(crate) fn run() {
    let waiting = QUEUE.with(|q| q.borrow().entries.iter().any(|e| !e.ready));
    if waiting {
        for completion in crate::event_pump::take_closed_completions(
            transport::is_queued_close,
            transport::link_cell,
        ) {
            transport::dispatch(completion);
        }
    }
    let Some(horizon) = QUEUE.with(|q| q.borrow().entries.back().map(|e| e.seq)) else {
        return;
    };
    // One taken at a time, with no borrow held: delivery runs JS, which may
    // close more sockets (they wait for the next phase) or deliver a `Closed`.
    while let Some(entry) = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let newest = q
            .entries
            .iter()
            .rposition(|e| e.ready && e.seq <= horizon)?;
        q.entries.remove(newest)
    }) {
        // SAFETY: the entry's ref pins the cell until this call.
        unsafe { transport::deliver_closed(OwnerLink(entry.cell), entry.route, entry.handle) };
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    }
}

/// Drop every queued close (agent teardown, test reset), returning the cells
/// whose refs the caller releases.
pub(super) fn take_all() -> Vec<usize> {
    QUEUE.with(|q| q.borrow_mut().entries.drain(..).map(|e| e.cell).collect())
}
