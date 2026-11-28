//! The close-callbacks phase of link sockets.
//!
//! Node emits a socket's `'close'` from its handle's close callback, in the
//! phase after check, and libuv runs that phase newest close first because it
//! prepends each closing handle. A close the phase itself starts waits for the
//! next one. Every link close is queued here, in call order: a handle's entry
//! becomes ready when the driver's `Closed` arrives, and a connect still
//! resolving has no handle, so its entry is ready at once. The end of each
//! check phase turns the driver once for the `Closed` of handles closed since
//! the last poll, then delivers the ready entries queued before it, newest
//! first, each followed by a tick and microtask checkpoint. Each entry holds a
//! resource ref on its cell until it is delivered.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::*;
use crate::native_payload::OwnerLink;

struct Entry {
    cell: usize,
    route: u8,
    /// The handle whose `Closed` this entry waits for; `None` when the
    /// connect was still resolving.
    handle: Option<Handle>,
    /// The binding released the payload when it closed: any payload on the
    /// cell at delivery belongs to a newer incarnation.
    released: bool,
    /// The `Closed` arrived after the owner had moved to a newer incarnation.
    stale: bool,
}

/// Entries by sequence number (call order), with the ready ones and the
/// waiting handles indexed, so every step is O(log n).
#[derive(Default)]
struct Queue {
    next_seq: u64,
    entries: BTreeMap<u64, Entry>,
    ready: BTreeSet<u64>,
    waiting: HashMap<(usize, [u32; 4]), u64>,
}

impl Queue {
    fn push(&mut self, entry: Entry, ready: bool) {
        let seq = self.next_seq;
        self.next_seq += 1;
        match (ready, entry.handle) {
            (false, Some(handle)) => {
                self.waiting
                    .insert((entry.cell, transport::handle_parts(handle)), seq);
            }
            _ => {
                self.ready.insert(seq);
            }
        }
        self.entries.insert(seq, entry);
    }
}

crate::perry_thread_local! {
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_tlnet")]
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
}

/// Owe `link`'s `Closed` on `route` to the close-callbacks phase: the
/// driver's `Closed` for `handle` makes it ready, and a resolving connect
/// (`None`) is ready now. `released` says the binding releases the payload
/// with the close. Returns whether a new entry was queued; the caller then
/// holds the ref that pins the cell. A handle already waiting keeps its entry,
/// which only learns that the payload is released.
pub(super) fn queue(link: OwnerLink, route: u8, handle: Option<Handle>, released: bool) -> bool {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let waiting = handle.and_then(|h| {
            q.waiting
                .get(&(link.0, transport::handle_parts(h)))
                .copied()
        });
        if let Some(seq) = waiting {
            if let Some(entry) = q.entries.get_mut(&seq) {
                entry.released |= released;
            }
            return false;
        }
        let entry = Entry {
            cell: link.0,
            route,
            handle,
            released,
            stale: false,
        };
        q.push(entry, handle.is_none());
        true
    })
}

/// Owe the close phase a `Closed` that arrived without a queued close.
pub(super) fn queue_closed(link: OwnerLink, route: u8, handle: Option<Handle>, stale: bool) {
    let entry = Entry {
        cell: link.0,
        route,
        handle,
        released: false,
        stale,
    };
    QUEUE.with(|q| q.borrow_mut().push(entry, true));
}

/// Mark the entry waiting for `handle`'s `Closed` ready. Returns whether one
/// was waiting.
pub(super) fn mark_closed(link: OwnerLink, handle: Handle, stale: bool) -> bool {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let Some(seq) = q.waiting.remove(&(link.0, transport::handle_parts(handle))) else {
            return false;
        };
        if let Some(entry) = q.entries.get_mut(&seq) {
            entry.stale = stale;
        }
        q.ready.insert(seq);
        true
    })
}

/// Whether an entry waits for `handle`'s `Closed` on `link`.
pub(super) fn awaits(link: OwnerLink, handle: Handle) -> bool {
    QUEUE.with(|q| {
        q.borrow()
            .waiting
            .contains_key(&(link.0, transport::handle_parts(handle)))
    })
}

/// Whether a close is owed at all: the loop stays alive for it.
pub(crate) fn pending() -> bool {
    QUEUE.with(|q| q.try_borrow().is_ok_and(|q| !q.entries.is_empty()))
}

/// Whether a close is ready for the next close-callbacks phase: the loop must
/// not park ahead of it.
pub(crate) fn ready() -> bool {
    QUEUE.with(|q| q.try_borrow().is_ok_and(|q| !q.ready.is_empty()))
}

/// The close-callbacks phase.
pub(crate) fn run() {
    let waiting = QUEUE.with(|q| !q.borrow().waiting.is_empty());
    if waiting {
        for completion in crate::event_pump::take_closed_completions(
            transport::is_queued_close,
            transport::link_cell,
            transport::retires_with,
        ) {
            transport::dispatch(completion);
        }
    }
    let Some(horizon) = QUEUE.with(|q| q.borrow().next_seq.checked_sub(1)) else {
        return;
    };
    // One taken at a time, with no borrow held: delivery runs JS, which may
    // close more sockets (they wait for the next phase) or deliver a `Closed`.
    while let Some(entry) = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        let seq = *q.ready.range(..=horizon).next_back()?;
        q.ready.remove(&seq);
        q.entries.remove(&seq)
    }) {
        // SAFETY: the entry's ref pins the cell until this call.
        unsafe {
            transport::deliver_closed(
                OwnerLink(entry.cell),
                entry.route,
                entry.handle,
                entry.released,
                entry.stale,
            )
        };
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    }
}

/// Drop every queued close (agent teardown, test reset), returning the cells
/// whose refs the caller releases.
pub(super) fn take_all() -> Vec<usize> {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.ready.clear();
        q.waiting.clear();
        std::mem::take(&mut q.entries)
            .into_values()
            .map(|e| e.cell)
            .collect()
    })
}
